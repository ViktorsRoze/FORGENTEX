// lang.4 slice 1 — function values and indirect calls. A top-level `fn` used
// as a value (`Fn[P…, R]`), bound to locals, passed to higher-order functions,
// and called indirectly — with static arity/argument checking and locals-shadow
// dispatch identical between the type checker and the interpreter.
// Design record: docs/lang4_plan.md.

use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::{parse_agent, parse_human};

const FN_PROG: &str = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn triple(x: i32) -> i32:\n    return x * 3\n\nfn apply(f: Fn[i32, i32], x: i32) -> i32:\n    return f(x)\n\nfn direct() -> i32:\n    let f: Fn[i32, i32] = double\n    return f(3)\n\nfn higher_order() -> i32:\n    return apply(double, 5) + apply(triple, 5)\n";

fn expect_return(entry: &str, expected: i64) {
    let program = parse_human(FN_PROG).unwrap();
    let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
    let output = run_entry_json(&program, entry, &input).unwrap();
    assert!(
        output.contains("\"status\": \"pass\""),
        "{entry}:\n{output}"
    );
}

#[test]
fn function_value_binds_and_calls() {
    // let f = double; f(3) -> 6.
    expect_return("direct", 6);
}

#[test]
fn higher_order_dispatch_varies_by_value() {
    // apply(double, 5) + apply(triple, 5) = 10 + 15 = 25 — the same call site
    // dispatches to different functions depending on the value passed.
    expect_return("higher_order", 25);
}

