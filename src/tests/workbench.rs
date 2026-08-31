use std::path::Path;

use crate::workbench::{context_json, review_json, review_json_with_codebook, ContextOptions};

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
    assert!(json.contains("\"tokenizer\": \"heuristic-v1\""));
    assert!(json.contains("\"included_deps\": [\"effects\", \"types\", \"tests\"]"));
    assert!(json.contains("\"reason\": \"focused_symbol_only\""));
}

#[test]
fn context_budget_admits_safety_context_and_explains_optional_exclusions() {
    let root = manifest_path(".");
    let unrestricted = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests")),
    )
    .unwrap();
    let mandatory = json_usize(&unrestricted, "mandatory");

    let bounded = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests")).with_max_tokens(Some(mandatory)),
    )
    .unwrap();
    assert!(bounded.contains("\"decision\": \"within_budget\""));
    assert!(bounded.contains("\"included_deps\": [\"effects\"]"));
    assert!(bounded.contains("\"reason\": \"token_budget\""));
    assert!(bounded.contains(&format!("\"used_tokens\": {mandatory}")));

    let error = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("effects")).with_max_tokens(Some(mandatory - 1)),
    )
    .unwrap_err();
    assert!(error.contains("context_token_budget_too_small"));
}

#[test]
fn context_dependencies_are_a_small_exact_vocabulary() {
    let root = manifest_path(".");
    let unknown = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,secrets")),
    )
    .unwrap_err();
    assert!(unknown.contains("unknown context dependency 'secrets'"));

    let duplicate = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,types")),
    )
    .unwrap_err();
    assert!(duplicate.contains("requested more than once"));
}

#[test]
fn unchanged_context_sections_collapse_to_verified_hash_references() {
    let root = manifest_path(".");
    let full = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests")),
    )
    .unwrap();
    assert!(full.contains("\"mode\": \"full\""));
    assert!(full.contains("\"section_hashes\": {"));
    assert!(full.contains("\"state_sections\":"));

    let delta = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests"))
            .with_baseline_json(&full)
            .unwrap(),
    )
    .unwrap();
    assert!(delta.contains("\"mode\": \"delta\""));
    assert_eq!(
        json_string(&delta, "packet_hash"),
        json_string(&full, "packet_hash")
    );
    assert_eq!(
        json_string(&delta, "baseline_packet_hash"),
        json_string(&full, "packet_hash")
    );
    assert!(delta.contains(
        "\"elided_sections\": [\"symbol_map\", \"symbol_source\", \"types\", \"tests\"]"
    ));
    assert!(!delta.contains("\n  \"symbol_source\":"));
    assert!(!delta.contains("\n  \"symbol_map\":"));
    assert!(delta.contains("\n  \"effects\": {"));
    assert!(json_usize(&delta, "saved_by_hash_elision") > 0);
    assert!(json_usize(&delta, "total_context") < json_usize(&delta, "logical_context"));

    let floor = json_usize(&delta, "mandatory");
    let bounded = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests"))
            .with_max_tokens(Some(floor))
            .with_baseline_json(&full)
            .unwrap(),
    )
    .unwrap();
    assert!(bounded.contains(&format!("\"used_tokens\": {floor}")));
    assert!(bounded.contains("\"decision\": \"within_budget\""));
}

#[test]
fn changed_or_unknown_baseline_state_is_never_elided() {
    let root = manifest_path(".");
    let full = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests")),
    )
    .unwrap();
    let source_hash = json_string(&full, "symbol_source");
    let tampered = full.replacen(
        &format!("\"symbol_source\": \"{source_hash}\""),
        &format!("\"symbol_source\": \"{}\"", "0".repeat(64)),
        1,
    );
    let refreshed = context_json(
        &root,
        "@function.mix_samples",
        ContextOptions::from_csv(Some("types,effects,tests"))
            .with_baseline_json(&tampered)
            .unwrap(),
    )
    .unwrap();
    assert!(refreshed.contains("\n  \"symbol_source\":"));
    assert!(!refreshed.contains("\"elided_sections\": [\"symbol_map\", \"symbol_source\""));

    let mismatch = context_json(
        &root,
        "@function.clamp_i16",
        ContextOptions::from_csv(Some("effects"))
            .with_baseline_json(&full)
            .unwrap(),
    )
    .unwrap_err();
    assert!(mismatch.contains("context_baseline_symbol_mismatch"));

    let invalid = ContextOptions::from_csv(None)
        .with_baseline_json("{}")
        .unwrap_err();
    assert!(invalid.contains("context_baseline_invalid"));
}

