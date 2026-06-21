use crate::python_run::run;

use super::parse_fixture;

#[test]
fn celsius_capsule_runs_on_real_python_or_degrades() {
    let program = parse_fixture("examples/capsule/celsius_python.fgx");
    let report = run("celsius_python.fgx", &program, None).unwrap();

    assert_eq!(report.schema, "forgentex.python_run.v1");
    assert_eq!(report.capsule, "to_fahrenheit");
    assert_eq!(report.entry.as_deref(), Some("to_fahrenheit"));
    assert_eq!(report.total, 3);
    assert!(report.boundary_tokens > 0 && report.body_tokens > 0);

    // With a host python the fixtures are real oracles; without one the run
    // degrades gracefully. Both are a success for Forgentex.
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 3, "all fixtures should pass on real python");
        assert!(report.runtime.is_some());
        for case in &report.cases {
            assert!(case.pass, "fixture failed: {case:?}");
        }
    } else {
        assert_eq!(report.status, "runtime_unavailable");
    }
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/celsius_python.fgx");
    let json = run("celsius_python.fgx", &program, None).unwrap().to_json();
    assert!(json.contains("\"schema\": \"forgentex.python_run.v1\""));
    assert!(json.contains("\"boundary_tokens\""));
    assert!(json.contains("\"entry\""));
}

#[test]
fn boundary_is_cheaper_than_body() {
    // The agent-visible surface (boundary card) should never cost more than
    // the verbatim body — the whole point of the special relationship.
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = run("normalize_python.fgx", &program, None).unwrap();
    assert!(report.boundary_tokens <= report.body_tokens);
}
