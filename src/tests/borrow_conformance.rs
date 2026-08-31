use std::collections::BTreeSet;

use crate::borrow_conformance::{run_borrow_conformance, run_borrow_conformance_with_campaign};

#[test]
fn borrow_conformance_suite_covers_adversarial_and_reference_categories() {
    let report = run_borrow_conformance();

    assert_eq!(report.schema, "forgentex.borrow_conformance.v1");
    assert_eq!(report.status, "pass");
    assert_eq!(report.failed, 0);
    assert_eq!(report.total, report.passed);
    assert!(report.total >= 1962);
    assert_eq!(
        report.reference_model.schema,
        "forgentex.borrow_reference.v1"
    );
    assert_eq!(report.reference_model.generated, 1922);
    assert_eq!(report.reference_model.move_generated, 526);
    assert_eq!(report.reference_model.alias_generated, 610);
    assert_eq!(report.reference_model.lifetime_generated, 292);
    assert_eq!(report.reference_model.closure_generated, 38);
    assert_eq!(report.reference_model.property_generated, 256);
    assert_eq!(report.reference_model.persisted_regression_generated, 8);
    assert_eq!(report.reference_model.persisted_acceptance_generated, 4);
    assert_eq!(report.reference_model.campaign_seed, "0x736166655f763201");
    assert_eq!(report.reference_model.campaign_cases_per_family, 32);
    assert_eq!(report.reference_model.campaign_generated, 192);
    assert_eq!(report.reference_model.campaign_closure_generated, 32);
    assert_eq!(report.reference_model.matched, 1922);
    assert_eq!(report.reference_model.mismatched, 0);
    assert_eq!(report.reference_model.move_mismatched, 0);
    assert_eq!(report.reference_model.alias_mismatched, 0);
    assert_eq!(report.reference_model.lifetime_mismatched, 0);
    assert_eq!(report.reference_model.closure_mismatched, 0);
    assert_eq!(report.reference_model.property_mismatched, 0);
    assert_eq!(report.reference_model.persisted_regression_mismatched, 0);
    assert_eq!(report.reference_model.persisted_acceptance_mismatched, 0);
    assert_eq!(report.reference_model.campaign_mismatched, 0);
    assert_eq!(report.reference_model.campaign_closure_mismatched, 0);
    assert!(report.reference_model.campaign_failure_artifacts.is_empty());

    let categories = report
        .cases
        .iter()
        .map(|case| case.category.as_str())
        .collect::<BTreeSet<_>>();
    for category in [
        "moves",
        "borrows",
        "reborrows",
        "loops",
        "branches",
        "closures",
        "lifetimes",
        "projections",
        "contracts",
        "raw",
        "persisted_regressions",
        "persisted_acceptance_regressions",
        "reference_moves",
        "reference_aliases",
        "reference_lifetimes",
        "reference_closures",
        "reference_properties",
        "reference_campaign",
    ] {
        assert!(categories.contains(category), "missing category {category}");
    }

    for id in [
        "moves.use_after_move",
        "borrows.mutable_call_alias_rejected",
        "borrows.parent_before_child_rejected",
        "loops.loop_carried_alias_rejected",
        "loops.loop_local_alias_ok",
        "branches.conditional_alias_reassign_rejected",
        "branches.conditional_alias_call_rejected",
        "branches.conditional_alias_last_use_ok",
        "closures.copy_capture_remains_available",
        "closures.owned_capture_consumes_root",
        "closures.owned_field_capture_consumes_root",
        "closures.capture_after_move_rejected",
        "closures.parameter_shadow_does_not_capture",
        "closures.owned_capture_used_through_closure",
        "closures.immutable_borrow_capture_ok",
        "closures.mutable_borrow_capture_rejected",
        "closures.mutable_alias_capture_rejected",
        "closures.mutable_parameter_shadow_does_not_capture",
        "lifetimes.conditional_return_ok",
        "lifetimes.unproven_conditional_arg_rejected",
        "projections.disjoint_fields_ok",
        "projections.constant_indices_disjoint_ok",
        "projections.unknown_index_overlap_rejected",
        "contracts.report_wrapper_mutates",
        "raw.raw_pointer_param_safe_fn_rejected",
        "raw.raw_pointer_deref_safe_fn_rejected",
        "reference.moves.linear.move_left_read_right",
        "reference.moves.branch.move_left_read_left_read_left",
        "reference.moves.loop.move_right_read_left",
        "reference.moves.associated.copy_depth_4_alias_true",
        "reference.moves.associated.owned_depth_3_alias_false",
        "reference.aliases.linear.write_child_write_parent",
        "reference.aliases.linear.write_parent_write_child",
        "reference.aliases.projection.left_right.write_first_write_second",
        "reference.aliases.projection.left_left.write_first_write_second",
        "reference.aliases.branch.left_to_right.right_direct_then_alias",
        "reference.aliases.deep_projection.branch_left_branch_left.write_first_write_second",
        "reference.aliases.deep_projection.branch_left_branch_right.write_first_write_second",
        "reference.aliases.loop.left_left.alias_then_direct",
        "reference.aliases.loop.left_right.direct_then_alias",
        "reference.aliases.index.constant_0_constant_1",
        "reference.aliases.index.dynamic_i_dynamic_j",
        "reference.aliases.range.constant_0_2_constant_2_4",
        "reference.aliases.range.constant_0_2_constant_1_3",
        "reference.aliases.range.dynamic_0_i_dynamic_i_4",
        "reference.aliases.range_index.constant_2_4_constant_0",
        "reference.aliases.range_index.constant_0_2_constant_1",
        "reference.aliases.range_index.dynamic_i_j_dynamic_i",
        "reference.aliases.branch_in_loop.left_to_right.left_alias_then_direct",
        "reference.aliases.branch_in_loop.left_to_left.right_direct_then_alias",
        "reference.aliases.nested_loop.left_to_right_to_left.right_alias_then_direct",
        "reference.aliases.nested_loop.left_to_left_to_left.right_direct_then_alias",
        "reference.aliases.triple_nested_loop.left_to_right_to_left_to_right.right_alias_then_direct",
        "reference.aliases.triple_nested_loop.left_to_left_to_left_to_left.right_direct_then_alias",
        "reference.aliases.quadruple_nested_loop.left_to_right_to_left_to_right_to_left.right_alias_then_direct",
        "reference.aliases.quadruple_nested_loop.left_to_left_to_left_to_left_to_left.right_direct_then_alias",
        "reference.aliases.generic.left_left_depth_3",
        "reference.aliases.generic.left_right_depth_2",
        "reference.aliases.associated.left_left_depth_4",
        "reference.aliases.associated.left_right_depth_3",
        "reference.lifetimes.direct.parameter_aliases_2",
        "reference.lifetimes.conditional.parameter_unproven",
        "reference.lifetimes.call.returns_left_parameter_unproven_helper_alias_true_caller_alias_true",
        "reference.lifetimes.call.returns_both_parameter_unproven_helper_alias_false_caller_alias_false",
        "reference.lifetimes.match.parameter_unproven",
        "reference.lifetimes.loop.for_parameter_parameter",
        "reference.lifetimes.loop.while_unproven_parameter",
        "reference.lifetimes.wrapper.returns_left_parameter_unproven_depth_3",
        "reference.lifetimes.projection.direct.field_aliases_2",
        "reference.lifetimes.projection.call.index_unselected",
        "reference.lifetimes.deep_projection.wrapper.field_unselected_depth_4",
        "reference.lifetimes.deep_projection.wrapper.index_selected_depth_4",
        "reference.lifetimes.deep_projection.recursive.field_self_projection_first_alias_true",
        "reference.lifetimes.deep_projection.recursive.index_mutual_projection_second_alias_false",
        "reference.lifetimes.recursive.self.base_parameter_recursive_parameter_alias_false",
        "reference.lifetimes.recursive.mutual.unproven_alias_true",
        "reference.lifetimes.collection.vec_depth_3_alias_true_parameter",
        "reference.lifetimes.collection.array_depth_2_alias_false_unproven",
        "reference.lifetimes.generic.depth_4_alias_true_parameter",
        "reference.lifetimes.generic.depth_3_alias_false_unproven",
        "reference.lifetimes.generic_collection.vec_carrier_3_wrapper_3_alias_true_parameter",
        "reference.lifetimes.generic_collection.array_carrier_2_wrapper_3_alias_false_unproven",
        "reference.lifetimes.associated.depth_4_alias_true_parameter",
        "reference.lifetimes.associated.depth_3_alias_false_unproven",
        "reference.closures.affine.call",
        "reference.closures.affine.move_call",
        "reference.closures.capture.mutable_alias_true_outer_use_true",
        "reference.closures.capture.shared_alias_true_outer_use_true",
        "reference.properties.moves.000",
        "reference.properties.aliases.000",
        "reference.properties.projections.000",
        "reference.properties.lifetimes.000",
        "regression.move_after_whole_move",
        "regression.loop_parent_child_conflict",
        "regression.unknown_index_overlap",
        "regression.field_return_escape",
        "regression.accept.field_sibling_after_move",
        "regression.accept.loop_local_alias_release",
        "regression.accept.constant_indices_disjoint",
        "regression.accept.affine_reinitialize_after_call",
        "reference.campaign.v1.736166655f763201.moves.0000",
        "reference.campaign.v1.736166655f763201.aliases.0000",
        "reference.campaign.v1.736166655f763201.projections.0000",
        "reference.campaign.v1.736166655f763201.nested_loops.0000",
        "reference.campaign.v1.736166655f763201.lifetimes.0000",
        "reference.campaign.v1.736166655f763201.closures.0000",
    ] {
        assert!(
            report.cases.iter().any(|case| case.id == id && case.passed),
            "missing passing conformance case {id}"
        );
    }
}

