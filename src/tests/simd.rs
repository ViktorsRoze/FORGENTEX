use std::path::Path;

use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::simd::{analyze_simd, check_simd_fallbacks, review_json, review_text};
use crate::targets::check_program_target;
use crate::typecheck::check;

use super::{fixture, parse_fixture};

#[test]
fn audio_mix_simd_typechecks_and_reviews_for_arm_and_riscv() {
    let program = parse_fixture("examples/media/audio_mix_simd.fgx");
    assert_eq!(check(&program), Vec::new());
    assert_eq!(
        check_program_target(&program, "aarch64-unknown-linux-gnu"),
        Vec::new()
    );
    assert_eq!(
        check_simd_fallbacks(&program, "riscv64gc-unknown-linux-gnu"),
        Vec::new()
    );

    let arm_review = analyze_simd(&program, "aarch64-unknown-linux-gnu");
    assert_eq!(arm_review.diagnostics, Vec::new());
    assert_eq!(arm_review.functions.len(), 1);
    assert_eq!(arm_review.functions[0].name, "mix_samples_simd");
    assert!(arm_review.functions[0].scalar_fallback);
    assert!(arm_review.functions[0].equivalence_test);
    assert!(arm_review.functions[0]
        .lowering
        .contains(&"aarch64.neon".to_string()));

    let text = review_text(&arm_review);
    assert!(text.contains("scalar_fallback=yes"));
    assert!(text.contains("equivalence_tests=yes"));
    let json = review_json(&arm_review);
    assert!(json.contains("\"schema\": \"forgentex.simd_review.v1\""));
}

#[test]
fn vector_and_mask_intrinsics_have_stable_type_diagnostics() {
    let source = r#"
fn bad_lane_mix() -> vec[i16, 8] effects [cpu.simd]:
    let a: vec[i16, 8] = simd_vec_i16x8(1, 2, 3, 4, 5, 6, 7, 8)
    let b: vec[i16, 4] = simd_vec_i16x4(1, 2, 3, 4)
    return simd_add(a, b)

fn bad_store(dst: &[i16], value: vec[i16, 8]) -> void effects [cpu.simd]:
    let zero: usize = 0
    simd_store_i16x8(dst, zero, value)
"#;
    let program = parse_file(Path::new("bad_simd.fgx"), source).unwrap();
    let diagnostics = check(&program);

    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "simd_type_mismatch"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "simd_requires_mutable_slice"));
}

#[test]
fn target_checks_require_scalar_fallback_unless_target_only() {
    let missing_fallback = r#"
fn fast_copy(dst: &mut [i16], src: &[i16]) -> void effects [cpu.simd]:
    let zero: usize = 0
    let value: vec[i16, 8] = simd_load_i16x8(src, zero)
    simd_store_i16x8(dst, zero, value)
"#;
    let program = parse_file(Path::new("missing_fallback.fgx"), missing_fallback).unwrap();
    let diagnostics = check_simd_fallbacks(&program, "aarch64-unknown-linux-gnu");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "simd_fallback_required"));

    let target_only = r#"
fn fast_copy(dst: &mut [i16], src: &[i16]) -> void effects [cpu.simd, target.only]:
    let zero: usize = 0
    let value: vec[i16, 8] = simd_load_i16x8(src, zero)
    simd_store_i16x8(dst, zero, value)
"#;
    let program = parse_file(Path::new("target_only.fgx"), target_only).unwrap();
    assert_eq!(
        check_simd_fallbacks(&program, "aarch64-unknown-linux-gnu"),
        Vec::new()
    );
    assert_eq!(
        check_program_target(&program, "aarch64-unknown-linux-gnu"),
        Vec::new()
    );
}

#[test]
fn interpreter_executes_portable_simd_intrinsics() {
    let source = r#"
fn sum_vec() -> i32 effects [cpu.simd]:
    let a: vec[i32, 4] = simd_vec_i32x4(1, 2, 3, 4)
    let b: vec[i32, 4] = simd_splat_i32x4(1)
    let c: vec[i32, 4] = simd_add(a, b)
    let d: vec[i32, 4] = simd_shuffle(c, 3, 2, 1, 0)
    return simd_reduce_add(d)

fn copy4(dst: &mut [i32], src: &[i32]) -> void effects [cpu.simd]:
    let zero: usize = 0
    let value: vec[i32, 4] = simd_load_i32x4(src, zero)
    simd_store_i32x4(dst, zero, value)
"#;
    let program = parse_file(Path::new("run_simd.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let sum = run_entry_json(
        &program,
        "sum_vec",
        r#"{ "args": {}, "expect": { "return": 14 } }"#,
    )
    .unwrap();
    assert!(sum.contains("\"status\": \"pass\""));

    let copy = run_entry_json(
        &program,
        "copy4",
        r#"{
          "args": { "dst": [0, 0, 0, 0], "src": [9, 8, 7, 6] },
          "expect": { "bindings": { "dst": [9, 8, 7, 6] } }
        }"#,
    )
    .unwrap();
    assert!(copy.contains("\"status\": \"pass\""));
}

#[test]
fn fixture_roundtrips_through_parser() {
    let source = fixture("examples/media/audio_mix_simd.fgx");
    let program = parse_file(Path::new("audio_mix_simd.fgx"), source).unwrap();
    assert_eq!(program.functions().count(), 4);
}
