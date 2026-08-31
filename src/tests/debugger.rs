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

// --- Phase 1: the debugger reaches the governed surface ---------------------

const CAPSULE: &str = "foreign twice_c c version \"c11\" effects [alloc]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdint.h>\n        int32_t twice_c(int32_t value) { return value * 2; }\n\nfn driver(x: i32) -> i32 effects [alloc]:\n    let doubled: i32 = twice_c(x)\n    return doubled + 1\n";

#[test]
fn a_plain_debug_run_still_refuses_to_execute_capsules() {
    // Runtime reach is opt-in: without `live` the debugger must stay pure, so
    // an ordinary debug run cannot reach a foreign toolchain.
    let program = parse_human(CAPSULE).unwrap();
    let report = debug_entry(
        &program,
        "driver",
        r#"{ "args": { "x": 21 } }"#,
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "trap");
    assert!(
        report
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("capsule_call_not_permitted"),
        "{report:?}"
    );
    assert!(report.capsule_calls.is_empty());
}

#[test]
fn a_live_debug_run_steps_through_a_capsule_and_records_its_transcript() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(CAPSULE).unwrap();
    let report = debug_entry(
        &program,
        "driver",
        r#"{ "args": { "x": 21 } }"#,
        DebugOptions {
            live: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.status, "completed", "{report:?}");
    assert_eq!(report.return_text.as_deref(), Some("43"));

    // The capsule call is one step, and its result lands in the env delta.
    let call_step = report
        .steps
        .iter()
        .find(|step| step.node == "@function.driver.stmt.0")
        .expect("capsule call step");
    assert!(
        call_step
            .changed
            .iter()
            .any(|(n, v)| n == "doubled" && v == "42"),
        "{call_step:?}"
    );

    // A capsule call is opaque in the trace, so the transcript is the evidence
    // of what happened across the boundary.
    assert_eq!(report.capsule_calls.len(), 1);
    assert_eq!(report.capsule_calls[0].capsule, "twice_c");
    assert_eq!(report.capsule_calls[0].decision, "allow");
    assert!(report.to_json().contains("\"capsule_calls\""));
    assert!(report.to_text().contains("twice_c [c] allow"));
}

#[test]
fn traced_values_elide_long_collections_by_default() {
    // Debugging anything that reads a real file used to render the whole buffer
    // on every step: 40 steps over a 2.6 MB read produced 11.3 MB of trace.
    let source = "fn build(limit: i32) -> i32 effects [alloc]:\n    let data: Vec[i32] = vec_new()\n    let i: i32 = 0\n    while i < limit:\n        push(data, i)\n        i = i + 1\n    return i32(data.len)\n";
    let program = parse_human(source).unwrap();

    let summarized = debug_entry(
        &program,
        "build",
        r#"{ "args": { "limit": 40 } }"#,
        DebugOptions::default(),
    )
    .unwrap();
    let text = summarized.to_text();
    assert!(text.contains("+"), "expected an elision marker in {text}");
    assert!(
        text.contains("more"),
        "expected an elision marker in {text}"
    );

    let full = debug_entry(
        &program,
        "build",
        r#"{ "args": { "limit": 40 } }"#,
        DebugOptions {
            value_elements: usize::MAX,
            ..Default::default()
        },
    )
    .unwrap();
    // `--values full` opts back into the whole buffer, and must be bigger.
    assert!(
        full.to_text().len() > text.len(),
        "full render should exceed the summarised one"
    );
    assert!(!full.to_text().contains(" more]"));
}

#[test]
fn tracing_a_growing_buffer_stays_linear() {
    // The tracer used to clone the whole environment on every step to diff it,
    // so tracing a program holding a large buffer was O(heap) per step and
    // quadratic over the run: overhead measured 71 -> 159 -> 600 ms as the
    // buffer doubled. Reporting the statement's write targets removed the
    // clone. At this size the old path took tens of seconds; the bound is
    // generous enough not to flake on a loaded host but still separates linear
    // from quadratic by orders of magnitude.
    let source = "fn build(limit: i32) -> i32 effects [alloc]:\n    let data: Vec[i32] = vec_new()\n    let i: i32 = 0\n    while i < limit:\n        push(data, i)\n        i = i + 1\n    return i32(data.len)\n";
    let program = parse_human(source).unwrap();

    let started = std::time::Instant::now();
    let report = debug_entry(
        &program,
        "build",
        r#"{ "args": { "limit": 30000 } }"#,
        DebugOptions {
            max_steps: 200_000,
            ..Default::default()
        },
    )
    .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(report.status, "completed", "{:?}", report.error);
    assert_eq!(report.return_text.as_deref(), Some("30000"));
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "tracing took {elapsed:?}, which suggests the per-step environment clone returned"
    );
}

