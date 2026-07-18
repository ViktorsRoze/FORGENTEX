use std::fs;
use std::path::Path;

use crate::backend::{
    backend_catalog_json, backend_catalog_text, backend_ir_json, backend_ir_text,
    build_project_with_backend, frontend_contract_json, generate_backend_ir,
    run_entry_with_backend,
};
use crate::interp::run_entry_json_with_ir;
use crate::parser::parse_file;

use super::manifest_path;

#[test]
fn backend_catalog_marks_c_available_and_native_backends_planned() {
    let text = backend_catalog_text();
    assert!(text.contains("Backend info schema: forgentex.backend_info.v1"));
    assert!(text.contains("Backend IR schema: forgentex.backend_ir.v1"));
    assert!(text.contains("interp status=available interface=run"));
    assert!(text.contains("c status=available"));
    assert!(text.contains("zig status=available"));
    assert!(text.contains("llvm status=planned"));
    assert!(text.contains("feature=jit-cranelift"));
    assert!(text.contains("Default cargo features: []"));
    assert!(text.contains("riscv64"));

    let json = backend_catalog_json();
    assert!(json.contains("\"schema\": \"forgentex.backend_info.v1\""));
    assert!(json.contains("\"default_zero_dependency\": true"));
    assert!(json.contains("\"name\": \"interp\""));
    assert!(json.contains("\"name\": \"zig\""));
    assert!(json.contains("\"name\": \"cranelift\""));
    assert!(json.contains("\"feature\": \"jit-cranelift\""));
    assert!(json.contains("\"default_enabled\": false"));
    assert!(json.contains("\"dependency_status\""));
    assert!(json.contains("\"status\": \"planned\""));
}

#[test]
fn default_build_is_zero_dependency_and_optional_backends_are_feature_scaffolds() {
    let manifest = fs::read_to_string(manifest_path("Cargo.toml")).unwrap();
    assert!(manifest.contains("[features]"));
    assert!(manifest.contains("default = []"));
    assert!(manifest.contains("jit-cranelift = []"));
    assert!(manifest.contains("backend-llvm = []"));
    assert!(manifest.contains("backend-mlir = []"));

    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("Cargo.toml should have a dependencies section")
        .split("\n[")
        .next()
        .unwrap_or("");
    assert!(
        dependencies.trim().is_empty(),
        "default crate dependencies must stay empty; found {dependencies:?}"
    );
}

#[test]
fn backend_trait_runs_the_interpreter_backend() {
    let source = r#"
fn add_mul(a: i32, b: i32, c: i32) -> i32:
    return a + b * c
"#;
    let program = parse_file(Path::new("arith.fgx"), source).unwrap();
    let run = run_entry_with_backend(
        &program,
        "interp",
        "add_mul",
        r#"{ "args": { "a": 2, "b": 3, "c": 4 }, "expect": { "return": 14 } }"#,
    )
    .unwrap();
    assert_eq!(run.backend, "interp");
    assert_eq!(run.fir_schema, "forgentex.backend_ir.v1");
    assert_eq!(run.contract_status, "ready");
    assert!(run.json.contains("\"status\": \"pass\""));

    let err = run_entry_with_backend(&program, "c", "add_mul", "{}").unwrap_err();
    assert!(err.contains("does not run entries in-process"));
}