#[test]
fn current_project_codebook_encodes_context_and_roundtrips_to_readable_source() {
    let root = manifest_path("examples/apps/wav_waveform");
    let codebook =
        crate::codebook::build(&root, crate::codebook::DEFAULT_CODEBOOK_BUDGET, false).unwrap();
    let plain = context_json(
        &root,
        "@function.render_waveform",
        ContextOptions::from_csv(Some("types,effects,tests")),
    )
    .unwrap();
    let admitted = context_json(
        &root,
        "@function.render_waveform",
        ContextOptions::from_csv(Some("types,effects,tests")).with_codebook(codebook.clone()),
    )
    .unwrap();
    assert!(admitted.contains("\"codebook_ref\": \""));
    assert!(admitted.contains('§'));
    assert!(crate::tokens::estimate(&admitted) < crate::tokens::estimate(&plain));

    let encoded = context_json(
        &root,
        "@function.render_waveform",
        ContextOptions::from_csv(Some("types,effects,tests"))
            .with_codebook(codebook.clone())
            .with_force_codebook(true),
    )
    .unwrap();
    assert!(encoded.contains("\"codebook_decision\": \"forced\""));
    assert!(encoded.contains("\"codebook_ref\": \""));
    assert!(encoded.contains('§'));
    assert!(json_usize(&encoded, "saved_by_codebook") > 0);
    assert!(encoded.contains("\n  \"effects\": {"));

    let decoded = codebook.decode_text(&encoded);
    assert!(decoded.contains("let data_at: usize = find_data_chunk(bytes, total)"));
    assert!(!decoded.contains('§'));

    let coded_delta = context_json(
        &root,
        "@function.render_waveform",
        ContextOptions::from_csv(Some("types,effects,tests"))
            .with_codebook(codebook.clone())
            .with_force_codebook(true)
            .with_baseline_json(&encoded)
            .unwrap(),
    )
    .unwrap();
    assert!(coded_delta.contains(
        "\"elided_sections\": [\"symbol_map\", \"symbol_source\", \"types\", \"tests\"]"
    ));
    assert!(coded_delta.contains("\n  \"effects\": {"));

    let refreshed = context_json(
        &root,
        "@function.render_waveform",
        ContextOptions::from_csv(Some("types,effects,tests"))
            .with_codebook(codebook)
            .with_force_codebook(true)
            .with_baseline_json(&plain)
            .unwrap(),
    )
    .unwrap();
    assert!(refreshed.contains("\n  \"symbol_source\":"));
}

fn json_usize(json: &str, key: &str) -> usize {
    let marker = format!("\"{key}\": ");
    let start = json.find(&marker).expect("JSON key") + marker.len();
    let digits = json[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits.parse().expect("JSON usize")
}

fn json_string(json: &str, key: &str) -> String {
    let marker = format!("\"{key}\": \"");
    let start = json.find(&marker).expect("JSON string key") + marker.len();
    json[start..]
        .chars()
        .take_while(|character| *character != '"')
        .collect()
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

#[test]
fn agent_review_decodes_artifact_bound_codebook_patches_before_semantic_review() {
    let root = manifest_path("examples/apps/wav_waveform");
    let codebook =
        crate::codebook::build(&root, crate::codebook::DEFAULT_CODEBOOK_BUDGET, false).unwrap();
    let patch = "VERSION 1\nTARGET @function.render_waveform\nPRECONDITION NODE_EXISTS @function.render_waveform\nREVIEW helper=read_u32_le\nOP ADD_EFFECT effect=io.write\n";
    let encoded = codebook.encode_payload(patch).unwrap();
    let path = std::env::temp_dir().join(format!(
        "forgentex_{}_codebook_review.fgp",
        std::process::id()
    ));
    std::fs::write(&path, encoded).unwrap();

    let json = review_json_with_codebook(
        &path,
        Some(&root.join("src/main.fgx")),
        &root,
        Some(&codebook),
    )
    .unwrap();
    assert!(json.contains("\"codebook_ref\": \""));
    assert!(json.contains("\"transport_patch\":"));
    assert!(json.contains("ADD_EFFECT @function.render_waveform io.write"));

    let wrong =
        crate::codebook::build(&root, crate::codebook::DEFAULT_CODEBOOK_BUDGET, true).unwrap();
    let error =
        review_json_with_codebook(&path, Some(&root.join("src/main.fgx")), &root, Some(&wrong))
            .unwrap_err();
    assert!(error.contains("different artifact hash"));
    let _ = std::fs::remove_file(path);
}