#[test]
fn borrow_conformance_json_is_stable_for_agents() {
    let report = run_borrow_conformance();
    let json = report.to_json();

    assert!(json.contains("\"schema\": \"forgentex.borrow_conformance.v1\""));
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"failed\": 0"));
    assert!(json.contains("\"schema\": \"forgentex.borrow_reference.v1\""));
    assert!(json.contains("\"generated\": 1922"));
    assert!(json.contains("\"move_generated\": 526"));
    assert!(json.contains("\"alias_generated\": 610"));
    assert!(json.contains("\"lifetime_generated\": 292"));
    assert!(json.contains("\"closure_generated\": 38"));
    assert!(json.contains("\"property_generated\": 256"));
    assert!(json.contains("\"persisted_regression_generated\": 8"));
    assert!(json.contains("\"persisted_acceptance_generated\": 4"));
    assert!(json.contains("\"campaign_seed\": \"0x736166655f763201\""));
    assert!(json.contains("\"campaign_cases_per_family\": 32"));
    assert!(json.contains("\"campaign_generated\": 192"));
    assert!(json.contains("\"campaign_closure_generated\": 32"));
    assert!(json.contains("\"mismatched\": 0"));
    assert!(json.contains("\"move_mismatched\": 0"));
    assert!(json.contains("\"alias_mismatched\": 0"));
    assert!(json.contains("\"lifetime_mismatched\": 0"));
    assert!(json.contains("\"closure_mismatched\": 0"));
    assert!(json.contains("\"property_mismatched\": 0"));
    assert!(json.contains("\"persisted_regression_mismatched\": 0"));
    assert!(json.contains("\"persisted_acceptance_mismatched\": 0"));
    assert!(json.contains("\"campaign_mismatched\": 0"));
    assert!(json.contains("\"campaign_closure_mismatched\": 0"));
    assert!(json.contains("\"campaign_failure_artifacts\": ["));
    assert!(json.contains("\"id\": \"loops.loop_carried_alias_rejected\""));
    assert!(json.contains("\"id\": \"branches.conditional_alias_reassign_rejected\""));
    assert!(json.contains("\"id\": \"lifetimes.conditional_return_ok\""));
    assert!(json.contains("\"id\": \"lifetimes.unproven_conditional_arg_rejected\""));
    assert!(json.contains("\"id\": \"contracts.report_choose_returns\""));
    assert!(json.contains("\"id\": \"raw.raw_pointer_param_safe_fn_rejected\""));
    assert!(json.contains("\"expected_returns\": [\"xs\", \"ys\"]"));
    assert!(json.contains("\"found_mutates\": [\"xs\"]"));

    let text = report.to_text();
    assert!(text.contains("Forgentex borrow conformance suite"));
    assert!(text.contains("status=pass"));
    assert!(text.contains("reference_schema=forgentex.borrow_reference.v1"));
    assert!(text.contains("generated=1922 matched=1922 mismatched=0"));
    assert!(
        text.contains("persisted_regressions=8 mismatched=0 acceptance=4 acceptance_mismatched=0")
    );
    assert!(text.contains("campaign_seed=0x736166655f763201"));
    assert!(text.contains("branches.conditional_alias_call_rejected"));
    assert!(text.contains("contracts.report_wrapper_mutates"));
}

