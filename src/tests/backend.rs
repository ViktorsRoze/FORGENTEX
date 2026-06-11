use std::fs;

use crate::backend::{
    backend_catalog_json, backend_catalog_text, backend_ir_json, backend_ir_text,
    build_project_with_backend, frontend_contract_json, generate_backend_ir,
};

use super::manifest_path;

#[test]
fn backend_catalog_marks_c_available_and_native_backends_planned() {
    let text = backend_catalog_text();
    assert!(text.contains("Backend IR schema: forgentex.backend_ir.v1"));
    assert!(text.contains("c status=available"));
    assert!(text.contains("zig status=available"));
    assert!(text.contains("llvm status=planned"));
    assert!(text.contains("riscv64"));

    let json = backend_catalog_json();
    assert!(json.contains("\"schema\": \"forgentex.backends.v1\""));
    assert!(json.contains("\"name\": \"zig\""));
    assert!(json.contains("\"name\": \"cranelift\""));
    assert!(json.contains("\"status\": \"planned\""));
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

    let json = backend_ir_json(&ir);
    assert!(json.contains("\"schema\": \"forgentex.backend_ir.v1\""));
    assert!(json.contains("\"name\": \"mix_samples\""));
    assert!(json.contains("\"op\": \"assign\""));
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

    let after = frontend_contract_json(&root).unwrap();
    assert_eq!(before, after);
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn planned_native_backends_fail_explicitly_without_frontend_changes() {
    let llvm =
        build_project_with_backend(&manifest_path("."), "llvm", &manifest_path("build/llvm"))
            .unwrap_err();
    assert!(llvm.contains("backend 'llvm' is planned for Stage 15"));

    let unknown =
        build_project_with_backend(&manifest_path("."), "wat", &manifest_path("build/wat"))
            .unwrap_err();
    assert!(unknown.contains("unknown backend 'wat'"));
}
