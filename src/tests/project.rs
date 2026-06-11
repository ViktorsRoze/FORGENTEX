use std::fs;
use std::path::{Path, PathBuf};

use crate::project::{check_project, generate_project_map, public_api};

use super::manifest_path;

#[test]
fn manifest_backed_media_project_checks_as_one_program() {
    let diagnostics = check_project(&manifest_path(".")).unwrap();
    assert_eq!(diagnostics, Vec::new());
}

#[test]
fn public_api_summary_lists_exported_symbols() {
    let api = public_api(&manifest_path(".")).unwrap();
    assert!(api.contains("API forgentex-demo-media 0.1.0"));
    assert!(api.contains("M media.audio examples/project/media/audio.fgx"));
    assert!(api.contains("fn mix_samples @function.mix_samples"));
    assert!(api.contains("struct H264NalHeader @struct.H264NalHeader"));
    assert!(!api.contains("module media"));
}

#[test]
fn project_map_reports_modules_imports_exports_and_hashes() {
    let map = generate_project_map(&manifest_path(".")).unwrap();
    assert!(map.contains("P forgentex-demo-media 0.1.0 root=examples/project"));
    assert!(map.contains(
        "M media.audio examples/project/media/audio.fgx package=forgentex-demo-media imports=core.math,core.slice,media.types"
    ));
    assert!(map.contains("M core.math "));
    assert!(map.contains("package=forgentex-stdlib imports=- exports=5"));
    assert!(map.contains("source=stdlib"));
    assert!(map.contains("F mix_samples @function.mix_samples public params=4"));
    assert!(map.contains("hash="));
}

#[test]
fn project_diagnostics_report_missing_imports_and_cycles() {
    let root = temp_project_dir("bad_graph");
    write_file(
        &root.join("Forgentex.toml"),
        "[package]\nname = \"bad-graph\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    );
    write_file(
        &root.join("src/a.fgx"),
        "module a\nimport b\nexport fn a() -> i32:\n    return b()\n",
    );
    write_file(
        &root.join("src/b.fgx"),
        "module b\nimport a\nimport missing\nexport fn b() -> i32:\n    return 1\n",
    );

    let diagnostics = check_project(&root).unwrap();
    let codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"import_cycle"));
    assert!(codes.contains(&"unresolved_import"));

    let _ = fs::remove_dir_all(root);
}

fn temp_project_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("forgentex_{}_{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    root
}

fn write_file(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, source).unwrap();
}
