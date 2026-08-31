use crate::release::release_check;

#[test]
fn release_check_reports_stage26_maturity_gates() {
    let report = release_check("v1.0");

    assert_eq!(report.schema, "forgentex.release_check.v1");
    assert_eq!(report.current_track, "v1.0-cfg-dataflow-borrowck");
    assert_eq!(report.decision, "v1_ready");
    assert_eq!(report.partial(), 0);
    assert_eq!(report.hardening_failed(), 0);
    assert!(report
        .labels
        .iter()
        .any(|label| { label.label == "public-0.1" && label.status == "in_progress" }));
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
    assert!(json.contains("reference_move_cases=526"));
    assert!(json.contains("reference_alias_cases=610"));
    assert!(json.contains("reference_lifetime_cases=292"));
    assert!(json.contains("reference_closure_cases=38"));
    assert!(json.contains(
        "reference.lifetimes.generic_collection.vec_carrier_3_wrapper_3_alias_true_parameter"
    ));
    assert!(json.contains(
        "reference.aliases.quadruple_nested_loop.left_to_right_to_left_to_right_to_left.right_alias_then_direct"
    ));
    assert!(json.contains("reference_property_cases=256"));
    assert!(json.contains("persisted_regression_cases=8"));
    assert!(json.contains("persisted_acceptance_cases=4"));
    assert!(json.contains("reference_campaign_seed=0x736166655f763201"));
    assert!(json.contains("reference_campaign_cases=192"));
    assert!(json.contains("reference_campaign_closure_cases=32"));
    assert!(json.contains("reference_alias_mismatches=0"));
    assert!(json.contains("reference_lifetime_mismatches=0"));
    assert!(json.contains("reference_property_mismatches=0"));
    assert!(json.contains("persisted_regression_mismatches=0"));
    assert!(json.contains("persisted_acceptance_mismatches=0"));
    assert!(json.contains("reference_campaign_mismatches=0"));
    assert!(json.contains("reference_campaign_closure_mismatches=0"));
    assert!(json.contains("reference_campaign_failure_artifacts=0"));
    assert!(json.contains("reference_move_mismatches=0"));
    assert!(json.contains("loops.loop_carried_alias_rejected"));
    assert!(json.contains("branches.conditional_alias_reassign_rejected"));
    assert!(json.contains("closures.owned_capture_consumes_root"));
    assert!(json.contains("closures.capture_after_move_rejected"));
    assert!(json.contains("closures.mutable_borrow_capture_rejected"));
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
    assert!(report.is_target_ready());
}

#[test]
fn public_0_1_target_reports_explicit_release_blockers() {
    let report = release_check("0.1");

    assert_eq!(report.target, "0.1");
    assert_eq!(report.current_track, "public-0.1-stabilization");
    assert_eq!(report.decision, "public_0_1_not_ready");
    assert!(!report.is_target_ready());
    assert!(report.passed() >= 17);
    assert_eq!(report.partial(), 1);
    assert_eq!(report.planned(), 0);
    assert_eq!(report.hardening_failed(), 0);

    let profile = report
        .gates
        .iter()
        .find(|gate| gate.id == "public_0_1_profile")
        .unwrap();
    assert_eq!(profile.status, "pass");
    assert!(profile
        .evidence
        .iter()
        .any(|item| item == "docs/public_0_1_profile.md"));

    let execution = report
        .gates
        .iter()
        .find(|gate| gate.id == "public_0_1_execution_contract")
        .unwrap();
    assert_eq!(execution.status, "pass");
    assert!(execution.gaps.is_empty());
    assert!(execution
        .evidence
        .iter()
        .any(|item| item == "docs/public_0_1_execution_matrix.md"));

    let compatibility = report
        .gates
        .iter()
        .find(|gate| gate.id == "public_0_1_compatibility")
        .unwrap();
    assert_eq!(compatibility.status, "pass");
    assert!(compatibility.gaps.is_empty());
    assert!(compatibility
        .evidence
        .iter()
        .any(|item| item == "docs/public_0_1_compatibility.md"));

    for id in [
        "public_0_1_standard_foundation",
        "public_0_1_examples_evidence",
    ] {
        let gate = report.gates.iter().find(|gate| gate.id == id).unwrap();
        assert_eq!(gate.status, "pass", "{id} is now machine-checked and green");
        assert!(gate.gaps.is_empty(), "{id} must have no residual gaps");
    }

    let distribution = report
        .gates
        .iter()
        .find(|gate| gate.id == "public_0_1_distribution")
        .unwrap();
    assert_eq!(distribution.status, "partial");
    assert_eq!(
        distribution.gaps.len(),
        1,
        "distribution's only residual is the CI-runtime proof: {:?}",
        distribution.gaps
    );
    assert!(distribution.gaps[0].contains("release-artifacts CI job"));

    let alias = release_check("public-0.1");
    assert_eq!(alias.decision, "public_0_1_not_ready");
    assert_eq!(alias.current_track, "public-0.1-stabilization");
}
