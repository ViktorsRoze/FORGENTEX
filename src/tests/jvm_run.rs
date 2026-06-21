use crate::jvm_run::run;

use super::parse_fixture;

#[test]
fn java_capsule_compiles_and_runs_or_degrades() {
    let program = parse_fixture("examples/jvm/tax.fgx");
    let report = run("tax.fgx", &program, None).unwrap();

    assert_eq!(report.schema, "forgentex.jvm_run.v1");
    assert_eq!(report.capsule, "cents_with_tax");
    assert_eq!(report.entry.as_deref(), Some("OrderTotal.centsWithTax"));
    assert_eq!(report.total, 3);
    assert!(report.boundary_tokens > 0 && report.body_tokens > 0);

    // With a host JDK the fixtures are real oracles; without one the run
    // degrades gracefully. Both are a success for Forgentex.
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 3);
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
    let program = parse_fixture("examples/jvm/tax.fgx");
    let json = run("tax.fgx", &program, None).unwrap().to_json();
    assert!(json.contains("\"schema\": \"forgentex.jvm_run.v1\""));
    assert!(json.contains("\"boundary_tokens\""));
    assert!(json.contains("\"entry\""));
}

#[test]
fn java_routes_through_the_polyglot_abi() {
    let program = parse_fixture("examples/jvm/tax.fgx");
    let out = std::env::temp_dir().join(format!("forgentex_{}_jvmpoly", std::process::id()));
    let report = crate::polyglot::run("tax.fgx", &program, &out);
    let capsule = report
        .capsules
        .iter()
        .find(|c| c.name == "cents_with_tax")
        .unwrap();
    assert_eq!(capsule.backend, "jvm");
    assert!(capsule.ok);
    let _ = std::fs::remove_dir_all(&out);
}
