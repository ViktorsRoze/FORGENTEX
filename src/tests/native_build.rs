use std::fs;
use std::path::PathBuf;

use crate::native_build::{build, ArtifactKind};

use super::manifest_path;

fn out_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_native_{tag}", std::process::id()))
}

#[test]
fn native_build_collects_facts_and_honors_exact() {
    let out = out_dir("facts");
    let _ = fs::remove_dir_all(&out);
    let report = build(
        &manifest_path("examples/native"),
        &out,
        ArtifactKind::Object,
    );

    assert_eq!(report.schema, "forgentex.native_build.v1");
    assert_eq!(report.functions, 4);
    for name in ["clamp_i32", "mix_avg_i32", "scale_q15"] {
        assert!(
            report.exact_functions.iter().any(|f| f == name),
            "expected @exact function {name}, got {:?}",
            report.exact_functions
        );
    }
    assert!(!report.c_source_hash.is_empty());

    // Either the host compiled a real artifact, or no compiler is present.
    // Both are a Forgentex success.
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "built" {
        let path = report.artifact_path.as_ref().expect("artifact path");
        assert!(
            std::path::Path::new(path).exists(),
            "artifact missing: {path}"
        );
        assert!(report.artifact_bytes.unwrap_or(0) > 0);
        assert!(report.toolchain.is_some());
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn native_build_c_source_hash_is_deterministic() {
    let a = build(
        &manifest_path("examples/native"),
        &out_dir("d1"),
        ArtifactKind::Object,
    );
    let b = build(
        &manifest_path("examples/native"),
        &out_dir("d2"),
        ArtifactKind::Object,
    );
    assert_eq!(a.c_source_hash, b.c_source_hash);
    assert_eq!(a.c_header_hash, b.c_header_hash);
    let _ = fs::remove_dir_all(out_dir("d1"));
    let _ = fs::remove_dir_all(out_dir("d2"));
}

#[test]
fn native_build_json_is_well_formed() {
    let report = build(
        &manifest_path("examples/native"),
        &out_dir("json"),
        ArtifactKind::Object,
    );
    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.native_build.v1\""));
    assert!(json.contains("\"exact_functions\""));
    assert!(json.contains("\"c_source_hash\""));
    let _ = fs::remove_dir_all(out_dir("json"));
}

#[test]
fn artifact_kind_parses_known_forms() {
    assert_eq!(ArtifactKind::parse("obj"), Some(ArtifactKind::Object));
    assert_eq!(ArtifactKind::parse("object"), Some(ArtifactKind::Object));
    assert_eq!(ArtifactKind::parse("shared"), Some(ArtifactKind::Shared));
    assert!(ArtifactKind::parse("exe").is_none());
}