#[test]
fn a_statement_reports_the_bindings_it_writes() {
    // Write targets are the `let` name, the assignment root, and bare-variable
    // call arguments (the only shape a `&mut` copy-back can target). A read
    // through a projection is not a write, so a large buffer that is only
    // indexed must not be reported on every step.
    let source = "fn scan() -> i32 effects [alloc]:\n    let data: Vec[i32] = vec_new()\n    push(data, 7)\n    let total: i32 = 0\n    let at: usize = 0\n    while at < 1:\n        total = total + data[at]\n        at = at + 1\n    return total\n";
    let program = parse_human(source).unwrap();
    let report = debug_entry(
        &program,
        "scan",
        "{ \"args\": {} }",
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "completed", "{:?}", report.error);

    // `push(data, 7)` names data as a bare argument, so it is reported.
    let push_step = report
        .steps
        .iter()
        .find(|step| step.text.contains("push"))
        .expect("push step");
    assert!(
        push_step.changed.iter().any(|(name, _)| name == "data"),
        "{push_step:?}"
    );

    // `total = total + data[at]` writes total and only reads data.
    let read_step = report
        .steps
        .iter()
        .find(|step| step.text.contains("total = total +"))
        .expect("read step");
    assert!(read_step.changed.iter().any(|(name, _)| name == "total"));
    assert!(
        !read_step.changed.iter().any(|(name, _)| name == "data"),
        "an indexed read must not report the buffer as written: {read_step:?}"
    );
}

#[test]
fn resolved_positions_point_at_the_right_source_lines() {
    // Node ids are the tracer's vocabulary; line numbers are a projection from
    // the source map, and DAP will consume them. A step's line must be the line
    // its statement is written on, and a frame's the line of its function
    // header — including across a call, where the ids interleave.
    let source = "fn inner(a: i32, b: i32) -> i32:\n    return a / b\n\nfn outer(x: i32) -> i32:\n    let z: i32 = 0\n    return inner(x, z)\n";
    let program = parse_human(source).unwrap();
    let parsed = crate::parser::parse_file_detailed(std::path::Path::new("positions.fgx"), source);
    let mut report = debug_entry(
        &program,
        "outer",
        r#"{ "args": { "x": 10 } }"#,
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "trap");

    // Unresolved until asked: the interpreter stays free of span concerns.
    assert!(report.steps.iter().all(|step| step.line.is_none()));
    report.resolve_positions(&parsed.source_map);

    let line_of = |node: &str| {
        report
            .steps
            .iter()
            .find(|step| step.node == node)
            .and_then(|step| step.line)
    };
    assert_eq!(line_of("@function.outer.stmt.0"), Some(5));
    assert_eq!(line_of("@function.outer.stmt.1"), Some(6));
    assert_eq!(line_of("@function.inner.stmt.0"), Some(2));

    let frame_line = |name: &str| {
        report
            .frames
            .iter()
            .find(|frame| frame.function == name)
            .and_then(|frame| frame.line)
    };
    assert_eq!(frame_line("inner"), Some(1));
    assert_eq!(frame_line("outer"), Some(4));

    assert!(report.to_json().contains("\"line\": 5"));
    assert!(report.to_text().contains("fn inner (L1)"));
}

// --- Phase 4: conditional breakpoints and watch expressions -----------------

