use std::fs;

use crate::c_backend::build_c_project;
use crate::cfg::{back_edges, build_cfg};
use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::{parse_agent, parse_human};

const CLASSIFY: &str = "fn classify(x: i32) -> i32:\n    if x > 10:\n        return 2\n    else if x > 0:\n        return 1\n    else:\n        return 0\n";

const COUNT_DOWN: &str = "fn count_down(n: i32) -> i32:\n    let total: i32 = 0\n    while n > 0:\n        total = total + n\n        n = n - 1\n    return total\n";

#[test]
fn if_else_roundtrips_both_syntaxes() {
    let program = parse_human(CLASSIFY).unwrap();
    // .fgx -> AST -> .fgai -> AST is lossless.
    let from_agent = parse_agent(&format_agent(&program)).unwrap();
    assert!(program.canonical_eq(&from_agent));
    // Human formatting is idempotent at the AST level.
    let reparsed = parse_human(&format_human(&program)).unwrap();
    assert!(program.canonical_eq(&reparsed));
}

#[test]
fn agent_uses_dense_else_keywords() {
    let agent = format_agent(&parse_human(CLASSIFY).unwrap());
    assert!(agent.contains("EI x > 0 {"), "agent:\n{agent}");
    assert!(agent.contains("EL {"), "agent:\n{agent}");
    assert!(
        !agent.contains("else"),
        "agent must not use the human keyword"
    );
}

#[test]
fn interpreter_runs_each_branch() {
    let program = parse_human(CLASSIFY).unwrap();
    for (x, expected) in [(15, 2), (5, 1), (-3, 0)] {
        let input =
            format!("{{ \"args\": {{ \"x\": {x} }}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, "classify", &input).unwrap();
        assert!(output.contains("\"status\": \"pass\""), "x={x}:\n{output}");
    }
}

#[test]
fn cfg_models_the_else_branch_for_soundness() {
    // The borrow checker runs over the CFG, so the else branch must be a real
    // block in the graph — otherwise it would go unanalyzed (unsound).
    let with_else = build_cfg(
        parse_human(CLASSIFY)
            .unwrap()
            .find_function("classify")
            .unwrap(),
    );
    let guard_only = build_cfg(
        parse_human("fn g(x: i32) -> i32:\n    if x > 0:\n        return 1\n    return 0\n")
            .unwrap()
            .find_function("g")
            .unwrap(),
    );
    assert!(
        with_else.blocks.len() > guard_only.blocks.len(),
        "an else branch must add a block to the CFG"
    );
}

#[test]
fn c_backend_emits_else() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_else_c", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"elsedemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m.fgx"),
        "export fn classify(x: i32) -> i32:\n    if x > 0:\n        return 1\n    else:\n        return 0\n",
    )
    .unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let source = fs::read_to_string(&artifacts.source).unwrap();
    assert!(source.contains("} else {"), "expected else in C:\n{source}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn else_without_matching_if_is_rejected() {
    let err = parse_human("fn f() -> i32:\n    else:\n        return 0\n").unwrap_err();
    assert!(err.contains("no matching 'if'"), "got: {err}");
}

#[test]
fn while_roundtrips_and_uses_dense_keyword() {
    let program = parse_human(COUNT_DOWN).unwrap();
    let agent = format_agent(&program);
    assert!(agent.contains("WH n > 0 {"), "agent:\n{agent}");
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn while_loop_executes_in_the_interpreter() {
    let program = parse_human(COUNT_DOWN).unwrap();
    // 5 + 4 + 3 + 2 + 1 = 15
    let output = run_entry_json(
        &program,
        "count_down",
        "{ \"args\": { \"n\": 5 }, \"expect\": { \"return\": 15 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn while_is_a_loop_in_the_cfg_for_soundness() {
    // A while must lower to a loop with a back edge so the borrow checker's
    // loop-carried liveness applies (sound).
    let cfg = build_cfg(
        parse_human(COUNT_DOWN)
            .unwrap()
            .find_function("count_down")
            .unwrap(),
    );
    assert!(
        !back_edges(&cfg).is_empty(),
        "a while loop must produce a CFG back edge"
    );
}

#[test]
fn while_condition_must_be_bool() {
    let diagnostics = crate::typecheck::check(
        &parse_human("fn f(n: i32) -> i32:\n    while n:\n        return 1\n    return 0\n")
            .unwrap(),
    );
    assert!(
        diagnostics.iter().any(|d| d.code == "while_not_bool"),
        "expected while_not_bool, got: {diagnostics:?}"
    );
}

const FIRST_GE: &str = "fn first_ge(limit: i32, target: i32) -> i32:\n    let i: i32 = 0\n    let found: i32 = -1\n    while i < limit:\n        if i >= target:\n            found = i\n            break\n        i = i + 1\n    return found\n";

const COUNT_BELOW: &str = "fn count_below(limit: i32, threshold: i32) -> i32:\n    let i: i32 = 0\n    let count: i32 = 0\n    while i < limit:\n        i = i + 1\n        if i > threshold:\n            continue\n        count = count + 1\n    return count\n";

#[test]
fn break_continue_roundtrip_with_dense_keywords() {
    let program = parse_human(FIRST_GE).unwrap();
    let agent = format_agent(&program);
    assert!(agent.contains("BR\n"), "agent:\n{agent}");
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let cont = format_agent(&parse_human(COUNT_BELOW).unwrap());
    assert!(cont.contains("CN\n"), "agent:\n{cont}");
}

#[test]
fn break_terminates_the_loop() {
    let output = run_entry_json(
        &parse_human(FIRST_GE).unwrap(),
        "first_ge",
        "{ \"args\": { \"limit\": 100, \"target\": 7 }, \"expect\": { \"return\": 7 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn continue_skips_the_rest_of_the_iteration() {
    // i runs 1..=10; count++ only when i <= 4, continue skips the rest.
    let output = run_entry_json(
        &parse_human(COUNT_BELOW).unwrap(),
        "count_below",
        "{ \"args\": { \"limit\": 10, \"threshold\": 4 }, \"expect\": { \"return\": 4 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn break_outside_a_loop_is_rejected() {
    let diagnostics =
        crate::typecheck::check(&parse_human("fn f() -> i32:\n    break\n    return 0\n").unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "break_outside_loop"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn c_backend_emits_break() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_break_c", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"breakdemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/m.fgx"), FIRST_GE.replace("fn ", "export fn ")).unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let source = fs::read_to_string(&artifacts.source).unwrap();
    assert!(source.contains("break;"), "expected break in C:\n{source}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn c_backend_emits_while() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_while_c", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"whiledemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m.fgx"),
        COUNT_DOWN.replace("fn ", "export fn "),
    )
    .unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let source = fs::read_to_string(&artifacts.source).unwrap();
    assert!(source.contains("while ("), "expected while in C:\n{source}");
    let _ = fs::remove_dir_all(&dir);
}
