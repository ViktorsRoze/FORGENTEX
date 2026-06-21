use std::path::PathBuf;

use crate::ffi::run;

use super::parse_fixture;

fn out_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_ffi_{tag}", std::process::id()))
}

#[test]
fn derives_signature_and_marshaling_and_gates_a_c_capsule() {
    let program = parse_fixture("examples/capsule/clamp_c.fgx");
    let plan = run("clamp_c.fgx", &program, None, &out_dir("plan")).unwrap();

    assert_eq!(plan.schema, "forgentex.ffi_plan.v1");
    assert_eq!(plan.language, "c");
    assert_eq!(plan.risk_class, "native");
    assert_eq!(plan.extern_c_signature, "int16_t clamp_sample(float x)");

    // Marshaling: f32 -> float (in), i16 -> int16_t (out).
    let input = plan
        .marshaling
        .iter()
        .find(|m| m.direction == "in")
        .unwrap();
    assert_eq!(input.c_type, "float");
    let output = plan
        .marshaling
        .iter()
        .find(|m| m.direction == "out")
        .unwrap();
    assert_eq!(output.c_type, "int16_t");

    // The contract passes check and pins its deps, so the gate allows it.
    assert_eq!(plan.gate_decision, "allow");
    assert!(plan.deps_pinned);

    // Either it really executed (compiler present) or there was no compiler.
    assert!(
        plan.is_success(),
        "unexpected status {}",
        plan.execution_status
    );
    if plan.execution_status == "executed" {
        assert_eq!(plan.passed, 3);
        assert_eq!(plan.total, 3);
        assert!(plan.transcript.iter().all(|r| r.pass));
        assert!(plan.toolchain.is_some());
        assert_eq!(plan.link_units.len(), 2);
    }
    let _ = std::fs::remove_dir_all(out_dir("plan"));
}

#[test]
fn managed_language_capsule_is_rejected() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let err = run("normalize_python.fgx", &program, None, &out_dir("managed")).unwrap_err();
    assert!(
        err.contains("managed"),
        "expected managed rejection, got: {err}"
    );
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/clamp_c.fgx");
    let json = run("clamp_c.fgx", &program, None, &out_dir("json"))
        .unwrap()
        .to_json();
    assert!(json.contains("\"schema\": \"forgentex.ffi_plan.v1\""));
    assert!(json.contains("\"extern_c_signature\": \"int16_t clamp_sample(float x)\""));
    assert!(json.contains("\"gate_decision\""));
    assert!(json.contains("\"transcript\""));
    let _ = std::fs::remove_dir_all(out_dir("json"));
}