fn watched(source: &str, entry: &str, input: &str, watches: &[&str]) -> crate::interp::DebugReport {
    let program = parse_human(source).unwrap();
    let watches = watches
        .iter()
        .map(|text| {
            (
                (*text).to_string(),
                crate::parser::parse_expr(text).unwrap(),
            )
        })
        .collect();
    debug_entry(
        &program,
        entry,
        input,
        DebugOptions {
            watches,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn watch_expressions_are_evaluated_after_every_step() {
    let source = "fn run(limit: i32) -> i32:\n    let total: i32 = 0\n    let i: i32 = 0\n    while i < limit:\n        total = total + i\n        i = i + 1\n    return total\n";
    let report = watched(
        source,
        "run",
        r#"{ "args": { "limit": 3 } }"#,
        &["total * 10"],
    );
    assert_eq!(report.status, "completed");

    // Every leaf step carries the watch, and it tracks the running total.
    let values: Vec<&str> = report
        .steps
        .iter()
        .filter_map(|step| step.watches.first())
        .map(|(_, value)| value.as_str())
        .collect();
    assert!(values.contains(&"0"), "{values:?}");
    assert!(values.contains(&"10"), "{values:?}");
    assert!(values.contains(&"30"), "{values:?}");
    assert!(report.to_json().contains("\"watches\""));
    assert!(report.to_text().contains("total * 10 = 30"));
}

#[test]
fn a_watch_that_cannot_be_evaluated_records_its_error_and_does_not_abort() {
    // A watch naming something not yet in scope must not change whether the
    // program completes: a debugger that alters the run is worse than useless.
    let source = "fn run() -> i32:\n    let a: i32 = 1\n    return a\n";
    let report = watched(source, "run", "{ \"args\": {} }", &["missing_name"]);
    assert_eq!(report.status, "completed");
    assert_eq!(report.return_text.as_deref(), Some("1"));
    assert!(report.steps.iter().any(|step| step
        .watches
        .iter()
        .any(|(_, value)| value.contains("unknown_symbol"))));
}

#[test]
fn a_watch_that_calls_a_function_does_not_pollute_the_trace() {
    // Evaluating a watch re-enters the statement executor. Those statements are
    // not program steps, and tracing them would also re-enter the tracer's own
    // borrow.
    let source = "fn double(x: i32) -> i32:\n    let scaled: i32 = x * 2\n    return scaled\n\nfn run() -> i32:\n    let a: i32 = 21\n    return a\n";
    let plain = watched(source, "run", "{ \"args\": {} }", &[]);
    let with_watch = watched(source, "run", "{ \"args\": {} }", &["double(a)"]);

    assert_eq!(with_watch.status, "completed");
    assert_eq!(with_watch.return_text.as_deref(), Some("21"));
    // The watch ran — and added no steps of its own.
    assert!(with_watch
        .steps
        .iter()
        .any(|step| step.watches.iter().any(|(_, value)| value == "42")));
    assert_eq!(
        with_watch.steps.len(),
        plain.steps.len(),
        "watch evaluation must not add steps"
    );
    assert_eq!(
        with_watch.calls, plain.calls,
        "watch evaluation must not add synthetic call records"
    );
    assert!(!with_watch
        .steps
        .iter()
        .any(|step| step.node.starts_with("@function.double")));
}

#[test]
fn a_conditional_breakpoint_stops_only_when_its_expression_holds() {
    let source = "fn run(limit: i32) -> i32:\n    let i: i32 = 0\n    while i < limit:\n        i = i + 1\n    return i\n";
    let program = parse_human(source).unwrap();
    let node = "@function.run.stmt.1.body.0";

    let hit = debug_entry(
        &program,
        "run",
        r#"{ "args": { "limit": 9 } }"#,
        DebugOptions {
            break_if: Some((
                node.to_string(),
                crate::parser::parse_expr("i == 4").unwrap(),
            )),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hit.status, "breakpoint");
    let (stopped_at, locals) = hit.breakpoint.as_ref().expect("breakpoint state");
    assert_eq!(stopped_at, node);
    assert!(
        locals.contains(&("i".to_string(), "4".to_string())),
        "{locals:?}"
    );

    // A condition that never holds must let the program run to completion.
    let never = debug_entry(
        &program,
        "run",
        r#"{ "args": { "limit": 3 } }"#,
        DebugOptions {
            break_if: Some((
                node.to_string(),
                crate::parser::parse_expr("i == 99").unwrap(),
            )),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(never.status, "completed");
    assert_eq!(never.return_text.as_deref(), Some("3"));

    // A condition that cannot be evaluated must not masquerade as a hit.
    let broken = debug_entry(
        &program,
        "run",
        r#"{ "args": { "limit": 3 } }"#,
        DebugOptions {
            break_if: Some((
                node.to_string(),
                crate::parser::parse_expr("nope == 1").unwrap(),
            )),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(broken.status, "completed");
}

#[test]
fn every_nested_statement_resolves_to_its_own_line() {
    // `while` bodies, `else` bodies, and match arms previously had no spans, so
    // a step inside a loop had no line to open in an editor. The `else:` and
    // `case ...:` headers also have to be consumed, or the statement following
    // a branch is silently given the header's line.
    let source = concat!(
        "enum Mode:\n",
        "    Fast\n",
        "    Careful\n",
        "\n",
        "fn run(limit: i32) -> i32:\n",
        "    let mode: Mode = Mode::Fast\n",
        "    let total: i32 = 0\n",
        "    let i: i32 = 0\n",
        "    while i < limit:\n",
        "        if i == 1:\n",
        "            total = total + 100\n",
        "        else:\n",
        "            total = total + 1\n",
        "        i = i + 1\n",
        "    match mode:\n",
        "        case Mode::Fast:\n",
        "            total = total + 10\n",
        "        case Mode::Careful:\n",
        "            total = total + 20\n",
        "    return total\n",
    );
    let program = parse_human(source).unwrap();
    let parsed = crate::parser::parse_file_detailed(std::path::Path::new("nested.fgx"), source);
    let mut report = debug_entry(
        &program,
        "run",
        r#"{ "args": { "limit": 2 } }"#,
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "completed", "{:?}", report.error);
    report.resolve_positions(&parsed.source_map);

    // Not one step may be left without a position.
    let unresolved: Vec<&str> = report
        .steps
        .iter()
        .filter(|step| step.line.is_none())
        .map(|step| step.node.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "steps without a line: {unresolved:?}"
    );

    let line_of = |node: &str| {
        report
            .steps
            .iter()
            .find(|step| step.node == node)
            .and_then(|step| step.line)
    };
    assert_eq!(line_of("@function.run.stmt.3"), Some(9)); // while
    assert_eq!(line_of("@function.run.stmt.3.body.0"), Some(10)); // if
    assert_eq!(line_of("@function.run.stmt.3.body.0.then.0"), Some(11));
    assert_eq!(line_of("@function.run.stmt.3.body.0.else.0"), Some(13));
    // The statement after the branch must not inherit the `else:` line.
    assert_eq!(line_of("@function.run.stmt.3.body.1"), Some(14));
    assert_eq!(line_of("@function.run.stmt.4"), Some(15)); // match
    assert_eq!(line_of("@function.run.stmt.4.arm.0.0"), Some(17));
    assert_eq!(line_of("@function.run.stmt.5"), Some(20)); // return
}

// --- Phase 4: suspendable interactive sessions ------------------------------

const STEPPING: &str = "fn helper(x: i32) -> i32:\n    let doubled: i32 = x * 2\n    return doubled\n\nfn run() -> i32:\n    let a: i32 = 21\n    let b: i32 = helper(a)\n    return b\n";

/// Drive a session with a fixed command script, collecting the nodes it
/// stopped at. The script is consumed in order; once exhausted the session
/// continues to completion.
fn drive(
    source: &str,
    entry: &str,
    script: &[crate::interp::DebugCommand],
) -> (Vec<String>, String) {
    use crate::interp::{DebugCommand, DebugEvent};
    let program = parse_human(source).unwrap();
    let mut stops = Vec::new();
    let mut next = 0usize;
    let report = crate::interp::debug_session(
        &program,
        entry,
        "{ \"args\": {} }",
        DebugOptions::default(),
        |event| {
            if let DebugEvent::Stopped(stop) = event {
                stops.push(stop.node.clone());
            }
            let command = script.get(next).cloned().unwrap_or(DebugCommand::Continue);
            next += 1;
            command
        },
    )
    .unwrap();
    (stops, report.status)
}

#[test]
fn a_session_starts_paused_and_steps_into_calls() {
    use crate::interp::DebugCommand::StepIn;
    // StepIn descends into `helper`, so the callee's statements appear.
    let (stops, status) = drive(STEPPING, "run", &[StepIn, StepIn, StepIn, StepIn]);
    assert_eq!(status, "completed");
    assert_eq!(stops[0], "@function.run.stmt.0");
    assert_eq!(stops[1], "@function.run.stmt.1");
    assert!(
        stops
            .iter()
            .any(|node| node.starts_with("@function.helper")),
        "step-in must enter the callee: {stops:?}"
    );
}

#[test]
fn next_steps_over_a_call_instead_of_into_it() {
    use crate::interp::DebugCommand::Next;
    // `let b = helper(a)` calls helper; `next` must run it to completion and
    // stop at the following statement in the same frame.
    let (stops, status) = drive(STEPPING, "run", &[Next, Next, Next]);
    assert_eq!(status, "completed");
    assert!(
        !stops
            .iter()
            .any(|node| node.starts_with("@function.helper")),
        "next must not descend into the callee: {stops:?}"
    );
    assert_eq!(
        stops,
        vec![
            "@function.run.stmt.0",
            "@function.run.stmt.1",
            "@function.run.stmt.2",
        ]
    );
}

#[test]
fn next_reports_the_caller_after_the_callee_returns() {
    use crate::interp::{DebugCommand, DebugEvent};
    let program = parse_human(STEPPING).unwrap();
    let mut stops = Vec::new();
    let report = crate::interp::debug_session(
        &program,
        "run",
        "{ \"args\": {} }",
        DebugOptions::default(),
        |event| {
            if let DebugEvent::Stopped(stop) = event {
                stops.push((stop.node.clone(), stop.function.clone()));
            }
            DebugCommand::Next
        },
    )
    .unwrap();

    assert_eq!(report.status, "completed");
    assert_eq!(
        stops.last(),
        Some(&("@function.run.stmt.2".to_string(), "run".to_string()))
    );
}

#[test]
fn session_breakpoints_pause_and_then_resume_the_run() {
    use crate::interp::{DebugCommand, DebugEvent};
    let program = parse_human(STEPPING).unwrap();
    let target = "@function.run.stmt.1";
    let mut stops = Vec::new();
    let report = crate::interp::debug_session(
        &program,
        "run",
        "{ \"args\": {} }",
        DebugOptions {
            break_at: Some(target.to_string()),
            ..Default::default()
        },
        |event| {
            if let DebugEvent::Stopped(stop) = event {
                stops.push(stop.node.clone());
            }
            DebugCommand::Continue
        },
    )
    .unwrap();

    assert_eq!(stops, vec!["@function.run.stmt.0", target]);
    assert_eq!(report.status, "completed");
    assert_eq!(report.return_text.as_deref(), Some("42"));
}

#[test]
fn a_conditional_breakpoint_suspends_an_interactive_session() {
    use crate::interp::{DebugCommand, DebugEvent};
    let source =
        "fn run() -> i32:\n    let i: i32 = 0\n    while i < 4:\n        i = i + 1\n    return i\n";
    let program = parse_human(source).unwrap();
    let target = "@function.run.stmt.1.body.0";
    let mut hits = Vec::new();
    let report = crate::interp::debug_session(
        &program,
        "run",
        "{ \"args\": {} }",
        DebugOptions {
            break_if: Some((
                target.to_string(),
                crate::parser::parse_expr("i == 2").unwrap(),
            )),
            ..Default::default()
        },
        |event| {
            if let DebugEvent::Stopped(stop) = event {
                if stop.node == target {
                    hits.push(stop.locals.clone());
                }
            }
            DebugCommand::Continue
        },
    )
    .unwrap();

    assert_eq!(report.status, "completed");
    assert_eq!(report.return_text.as_deref(), Some("4"));
    assert_eq!(hits.len(), 1);
    assert!(hits[0].contains(&("i".to_string(), "2".to_string())));
}

#[test]
fn step_out_runs_until_the_frame_returns() {
    use crate::interp::DebugCommand::{StepIn, StepOut};
    // Step into helper, then step out: the next stop must be back in `run`.
    let (stops, status) = drive(STEPPING, "run", &[StepIn, StepIn, StepIn, StepOut]);
    assert_eq!(status, "completed");
    // The script is consumed one command per stop, so `StepOut` (index 3) is
    // issued at stops[3]; the very next stop must already be back in `run`.
    assert!(
        stops[3].starts_with("@function.helper"),
        "expected to be inside helper when stepping out: {stops:?}"
    );
    assert!(
        !stops[4].starts_with("@function.helper"),
        "step-out must resume in the caller: {stops:?}"
    );
    assert_eq!(stops[4], "@function.run.stmt.2");
}

#[test]
fn a_paused_session_evaluates_expressions_in_its_own_frame() {
    use crate::interp::{DebugCommand, DebugEvent};
    let program = parse_human(STEPPING).unwrap();
    let mut answers = Vec::new();
    let mut asked = false;
    let report = crate::interp::debug_session(
        &program,
        "run",
        "{ \"args\": {} }",
        DebugOptions::default(),
        |event| match event {
            // Stop once `a` is bound, then interrogate the frame.
            DebugEvent::Stopped(stop) if stop.node == "@function.run.stmt.1" && !asked => {
                asked = true;
                DebugCommand::Evaluate("helper(a)".to_string())
            }
            DebugEvent::Evaluated { source, value } => {
                answers.push((source.clone(), value.clone()));
                DebugCommand::Continue
            }
            _ => DebugCommand::StepIn,
        },
    )
    .unwrap();

    assert_eq!(report.status, "completed");
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0, "helper(a)");
    assert_eq!(answers[0].1.as_deref(), Ok("42"));
    assert_eq!(
        report
            .calls
            .iter()
            .filter(|call| call.event == "enter" && call.function == "helper")
            .count(),
        1,
        "the paused evaluation must not add a synthetic helper call"
    );
}

#[test]
fn quitting_a_session_cancels_the_run() {
    use crate::interp::DebugCommand;
    let program = parse_human(STEPPING).unwrap();
    let report = crate::interp::debug_session(
        &program,
        "run",
        "{ \"args\": {} }",
        DebugOptions::default(),
        |_| DebugCommand::Quit,
    )
    .unwrap();
    assert_eq!(report.status, "cancelled");
    // It stopped before finishing, so the return value was never produced.
    assert!(report.return_text.is_none());
}

#[test]
fn a_session_reports_where_it_is_paused() {
    use crate::interp::{DebugCommand, DebugEvent};
    let program = parse_human(STEPPING).unwrap();
    let mut first: Option<crate::interp::DebugStop> = None;
    crate::interp::debug_session(
        &program,
        "run",
        "{ \"args\": {} }",
        DebugOptions::default(),
        |event| {
            if let DebugEvent::Stopped(stop) = event {
                if first.is_none() {
                    first = Some(stop.clone());
                }
            }
            DebugCommand::Continue
        },
    )
    .unwrap();
    let stop = first.expect("a session starts paused");
    assert_eq!(stop.node, "@function.run.stmt.0");
    assert_eq!(stop.function, "run");
    assert_eq!(stop.text, "let a = 21");
    assert_eq!(stop.depth, 1);
}

#[test]
fn tail_keeps_the_end_of_a_long_run_and_says_what_it_dropped() {
    // `max_steps` bounds a run by keeping the *first* N steps, which is the
    // wrong end when a failure is at the finish. `tail` keeps the last N, and
    // must say so: a tail read as a complete trace makes its first line look
    // like the start of the run.
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 40 } }",
        DebugOptions {
            tail: Some(3),
            max_steps: 10_000,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(report.status, "completed");
    assert_eq!(report.return_text.as_deref(), Some("820"));
    assert_eq!(report.steps.len(), 3, "only the window is retained");
    assert!(report.dropped_steps > 0);

    // Sequence numbers keep their true position in the run, so the retained
    // window can be located within it. `seq` is a global event ordinal shared
    // with the call tree's enter/exit records, not a step index, so it is at
    // least the number of steps executed.
    let last = report.steps.last().expect("a retained step");
    assert_eq!(last.node, "@function.count.stmt.2");
    assert!(
        last.seq >= report.dropped_steps + report.steps.len(),
        "seq {} should place the window at the end of the run",
        last.seq
    );
    let seqs: Vec<usize> = report.steps.iter().map(|step| step.seq).collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "{seqs:?}");

    let text = report.to_text();
    assert!(
        text.contains("showing the last 3 steps"),
        "the trace must declare that it is a tail: {text}"
    );
    assert!(report.to_json().contains("\"dropped_steps\":"));
}

#[test]
fn without_tail_the_whole_trace_is_kept() {
    // The default must be unchanged: a bounded run reports nothing dropped.
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 3 } }",
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.dropped_steps, 0);
    assert_eq!(report.steps.len(), 9);
    assert!(!report.to_text().contains("dropped"));
}

#[test]
fn the_step_cap_counts_executed_steps_not_retained_ones() {
    // With a sliding window the cap must still bound the run; counting only
    // retained steps would let a runaway loop spin forever.
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 100000 } }",
        DebugOptions {
            tail: Some(5),
            max_steps: 200,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.status, "step_limit");
    assert_eq!(report.steps.len(), 5);
    assert_eq!(report.steps.len() + report.dropped_steps, 200);
}

