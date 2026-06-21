use crate::native_capsule::{analyze, validate};
use crate::parser::parse_human;

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn c_capsule_is_native_value_boundary_with_fixtures() {
    let program = parse_fixture("examples/capsule/clamp_c.fgx");
    let report = analyze("clamp_c.fgx", &program);
    assert_eq!(report.status, "review");
    let entry = &report.capsules[0];
    assert_eq!(entry.risk_class, "native");
    assert_eq!(entry.toolchain, "c11");
    assert!(!entry.memory_boundary);
    assert_eq!(entry.fallback_fixtures, 3);
    assert_eq!(entry.decision, "review");
    assert!(entry.gpu_arch.is_none());
}

#[test]
fn cuda_capsule_reports_gpu_arch_and_memory_boundary() {
    let program = parse_fixture("examples/capsule/saxpy_cuda.fgx");
    let report = analyze("saxpy_cuda.fgx", &program);
    let entry = &report.capsules[0];
    assert_eq!(entry.risk_class, "gpu");
    assert_eq!(entry.gpu_arch.as_deref(), Some("sm_80"));
    assert!(entry.memory_boundary);
    assert_eq!(entry.decision, "review");

    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.native_review.v1\""));
    assert!(json.contains("\"gpu_arch\": \"sm_80\""));
    assert!(json.contains("\"memory_boundary\": true"));
}

#[test]
fn cuda_capsule_without_gpu_target_is_rejected() {
    let source = "\
foreign k cuda version \"cuda-12.4\" env \"linux-x86_64\" effects [gpu.compute]:
    input x: &[f32]
    output ok: bool
    sandbox \"gpu.compute device:0\"
    code:
        __global__ void k() {}
";
    let program = human(source);
    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"gpu_capsule_no_target".to_string()));
    assert_eq!(analyze("k.fgx", &program).status, "reject");
}

#[test]
fn cuda_capsule_with_arch_passes_validation() {
    let program = parse_fixture("examples/capsule/saxpy_cuda.fgx");
    assert!(validate(&program).is_empty());
}

#[test]
fn managed_capsules_are_not_in_native_review() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = analyze("normalize_python.fgx", &program);
    assert!(report.capsules.is_empty());
    assert_eq!(report.status, "pass");
}

#[test]
fn cpp_capsule_is_classified_native() {
    let source = "\
foreign engine cpp version \"c++17\" env \"x86_64-pc-windows-msvc\":
    input buf: &mut [f32]
    output ok: bool
    sandbox \"fs.none\"
    code:
        // c++ engine
";
    let report = analyze("engine.fgx", &human(source));
    let entry = &report.capsules[0];
    assert_eq!(entry.risk_class, "native");
    assert!(entry.memory_boundary);
}
