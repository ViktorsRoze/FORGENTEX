use std::path::PathBuf;

use crate::fuzzing::{run_backend_differential, run_fuzz_suite, BackendDifferentialOptions};

#[test]
fn fuzz_suite_covers_roundtrip_patch_no_panic_and_target_invariants() {
    let report = run_fuzz_suite(6);
    assert!(report.is_success(), "{}", report.to_text());
    assert_eq!(report.failed(), 0);
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.fuzz.v1\""));
    assert!(report
        .cases
        .iter()
        .any(|case| case.category == "roundtrip-generated"));
    assert!(report
        .cases
        .iter()
        .any(|case| case.category == "patch-format-check"));
    assert!(report
        .cases
        .iter()
        .any(|case| case.category == "target-validation"));
}

#[test]
fn backend_differential_runner_links_interpreter_c_backend_and_fgir() {
    let options = BackendDifferentialOptions {
        project: PathBuf::from("."),
        source: PathBuf::from("examples/media/audio_mix.fgx"),
        entry: "mix_samples".to_string(),
        input: PathBuf::from("examples/run/audio_mix.json"),
    };
    let report = run_backend_differential(&options).expect("differential report should run");
    assert!(report.is_success(), "{}", report.to_text());
    assert_eq!(report.interpreter_status, "pass");
    assert!(report.backend_entry_found);
    assert!(report.ir_entry_found);
    assert!(report.generated_header_bytes > 0);
    assert!(report.generated_source_bytes > 0);
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.backend_differential.v1\""));
}
