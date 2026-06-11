use std::path::Path;

use crate::parser::parse_file;
use crate::targets::{check_program_target, target_metadata_for_function, targets_text};

use super::{fixture, parse_fixture};

#[test]
fn targets_catalog_lists_stage11_triples_and_features() {
    let catalog = targets_text();
    assert!(catalog.contains("x86_64-pc-windows-msvc"));
    assert!(catalog.contains("aarch64-pc-windows-msvc"));
    assert!(catalog.contains("riscv64gc-unknown-linux-gnu"));
    assert!(catalog.contains("riscv32imac-unknown-none-elf"));
    assert!(catalog.contains("avx2"));
    assert!(catalog.contains("neon"));
    assert!(catalog.contains("zbb"));
}

#[test]
fn target_metadata_is_exposed_for_assembly_functions() {
    let program = parse_fixture("examples/media/popcount_x86.fgx");
    let function = program.find_function("popcount_x86").unwrap();
    let metadata = target_metadata_for_function(function);
    assert!(metadata.labels.contains(&"target.x86_64".to_string()));
    assert!(metadata.families.contains(&"x86_64".to_string()));
}

#[test]
fn unknown_target_triple_reports_stable_diagnostic() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let diagnostics = check_program_target(&program, "mips64-demo");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "unknown_target");
    assert_eq!(diagnostics[0].node, "@program");
}

#[test]
fn unknown_target_feature_reports_stable_diagnostic() {
    let source = "fn demo() -> void effects [cpu.spacewarp]:\n    return\n";
    let program = parse_file(Path::new("unknown_feature.fgx"), source).unwrap();
    let diagnostics = check_program_target(&program, "x86_64-pc-windows-msvc");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "unknown_target_feature"));
}

#[test]
fn unguarded_target_specific_call_is_rejected() {
    let source = "fn fast(x: u32) -> u32 effects [cpu.avx2]:\n    return x\n\nfn caller(x: u32) -> u32 effects [cpu.avx2]:\n    return fast(x)\n";
    let program = parse_file(Path::new("unguarded.fgx"), source).unwrap();
    let diagnostics = check_program_target(&program, "x86_64-pc-windows-msvc");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "target_guard_required"));
}

#[test]
fn guarded_target_specific_call_passes_target_check() {
    let source = "fn fast(x: u32) -> u32 effects [cpu.avx2]:\n    return x\n\nfn caller(x: u32) -> u32 effects [cpu.avx2]:\n    if cpu_has_avx2():\n        return fast(x)\n    return x\n";
    let program = parse_file(Path::new("guarded.fgx"), source).unwrap();
    let diagnostics = check_program_target(&program, "x86_64-pc-windows-msvc");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "target_guard_required"),
        "{diagnostics:?}"
    );
}

#[test]
fn incompatible_assembly_target_is_rejected() {
    let program = parse_file(
        Path::new("examples/media/popcount_x86.fgx"),
        fixture("examples/media/popcount_x86.fgx"),
    )
    .unwrap();
    let diagnostics = check_program_target(&program, "aarch64-unknown-linux-gnu");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "target_arch_mismatch"));
}