// --- Phase 7: concurrency is visible in a trace -----------------------------

const TASKS: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn parked_then_woken() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn_parked(double, 21)\n    let woke: bool = task_wake(task)\n    if !woke:\n        return -1\n    return task_await(task)\n";

#[test]
fn a_traced_run_carries_its_concurrency_transitions() {
    // A concurrent program's statements do not show why it is waiting: a park
    // or a wake is a transition, not a statement. Without these the trace shows
    // a task being created and later finishing, with nothing in between.
    let program = parse_human(TASKS).unwrap();
    let report = debug_entry(
        &program,
        "parked_then_woken",
        "{ \"args\": {} }",
        DebugOptions::default(),
    )
    .unwrap();
    assert_eq!(report.status, "completed", "{:?}", report.error);
    assert_eq!(report.return_text.as_deref(), Some("42"));

    let seen: Vec<(&str, &str)> = report
        .concurrency
        .iter()
        .map(|event| (event.operation.as_str(), event.decision.as_str()))
        .collect();
    assert!(seen.contains(&("task.spawn_parked", "parked")), "{seen:?}");
    assert!(seen.contains(&("task.wake", "woken")), "{seen:?}");
    assert!(seen.contains(&("task.await", "completed")), "{seen:?}");

    // Transitions are ordered, so a causality chain can be read off them.
    let sequences: Vec<usize> = report
        .concurrency
        .iter()
        .map(|event| event.sequence)
        .collect();
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "{sequences:?}"
    );
}

