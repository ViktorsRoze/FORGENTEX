// Debugger v0 — the tracing debugger (docs/debugger_plan.md). The core
// property under test: every trace step names the SAME canonical node id that
// semantic patches target, so an agent can go from a failing run to a patch
// target without any translation.

use crate::interp::{debug_entry, DebugOptions};
use crate::parser::parse_human;

const COUNT: &str = "fn count(n: i32) -> i32:\n    let total: i32 = 0\n    while n > 0:\n        total = total + n\n        n = n - 1\n    return total\n";

const CALLS: &str =
    "fn helper(x: i32) -> i32:\n    return x * 2\n\nfn outer() -> i32:\n    return helper(21)\n";

const TRAPPING: &str = "fn inner(d: i32) -> i32:\n    let k: i32 = 10\n    return k / d\n\nfn outer2(d: i32) -> i32:\n    let tag: i32 = 7\n    return inner(d)\n";

#[test]
fn trace_steps_use_canonical_node_ids_and_env_deltas() {
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 3 } }",
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "completed");
    assert_eq!(report.return_text.as_deref(), Some("6"));

    // let(1) + while container(1) + 3 iterations x 2 assigns(6) + return(1).
    assert_eq!(report.steps.len(), 9, "steps: {:#?}", report.steps);

    // The first step is the `let`, at the canonical root id, with its delta.
    let first = &report.steps[0];
    assert_eq!(first.node, "@function.count.stmt.0");
    assert_eq!(first.kind, "let");
    assert!(first
        .changed
        .contains(&("total".to_string(), "0".to_string())));

    // Loop-body steps carry the canonical `.body.` path and repeat per
    // iteration — the ids match what `fgx patch` would target.
    let body_steps: Vec<_> = report
        .steps
        .iter()
        .filter(|step| step.node == "@function.count.stmt.1.body.0")
        .collect();
    assert_eq!(body_steps.len(), 3, "one per iteration");
    // First iteration: total = 0 + 3.
    assert!(body_steps[0]
        .changed
        .contains(&("total".to_string(), "3".to_string())));
}

#[test]
fn call_tree_records_enter_and_exit() {
    let program = parse_human(CALLS).unwrap();
    let report = debug_entry(
        &program,
        "outer",
        "{ \"args\": {} }",
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "completed");
    let events: Vec<(String, String, String)> = report
        .calls
        .iter()
        .map(|call| {
            (
                call.event.clone(),
                call.function.clone(),
                call.detail.clone(),
            )
        })
        .collect();
    assert_eq!(events[0].0, "enter");
    assert_eq!(events[0].1, "outer");
    assert_eq!(
        events[1],
        (
            "enter".to_string(),
            "helper".to_string(),
            "x=21".to_string()
        )
    );
    assert_eq!(
        events[2],
        ("exit".to_string(), "helper".to_string(), "42".to_string())
    );
    assert_eq!(
        events[3],
        ("exit".to_string(), "outer".to_string(), "42".to_string())
    );
}

#[test]
fn breakpoint_stops_before_the_node_with_locals() {
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 3 } }",
        DebugOptions {
            break_at: Some("@function.count.stmt.2".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.status, "breakpoint");
    let (node, locals) = report.breakpoint.as_ref().expect("breakpoint state");
    assert_eq!(node, "@function.count.stmt.2");
    // The loop has fully run by the time the return is reached.
    assert!(locals.contains(&("total".to_string(), "6".to_string())));
    assert!(locals.contains(&("n".to_string(), "0".to_string())));
    // The broken-at node itself never executed.
    assert!(report
        .steps
        .iter()
        .all(|step| step.node != "@function.count.stmt.2"));
}

#[test]
fn trap_post_mortem_captures_the_frame_stack() {
    let program = parse_human(TRAPPING).unwrap();
    let report = debug_entry(
        &program,
        "outer2",
        "{ \"args\": { \"d\": 0 } }",
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "trap");
    assert!(
        report
            .error
            .as_deref()
            .unwrap_or("")
            .contains("division_by_zero"),
        "error: {:?}",
        report.error
    );
    // Innermost frame first, each with its locals at failure.
    assert_eq!(report.frames.len(), 2, "frames: {:#?}", report.frames);
    assert_eq!(report.frames[0].function, "inner");
    assert!(report.frames[0]
        .locals
        .contains(&("k".to_string(), "10".to_string())));
    assert_eq!(report.frames[1].function, "outer2");
    assert!(report.frames[1]
        .locals
        .contains(&("tag".to_string(), "7".to_string())));
}

#[test]
fn step_limit_bounds_runaway_loops() {
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 100000 } }",
        DebugOptions {
            max_steps: 50,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.status, "step_limit");
    assert_eq!(report.steps.len(), 50, "the trace is bounded");
}

#[test]
fn json_report_carries_the_schema_and_node_ids() {
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 1 } }",
        DebugOptions::default(),
    )
    .unwrap();
    let json = report.to_json();
    assert!(
        json.contains("\"schema\": \"forgentex.debug_trace.v1\""),
        "{json}"
    );
    assert!(json.contains("@function.count.stmt.0"), "{json}");
    assert!(json.contains("\"changed\""), "{json}");
    // Human text form renders too.
    let text = report.to_text();
    assert!(text.contains("status=completed"), "{text}");
}