#[test]
fn borrow_conformance_accepts_a_bounded_runtime_campaign_seed() {
    let report = run_borrow_conformance_with_campaign(7, 1);
    assert!(report.is_success());
    assert_eq!(report.reference_model.campaign_seed, "0x0000000000000007");
    assert_eq!(report.reference_model.campaign_cases_per_family, 1);
    assert_eq!(report.reference_model.campaign_generated, 6);
    assert_eq!(report.reference_model.campaign_closure_generated, 1);
    assert_eq!(report.reference_model.campaign_mismatched, 0);
    assert!(report
        .cases
        .iter()
        .any(|case| case.id == "reference.campaign.v1.0000000000000007.moves.0000"));
    assert!(report
        .cases
        .iter()
        .any(|case| case.id == "reference.campaign.v1.0000000000000007.closures.0000"));
}

#[test]
fn scheduled_ci_runs_multiple_replayable_borrow_campaign_seeds() {
    let workflow = include_str!("../../.github/workflows/ci.yml");
    for seed in [
        "0x736166655f763201",
        "0x243f6a8885a308d3",
        "0x13198a2e03707344",
        "0xa4093822299f31d0",
        "0x082efa98ec4e6c89",
        "0x452821e638d01377",
        "0xbe5466cf34e90c6c",
        "0xc0ac29b7c97c50dd",
    ] {
        assert!(workflow.contains(seed), "missing scheduled seed {seed}");
    }
    assert!(workflow.contains("--campaign-cases 96"));
    assert!(workflow.contains("--failure-dir $artifactDirectory"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("target/borrow-campaign-failures"));
}
