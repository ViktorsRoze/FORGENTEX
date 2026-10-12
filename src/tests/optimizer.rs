// v3.2 slice 1 — the dependency-free optimizer (docs/v32_optimizer_plan.md).
// The two properties under test: (1) folding/elimination actually happens,
// (2) trap semantics are NEVER optimized away — an overflowing or div-by-zero
// expression still traps after optimization, and results are bit-identical
// pre/post (differential equivalence).

use crate::ast::{Expr, Stmt};
use crate::interp::run_entry_json;
use crate::optimize::optimize_program;
use crate::parser::parse_human;

#[test]
fn const_folding_folds_literal_arithmetic() {
    let mut program = parse_human("fn f() -> i32:\n    return 2 + 3 * 4\n").unwrap();
    let report = optimize_program(&mut program);
    assert!(report.folded_exprs >= 2, "report: {report:?}");
    let function = program.find_function("f").unwrap();
    assert_eq!(
        function.body,
        vec![Stmt::Return(Some(Expr::Int(14)))],
        "body folded to the literal"
    );
    // Behavior unchanged.
    let output = run_entry_json(
        &program,
        "f",
        "{ \"args\": {}, \"expect\": { \"return\": 14 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn overflow_is_never_folded_away() {
    let src = "fn f() -> i64:\n    return 9223372036854775807 + 1\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert_eq!(report.folded_exprs, 0, "an overflowing fold must be left");
    // The runtime trap still fires.
    let err = run_entry_json(&program, "f", "{ \"args\": {} }").unwrap_err();
    assert!(err.contains("integer_overflow"), "got: {err}");
}

#[test]
fn division_by_zero_is_never_folded_away() {
    let mut program = parse_human("fn f() -> i32:\n    return 1 / 0\n").unwrap();
    let report = optimize_program(&mut program);
    assert_eq!(report.folded_exprs, 0);
    let err = run_entry_json(&program, "f", "{ \"args\": {} }").unwrap_err();
    assert!(err.contains("division_by_zero"), "got: {err}");
}

#[test]
fn literal_branches_are_spliced_and_dead_code_dropped() {
    // `1 < 2` folds to true, the if splices to its then-branch, and the
    // unreachable second return is dropped.
    let src = "fn f() -> i32:\n    if 1 < 2:\n        return 10\n    else:\n        return 20\n    return 30\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert!(report.eliminated_branches >= 1, "report: {report:?}");
    assert!(report.eliminated_stmts >= 1, "report: {report:?}");
    let function = program.find_function("f").unwrap();
    assert_eq!(function.body, vec![Stmt::Return(Some(Expr::Int(10)))]);
    let output = run_entry_json(
        &program,
        "f",
        "{ \"args\": {}, \"expect\": { \"return\": 10 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn while_false_is_dropped() {
    let src =
        "fn f() -> i32:\n    let n: i32 = 1\n    while 2 < 1:\n        n = n + 1\n    return n\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert!(report.eliminated_branches >= 1, "report: {report:?}");
    let function = program.find_function("f").unwrap();
    assert_eq!(function.body.len(), 2, "let + return remain");
}

#[test]
fn differential_equivalence_pre_and_post_optimize() {
    // A program exercising loops, branches, match, Vec, and strings: the
    // optimized program must produce byte-identical results.
    let src = "enum Mode:\n    Fast\n    Slow\n\nfn pick(m: Mode) -> i32:\n    let out: i32 = 0\n    match m:\n        case Mode::Fast:\n            out = 2 + 3\n        case Mode::Slow:\n            out = 10 - 4\n    return out\n\nfn work(n: i32) -> i32 effects [alloc]:\n    let v: Vec[i32] = vec_new()\n    let i: i32 = 0\n    while i < n:\n        if i % 2 == 0:\n            push(v, i * 2)\n        else:\n            push(v, i)\n        i = i + 1\n    return v[0] + v[n - 1] + pick(Mode::Fast) + pick(Mode::Slow)\n";
    let baseline = parse_human(src).unwrap();
    let mut optimized = parse_human(src).unwrap();
    let report = optimize_program(&mut optimized);
    assert!(
        report.total() > 0,
        "the optimizer did something: {report:?}"
    );
    for input in [
        "{ \"args\": { \"n\": 3 } }",
        "{ \"args\": { \"n\": 6 } }",
        "{ \"args\": { \"n\": 9 } }",
    ] {
        let before = run_entry_json(&baseline, "work", input).unwrap();
        let after = run_entry_json(&optimized, "work", input).unwrap();
        assert_eq!(before, after, "identical results for input {input}");
    }
}

#[test]
fn report_renders_json_schema_and_text() {
    let mut program = parse_human("fn f() -> i32:\n    return 1 + 1\n").unwrap();
    let report = optimize_program(&mut program);
    let json = report.to_json();
    assert!(
        json.contains("\"schema\": \"forgentex.optimize.v1\""),
        "{json}"
    );
    assert!(json.contains("\"folded_exprs\": 1"), "{json}");
    assert!(
        report.to_text().contains("const_fold folded 1"),
        "text form"
    );
}

// --- v3.2 slice 2: build-pipeline integration ---

#[test]
fn optimized_build_folds_into_the_emitted_c() {
    // The optimizer runs after project load and BEFORE FIR lowering, so the
    // emitted C reflects the folded program: 6 * 7 arrives as 42 and the dead
    // branch is gone.
    let dir = std::env::temp_dir().join(format!("forgentex_{}_optbuild", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"optdemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "export fn pick(n: i32) -> i32:\n    if 1 < 2:\n        return 6 * 7 + n\n    else:\n        return 0\n",
    )
    .unwrap();

    // Baseline (unoptimized) emission still carries the source shape.
    let (_, baseline) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    // Unfolded, and checked at the declared width: C's own `*` leaves signed
    // overflow undefined, so integer arithmetic is lowered through a helper.
    assert!(
        baseline.contains("fgx_checked_i32_mul(6, 7)"),
        "baseline keeps 6 * 7:\n{baseline}"
    );

    let (artifacts, _ir, report) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out")).unwrap();
    assert!(report.total() > 0, "report: {report:?}");
    let optimized = std::fs::read_to_string(&artifacts.source).unwrap();
    assert!(
        optimized.contains("42"),
        "folded constant in C:\n{optimized}"
    );
    assert!(
        !optimized.contains("fgx_checked_i32_mul(6, 7)"),
        "no unfolded arithmetic:\n{optimized}"
    );
    assert!(
        !optimized.contains("return 0;"),
        "dead branch eliminated:\n{optimized}"
    );
    assert!(
        optimized.len() < baseline.len(),
        "optimized C is smaller: {} < {}",
        optimized.len(),
        baseline.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn optimized_build_writes_fir_receipts_from_the_optimized_ir() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_optmeta", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"optmeta\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "export fn f() -> i32:\n    return 2 + 3\n",
    )
    .unwrap();
    let (artifacts, report) =
        crate::backend::build_project_with_backend_opt(&dir, "c", &dir.join("out")).unwrap();
    assert_eq!(report.folded_exprs, 1);
    assert!(!artifacts.metadata_files.is_empty(), "receipts written");
    // The FIR receipt derives from the optimized program: the folded literal
    // appears, the unfolded addition does not.
    let fir = artifacts
        .metadata_files
        .iter()
        .find(|path| path.to_string_lossy().contains("fgir"))
        .expect("fgir receipt");
    let fir_json = std::fs::read_to_string(fir).unwrap();
    assert!(fir_json.contains("5"), "folded literal in FIR receipt");
    assert!(
        !fir_json.contains("2 + 3"),
        "unfolded addition absent from FIR receipt"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- v3.2 slice 3: function inlining ---

#[test]
fn inline_then_fold_collapses_to_a_literal() {
    // double(21) inlines to 21 * 2, then folds to 42 — the passes compound.
    let src =
        "fn double(x: i32) -> i32:\n    return x * 2\n\nfn k() -> i32:\n    return double(21)\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert!(report.inlined_calls >= 1, "report: {report:?}");
    let k = program.find_function("k").unwrap();
    assert_eq!(k.body, vec![Stmt::Return(Some(Expr::Int(42)))]);
}

#[test]
fn nested_and_chained_inlining_across_rounds() {
    // quad(x) = double(double(x)); triple(x) = double(x) + x. triple becomes
    // call-free in round one, so ITS callers inline in a later round.
    let src = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn triple(x: i32) -> i32:\n    return double(x) + x\n\nfn m() -> i32:\n    return triple(5)\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert!(report.inlined_calls >= 2, "report: {report:?}");
    let m = program.find_function("m").unwrap();
    assert_eq!(m.body, vec![Stmt::Return(Some(Expr::Int(15)))]);
}

#[test]
fn recursive_functions_are_never_inlined() {
    // fact's body contains a call, so it is not a candidate — and its own
    // self-call stays intact. Behavior unchanged.
    let src = "fn fact(n: i32) -> i32:\n    if n <= 1:\n        return 1\n    return n * fact(n - 1)\n\nfn f() -> i32:\n    return fact(5)\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert_eq!(report.inlined_calls, 0, "report: {report:?}");
    let output = run_entry_json(
        &program,
        "f",
        "{ \"args\": {}, \"expect\": { \"return\": 120 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn shadowing_local_blocks_inlining() {
    // Inside f, `double` is a local Fn value bound to triple — the runtime
    // dispatches through the local, so the optimizer must NOT inline the
    // program-level double. Result must stay 9 (triple), not 6.
    let src = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn triple(x: i32) -> i32:\n    return x * 3\n\nfn f() -> i32:\n    let double: Fn[i32, i32] = triple\n    return double(3)\n";
    let baseline = parse_human(src).unwrap();
    let expect = "{ \"args\": {}, \"expect\": { \"return\": 9 } }";
    let before = run_entry_json(&baseline, "f", expect).unwrap();
    assert!(before.contains("\"status\": \"pass\""), "{before}");

    let mut optimized = parse_human(src).unwrap();
    optimize_program(&mut optimized);
    let after = run_entry_json(&optimized, "f", expect).unwrap();
    assert!(
        after.contains("\"status\": \"pass\""),
        "shadowed call preserved: {after}"
    );
}

#[test]
fn impure_arguments_block_inlining() {
    // The argument contains a call (an effect/work site): substituting it
    // would move or duplicate the effect, so the call is left alone.
    let src = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn f(s: str) -> i32:\n    return double(i32(str_len(s)))\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert_eq!(report.inlined_calls, 0, "report: {report:?}");
    let output = run_entry_json(
        &program,
        "f",
        "{ \"args\": { \"s\": \"abc\" }, \"expect\": { \"return\": 6 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn compound_args_do_not_duplicate_into_multi_use_params() {
    // sq uses x twice: an atom argument substitutes fine, but a compound
    // argument would evaluate twice — blocked.
    let src = "fn sq(x: i32) -> i32:\n    return x * x\n\nfn atom(y: i32) -> i32:\n    return sq(y)\n\nfn compound(a: i32, b: i32) -> i32:\n    return sq(a + b)\n";
    let mut program = parse_human(src).unwrap();
    let report = optimize_program(&mut program);
    assert_eq!(
        report.inlined_calls, 1,
        "only the atom call inlines: {report:?}"
    );
    // Both behave identically to the original.
    for (entry, input, expected) in [
        ("atom", "{ \"args\": { \"y\": 7 } }", "49"),
        ("compound", "{ \"args\": { \"a\": 3, \"b\": 4 } }", "49"),
    ] {
        let output = run_entry_json(&program, entry, input).unwrap();
        assert!(
            output.contains(&format!("\"return\": {expected}")),
            "{entry}: {output}"
        );
    }
}
