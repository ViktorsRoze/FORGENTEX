use std::path::Path;

use crate::workbench::{context_json, review_json, ContextOptions};

use super::manifest_path;

#[test]
fn agent_context_packet_contains_symbol_deps_and_token_estimates() {
    let json = context_json(
        &manifest_path("."),
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests")),
    )
    .unwrap();

    assert!(json.contains("\"schema\": \"forgentex.agent_context.v1\""));
    assert!(json.contains("\"symbol\": \"@function.mix_samples\""));
    assert!(json.contains("\"module\": \"media.audio\""));
    assert!(json.contains("fn mix_samples"));
    assert!(json.contains("@struct.AudioBlock"));
    assert!(json.contains("\"effects\": {"));
    assert!(json.contains("examples/run/audio_mix.json"));
    assert!(json.contains("\"token_estimate\":"));
}

#[test]
fn agent_review_packet_auto_locates_patch_target_from_project() {
    let json = review_json(
        &manifest_path("patches/signed_add_crypto_effect.fgp"),
        None,
        &manifest_path("."),
    )
    .unwrap();

    assert!(json.contains("\"schema\": \"forgentex.agent_review.v1\""));
    assert!(json.contains("\"source\":"));
    assert!(json.contains("examples/project/media/audio.fgx"));
    assert!(json.contains("\"patch_review\": {"));
    assert!(json.contains("\"signature_status\": \"signed-mock-accepted\""));
    assert!(json.contains("\"check_only\": true"));
    assert!(json.contains("Signed mock envelope was verified"));
}

#[test]
fn agent_review_packet_can_use_explicit_source() {
    let json = review_json(
        &manifest_path("patches/signed_add_crypto_effect.fgp"),
        Some(Path::new("examples/media/audio_mix.fgx")),
        &manifest_path("."),
    )
    .unwrap();

    assert!(json.contains("\"source\": \"examples/media/audio_mix.fgx\""));
    assert!(json.contains("\"effects\": true"));
}
