use std::fs;

use crate::package::{build_package_matrix, create_package, run_package_tests};
use crate::security::{audit_project, verify_project};

use super::manifest_path;

#[test]
fn audit_and_verify_accept_signed_untrusted_packages() {
    let project_root = manifest_path("examples/packages/codec_app");
    let audit = audit_project(&project_root).unwrap();
    assert_eq!(audit.schema, "forgentex.audit.v1");
    assert!(audit.is_success(), "{}", audit.to_text());
    let audit_json = audit.to_json();
    assert!(audit_json.contains("\"status\": \"pass\""));
    assert!(audit_json.contains("\"trust\": \"untrusted\""));
    assert!(audit_json.contains("\"signature_status\": \"mock-accepted\""));
    assert!(audit_json.contains("fs.read"));
    assert!(audit_json.contains("build_scripts=deny"));

    let verify = verify_project(&project_root).unwrap();
    assert_eq!(verify.schema, "forgentex.verify.v1");
    assert!(verify.is_success(), "{}", verify.to_text());
    let verify_json = verify.to_json();
    assert!(verify_json.contains("\"signature_status\": \"mock-accepted\""));
    assert!(verify_json.contains("\"reproducible\": true"));
    assert!(verify_json.contains("\"reproducible_hash\":"));
}

#[test]
fn untrusted_package_actions_require_declared_permissions() {
    let package_dir =
        std::env::temp_dir().join(format!("forgentex_stage24_blocked_{}", std::process::id()));
    let _ = fs::remove_dir_all(&package_dir);
    let created = create_package(&package_dir, Some("stage24-blocked")).unwrap();
    fs::write(
        &created.manifest,
        "[package]\nname = \"stage24-blocked\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[security]\ntrust = \"untrusted\"\nsigned_by = \"agent-public-demo\"\nsignature_algorithm = \"Ed25519\"\nsignature = \"mock-sig-stage24-blocked\"\nreproducible = true\nbuild_script = \"scripts/build.ps1\"\n\n[permissions]\nfs.read = [\"src\"]\n\n[sandbox]\nbuild_scripts = \"sandboxed\"\ncodegen = \"deny\"\ntests = \"sandboxed\"\nagent_tools = \"deny\"\n",
    )
    .unwrap();

    let audit = audit_project(&package_dir).unwrap();
    assert_eq!(audit.status, "fail");
    let audit_json = audit.to_json();
    assert!(audit_json.contains("\"code\": \"undeclared_permission\""));
    assert!(audit_json.contains("\"required_permission\": \"build.scripts\""));

    let test_error = run_package_tests(&package_dir, None, false).unwrap_err();
    assert!(test_error.contains("supply-chain audit failed"));

    let build_dir = std::env::temp_dir().join(format!(
        "forgentex_stage24_blocked_build_{}",
        std::process::id()
    ));
    let build_error = build_package_matrix(
        &package_dir,
        "c",
        &build_dir,
        "dev",
        Some("x86_64-unknown-linux-gnu"),
        false,
    )
    .unwrap_err();
    assert!(build_error.contains("supply-chain audit failed"));

    let _ = fs::remove_dir_all(package_dir);
    let _ = fs::remove_dir_all(build_dir);
}

#[test]
fn untrusted_effects_require_permission_manifest_entries() {
    let package_dir =
        std::env::temp_dir().join(format!("forgentex_stage24_effects_{}", std::process::id()));
    let _ = fs::remove_dir_all(&package_dir);
    let created = create_package(&package_dir, Some("stage24-effects")).unwrap();
    fs::write(
        &created.manifest,
        "[package]\nname = \"stage24-effects\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[security]\ntrust = \"untrusted\"\nsigned_by = \"agent-public-demo\"\nsignature_algorithm = \"Ed25519\"\nsignature = \"mock-sig-stage24-effects\"\nreproducible = true\n\n[permissions]\ncrypto = [\"verify\"]\n\n[sandbox]\nbuild_scripts = \"deny\"\ncodegen = \"deny\"\ntests = \"sandboxed\"\nagent_tools = \"deny\"\n",
    )
    .unwrap();
    fs::write(
        &created.source,
        "module stage24_effects\n\nexport fn read_demo(path: str) -> str effects [fs.read]:\n    return core_fs_read_text(path)\n",
    )
    .unwrap();

    let audit = audit_project(&package_dir).unwrap();
    assert_eq!(audit.status, "fail");
    assert!(audit
        .to_json()
        .contains("\"code\": \"missing_effect_permission\""));
    assert!(audit.to_json().contains("\"required_permission\": \"fs\""));

    let _ = fs::remove_dir_all(package_dir);
}
