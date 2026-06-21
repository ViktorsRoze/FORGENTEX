use crate::ast::CapsuleDep;
use crate::capsule_lock::{audit, lock, parse_dependency};
use crate::parser::parse_human;

use super::parse_fixture;

fn dep(source: &str) -> crate::capsule_lock::DependencyRef {
    parse_dependency(&CapsuleDep {
        source: source.to_string(),
    })
}

#[test]
fn parses_pypi_with_checksum() {
    let parsed = dep("pypi:numpy==2.2.0#sha256:abc");
    assert_eq!(parsed.ecosystem, "pypi");
    assert_eq!(parsed.name, "numpy");
    assert_eq!(parsed.version.as_deref(), Some("2.2.0"));
    assert_eq!(parsed.checksum_algo.as_deref(), Some("sha256"));
    assert_eq!(parsed.checksum.as_deref(), Some("abc"));
    assert!(parsed.pinned);
    assert_eq!(parsed.risk(), "low");
}

#[test]
fn parses_github_commit_revision() {
    let parsed = dep("github:org/repo@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2");
    assert_eq!(parsed.ecosystem, "github");
    assert_eq!(parsed.name, "org/repo");
    assert!(parsed.version.is_none());
    assert!(parsed.revision.is_some());
    assert!(parsed.pinned);
    // commit-pinned but no checksum -> medium
    assert_eq!(parsed.risk(), "medium");
}

#[test]
fn version_only_is_medium_and_floating_is_high() {
    assert_eq!(dep("pypi:pandas==2.2.3").risk(), "medium");
    assert_eq!(dep("pypi:requests").risk(), "high");
    assert_eq!(dep("github:org/repo@main").risk(), "high");
}

#[test]
fn lock_lists_dependencies_with_required_by() {
    let program = parse_fixture("examples/capsule/stats_python.fgx");
    let report = lock("stats_python.fgx", &program);
    assert_eq!(report.dependencies.len(), 2);
    // Sorted deterministically by raw reference (numpy < pandas).
    assert!(report.dependencies[0].0.name == "numpy");
    assert_eq!(report.dependencies[0].1, vec!["stats".to_string()]);
    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_lock.v1\""));
    assert!(json.contains("\"ecosystem\": \"pypi\""));
}

#[test]
fn audit_status_is_worst_dependency_risk() {
    // numpy (low) + pandas (medium) -> overall medium.
    let program = parse_fixture("examples/capsule/stats_python.fgx");
    let report = audit("stats_python.fgx", &program);
    assert_eq!(report.status, "medium");
    assert!(!report.has_high_risk());
    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_audit.v1\""));
    assert!(json.contains("\"risk\": \"low\""));
    assert!(json.contains("\"risk\": \"medium\""));
}

#[test]
fn audit_flags_floating_dependency_as_high() {
    let program = parse_human(
        "\
foreign f python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    dep \"pypi:requests\"
    code:
        x = 1
",
    )
    .unwrap();
    let report = audit("floating.fgx", &program);
    assert_eq!(report.status, "high");
    assert!(report.has_high_risk());
}

#[test]
fn duplicate_dependency_collapses_with_combined_required_by() {
    let program = parse_human(
        "\
foreign a python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    dep \"pypi:numpy==2.2.0#sha256:abc\"
    code:
        x = 1

foreign b python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    dep \"pypi:numpy==2.2.0#sha256:abc\"
    code:
        y = 2
",
    )
    .unwrap();
    let report = lock("dup.fgx", &program);
    assert_eq!(report.dependencies.len(), 1);
    assert_eq!(
        report.dependencies[0].1,
        vec!["a".to_string(), "b".to_string()]
    );
}