#[test]
fn fn_types_roundtrip_both_syntaxes() {
    // `Fn[i32, i32]` params survive .fgx -> AST -> .fgai -> AST losslessly.
    let program = parse_human(FN_PROG).unwrap();
    assert!(program.canonical_eq(&parse_agent(&format_agent(&program)).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn fn_value_program_type_checks_clean() {
    // Bindings check against the function's signature; indirect calls check
    // arity and argument types and yield the Fn's return type.
    let diagnostics = crate::typecheck::check(&parse_human(FN_PROG).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn native_backends_lower_named_function_values_and_indirect_calls() {
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_native_fn_values", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"nativefn\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn twice(x: i32) -> i32:\n    return x * 2\n\nfn thrice(x: i32) -> i32:\n    return x * 3\n\nfn apply(f: Fn[i32, i32], x: i32) -> i32:\n    return f(x)\n\nexport fn choose(use_twice: bool, x: i32) -> i32:\n    let selected: Fn[i32, i32] = thrice\n    if use_twice:\n        selected = twice\n    return apply(selected, x)\n",
    )
    .unwrap();

    let (header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(
        header.contains("typedef int32_t (*fgx_fn_i32_i32)(int32_t);"),
        "{header}"
    );
    assert!(
        c_source.contains("static int32_t apply(fgx_fn_i32_i32 f, int32_t x);"),
        "{c_source}"
    );
    assert!(
        c_source.contains("fgx_fn_i32_i32 selected = thrice;")
            && c_source.contains("return f(x);")
            && c_source.contains("selected = twice;"),
        "{c_source}"
    );
    assert!(
        zig_source.contains("fn apply(f: *const fn (i32) i32, x: i32) i32"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("var selected: *const fn (i32) i32 = thrice;")
            && zig_source.contains("return f(x);")
            && zig_source.contains("selected = twice;"),
        "{zig_source}"
    );

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let _zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("fn_value_harness.c");
        std::fs::write(
            &harness,
            "#include \"nativefn.h\"\nint main(void) { return choose(true, 7) == 14 && choose(false, 7) == 21 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "fn_value_harness.exe"
        } else {
            "fn_value_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated function-value C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        assert!(
            std::process::Command::new(&executable)
                .status()
                .expect("failed to execute function-value C harness")
                .success(),
            "generated function-value C behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("fn_value_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/nativefn.zig\");\nconst std = @import(\"std\");\ntest \"named function values\" {\n    try std.testing.expectEqual(@as(i32, 14), lib.choose(true, 7));\n    try std.testing.expectEqual(@as(i32, 21), lib.choose(false, 7));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig function-value harness");
        assert!(
            run.status.success(),
            "generated function-value Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signature_mismatch_is_rejected_at_the_binding() {
    // stringify has signature Fn[i32, str], not Fn[i32, i32].
    let src = "fn stringify(x: i32) -> str:\n    return int_to_str(x)\n\nfn f() -> i32:\n    let g: Fn[i32, i32] = stringify\n    return g(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "type_mismatch"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn wrong_argument_through_fn_value_is_rejected() {
    let src = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn f() -> i32:\n    let g: Fn[i32, i32] = double\n    return g(\"oops\")\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "fn_value_arg_type"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn calling_a_non_function_local_is_rejected_statically_and_at_runtime() {
    let src = "fn f() -> i32:\n    let g: i32 = 3\n    return g(1)\n";
    let program = parse_human(src).unwrap();
    // Static: not_callable diagnostic.
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.iter().any(|d| d.code == "not_callable"),
        "got: {diagnostics:?}"
    );
    // Runtime (unchecked execution): a trap, not a miscall.
    let err = run_entry_json(&program, "f", "{ \"args\": {} }").unwrap_err();
    assert!(err.contains("not_callable"), "got: {err}");
}

// --- lang.4 slice 4: effect-annotated function values ---

const EFFECT_FN_PROG: &str = "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn apply_read(f: Fn[i32, i32, effects[io.read]], x: i32) -> i32 effects [io.read]:\n    return f(x)\n\nfn via_local() -> i32 effects [io.read]:\n    let f: Fn[i32, i32, effects[io.read]] = read_value\n    return f(3)\n\nfn via_higher_order() -> i32 effects [io.read]:\n    return apply_read(read_value, 4)\n";

#[test]
fn effect_annotated_fn_types_roundtrip_and_execute() {
    let program = parse_human(EFFECT_FN_PROG).unwrap();
    assert!(program.canonical_eq(&parse_agent(&format_agent(&program)).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
    for (entry, expected) in [("via_local", 3), ("via_higher_order", 4)] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{output}");
    }
}

#[test]
fn indirect_calls_propagate_annotated_effects() {
    let src = "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing() -> i32:\n    let f: Fn[i32, i32, effects[io.read]] = read_value\n    return f(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
        }),
        "got: {diagnostics:?}"
    );
}

#[test]
fn pure_fn_contract_rejects_an_effectful_function() {
    let src = "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn bad() -> i32:\n    let f: Fn[i32, i32] = read_value\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "type_mismatch"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn fn_effect_contract_must_cover_every_callee_effect() {
    let src = "fn remote(x: i32) -> i32 effects [io.read, network.read]:\n    return x\n\nfn bad() -> i32 effects [io.read]:\n    let f: Fn[i32, i32, effects[io.read]] = remote\n    return f(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "type_mismatch"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn closure_effects_are_part_of_the_fn_contract() {
    let src = "fn bad() -> i32 effects [io.read]:\n    let f: Fn[i32, i32] = |x: i32| x + i32(io_read())\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "type_mismatch"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn indirect_closure_calls_propagate_effects_at_the_call_site() {
    let src = "fn missing() -> i32:\n    let f: Fn[i32, i32, effects[io.read]] = |x: i32| x + i32(io_read())\n    return f(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
        }),
        "got: {diagnostics:?}"
    );
}

#[test]
fn malformed_fn_effect_contracts_are_rejected_without_a_call() {
    let src = "fn bad(unknown: Fn[i32, i32, effects[teleport]], misplaced: Fn[effects[io.read], i32, i32], duplicate: Fn[i32, i32, effects[io.read, io.read]]) -> i32:\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "unknown_fn_effect",
        "fn_effect_position",
        "duplicate_fn_effect",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_lower_non_runtime_effectful_fn_values() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_effect_fn_backend",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"effectfn\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn plain(x: i32) -> i32:\n    return x\n\nfn checked(x: i32) -> i32 effects [cpu.avx2]:\n    return x * 2\n\nfn apply_plain(f: Fn[i32, i32], x: i32) -> i32:\n    return f(x)\n\nfn apply_checked(f: Fn[i32, i32, effects[cpu.avx2]], x: i32) -> i32 effects [cpu.avx2]:\n    return f(x)\n\nexport fn run_checked(x: i32) -> i32 effects [cpu.avx2]:\n    let selected: Fn[i32, i32, effects[cpu.avx2]] = checked\n    return apply_checked(selected, x) + apply_plain(plain, x)\n",
    )
    .unwrap();

    let (header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert_eq!(
        header
            .matches("typedef int32_t (*fgx_fn_i32_i32)(int32_t);")
            .count(),
        1,
        "effect erasure must not duplicate the C ABI typedef:\n{header}"
    );
    assert!(
        c_source.contains("fgx_fn_i32_i32 selected = checked;")
            && c_source.contains("return f(x);"),
        "{c_source}"
    );
    assert!(
        zig_source.contains("f: *const fn (i32) i32")
            && zig_source.contains("var selected: *const fn (i32) i32 = checked;"),
        "{zig_source}"
    );
    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("effect_fn_harness.c");
        std::fs::write(
            &harness,
            "#include \"effectfn.h\"\nint main(void) { return run_checked(7) == 21 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "effect_fn_harness.exe"
        } else {
            "effect_fn_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated effectful function-value C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        assert!(
            std::process::Command::new(&executable)
                .status()
                .expect("failed to execute effectful function-value C harness")
                .success(),
            "generated effectful function-value C behavior diverged"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_effectful_fn_values_keep_the_effect_blocker() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_runtime_effect_fn_backend",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"runtimeeffectfn\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nexport fn apply_read(f: Fn[i32, i32, effects[io.read]], x: i32) -> i32 effects [io.read]:\n    return f(x)\n",
    )
    .unwrap();
    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    for error in [&c_err, &zig_err] {
        assert!(error.contains("effect:io.read"), "{error}");
        assert!(!error.contains("generic_type:Fn"), "{error}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5a: bounded generics / built-in Eq interface ---

const BOUNDED_EQ_PROG: &str = "fn same[T: Eq](a: T, b: T) -> bool:\n    return a == b\n\nfn forwarded[T: Eq](a: T, b: T) -> bool:\n    return same(a, b)\n\nfn same_int() -> bool:\n    return forwarded(7, 7)\n\nfn different_text() -> bool:\n    return same(\"left\", \"right\")\n";

#[test]
fn eq_bounded_generics_roundtrip_typecheck_and_execute() {
    let program = parse_human(BOUNDED_EQ_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("fn same[T: Eq]("), "{human}");
    assert!(agent.contains("F same[T: Eq]("), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let canonical = program.to_canonical_json();
    assert!(
        canonical.contains("\"type_bounds\": [\"T:Eq\"]"),
        "{canonical}"
    );
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    for (entry, expected) in [("same_int", true), ("different_text", false)] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{output}");
    }
}

#[test]
fn generic_equality_requires_an_eq_bound() {
    let src = "fn same[T](a: T, b: T) -> bool:\n    return a == b\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing_trait_bound"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn eq_bound_rejects_non_eq_function_values() {
    let src = "fn inc(x: i32) -> i32:\n    return x + 1\n\nfn same[T: Eq](a: T, b: T) -> bool:\n    return a == b\n\nfn bad() -> bool:\n    return same(inc, inc)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "trait_bound_not_satisfied"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn trait_bound_names_and_duplicates_are_validated() {
    let src = "fn bad[T: Hash + Eq + Eq](value: T) -> T:\n    return value\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in ["unknown_trait", "duplicate_trait_bound"] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn generic_function_values_preserve_bounds_and_instantiate_per_call() {
    let src = "fn identity[T](value: T) -> T:\n    return value\n\nfn same[T: Eq](a: T, b: T) -> bool:\n    return a == b\n\nfn apply(f: Fn[forall[U], U, U], value: i32) -> i32:\n    return f(value)\n\nfn answer() -> bool:\n    let id: Fn[forall[T], T, T] = identity\n    let equal: Fn[forall[T: Eq], T, T, bool] = same\n    let number: i32 = id(42)\n    let flag: bool = id(true)\n    let applied: i32 = apply(identity, number)\n    return equal(applied, 42) && equal(flag, true)\n";
    let program = parse_human(src).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("Fn[forall[T], T, T]"), "{human}");
    assert!(human.contains("Fn[forall[T:Eq], T, T, bool]"), "{human}");
    assert!(agent.contains("Fn[forall[T],T,T]"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": {}, \"expect\": { \"return\": true } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn generic_fn_binders_validate_contracts_and_call_bounds() {
    let src = "fn identity[T](value: T) -> T:\n    return value\n\nfn same[T: Eq](a: T, b: T) -> bool:\n    return a == b\n\nfn inc(value: i32) -> i32:\n    return value + 1\n\nfn bad() -> bool:\n    let narrowed: Fn[forall[T: Eq], T, T] = identity\n    let wrong: Fn[forall[T], T, T, bool] = same\n    let equal: Fn[forall[T: Eq], T, T, bool] = same\n    return equal(inc, inc)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in ["type_mismatch", "trait_bound_not_satisfied"] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }

    let malformed =
        "fn bad(value: Fn[forall[T: Missing + Eq + Eq], T, T]) -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(malformed).unwrap());
    for code in ["unknown_trait", "duplicate_fn_binder_bound"] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_keep_generic_function_values_explicitly_blocked() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_generic_fn_value_blocker",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"genericfnvalue\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn identity[T](value: T) -> T:\n    return value\n\nexport fn answer(value: i32) -> i32:\n    let id: Fn[forall[T], T, T] = identity\n    return id(value)\n",
    )
    .unwrap();
    for error in [
        crate::c_backend::generate_c_for_project(&dir).unwrap_err(),
        crate::zig_backend::generate_zig_for_project(&dir).unwrap_err(),
    ] {
        assert!(error.contains("generic_type:Fn"), "{error}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_backends_monomorphize_eq_bounded_helpers() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_bounded_eq_monomorph",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"boundedeq\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn same[T: Eq](a: T, b: T) -> bool:\n    return a == b\n\nexport fn same_i32(a: i32, b: i32) -> bool:\n    return same(a, b)\n",
    )
    .unwrap();

    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("same__fgx_mono_i32"), "{source}");
        assert!(!source.contains("T: Eq"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5b: user-defined marker traits and explicit impls ---

const MARKER_TRAIT_PROG: &str = "trait Stable:\n\nimpl Stable for i32:\n\nfn keep[T: Stable](value: T) -> T:\n    return value\n\nfn forward[T: Stable](value: T) -> T:\n    return keep(value)\n\nfn answer() -> i32:\n    return forward(42)\n";

#[test]
fn marker_traits_roundtrip_typecheck_and_execute() {
    let program = parse_human(MARKER_TRAIT_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("trait Stable:"), "{human}");
    assert!(human.contains("impl Stable for i32:"), "{human}");
    assert!(agent.contains("TR Stable"), "{agent}");
    assert!(agent.contains("IM Stable i32"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"kind\": \"trait\""), "{canonical}");
    assert!(
        canonical.contains("\"kind\": \"trait_impl\""),
        "{canonical}"
    );
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": {}, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn marker_trait_bound_rejects_a_type_without_an_impl() {
    let src = "trait Stable:\n\nimpl Stable for i32:\n\nfn keep[T: Stable](value: T) -> T:\n    return value\n\nfn bad() -> str:\n    return keep(\"not stable\")\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "trait_bound_not_satisfied"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn marker_trait_declarations_and_impls_are_validated() {
    let src = "trait Eq:\n\nimpl Eq for i32:\n\nimpl Missing for i32:\n\ntrait Stable:\n\ntrait Stable:\n\nimpl Stable for i32:\n\nimpl Stable for i32:\n\nfn done() -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "reserved_trait_name",
        "builtin_trait_impl",
        "unknown_impl_trait",
        "duplicate_trait",
        "duplicate_trait_impl",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn every_user_trait_bound_must_be_implemented() {
    let src = "trait Stable:\n\ntrait Reviewed:\n\nimpl Stable for i32:\n\nfn accept[T: Stable + Reviewed](value: T) -> T:\n    return value\n\nfn bad() -> i32:\n    return accept(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "trait_bound_not_satisfied"
                && diagnostic.message.contains("Reviewed")
        }),
        "got: {diagnostics:?}"
    );
}

#[test]
fn native_backends_monomorphize_marker_bounded_helpers() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_marker_trait_monomorph",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"markertrait\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "trait Stable:\n\nimpl Stable for i32:\n\nfn keep[T: Stable](value: T) -> T:\n    return value\n\nexport fn answer() -> i32:\n    return keep(42)\n",
    )
    .unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("keep__fgx_mono_i32"), "{source}");
        assert!(!source.contains("Stable"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn required_trait_methods_roundtrip_and_validate() {
    let src = "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n    measure = measure_i32\n\nfn measure_i32(value: i32) -> i32:\n    return value\n\nfn answer() -> i32:\n    return measure_i32(9)\n";
    let program = parse_human(src).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(agent.contains("REQ measure(value Self) -> i32"), "{agent}");
    assert!(agent.contains("BIND measure measure_i32"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": {}, \"expect\": { \"return\": 9 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn trait_method_impl_completeness_and_signatures_are_checked() {
    let src = "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n\nimpl Measure for bool:\n    measure = wrong\n\nfn wrong(value: i32) -> bool:\n    return true\n\nfn done() -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "missing_trait_method_impl",
        "trait_method_signature_mismatch",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

// --- lang.4 slice 5d: trait-qualified method dispatch ---

const TRAIT_DISPATCH_PROG: &str = "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n    measure = measure_i32\n\nfn measure_i32(value: i32) -> i32:\n    return value * 2\n\nfn generic_measure[T: Measure](value: T) -> i32:\n    return Measure::measure(value)\n\nfn concrete() -> i32:\n    return Measure::measure(4)\n\nfn generic() -> i32:\n    return generic_measure(5)\n";

#[test]
fn trait_qualified_dispatch_roundtrips_and_executes() {
    let program = parse_human(TRAIT_DISPATCH_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("Measure::measure(value)"), "{human}");
    assert!(agent.contains("Measure::measure(value)"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");

    for (entry, expected) in [("concrete", 8), ("generic", 10)] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{output}");
    }
}

#[test]
fn trait_qualified_dispatch_requires_a_bound_or_concrete_impl() {
    let src = "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n    measure = measure_i32\n\nfn measure_i32(value: i32) -> i32:\n    return value\n\nfn unbounded[T](value: T) -> i32:\n    return Measure::measure(value)\n\nfn wrong_concrete() -> i32:\n    return Measure::measure(true)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    let failures = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "trait_method_bound_not_satisfied")
        .count();
    assert_eq!(failures, 2, "got: {diagnostics:?}");
}

#[test]
fn unknown_trait_qualified_method_is_rejected() {
    let src = "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n    measure = measure_i32\n\nfn measure_i32(value: i32) -> i32:\n    return value\n\nfn bad() -> i32:\n    return Measure::missing(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "unknown_trait_method_call"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn source_backends_specialize_concrete_and_generic_trait_dispatch() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_trait_dispatch_specialization",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"traitdispatch\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n    measure = measure_i32\n\nfn measure_i32(value: i32) -> i32:\n    return value * 2\n\nfn generic_measure[T: Measure](value: T) -> i32:\n    return Measure::measure(value)\n\nexport fn measured(value: i32) -> i32:\n    return Measure::measure(value)\n\nexport fn measured_generic(value: i32) -> i32:\n    return generic_measure(value)\n",
    )
    .unwrap();

    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("measure_i32(value)"), "{source}");
        assert!(source.contains("generic_measure__fgx_mono_i32"), "{source}");
        assert!(!source.contains("Measure::measure"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn source_backends_keep_ambiguous_trait_dispatch_blocked() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_ambiguous_trait_dispatch",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"ambiguousdispatch\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "trait Measure:\n    requires measure(value: Self) -> i32\n\nimpl Measure for i32:\n    measure = measure_i32\n\nimpl Measure for i64:\n    measure = measure_i64\n\nfn measure_i32(value: i32) -> i32:\n    return value\n\nfn measure_i64(value: i64) -> i32:\n    return i32(value)\n\nexport fn measured() -> i32:\n    return Measure::measure(1)\n",
    )
    .unwrap();

    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    for error in [&c_err, &zig_err] {
        assert!(error.contains("op:trait_dispatch"), "{error}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5f: associated constants ---

const TRAIT_CONST_PROG: &str = "trait Limits:\n    requires const max: Self\n\nimpl Limits for i32:\n    const max = 21\n\nfn answer() -> i32:\n    return Limits::max[i32] * 2\n";

#[test]
fn trait_associated_constants_roundtrip_typecheck_and_execute() {
    let program = parse_human(TRAIT_CONST_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("requires const max: Self"), "{human}");
    assert!(human.contains("const max = 21"), "{human}");
    assert!(agent.contains("CONST max Self"), "{agent}");
    assert!(agent.contains("CONST max = 21"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program
        .to_canonical_json()
        .contains("\"constants\": [\"max:Self\"]"));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": {}, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn trait_associated_constant_contracts_are_enforced() {
    let src = "trait Limits:\n    requires const max: i32\n    requires const dynamic: i32\n    requires const absent: i32\n\nimpl Limits for i32:\n    const max = true\n    const max = 1\n    const other = 2\n    const dynamic = helper()\n\nfn helper() -> i32:\n    return 3\n\nfn done() -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "trait_const_type_mismatch",
        "duplicate_trait_const_impl",
        "unknown_trait_const",
        "trait_const_not_constant",
        "missing_trait_const_impl",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_inline_trait_associated_constants() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_trait_associated_const",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"traitconst\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "trait Limits:\n    requires const max: Self\n\nimpl Limits for i32:\n    const max = 21\n\nexport fn answer() -> i32:\n    return Limits::max[i32] * 2\n",
    )
    .unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        // The associated constant is inlined in both backends. How the multiply
        // is then spelled differs: C routes integer arithmetic through a
        // width-checked helper, Zig still emits the operator.
        assert!(!source.contains("Limits::max"), "{source}");
        assert!(source.contains("21"), "{source}");
    }
    assert!(c.contains("fgx_checked_i32_mul(21, 2)"), "{c}");
    assert!(zig.contains("21 * 2"), "{zig}");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5g: associated types in trait contracts ---

const TRAIT_TYPE_PROG: &str = "trait Extract:\n    requires type Item\n    requires extract(value: Self) -> Self::Item\n    requires const fallback: Self::Item\n\nimpl Extract for i32:\n    type Item = bool\n    extract = is_positive\n    const fallback = true\n\nfn is_positive(value: i32) -> bool:\n    return value > 0\n\nfn answer() -> bool:\n    return Extract::extract(1) && Extract::fallback[i32]\n";

#[test]
fn trait_associated_types_roundtrip_resolve_and_execute() {
    let program = parse_human(TRAIT_TYPE_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("requires type Item"), "{human}");
    assert!(human.contains("type Item = bool"), "{human}");
    assert!(agent.contains("TYPE Item"), "{agent}");
    assert!(agent.contains("TYPE Item = bool"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"associated_types\": [\"Item\"]"));
    assert!(canonical.contains("\"associated_types\": [\"Item=bool\"]"));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": {}, \"expect\": { \"return\": true } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn trait_associated_type_bindings_are_complete_unique_and_declared() {
    let src = "trait Extract:\n    requires type Item\n    requires type Error\n\nimpl Extract for i32:\n    type Item = bool\n    type Item = i32\n    type Other = str\n\nfn done() -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "duplicate_trait_type_impl",
        "unknown_trait_type",
        "missing_trait_type_impl",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_resolve_trait_associated_type_contracts() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_trait_associated_type",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"traittype\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        TRAIT_TYPE_PROG.replace("fn answer", "export fn answer"),
    )
    .unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("is_positive(1)"), "{source}");
        assert!(!source.contains("Extract::"), "{source}");
        assert!(!source.contains("Self::Item"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5h: generic and blanket marker impls ---

const BLANKET_IMPL_PROG: &str = "trait Stable:\n\nimpl[T: Eq] Stable for T:\n\nfn keep[T: Stable](value: T) -> T:\n    return value\n\nfn answer() -> i32:\n    return keep(42)\n";

#[test]
fn generic_blanket_impls_roundtrip_check_and_execute() {
    let program = parse_human(BLANKET_IMPL_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("impl[T: Eq] Stable for T:"), "{human}");
    assert!(agent.contains("IM[T: Eq] Stable T {"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"type_params\": [\"T\"]"));
    assert!(canonical.contains("\"type_bounds\": [\"T:Eq\"]"));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": {}, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn generic_impl_patterns_match_nested_concrete_types() {
    let src = "trait Sequence:\n\nimpl[T: Eq] Sequence for Vec[T]:\n\nfn keep[T: Sequence](value: T) -> T:\n    return value\n\nfn keep_i32s(values: Vec[i32]) -> Vec[i32]:\n    return keep(values)\n";
    let program = parse_human(src).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(
        human.contains("impl[T: Eq] Sequence for Vec[T]:"),
        "{human}"
    );
    assert!(agent.contains("IM[T: Eq] Sequence Vec[T] {"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
}

#[test]
fn generic_impl_bounds_and_coherence_are_enforced() {
    let src = "trait Stable:\n\ntrait Measure:\n    requires measure(value: Self) -> i32\n\nimpl[T: Eq] Stable for T:\n\nimpl Stable for i32:\n\nimpl[U] Stable for bool:\n\nimpl[V] Measure for Vec[V]:\n\nfn keep[T: Stable](value: T) -> T:\n    return value\n\nfn bad(value: Fn[i32, i32]) -> Fn[i32, i32]:\n    return keep(value)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "overlapping_trait_impl",
        "unconstrained_impl_type_param",
        "missing_trait_method_impl",
        "trait_bound_not_satisfied",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn native_backends_specialize_blanket_bounded_helpers() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_blanket_trait_monomorph",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"blankettrait\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        BLANKET_IMPL_PROG.replace("fn answer", "export fn answer"),
    )
    .unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("keep__fgx_mono_i32"), "{source}");
        assert!(!source.contains("Stable"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5j: generic impls with associated items ---

const GENERIC_IMPL_ITEMS_PROG: &str = "trait Echo:\n    requires type Item\n    requires echo(value: Self) -> Self::Item\n    requires const tag: i32\n\nimpl[T: Eq] Echo for T:\n    type Item = T\n    echo = identity\n    const tag = 6\n\nfn identity[T](value: T) -> T:\n    return value\n\nfn answer(value: i32) -> i32:\n    return Echo::echo(value) + Echo::tag[i32]\n";

#[test]
fn generic_impl_associated_items_roundtrip_check_and_execute() {
    let program = parse_human(GENERIC_IMPL_ITEMS_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("impl[T: Eq] Echo for T:"), "{human}");
    assert!(human.contains("type Item = T"), "{human}");
    assert!(agent.contains("IM[T: Eq] Echo T {"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": { \"value\": 36 }, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn generic_impl_item_contracts_are_checked_symbolically() {
    let src = "trait Echo:\n    requires type Item\n    requires echo(value: Self) -> Self::Item\n    requires const tag: i32\n\nimpl[T] Echo for Vec[T]:\n    type Item = T\n    echo = wrong\n    const tag = true\n\nfn wrong[T](value: Vec[T]) -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "trait_method_signature_mismatch",
        "trait_const_type_mismatch",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_specialize_generic_impl_associated_items() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_generic_impl_items",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"genericimplitems\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        GENERIC_IMPL_ITEMS_PROG.replace("fn answer", "export fn answer"),
    )
    .unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("identity__fgx_mono_i32(value)"), "{source}");
        assert!(!source.contains("Echo::"), "{source}");
        assert!(!source.contains("Self::Item"), "{source}");
    }
    // The addition survives in both; C checks it at the declared width.
    assert!(
        c.contains("fgx_checked_i32_add(identity__fgx_mono_i32(value), 6)"),
        "{c}"
    );
    assert!(zig.contains("+ 6"), "{zig}");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5k: standalone and generic associated-type projections ---

const ASSOCIATED_PROJECTION_PROG: &str = "trait Extract:\n    requires type Item\n    requires extract(value: Self) -> Self::Item\n\nimpl[T: Eq] Extract for T:\n    type Item = T\n    extract = identity\n\nfn identity[T](value: T) -> T:\n    return value\n\nfn project[T: Extract](value: T) -> Extract::Item[T]:\n    return Extract::extract(value)\n\nfn answer(value: i32) -> i32:\n    let item: Extract::Item[i32] = project(value)\n    return item\n";

#[test]
fn associated_type_projections_roundtrip_check_and_execute() {
    let program = parse_human(ASSOCIATED_PROJECTION_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(
        human.contains("fn project[T: Extract](value: T) -> Extract::Item[T]:"),
        "{human}"
    );
    assert!(
        human.contains("let item: Extract::Item[i32] = project(value)"),
        "{human}"
    );
    assert!(agent.contains("-> Extract::Item[T]"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let nested = "trait Extract:\n    requires type Item\n    requires mapper(value: Self) -> Fn[Self::Item, Self::Item]\n\nimpl[T: Eq] Extract for T:\n    type Item = bool\n    mapper = choose_flip\n\nfn flip(value: bool) -> bool:\n    return !value\n\nfn choose_flip[T](value: T) -> Fn[bool, bool]:\n    return flip\n\nfn projected[T: Extract](value: T) -> Fn[Extract::Item[T], Extract::Item[T]]:\n    return Extract::mapper(value)\n";
    let nested_diagnostics = crate::typecheck::check(&parse_human(nested).unwrap());
    assert!(nested_diagnostics.is_empty(), "got: {nested_diagnostics:?}");
    let output = run_entry_json(
        &program,
        "answer",
        "{ \"args\": { \"value\": 42 }, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn associated_type_projection_contracts_are_enforced() {
    let src = "trait Extract:\n    requires type Item\n\nimpl Extract for i32:\n    type Item = bool\n\nfn unknown_trait(value: Missing::Item[i32]) -> i32:\n    return 0\n\nfn unknown_item(value: Extract::Other[i32]) -> i32:\n    return 0\n\nfn missing_impl(value: Extract::Item[str]) -> i32:\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "unknown_projection_trait",
        "unknown_associated_type_projection",
        "associated_type_impl_missing",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_resolve_generic_associated_type_projections() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_associated_type_projection",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"projection\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        ASSOCIATED_PROJECTION_PROG.replace("fn answer", "export fn answer"),
    )
    .unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("project__fgx_mono_i32"), "{source}");
        assert!(!source.contains("Extract::Item"), "{source}");
        assert!(!source.contains("Extract::extract"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 5i: default trait methods ---

const TRAIT_DEFAULT_PROG: &str = "trait Normalize:\n    requires normalize(value: Self) -> Self default identity\n\nimpl Normalize for i32:\n\nimpl Normalize for bool:\n    normalize = flip\n\nfn identity[T](value: T) -> T:\n    return value\n\nfn flip(value: bool) -> bool:\n    return !value\n\nfn answer() -> i32:\n    return Normalize::normalize(7)\n\nfn flag() -> bool:\n    return Normalize::normalize(true)\n";

#[test]
fn trait_default_methods_roundtrip_execute_and_allow_overrides() {
    let program = parse_human(TRAIT_DEFAULT_PROG).unwrap();
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(
        human.contains("requires normalize(value: Self) -> Self default identity"),
        "{human}"
    );
    assert!(
        agent.contains("REQ normalize(value Self) -> Self DEFAULT identity"),
        "{agent}"
    );
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program
        .to_canonical_json()
        .contains("normalize(Self)->Self=identity"));
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    for (entry, expected) in [("answer", "7"), ("flag", "false")] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{output}");
    }
}

#[test]
fn trait_default_functions_must_exist_be_pure_and_match_each_user() {
    let src = "trait Missing:\n    requires apply(value: Self) -> Self default absent\n\ntrait Impure:\n    requires apply(value: Self) -> Self default impure\n\ntrait Wrong:\n    requires apply(value: Self) -> Self default wrong\n\nimpl Wrong for i32:\n\nfn impure[T](value: T) -> T effects [io]:\n    return value\n\nfn wrong(value: bool) -> bool:\n    return value\n\nfn done() -> bool:\n    return true\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    for code in [
        "unknown_trait_default_function",
        "trait_default_not_pure",
        "trait_default_signature_mismatch",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "missing {code}: {diagnostics:?}"
        );
    }
}

#[test]
fn source_backends_specialize_default_trait_methods() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_trait_default_method",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"traitdefault\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    let source = TRAIT_DEFAULT_PROG
        .replace("fn answer", "export fn answer")
        .replace("fn flag", "export fn flag");
    std::fs::write(dir.join("src/m.fgx"), source).unwrap();
    let (_, c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for source in [&c, &zig] {
        assert!(source.contains("identity__fgx_mono_i32"), "{source}");
        assert!(source.contains("flip(true)"), "{source}");
        assert!(!source.contains("Normalize::normalize"), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 2: generic functions ---

// One generic function, two instantiations (T=i32, T=str) from call sites.
const GENERIC_PROG: &str = "fn first[T](v: Vec[T]) -> T:\n    return v[0]\n\nfn id[T](x: T) -> T:\n    return x\n\nfn pick() -> i32 effects [alloc]:\n    let v: Vec[i32] = vec_new()\n    push(v, 42)\n    push(v, 7)\n    return first(v)\n\nfn pick_str() -> str effects [alloc]:\n    let v: Vec[str] = vec_new()\n    push(v, \"a\")\n    push(v, \"b\")\n    return first(v)\n\nfn ident() -> i32:\n    return id(5)\n";

#[test]
fn generic_function_instantiates_per_call_site() {
    let program = parse_human(GENERIC_PROG).unwrap();
    for (entry, expected) in [("pick", "42"), ("pick_str", "\"a\""), ("ident", "5")] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(
            output.contains("\"status\": \"pass\""),
            "{entry}:\n{output}"
        );
    }
}

#[test]
fn generic_program_type_checks_clean() {
    // T binds to i32 at one call site and str at another; the substituted
    // return types satisfy each caller's declared return type.
    let diagnostics = crate::typecheck::check(&parse_human(GENERIC_PROG).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn generic_type_params_roundtrip_both_syntaxes() {
    // `fn first[T](…)` survives .fgx -> AST -> .fgai -> AST losslessly
    // (type_params ride the canonical JSON only when non-empty).
    let program = parse_human(GENERIC_PROG).unwrap();
    let agent = format_agent(&program);
    assert!(agent.contains("F first[T]("), "agent:\n{agent}");
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn wrong_instantiation_is_rejected() {
    // first(Vec[i32]) types as i32; returning it from a str function is an
    // ordinary return_type_mismatch — the binding flowed through.
    let src = "fn first[T](v: Vec[T]) -> T:\n    return v[0]\n\nfn bad() -> str effects [alloc]:\n    let v: Vec[i32] = vec_new()\n    push(v, 1)\n    return first(v)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "return_type_mismatch"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn c_backend_contract_blocks_generic_functions() {
    // Source backends cannot monomorphize yet, so the FIR contract must reject
    // a generic function honestly instead of emitting invalid C.
    let dir = std::env::temp_dir().join(format!("forgentex_{}_generic_c", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"genericdemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "export fn id[T](x: T) -> T:\n    return x\n",
    )
    .unwrap();
    let err = crate::c_backend::build_c_project(&dir, &dir.join("out")).unwrap_err();
    assert!(
        err.contains("generic_function"),
        "expected a contract blocker, got: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn source_backends_monomorphize_private_generic_call_graphs() {
    // A concrete exported surface can now call through private generic helpers.
    // The worklist also discovers the nested forward[T] -> id[T] edge and emits
    // one stable clone per actually used type.
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_generic_monomorph",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"monodemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn id[T](x: T) -> T:\n    return x\n\nfn forward[T](x: T) -> T:\n    return id(x)\n\nexport fn as_i32(x: i32) -> i32:\n    return forward(x)\n\nexport fn as_bool(x: bool) -> bool:\n    return id(x)\n\nexport fn literal() -> i32:\n    return id(42)\n",
    )
    .unwrap();

    let c = crate::backend::build_project_with_backend(&dir, "c", &dir.join("out-c"))
        .expect("C build should monomorphize eligible generic helpers");
    let c_source = std::fs::read_to_string(
        c.files
            .iter()
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("c"))
            .unwrap(),
    )
    .unwrap();
    assert!(c_source.contains("id__fgx_mono_bool"), "{c_source}");
    assert!(c_source.contains("id__fgx_mono_i32"), "{c_source}");
    assert!(c_source.contains("forward__fgx_mono_i32"), "{c_source}");
    assert!(c_source.contains("id__fgx_mono_i32(42)"), "{c_source}");
    assert!(
        !c_source.contains(" id("),
        "generic template leaked:\n{c_source}"
    );

    let zig = crate::backend::build_project_with_backend(&dir, "zig", &dir.join("out-zig"))
        .expect("Zig build should use the same monomorphized FIR");
    let zig_source = std::fs::read_to_string(&zig.files[0]).unwrap();
    assert!(zig_source.contains("id__fgx_mono_bool"), "{zig_source}");
    assert!(zig_source.contains("id__fgx_mono_i32"), "{zig_source}");
    assert!(zig_source.contains("forward__fgx_mono_i32"), "{zig_source}");
    assert!(zig_source.contains("id__fgx_mono_i32(42)"), "{zig_source}");
    assert!(
        !zig_source.contains("fn id("),
        "generic template leaked:\n{zig_source}"
    );

    // The source-only generation APIs run the identical preparation pass.
    let (_, generated_c) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let generated_zig = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(generated_c.contains("id__fgx_mono_i32(42)"));
    assert!(generated_zig.contains("id__fgx_mono_i32(42)"));

    // `--opt` runs after specialization, so inlining cannot strand an
    // uninstantiated template and reintroduce a generic FIR blocker.
    crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn uncalled_private_generic_remains_an_honest_backend_blocker() {
    // With no concrete call there is no principled ABI/type choice. Retaining
    // the declaration lets the established FIR blocker explain that boundary.
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_uncalled_generic", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"uncalledgeneric\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn id[T](x: T) -> T:\n    return x\n\nexport fn answer() -> i32:\n    return 42\n",
    )
    .unwrap();
    let err = crate::zig_backend::build_zig_project(&dir, &dir.join("out")).unwrap_err();
    assert!(err.contains("generic_function"), "got: {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- lang.4 slice 3: closures ---

const CLOSURE_PROG: &str = "fn apply2(f: Fn[i32, i32], x: i32) -> i32:\n    return f(x)\n\nfn direct_closure() -> i32:\n    let f: Fn[i32, i32] = |x: i32| x * 2\n    return f(4)\n\nfn capture() -> i32:\n    let n: i32 = 10\n    let f: Fn[i32, i32] = |x: i32| x + n\n    return f(5)\n\nfn by_value() -> i32:\n    let n: i32 = 1\n    let f: Fn[i32, i32] = |x: i32| x + n\n    n = 100\n    return f(0)\n\nfn inline_arg() -> i32:\n    return apply2(|x: i32| x * 10, 3)\n";

#[test]
fn closure_binds_captures_and_calls() {
    let program = parse_human(CLOSURE_PROG).unwrap();
    for (entry, expected) in [
        ("direct_closure", 8), // |x| x * 2 applied to 4
        ("capture", 15),       // captures n = 10, f(5) = 15
        ("inline_arg", 30),    // lambda passed directly to a higher-order fn
    ] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(
            output.contains("\"status\": \"pass\""),
            "{entry}:\n{output}"
        );
    }
}

#[test]
fn closure_captures_by_value_not_by_reference() {
    // n is captured at closure creation; the later `n = 100` is invisible to
    // the closure — f(0) sees the captured 1, not 100.
    let program = parse_human(CLOSURE_PROG).unwrap();
    let output = run_entry_json(
        &program,
        "by_value",
        "{ \"args\": {}, \"expect\": { \"return\": 1 } }",
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn closure_program_type_checks_clean() {
    // A lambda types as Fn[P…, R] from its declared params and body; bindings
    // and higher-order arguments check against it.
    let diagnostics = crate::typecheck::check(&parse_human(CLOSURE_PROG).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn affine_closure_captures_a_mutable_borrow_and_runs_once() {
    let source = "fn read_once(xs: &mut [u8]) -> u8:\n    let read: FnOnce[usize, u8] = |index: usize| xs[index]\n    return read(0)\n";
    let program = parse_human(source).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    let human = format_human(&program);
    let agent = format_agent(&program);
    assert!(human.contains("FnOnce[usize, u8]"), "{human}");
    assert!(agent.contains("FnOnce[usize,u8]"), "{agent}");
    assert!(program.canonical_eq(&parse_human(&human).unwrap()));
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let output = run_entry_json(
        &program,
        "read_once",
        r#"{"args":{"xs":[7]},"expect":{"return":7}}"#,
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn affine_closure_is_consumed_and_owns_mutable_capture_roots() {
    let source = "fn double_call(xs: &mut [u8]) -> u8:\n    let read: FnOnce[usize, u8] = |index: usize| xs[index]\n    let first: u8 = read(0)\n    return read(1)\n\nfn root_after_capture(xs: &mut [u8]) -> u8:\n    let read: FnOnce[usize, u8] = |index: usize| xs[index]\n    return xs[0]\n\nfn copyable_capture(xs: &mut [u8]) -> u8:\n    let read: Fn[usize, u8] = |index: usize| xs[index]\n    return read(0)\n\nfn cannot_upgrade(xs: &mut [u8]) -> u8:\n    let once: FnOnce[usize, u8] = |index: usize| xs[index]\n    let reusable: Fn[usize, u8] = once\n    return reusable(0)\n\nfn spawn_consumes() -> i32 effects [concurrency.task]:\n    let once: FnOnce[i32, i32] = |value: i32| value + 1\n    let task: Task[i32] = task_spawn(once, 1)\n    return once(2)\n";
    let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
    for (function, code) in [
        ("double_call", "use_after_move"),
        ("root_after_capture", "use_after_move"),
        ("copyable_capture", "mutable_borrow_alias"),
        ("cannot_upgrade", "type_mismatch"),
        ("spawn_consumes", "use_after_move"),
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.node.contains(function) && diagnostic.code == code
            }),
            "missing {code} for {function}: {diagnostics:?}"
        );
    }
}

#[test]
fn closures_roundtrip_both_syntaxes() {
    let program = parse_human(CLOSURE_PROG).unwrap();
    let agent = format_agent(&program);
    assert!(agent.contains("|x: i32| x * 2"), "agent:\n{agent}");
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn native_backends_lift_contextually_typed_non_capturing_scalar_closures() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_native_closure", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"nativeclosure\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn apply(f: Fn[i32, i32], x: i32) -> i32:\n    return f(x)\n\nfn make_tripler() -> Fn[i32, i32]:\n    return |value: i32| value * 3\n\nexport fn answer(x: i32) -> i32 effects [cpu.avx2]:\n    let doubler: Fn[i32, i32] = |value: i32| value * 2\n    let checked: Fn[i32, i32, effects[cpu.avx2]] = |value: i32| value - 1\n    let tripler: Fn[i32, i32] = make_tripler()\n    let selected: Fn[i32, i32] = doubler\n    selected = |value: i32| value + 4\n    return apply(doubler, x) + checked(x) + apply(|value: i32| value + 3, x) + tripler(x) + selected(x)\n",
    )
    .unwrap();

    let (header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(
        c_source.contains("static int32_t __fgx_lambda_m0_answer_0(int32_t value);")
            && c_source.contains("fgx_fn_i32_i32 doubler = __fgx_lambda_m0_answer_0;")
            && c_source.contains("fgx_fn_i32_i32 checked = __fgx_lambda_m0_answer_1;")
            && c_source.contains("selected = __fgx_lambda_m0_answer_2;")
            && c_source.contains("apply(__fgx_lambda_m0_answer_3, x)")
            && c_source.contains("return __fgx_lambda_m0_make_tripler_0;"),
        "{c_source}"
    );
    assert!(
        zig_source.contains("fn __fgx_lambda_m0_answer_0(value: i32) i32")
            && zig_source.contains("var doubler: *const fn (i32) i32 = __fgx_lambda_m0_answer_0;")
            && zig_source.contains("var checked: *const fn (i32) i32 = __fgx_lambda_m0_answer_1;")
            && zig_source.contains("selected = __fgx_lambda_m0_answer_2;")
            && zig_source.contains("apply(__fgx_lambda_m0_answer_3, x)")
            && zig_source.contains("return __fgx_lambda_m0_make_tripler_0;"),
        "{zig_source}"
    );

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("closure_harness.c");
        std::fs::write(
            &harness,
            "#include \"nativeclosure.h\"\nint main(void) { return answer(7) == 62 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "closure_harness.exe"
        } else {
            "closure_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated closure C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        assert!(
            std::process::Command::new(&executable)
                .status()
                .expect("failed to execute closure C harness")
                .success(),
            "generated closure C behavior diverged"
        );
    }
    assert!(header.contains("int32_t answer(int32_t x);"), "{header}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn source_backend_contracts_block_capturing_and_affine_closures() {
    // An environment layout and ownership policy do not exist yet, so FIR
    // preflight must keep capturing closures behind the explicit blocker.
    let dir = std::env::temp_dir().join(format!("forgentex_{}_closure_c", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"closuredemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/m.fgx"),
        "fn identity(x: i32) -> i32:\n    return x\n\nexport fn f() -> i32:\n    let offset: i32 = 1\n    let g: Fn[i32, i32] = identity\n    g = |x: i32| x + offset\n    return g(1)\n",
    )
    .unwrap();
    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    for err in [&c_err, &zig_err] {
        assert!(
            err.contains("lambda"),
            "expected a lambda contract blocker, got: {err}"
        );
    }
    std::fs::write(
        dir.join("src/m.fgx"),
        "export fn affine(x: i32) -> i32:\n    let once: FnOnce[i32, i32] = |value: i32| value + 1\n    return once(x)\n",
    )
    .unwrap();
    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-affine-c")).unwrap_err();
    let zig_err =
        crate::zig_backend::build_zig_project(&dir, &dir.join("out-affine-zig")).unwrap_err();
    for err in [&c_err, &zig_err] {
        assert!(
            err.contains("generic_type:FnOnce") && err.contains("lambda"),
            "expected explicit affine-closure blockers, got: {err}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
