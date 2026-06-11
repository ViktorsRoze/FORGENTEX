use crate::format::format_human;
use crate::patch::{apply_patch, apply_patch_with_options, node_hash, PatchOptions};
use crate::typecheck::check;

use super::{fixture, parse_fixture};

#[test]
fn fast_path_patch_adds_reviewable_unsafe_stub() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let result = apply_patch(program, fixture("patches/add_fast_path.fgp")).unwrap();
    assert!(result.program.find_function("mix_samples_avx2").is_some());
    assert!(result.review.unsafe_changed);
    assert!(format_human(&result.program).contains("unsafe fn mix_samples_avx2"));
    assert_eq!(check(&result.program), Vec::new());
}

#[test]
fn patch_review_snapshot_is_stable() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let result = apply_patch(program, fixture("patches/add_fast_path.fgp")).unwrap();
    let review = result.review.render();
    assert!(review.contains("Added x86_64,avx2 fast path to mix_samples."));
    assert!(review.contains("Safe scalar fallback remains"));
    assert!(review.contains("New target-specific effect required: cpu.avx2."));
    assert!(review.contains("Unsafe or assembly changed: yes"));
    assert!(review.contains("Architecture paths:"));
    assert!(review.contains("fallback=scalar"));
}

#[test]
fn missing_patch_target_fails() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let err = apply_patch(program, "PATCH @function.nope\nREPLACE_RETURN_TYPE i32\n").unwrap_err();
    assert!(err.contains("unknown function target"));
}

#[test]
fn v1_patch_envelope_validates_preconditions_and_emits_review_json() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let hash = node_hash(&program, "@function.mix_samples").unwrap();
    let patch = format!(
        "VERSION 1\n\
         AUTHOR agent.demo\n\
         MOCK_SIGNATURE mock-key-1\n\
         TARGET @function.mix_samples\n\
         PRECONDITION NODE_EXISTS @function.mix_samples\n\
         PRECONDITION NODE_HASH @function.mix_samples {hash}\n\
         EXPECT_DIAGNOSTIC none\n\
         REVIEW risk=low area=dsp\n\
         OP ADD_EFFECT effect=alloc\n\
         OP ADD_TEST name=mix_samples_fast_path_regression\n"
    );

    let result =
        apply_patch_with_options(program, &patch, PatchOptions { check_only: true }).unwrap();
    let json = result.review.render_json(&check(&result.program));

    assert!(result.review.check_only);
    assert!(result.review.effect_changed);
    assert!(result.review.allocation_changed);
    assert!(json.contains("\"schema\": \"forgentex.patch_review.v1\""));
    assert!(json.contains("\"signature_status\": \"mock-accepted\""));
    assert!(json.contains("\"allocation\": true"));
    assert!(json.contains("PRECONDITION NODE_HASH @function.mix_samples"));
}

#[test]
fn v1_patch_conflict_fails_before_application() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let patch = "VERSION 1\n\
                 TARGET @function.mix_samples\n\
                 PRECONDITION NODE_HASH @function.mix_samples deadbeef\n\
                 OP ADD_EFFECT effect=alloc\n";

    let err = apply_patch(program, patch).unwrap_err();
    assert!(err.contains("patch conflict: node hash mismatch"));
}

#[test]
fn v1_patch_supports_declaration_unsafe_assembly_and_test_operations() {
    let program = crate::ast::Program::empty();
    let patch = "VERSION 1\n\
                 OP ADD_STRUCT name=Pair fields=left:i32,right:i32\n\
                 OP ADD_FUNCTION name=answer ret=i32 return=42\n\
                 TARGET @function.answer\n\
                 OP REPLACE_RETURN_EXPR expr=7\n\
                 OP MARK_UNSAFE\n\
                 OP ADD_ASM_BLOCK target=aarch64 op=demo_arm64_path\n\
                 OP ADD_TEST name=answer_smoke\n";

    let result = apply_patch(program, patch).unwrap();
    let human = format_human(&result.program);

    assert!(human.contains("struct Pair"));
    assert!(human.contains("unsafe fn answer() -> i32"));
    assert!(human.contains("asm aarch64"));
    assert!(result.review.public_api_changed);
    assert!(result.review.unsafe_changed);
    assert!(result.review.architecture_changed);
    assert!(!result.review.target_notes.is_empty());
    assert!(result
        .review
        .render()
        .contains("ABI or layout changed: yes"));
}
