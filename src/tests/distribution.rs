use crate::distribution::{
    distribution_gaps, distribution_manifest, release_proof_gaps_in, CHECKSUM_ALGORITHM,
    DISTRIBUTION_SCHEMA, DIST_ARTIFACTS, DIST_HOSTS, DIST_PREREQUISITES, RELEASE_PROOF_PATH,
    VERIFIED_HOST,
};

/// A proof of the shape the gate accepts, used to prove that it does.
const FILLED_PROOF: &str = "\
run_url: https://github.com/example/forgentex/actions/runs/1234567890
commit: 0123456789abcdef0123456789abcdef01234567
host: x86_64-pc-windows-msvc
fgx: 9f2ad1c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714cae
forgentex-lsp: c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2
";

#[test]
fn distribution_is_wellformed_and_waits_only_on_a_recorded_release_run() {
    let gaps = distribution_gaps();
    // Every source-verifiable condition — manifest shape, support levels, the
    // release CI workflow, the documented install path and prerequisites —
    // passes, so what remains names only the release run still to be recorded.
    for gap in &gaps {
        assert!(
            gap.starts_with(RELEASE_PROOF_PATH),
            "unexpected distribution gap: {gap}"
        );
    }
    assert!(
        !gaps.is_empty(),
        "the proof is still pending, so the gate must say so"
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
    assert!(json.contains("\"support\""), "{json}");
}

#[test]
fn only_windows_is_declared_verified_for_0_1() {
    // Listing a host nobody has built is the promise this gate exists to stop.
    // Linux and macOS stay visible as source builds, explicitly unverified.
    let verified: Vec<&str> = DIST_HOSTS
        .iter()
        .filter(|host| host.support == "verified")
        .map(|host| host.triple)
        .collect();
    assert_eq!(verified, vec![VERIFIED_HOST]);
    for host in DIST_HOSTS {
        assert_eq!(
            host.support == "verified",
            host.ci_verified,
            "{} disagrees with itself about verification",
            host.triple
        );
        if host.support != "verified" {
            assert_eq!(host.support, "source-build-unverified", "{}", host.triple);
        }
    }
}

#[test]
fn the_release_proof_closes_the_gate_only_when_it_is_checkable() {
    // A reader must be able to open the run, see which commit it built, and
    // compare a downloaded binary's digest — so each field is checked, and a
    // placeholder counts as absent.
    assert!(
        release_proof_gaps_in(FILLED_PROOF).is_empty(),
        "{:?}",
        release_proof_gaps_in(FILLED_PROOF)
    );

    let cases = [
        ("run_url: pending", "a placeholder is not a proof"),
        (
            "run_url: https://example.com/build/7",
            "a link that is not a run",
        ),
        ("commit: 0123456", "a truncated commit"),
        ("host: x86_64-unknown-linux-gnu", "an unverified host"),
        ("fgx: not-a-digest", "a checksum that is not sha256"),
    ];
    for (line, why) in cases {
        let field = line.split(':').next().unwrap();
        let doctored: String = FILLED_PROOF
            .lines()
            .map(|existing| {
                if existing.starts_with(&format!("{field}:")) {
                    line
                } else {
                    existing
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let gaps = release_proof_gaps_in(&doctored);
        assert!(!gaps.is_empty(), "{why} must keep the gate open: {line}");
        assert!(
            gaps.iter().all(|gap| gap.starts_with(RELEASE_PROOF_PATH)),
            "a gap should name the file to fix: {gaps:?}"
        );
    }

    // The committed proof must at least have the shape, whatever is pending.
    let committed = include_str!("../../docs/public_0_1_release_proof.md");
    assert!(
        committed.contains("host: x86_64-pc-windows-msvc"),
        "{committed}"
    );
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
