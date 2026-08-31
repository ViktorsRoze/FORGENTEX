use crate::distribution::{
    distribution_gaps, distribution_manifest, CHECKSUM_ALGORITHM, DISTRIBUTION_SCHEMA,
    DIST_ARTIFACTS, DIST_HOSTS, DIST_PREREQUISITES,
};

#[test]
fn distribution_manifest_is_wellformed_with_a_single_operational_residual() {
    let gaps = distribution_gaps();
    // Every source-verifiable condition (manifest shape, release CI workflow,
    // documented install path and prerequisites) passes, so the only remaining
    // gap is the CI-runtime proof that cannot be asserted from a checkout.
    assert_eq!(gaps.len(), 1, "unexpected distribution gaps: {gaps:?}");
    assert!(
        gaps[0].contains("release-artifacts CI job"),
        "residual should name the CI proof: {}",
        gaps[0]
    );

    let manifest = distribution_manifest();
    assert_eq!(manifest.schema, DISTRIBUTION_SCHEMA);
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.checksum_algorithm, CHECKSUM_ALGORITHM);
    assert!(!manifest.artifacts.is_empty());
    assert!(manifest.hosts.iter().any(|host| host.primary));
    assert!(manifest.hosts.iter().any(|host| host.ci_verified));
    for artifact in DIST_ARTIFACTS {
        assert!(!artifact.install_path_unix.is_empty());
        assert!(!artifact.install_path_windows.is_empty());
    }
}

#[test]
fn distribution_manifest_json_carries_schema_and_shape() {
    let json = distribution_manifest().to_json();
    assert!(json.contains("\"schema\": \"forgentex.distribution.v1\""));
    for field in [
        "version",
        "checksum_algorithm",
        "artifacts",
        "hosts",
        "backends",
        "prerequisites",
        "status",
        "gaps",
    ] {
        assert!(json.contains(&format!("\"{field}\"")), "{json}");
    }
    // The manifest reports its own honest status.
    assert!(json.contains("\"status\": \"partial\""), "{json}");
}

#[test]
fn release_ci_workflow_declares_build_checksum_and_clean_install() {
    // These are the exact markers distribution_gaps() requires; asserting them
    // directly guards the workflow against silent regressions.
    let workflow = include_str!("../../.github/workflows/ci.yml");
    for marker in [
        "release-artifacts",
        "cargo build --release",
        "CHECKSUMS.txt",
        "clean install",
    ] {
        assert!(
            workflow.contains(marker),
            "release CI workflow is missing marker: {marker}"
        );
    }
}

#[test]
fn installation_docs_cover_every_prerequisite() {
    let readme = include_str!("../../README.md");
    assert!(readme.contains("## Installation"));
    for prerequisite in DIST_PREREQUISITES {
        assert!(
            readme.contains(prerequisite.name),
            "README does not document prerequisite {}",
            prerequisite.name
        );
    }
    assert!(!DIST_HOSTS.is_empty());
}