#[test]
fn interpreter_backend_uses_fir_contract_for_selected_entry() {
    let source = r#"
unsafe fn blocked() -> i32:
    return 1

fn safe_entry() -> i32:
    return 42
"#;
    let program = parse_file(Path::new("interp_fir_gate.fgx"), source).unwrap();
    let run = run_entry_with_backend(
        &program,
        "interp",
        "safe_entry",
        r#"{ "args": {}, "expect": { "return": 42 } }"#,
    )
    .unwrap();
    assert_eq!(run.contract_status, "ready");
    assert!(run.json.contains("\"status\": \"pass\""));

    let err =
        run_entry_with_backend(&program, "interp", "blocked", r#"{ "args": {} }"#).unwrap_err();
    assert!(
        err.contains("FIR contract blocks backend 'interp'"),
        "{err}"
    );
    assert!(
        err.contains("runtime::blocked blockers=[unsafe_function]"),
        "{err}"
    );
}

#[test]
fn interpreter_run_can_execute_fir_body_instead_of_ast_body() {
    let root =
        std::env::temp_dir().join(format!("forgentex_{}_interp_fir_body", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let ir_path = root.join("answer.fgx");
    fs::write(&ir_path, "fn answer() -> i32:\n    return 41\n").unwrap();
    let ir = generate_backend_ir(&ir_path).unwrap();
    let ast_program = parse_file(
        Path::new("answer_ast.fgx"),
        "fn answer() -> i32:\n    return 0\n",
    )
    .unwrap();

    let output = run_entry_json_with_ir(
        &ast_program,
        &ir,
        "answer",
        r#"{ "args": {}, "expect": { "return": 41 } }"#,
    )
    .unwrap();
    assert!(output.contains("\"return\": 41"), "{output}");
    assert!(output.contains("\"status\": \"pass\""), "{output}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn backend_ir_contains_modules_public_symbols_and_linearized_ops() {
    let ir = generate_backend_ir(&manifest_path(".")).unwrap();
    let text = backend_ir_text(&ir);

    assert!(text.contains("FGIR schema=forgentex.backend_ir.v1"));
    assert!(text.contains("package=forgentex-demo-media"));
    assert!(text.contains("M core.math"));
    assert!(text.contains("M media.audio"));
    assert!(text.contains("F mix_samples @function.mix_samples visibility=public"));
    assert!(text.contains("expr core_slice_require_mut_same_len_i16(dst, a)"));
    assert!(text.contains("for i 0 core_slice_len_mut_i16(dst) body_len=2 : usize"));
    assert!(text.contains("assign dst[i] core_math_clamp_i16(x)"));
    assert!(text.contains("S AudioBlock @struct.AudioBlock visibility=public"));
    assert!(text.contains("BACKEND interp status=ready blockers=[-]"));
    assert!(text.contains("BACKEND c status=ready blockers=[-]"));
    assert!(text.contains("BACKEND zig status=ready blockers=[-]"));

    let json = backend_ir_json(&ir);
    assert!(json.contains("\"schema\": \"forgentex.backend_ir.v1\""));
    assert!(json.contains("\"enums\": ["));
    assert!(json.contains("\"name\": \"mix_samples\""));
    assert!(json.contains("\"op\": \"assign\""));
    assert!(json.contains("\"backend_contracts\""));
}

#[test]
fn backend_ir_contracts_report_backend_readiness_and_blockers() {
    let root = std::env::temp_dir().join(format!("forgentex_{}_fir_contract", std::process::id()));
    let src = root.join("src");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&src).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"fir-contract\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        src.join("main.fgx"),
        "foreign to_fahrenheit python version \"3.13\" env \"cpython-3.13\" effects [alloc]:\n    input celsius: f64\n    output fahrenheit: f64\n    sandbox \"fs.none\"\n    mock 0.0 -> 32.0\n    code:\n        def to_fahrenheit(celsius):\n            return celsius * 9.0 / 5.0 + 32.0\n\nenum Flag:\n    On\n    Off\n\nenum Shape:\n    Circle(i32)\n    Empty\n\nfn with_vec(xs: Vec[i32]) -> usize effects [fs.read]:\n    return xs.len\n\nfn with_option(fallback: u8) -> u8:\n    return unwrap_or(Some(7), fallback)\n\nfn with_capsule(c: f64) -> f64:\n    return to_fahrenheit(c)\n\nfn with_match(flag: Flag) -> i32:\n    let out: i32 = 0\n    match flag:\n        case Flag::On:\n            out = 1\n        case Flag::Off:\n            out = 0\n    return out\n\nfn shape_value(shape: Shape) -> i32:\n    match shape:\n        case Shape::Circle(r):\n            return r\n        case Shape::Empty:\n            return 0\n",
    )
    .unwrap();

    let ir = generate_backend_ir(&root).unwrap();
    let text = backend_ir_text(&ir);
    assert!(text.contains("E Flag @enum.Flag visibility=private variants=2"));
    assert!(text.contains("VARIANT On payload=-"));
    assert!(text.contains("E Shape @enum.Shape visibility=private variants=2"));
    assert!(text.contains("VARIANT Circle payload=i32"));
    assert!(text.contains("F with_vec"));
    assert!(text.contains("BACKEND c status=blocked blockers=[effect:fs.read,generic_type:Vec]"));
    assert!(text.contains("BACKEND zig status=blocked blockers=[effect:fs.read,generic_type:Vec]"));
    assert!(text.contains("F with_option"));
    assert!(text.contains("BACKEND c status=blocked blockers=[intrinsic:Some,intrinsic:unwrap_or]"));
    assert!(
        text.contains("BACKEND zig status=blocked blockers=[intrinsic:Some,intrinsic:unwrap_or]")
    );
    assert!(text.contains("F with_capsule"));
    assert!(text.contains("BACKEND c status=blocked blockers=[capsule:to_fahrenheit]"));
    assert!(text.contains("BACKEND zig status=blocked blockers=[capsule:to_fahrenheit]"));
    assert!(text.contains("F with_match"));
    assert!(text.contains("BACKEND zig status=ready blockers=[-]"));
    assert!(text.contains("BACKEND c status=ready blockers=[-]"));
    assert!(
        !text.contains("op:match"),
        "Zig enum match lowering should no longer be reported as a FIR blocker:\n{text}"
    );

    let json = backend_ir_json(&ir);
    assert!(json.contains("\"enums\": ["));
    assert!(json.contains("{\"name\": \"Circle\", \"payload\": \"i32\"}"));
    assert!(json.contains("{\"name\": \"Empty\", \"payload\": null}"));
    assert!(json.contains("\"backend\": \"c\", \"status\": \"blocked\""));
    assert!(json.contains("\"blockers\": [\"effect:fs.read\", \"generic_type:Vec\"]"));
    assert!(json.contains("\"blockers\": [\"intrinsic:Some\", \"intrinsic:unwrap_or\"]"));
    assert!(json.contains("\"blockers\": [\"capsule:to_fahrenheit\"]"));
    assert!(!json.contains("\"op:match\""));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn backend_ir_can_be_generated_for_a_single_source_file() {
    let root =
        std::env::temp_dir().join(format!("forgentex_{}_single_file_ir", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let path = root.join("sample.fgx");
    fs::write(
        &path,
        "module sample.one\n\nexport enum Mode:\n    Fast\n    Slow\n\nexport struct Boxed:\n    value: i32\n\nexport fn choose(mode: Mode) -> i32:\n    match mode:\n        case Mode::Fast:\n            return 1\n        case Mode::Slow:\n            return 0\n",
    )
    .unwrap();

    let ir = generate_backend_ir(&path).unwrap();
    assert_eq!(ir.package, "single-file");
    assert_eq!(ir.version, "0.0.0");
    assert_eq!(ir.modules.len(), 1);
    let text = backend_ir_text(&ir);
    assert!(text.contains("M sample.one path="));
    assert!(text.contains("E Mode @enum.Mode visibility=public variants=2"));
    assert!(text.contains("S Boxed @struct.Boxed visibility=public fields=1"));
    assert!(text.contains("F choose @function.choose visibility=public"));

    let json = backend_ir_json(&ir);
    assert!(json.contains("\"package\": \"single-file\""));
    assert!(json.contains("\"name\": \"Mode\""));
    assert!(json.contains("\"name\": \"choose\""));

    let contract = frontend_contract_json(&path).unwrap();
    assert!(contract.contains("\"package\": \"single-file\""));
    assert!(contract.contains("\"schema\": \"forgentex.frontend_contract.v1\""));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn build_backend_uses_fir_contract_blockers_before_emission() {
    let root =
        std::env::temp_dir().join(format!("forgentex_{}_fir_build_gate", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"fir-build-gate\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "module gate.demo\n\nstruct Bag:\n    items: Vec[i32]\n\nexport fn blocked(path: str) -> str effects [fs.read]:\n    return path\n",
    )
    .unwrap();

    let err = build_project_with_backend(&root, "c", &root.join("out-c")).unwrap_err();
    assert!(err.contains("FIR contract blocks backend 'c'"), "{err}");
    assert!(
        err.contains("gate.demo::struct Bag blockers=[generic_type:Vec]"),
        "{err}"
    );
    assert!(
        err.contains("gate.demo::blocked blockers=[effect:fs.read]"),
        "{err}"
    );
    assert!(!root.join("out-c").exists());

    let err = build_project_with_backend(&root, "zig", &root.join("out-zig")).unwrap_err();
    assert!(err.contains("FIR contract blocks backend 'zig'"), "{err}");
    assert!(
        err.contains("gate.demo::struct Bag blockers=[generic_type:Vec]"),
        "{err}"
    );
    assert!(
        err.contains("gate.demo::blocked blockers=[effect:fs.read]"),
        "{err}"
    );
    assert!(!root.join("out-zig").exists());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn backend_choice_does_not_change_frontend_contract() {
    let root = manifest_path(".");
    let before = frontend_contract_json(&root).unwrap();
    let ir = generate_backend_ir(&root).unwrap();
    assert_eq!(ir.package, "forgentex-demo-media");

    let out_dir =
        std::env::temp_dir().join(format!("forgentex_{}_stage15_backend", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let artifacts = build_project_with_backend(&root, "c", &out_dir).unwrap();
    assert_eq!(artifacts.backend, "c");
    assert_eq!(artifacts.files.len(), 2);
    assert_eq!(artifacts.metadata_files.len(), 2);
    let receipt = fs::read_to_string(out_dir.join("forgentex.backend_build.json")).unwrap();
    assert!(receipt.contains("\"schema\": \"forgentex.backend_build.v1\""));
    assert!(receipt.contains("\"backend\": \"c\""));
    assert!(receipt.contains("\"fir_hash\":"));
    assert!(out_dir.join("forgentex.fgir.json").exists());

    let after = frontend_contract_json(&root).unwrap();
    assert_eq!(before, after);
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn zig_backend_choice_does_not_change_frontend_contract() {
    let root = manifest_path(".");
    let before = frontend_contract_json(&root).unwrap();

    let out_dir = std::env::temp_dir().join(format!(
        "forgentex_{}_stage28_zig_backend",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out_dir);
    let artifacts = build_project_with_backend(&root, "zig", &out_dir).unwrap();
    assert_eq!(artifacts.backend, "zig");
    assert_eq!(artifacts.files.len(), 1);
    assert_eq!(artifacts.metadata_files.len(), 2);
    let receipt = fs::read_to_string(out_dir.join("forgentex.backend_build.json")).unwrap();
    assert!(receipt.contains("\"backend\": \"zig\""));
    assert!(receipt.contains("\"source_files\":"));
    assert!(out_dir.join("forgentex.fgir.json").exists());

    let after = frontend_contract_json(&root).unwrap();
    assert_eq!(before, after);
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn planned_native_backends_fail_explicitly_without_frontend_changes() {
    let llvm =
        build_project_with_backend(&manifest_path("."), "llvm", &manifest_path("build/llvm"))
            .unwrap_err();
    assert!(llvm.contains("backend 'llvm' is planned for v3.0"));
    assert!(llvm.contains("feature=backend-llvm"));

    let unknown =
        build_project_with_backend(&manifest_path("."), "wat", &manifest_path("build/wat"))
            .unwrap_err();
    assert!(unknown.contains("unknown backend 'wat'"));
}
