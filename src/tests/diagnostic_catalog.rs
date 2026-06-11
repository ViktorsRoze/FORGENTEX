use std::fs;

use crate::diagnostic_catalog::{
    build_fix_plan, diagnostic_catalog_json, explain_json, FixPlanOptions,
};

#[test]
fn diagnostic_catalog_explains_normalized_and_compiler_codes() {
    let json = diagnostic_catalog_json();
    assert!(json.contains("\"schema\": \"forgentex.diagnostic_catalog.v1\""));
    assert!(json.contains("\"code\": \"TYP001\""));
    assert!(json.contains("\"canonical_code\": \"return_type_mismatch\""));

    let alias = explain_json("TYP001").unwrap();
    assert!(alias.contains("\"schema\": \"forgentex.diagnostic_explain.v1\""));
    assert!(alias.contains("\"rule\": \"return_assignability\""));

    let canonical = explain_json("return_type_mismatch").unwrap();
    assert!(canonical.contains("\"code\": \"TYP001\""));
}

#[test]
fn fix_plan_generates_common_type_repairs() {
    let path = std::env::temp_dir().join(format!(
        "forgentex_stage29_type_repairs_{}.fgx",
        std::process::id()
    ));
    fs::write(&path, "fn bad() -> i32:\n    let x = 1\n    return 1.0\n").unwrap();

    let report = build_fix_plan(&FixPlanOptions {
        source: path.to_string_lossy().to_string(),
        target: None,
    })
    .unwrap();

    assert!(!report.ok);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "TYP002"));
    assert!(report
        .plans
        .iter()
        .any(|plan| plan.operation == "add_local_type_annotation"));
    assert!(report
        .plans
        .iter()
        .any(|plan| plan.operation == "replace_return_type"));
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.fix_plan.v1\""));

    let _ = fs::remove_file(path);
}

#[test]
fn unsafe_assembly_plan_requires_human_review() {
    let path = std::env::temp_dir().join(format!(
        "forgentex_stage29_safe_asm_{}.fgx",
        std::process::id()
    ));
    fs::write(
        &path,
        "fn bad(x: u64) -> u64:\n    asm x86_64:\n        in rax: x\n        out rax: y\n        op popcnt rax rax\n    return y\n",
    )
    .unwrap();

    let report = build_fix_plan(&FixPlanOptions {
        source: path.to_string_lossy().to_string(),
        target: Some("x86_64-pc-windows-msvc".to_string()),
    })
    .unwrap();

    let plan = report
        .plans
        .iter()
        .find(|plan| plan.operation == "mark_function_unsafe")
        .expect("unsafe assembly should produce a repair plan");
    assert_eq!(plan.safety, "human-review");
    assert!(plan.review_gates.iter().any(|gate| gate == "human-review"));
    assert!(plan.patch_preview.contains("OP MARK_FUNCTION_UNSAFE"));

    let _ = fs::remove_file(path);
}

#[test]
fn project_fix_plan_reports_import_repairs() {
    let root =
        std::env::temp_dir().join(format!("forgentex_stage29_project_{}", std::process::id()));
    let src = root.join("src");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&src).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"stage29-project\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        src.join("main.fgx"),
        "module stage29.main\nimport stage29.missing\n\nexport fn hello() -> i32:\n    return 1\n",
    )
    .unwrap();

    let report = build_fix_plan(&FixPlanOptions {
        source: root.to_string_lossy().to_string(),
        target: None,
    })
    .unwrap();

    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "NAM001"));
    assert!(report
        .plans
        .iter()
        .any(|plan| plan.operation == "fix_import_name"));

    let _ = fs::remove_dir_all(root);
}
