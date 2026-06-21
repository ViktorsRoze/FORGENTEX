use crate::capsule_workbench::{context, inventory};

use super::parse_fixture;

#[test]
fn inventory_lists_all_capsules_with_decisions() {
    let program = parse_fixture("examples/capsule/web_session.fgx");
    let report = inventory("web_session.fgx", &program);
    assert_eq!(report.capsules.len(), 2);
    assert!(report.capsules.iter().all(|c| c.decision == "allow"));
    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_inventory.v1\""));
    assert!(json.contains("\"name\": \"backend\""));
}

#[test]
fn managed_capsule_context_aggregates_to_allow() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let ctx = context("normalize_python.fgx", &program, "normalize_py").unwrap();
    assert_eq!(ctx.risk_class, "managed");
    assert_eq!(ctx.overall_decision, "allow");
    assert!(ctx.class_review.is_none());
}

#[test]
fn gpu_capsule_context_aggregates_native_review() {
    let program = parse_fixture("examples/capsule/saxpy_cuda.fgx");
    let ctx = context("saxpy_cuda.fgx", &program, "saxpy").unwrap();
    assert_eq!(ctx.risk_class, "gpu");
    assert_eq!(ctx.overall_decision, "review");
    assert_eq!(
        ctx.class_review,
        Some(("native".to_string(), "review".to_string()))
    );
    assert!(ctx.risk_summary.iter().any(|n| n.contains("gpu capsule")));
}

#[test]
fn shell_capsule_context_aggregates_shell_review() {
    let program = parse_fixture("examples/capsule/backup_bash.fgx");
    let ctx = context("backup_bash.fgx", &program, "backup").unwrap();
    assert_eq!(
        ctx.class_review.as_ref().map(|(k, _)| k.as_str()),
        Some("shell")
    );
    assert_eq!(ctx.overall_decision, "review");

    let json = ctx.to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_context.v1\""));
    assert!(json.contains("\"overall_decision\": \"review\""));
}

#[test]
fn contract_capsule_context_aggregates_contract_review() {
    let program = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    let ctx = context("price_oracle.fgx", &program, "price_oracle").unwrap();
    assert_eq!(
        ctx.class_review.as_ref().map(|(k, _)| k.as_str()),
        Some("contract")
    );
    assert_eq!(ctx.overall_decision, "review");
}

#[test]
fn rw_binding_shows_in_risk_summary() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let ctx = context("web_counter.fgx", &program, "backend").unwrap();
    assert!(ctx.bindings.iter().any(|b| b.contains("rw")));
    assert!(ctx.risk_summary.iter().any(|n| n.contains("shared state")));
}

#[test]
fn context_for_unknown_capsule_errors() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    assert!(context("normalize_python.fgx", &program, "nope").is_err());
}
