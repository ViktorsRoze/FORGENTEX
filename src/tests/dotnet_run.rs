use crate::dotnet_run::run;

use super::parse_fixture;

#[test]
fn csharp_capsule_builds_and_runs_or_degrades() {
    let program = parse_fixture("examples/dotnet/discount.fgx");
    let report = run("discount.fgx", &program, None).unwrap();

    assert_eq!(report.schema, "forgentex.dotnet_run.v1");
    assert_eq!(report.capsule, "apply_discount");
    assert_eq!(report.entry.as_deref(), Some("Pricing.ApplyDiscount"));
    assert_eq!(report.total, 3);
    assert!(report.boundary_tokens > 0 && report.body_tokens > 0);

    // With a host .NET SDK the fixtures are real oracles; without one the run
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
    let program = parse_fixture("examples/dotnet/discount.fgx");
    let json = run("discount.fgx", &program, None).unwrap().to_json();
    assert!(json.contains("\"schema\": \"forgentex.dotnet_run.v1\""));
    assert!(json.contains("\"boundary_tokens\""));
    assert!(json.contains("\"entry\""));
}