#[test]
fn concurrency_is_summarised_in_text_and_itemised_in_json() {
    let program = parse_human(TASKS).unwrap();
    let report = debug_entry(
        &program,
        "parked_then_woken",
        "{ \"args\": {} }",
        DebugOptions::default(),
    )
    .unwrap();

    // Text groups by operation and decision: the shape of a stall should read
    // at a glance rather than as a long repetitive list.
    let text = report.to_text();
    assert!(text.contains("concurrency: 3 transitions"), "{text}");
    assert!(text.contains("task.spawn_parked parked x1"), "{text}");

    // JSON keeps every transition, because an agent reads the sequence.
    let json = report.to_json();
    assert!(json.contains("\"concurrency\": ["), "{json}");
    assert!(json.contains("\"operation\": \"task.wake\""), "{json}");
    assert!(json.contains("\"decision\": \"woken\""), "{json}");
}

#[test]
fn a_run_without_concurrency_reports_none() {
    // The section must not appear for an ordinary program.
    let program = parse_human(COUNT).unwrap();
    let report = debug_entry(
        &program,
        "count",
        "{ \"args\": { \"n\": 2 } }",
        DebugOptions::default(),
    )
    .unwrap();
    assert!(report.concurrency.is_empty());
    assert!(!report.to_text().contains("concurrency:"));
    assert!(report.to_json().contains("\"concurrency\": []"));
}
