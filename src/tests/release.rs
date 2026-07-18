use crate::release::release_check;

#[test]
fn release_check_reports_stage26_maturity_gates() {
    let report = release_check("v1.0");

    assert_eq!(report.schema, "forgentex.release_check.v1");
    assert_eq!(report.current_track, "v1.0-cfg-dataflow-borrowck");
    assert_eq!(report.decision, "v1_ready");
    assert_eq!(report.partial(), 0);
    assert_eq!(report.hardening_failed(), 0);
    assert!(report.hardening_passed() >= 5);
    assert!(report.passed() >= 15);
    assert!(report
        .gates
        .iter()
        .any(|gate| gate.id == "ownership_borrowing" && gate.status == "pass"));
    let ownership_gate = report
        .gates
        .iter()
        .find(|gate| gate.id == "ownership_borrowing")
        .unwrap();
    assert!(ownership_gate
        .evidence
        .iter()
        .any(|item| item == "fgx borrow-conformance --json"));
    assert!(ownership_gate
        .evidence
        .iter()
        .any(|item| item == "src/borrowck.rs"));
    assert!(ownership_gate
        .evidence
        .iter()
        .any(|item| item == "docs/cfg_dataflow_foundation.md"));
    assert!(ownership_gate.gaps.is_empty());
    assert!(report
        .gates
        .iter()
        .any(|gate| gate.id == "architecture_model"
            && gate
                .evidence
                .iter()
                .any(|item| item == "fgx targets --json")));
    assert!(report
        .gates
        .iter()
        .any(|gate| gate.id == "diagnostic_planning" && gate.status == "pass"));
    assert!(report
        .gates
        .iter()
        .any(|gate| gate.id == "inline_tests" && gate.status == "pass"));
    assert!(report
        .gates
        .iter()
        .any(|gate| gate.id == "graph_artifact" && gate.status == "pass"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.10" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.11" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.12" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.13" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.14" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.15" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.16" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.17" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.18" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.19" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.20" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.21" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.22" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.23" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.24" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.25" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v0.26" && label.status == "completed"));
    assert!(report
        .labels
        .iter()
        .any(|label| label.label == "v1.0" && label.status == "completed"));

    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.release_check.v1\""));
    assert!(json.contains("\"decision\": \"v1_ready\""));
    assert!(json.contains("\"hardening_fail\": 0"));
    assert!(json.contains("\"hardening_checks\": ["));
    assert!(json.contains("\"id\": \"diagnostic_shape_freeze\""));
    assert!(json.contains("\"id\": \"gate_evidence_complete\""));
    assert!(json.contains("\"id\": \"borrow_escape_hatch_conformance\""));
    assert!(json.contains("forgentex.borrow_reference.v1"));
    assert!(json.contains("reference_move_cases=510"));
    assert!(json.contains("reference_alias_cases=277"));
    assert!(json.contains("reference_lifetime_cases=132"));
    assert!(json.contains("reference_property_cases=256"));
    assert!(json.contains("persisted_regression_cases=4"));
    assert!(json.contains("reference_campaign_seed=0x736166655f763201"));
    assert!(json.contains("reference_campaign_cases=128"));
    assert!(json.contains("reference_alias_mismatches=0"));
    assert!(json.contains("reference_lifetime_mismatches=0"));
    assert!(json.contains("reference_property_mismatches=0"));
    assert!(json.contains("persisted_regression_mismatches=0"));
    assert!(json.contains("reference_campaign_mismatches=0"));
    assert!(json.contains("reference_campaign_failure_artifacts=0"));
    assert!(json.contains("reference_move_mismatches=0"));
    assert!(json.contains("loops.loop_carried_alias_rejected"));
    assert!(json.contains("branches.conditional_alias_reassign_rejected"));
    assert!(json.contains("\"id\": \"semantic_patch\""));

    let text = report.to_text();
    assert!(text.contains("Forgentex release check"));
    assert!(text.contains("Release hardening:"));
    assert!(text.contains("ownership_borrowing status=pass"));
    assert!(text.contains("v1.0 status=completed"));
}

#[test]
fn strict_v1_target_is_ready() {
    let report = release_check("v1.0");
    assert!(report.is_v1_ready());
}
