use crate::perf::{run_bench, run_perf_report, BenchOptions};

use super::manifest_path;

#[test]
fn bench_reports_correctness_bounds_allocations_and_target_path() {
    let report = run_bench(&BenchOptions {
        source: manifest_path("examples/media/audio_mix_simd.fgx"),
        entry: "mix_samples_simd".to_string(),
        input: manifest_path("examples/bench/audio_mix.json"),
        target: "aarch64-unknown-linux-gnu".to_string(),
        iterations: 8,
    })
    .unwrap();
    assert_eq!(report.schema, "forgentex.bench.v1");
    assert!(report.is_success(), "{}", report.to_text());
    assert_eq!(report.sample_count, 8);
    assert_eq!(report.architecture_path, "aarch64.neon-review");
    assert!(report.operations.simd_calls > 0);
    assert!(report.bounds_report.checks > 0);
    assert_eq!(report.allocation_report.allocations, 0);
    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.bench.v1\""));
    assert!(json.contains("\"correctness\": \"pass\""));
    assert!(json.contains("\"allocation_report\""));
    assert!(json.contains("\"bounds_report\""));
}

#[test]
fn perf_report_compares_scalar_and_simd_across_stage25_targets() {
    let report = run_perf_report(
        &manifest_path("examples/media/audio_mix.fgx"),
        &manifest_path("examples/media/audio_mix_simd.fgx"),
        "mix_samples",
        "mix_samples_simd",
        &manifest_path("examples/bench/audio_mix.json"),
        8,
    )
    .unwrap();
    assert_eq!(report.schema, "forgentex.perf_report.v1");
    assert!(report.is_success(), "{}", report.to_text());
    assert_eq!(report.comparisons.len(), 3);
    assert!(report
        .comparisons
        .iter()
        .any(|comparison| comparison.architecture_path == "x86_64.avx2-review"));
    assert!(report
        .comparisons
        .iter()
        .any(|comparison| comparison.architecture_path == "aarch64.neon-review"));
    assert!(report
        .comparisons
        .iter()
        .any(|comparison| comparison.architecture_path == "riscv.vector-review"));
    assert!(report
        .comparisons
        .iter()
        .all(|comparison| comparison.correctness == "pass"));
    let json = report.to_json();
    assert!(json.contains("\"patch_gate\": \"pass\""));
    assert!(json.contains("target_specific_evidence=yes"));
}

#[test]
fn bench_rejects_unknown_targets() {
    let error = run_bench(&BenchOptions {
        source: manifest_path("examples/media/audio_mix.fgx"),
        entry: "mix_samples".to_string(),
        input: manifest_path("examples/bench/audio_mix.json"),
        target: "mips64-demo".to_string(),
        iterations: 1,
    })
    .unwrap_err();
    assert!(error.contains("unknown target triple"));
}
