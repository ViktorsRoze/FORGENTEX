use std::fs;

use crate::package::{
    build_package_matrix, create_package, generate_lock, publish_dry_run, run_package_tests,
};
use crate::project::{check_project, load_project};

use super::manifest_path;

#[test]
fn local_path_dependencies_are_loaded_and_locked() {
    let project_root = manifest_path("examples/packages/codec_app");
    let project = load_project(&project_root).unwrap();
    assert_eq!(project.manifest.name, "codec-app");
    assert_eq!(project.manifest.dependencies.len(), 1);
    assert!(project
        .modules
        .iter()
        .any(|module| module.package == "codec-common" && module.dependency));
    assert!(check_project(&project_root).unwrap().is_empty());

    let lock = generate_lock(&project_root).unwrap();
    assert_eq!(lock.schema, "forgentex.lock.v1");
    assert_eq!(lock.packages.len(), 2);
    let lock_json = lock.to_json();
    assert!(lock_json.contains("\"name\": \"codec-app\""));
    assert!(lock_json.contains("\"name\": \"codec-common\""));
    assert!(lock_json.contains("riscv64gc-unknown-linux-gnu"));
}

#[test]
fn package_test_and_build_matrix_cover_stage23_targets() {
    let project_root = manifest_path("examples/packages/codec_app");
    let report = run_package_tests(&project_root, None, true).unwrap();
    assert!(report.is_success(), "{}", report.to_text());
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.package_test.v1\""));
    for package in &report.packages {
        assert_eq!(package.targets.len(), 3);
        assert!(package
            .targets
            .iter()
            .any(|target| target.triple == "x86_64-unknown-linux-gnu"));
        assert!(package
            .targets
            .iter()
            .any(|target| target.triple == "aarch64-unknown-linux-gnu"));
        assert!(package
            .targets
            .iter()
            .any(|target| target.triple == "riscv64gc-unknown-linux-gnu"));
    }

    let out_dir =
        std::env::temp_dir().join(format!("forgentex_stage23_build_{}", std::process::id()));
    let build = build_package_matrix(
        &project_root,
        "c",
        &out_dir,
        "dev",
        Some("aarch64-unknown-linux-gnu"),
        false,
    )
    .unwrap();
    assert_eq!(build.schema, "forgentex.package_build.v1");
    assert_eq!(build.targets.len(), 1);
    assert!(build.targets[0].files.iter().all(|path| path.exists()));
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn package_new_and_publish_dry_run_are_available() {
    let package_dir =
        std::env::temp_dir().join(format!("forgentex_stage23_new_{}", std::process::id()));
    let _ = fs::remove_dir_all(&package_dir);
    let created = create_package(&package_dir, Some("stage23-demo")).unwrap();
    assert!(created.manifest.exists());
    assert!(created.source.exists());
    assert!(check_project(&package_dir).unwrap().is_empty());
    let manifest = fs::read_to_string(&created.manifest).unwrap();
    assert!(manifest.contains("[targets]"));
    assert!(manifest.contains("[profile.dev]"));
    let _ = fs::remove_dir_all(&package_dir);

    let publish = publish_dry_run(&manifest_path("examples/packages/codec_app")).unwrap();
    assert_eq!(publish.schema, "forgentex.publish_dry_run.v1");
    assert!(publish.ready);
    assert!(publish.to_json().contains("registry_upload=deferred"));
    assert!(publish.to_json().contains("package_verify=pass"));
}
