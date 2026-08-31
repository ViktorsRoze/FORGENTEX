use std::fs;

use crate::codebook::{build, check, parse, DEFAULT_CODEBOOK_BUDGET};

use super::manifest_path;

#[test]
fn identifier_codebook_is_deterministic_bounded_and_measured() {
    let root = manifest_path("examples/apps/wav_waveform");
    let first = build(&root, DEFAULT_CODEBOOK_BUDGET, false).unwrap();
    let second = build(&root, DEFAULT_CODEBOOK_BUDGET, false).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.schema, "forgentex.codebook.v1");
    assert_eq!(first.tokenizer, "heuristic-v1");
    assert_eq!(first.tier, "semantic");
    assert!(!first.entries.is_empty());
    assert!(first.codebook_tokens <= first.budget_tokens);
    assert!(first.encoded_tokens < first.corpus_tokens);
    assert_eq!(
        first.gross_savings,
        first.corpus_tokens - first.encoded_tokens
    );
    assert_eq!(
        first.codebook_tokens,
        crate::tokens::estimate(&first.to_json())
    );
    assert!(first.break_even_interactions.is_some_and(|value| value > 1));

    let parsed = parse(&first.to_json()).unwrap();
    assert_eq!(parsed, first);
    let codes = first
        .entries
        .iter()
        .map(|entry| entry.code.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(codes.len(), first.entries.len());
    assert!(codes.iter().all(|code| code.starts_with(&first.sigil)));

    let repeated_expansion = first
        .entries
        .iter()
        .find(|entry| !entry.kind.ends_with("_template"))
        .unwrap()
        .expansion
        .clone();
    let source = format!("{repeated_expansion} + {repeated_expansion}");
    let encoded = first.encode_text(&source).unwrap();
    assert_ne!(encoded, source);
    assert_eq!(first.decode_text(&encoded), source);
    let payload = first.encode_payload(&source).unwrap();
    assert!(payload.starts_with("#FGCB v1 hash="));
    assert_eq!(first.decode_payload(&payload).unwrap(), source);
    assert_eq!(first.decode_payload(&source).unwrap(), source);
    let guarded = first.encode_payload_if_smaller(&source).unwrap();
    assert_eq!(guarded.decision, "skipped_not_smaller");
    assert_eq!(guarded.text, source);
    assert!(guarded.encoded_tokens >= guarded.plain_tokens);
    let repeated = format!("{repeated_expansion} ").repeat(128);
    let positive = first.encode_payload_if_smaller(&repeated).unwrap();
    assert_eq!(positive.decision, "applied");
    assert!(positive.encoded_tokens < positive.plain_tokens);
    assert_eq!(first.decode_payload(&positive.text).unwrap(), repeated);
    assert!(first
        .encode_text(&format!("{}collision", first.sigil))
        .is_err());
    assert_eq!(first.artifact_hash().len(), 64);

    let wrong = build(&root, DEFAULT_CODEBOOK_BUDGET, true).unwrap();
    assert!(wrong.decode_payload(&payload).is_err());
}

#[test]
fn codebook_check_detects_stale_content_and_modes_are_distinct() {
    let root = manifest_path("examples/apps/wav_waveform");
    let mnemonic = build(&root, DEFAULT_CODEBOOK_BUDGET, false).unwrap();
    let cryptic = build(&root, DEFAULT_CODEBOOK_BUDGET, true).unwrap();
    assert_eq!(mnemonic.mode, "mnemonic");
    assert_eq!(cryptic.mode, "cryptic");
    assert_ne!(mnemonic.entries, cryptic.entries);

    let path = std::env::temp_dir().join(format!("forgentex_{}_codebook.json", std::process::id()));
    fs::write(&path, mnemonic.to_json()).unwrap();
    assert!(check(&root, &path).unwrap().is_success());

    let changed_root = std::env::temp_dir().join(format!(
        "forgentex_{}_changed_codebook_project",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&changed_root);
    fs::create_dir_all(changed_root.join("src")).unwrap();
    fs::copy(
        root.join("Forgentex.toml"),
        changed_root.join("Forgentex.toml"),
    )
    .unwrap();
    let source = fs::read_to_string(root.join("src/main.fgx")).unwrap();
    fs::write(
        changed_root.join("src/main.fgx"),
        source.replacen("let width: usize = 64", "let width: usize = 63", 1),
    )
    .unwrap();
    let report = check(&changed_root, &path).unwrap();
    assert_eq!(report.status, "stale");
    let _ = fs::remove_dir_all(changed_root);
    let _ = fs::remove_file(path);
}

#[test]
fn codebook_rejects_tiny_budgets_and_malformed_entries() {
    let root = manifest_path("examples/apps/wav_waveform");
    assert!(build(&root, 1, false)
        .unwrap_err()
        .contains("budget must be between"));

    let valid = build(&root, DEFAULT_CODEBOOK_BUDGET, false).unwrap();
    let malformed = valid
        .to_json()
        .replacen("\"frequency\":3", "\"frequency\":1", 1);
    assert!(parse(&malformed)
        .unwrap_err()
        .contains("inconsistent entry"));
    let malformed_morphology =
        valid
            .to_json()
            .replacen("\"kind\":\"expression\"", "\"kind\":\"morphology\"", 1);
    assert!(parse(&malformed_morphology)
        .unwrap_err()
        .contains("inconsistent entry"));
}

#[test]
fn evolved_codebooks_preserve_live_codes_and_never_reuse_retired_meanings() {
    let source_root = manifest_path("examples/apps/wav_waveform");
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_codebook_evolution",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::copy(
        source_root.join("Forgentex.toml"),
        root.join("Forgentex.toml"),
    )
    .unwrap();
    let original = fs::read_to_string(source_root.join("src/main.fgx")).unwrap();
    fs::write(root.join("src/main.fgx"), &original).unwrap();

    let first = crate::codebook::build(&root, 2048, false).unwrap();
    assert_eq!(first.generation, 1);
    assert_eq!(first.parent_hash, None);
    assert_eq!(
        crate::codebook::build_with_previous(&root, 2048, false, Some(&first)).unwrap(),
        first,
        "unchanged corpus must not create a new generation"
    );
    let old_read_code = first
        .entries
        .iter()
        .find(|entry| entry.expansion == "read_u32_le")
        .unwrap()
        .code
        .clone();

    let renamed = original.replace("read_u32_le", "read_word_le");
    fs::write(root.join("src/main.fgx"), renamed).unwrap();
    let second = crate::codebook::build_with_previous(&root, 2048, false, Some(&first)).unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(
        second.parent_hash.as_deref(),
        Some(first.artifact_hash().as_str())
    );
    assert!(second.retired_codes.contains(&old_read_code));
    let new_read_code = second
        .entries
        .iter()
        .find(|entry| entry.expansion == "read_word_le")
        .unwrap()
        .code
        .clone();
    assert_ne!(new_read_code, old_read_code);
    assert_eq!(crate::codebook::parse(&second.to_json()).unwrap(), second);
    assert_eq!(second.ensure_current(&root).unwrap().status, "current");
    for old in &first.entries {
        if let Some(current) = second
            .entries
            .iter()
            .find(|entry| entry.expansion == old.expansion)
        {
            assert_eq!(current.code, old.code);
        }
    }

    fs::write(root.join("src/main.fgx"), original).unwrap();
    let third = crate::codebook::build_with_previous(&root, 2048, false, Some(&second)).unwrap();
    let restored_read_code = third
        .entries
        .iter()
        .find(|entry| entry.expansion == "read_u32_le")
        .unwrap()
        .code
        .clone();
    assert_ne!(restored_read_code, old_read_code);
    assert!(third.retired_codes.contains(&old_read_code));
    assert!(third.retired_codes.contains(&new_read_code));
    assert_eq!(third.ensure_current(&root).unwrap().status, "current");
    assert!(
        crate::codebook::build_with_previous(&root, 2048, true, Some(&third))
            .unwrap_err()
            .contains("mode")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn semantic_tier_learns_whole_type_shapes_and_effect_sets() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_semantic_codebook",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"semantic-codebook\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "fn alpha(values: &mut [i16]) -> void effects [alloc, io.read]:\n    require values.len > 0\n    return\n\nfn beta(values: &mut [i16]) -> void effects [alloc, io.read]:\n    require values.len > 0\n    return\n\nfn gamma(values: &mut [i16]) -> void effects [alloc, io.read]:\n    require values.len > 0\n    return\n",
    )
    .unwrap();

    let codebook = crate::codebook::build(&root, 2048, false).unwrap();
    assert_eq!(codebook.tier, "semantic");
    let type_entry = codebook
        .entries
        .iter()
        .find(|entry| entry.kind == "type" && entry.expansion == "&mut[i16]")
        .expect("whole type-shape entry");
    let effects_entry = codebook
        .entries
        .iter()
        .find(|entry| entry.kind == "effect_set" && entry.expansion == "effects[alloc,io.read]")
        .expect("whole effect-set entry");
    let pattern_entry = codebook
        .entries
        .iter()
        .find(|entry| {
            matches!(entry.kind.as_str(), "statement" | "expression")
                && entry.expansion.contains("values.len > 0")
        })
        .expect("exact AST pattern entry");
    let source =
        "F alpha(values &mut[i16]) -> void effects[alloc,io.read] {\n  REQ values.len > 0\n}\n";
    let encoded = codebook.encode_text(source).unwrap();
    assert!(encoded.contains(&type_entry.code));
    assert!(encoded.contains(&effects_entry.code));
    assert!(encoded.contains(&pattern_entry.code));
    assert_eq!(codebook.decode_text(&encoded), source);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn naming_morphology_compresses_new_identifiers_from_a_learned_scheme() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_morphology_codebook",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"morphology-codebook\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "fn project_media_transform_alpha(value: i32) -> i32:\n    return value\n\nfn project_media_transform_beta(value: i32) -> i32:\n    return value\n\nfn project_media_transform_gamma(value: i32) -> i32:\n    return value\n\nfn project_media_transform_delta(value: i32) -> i32:\n    return value\n",
    )
    .unwrap();

    let codebook = crate::codebook::build(&root, 4096, false).unwrap();
    let morphology = codebook
        .entries
        .iter()
        .find(|entry| entry.kind == "morphology" && entry.expansion == "project_media_transform_")
        .expect("shared naming prefix");
    assert!(morphology.code.starts_with(&format!("{}~", codebook.sigil)));
    assert!(morphology.code.ends_with('~'));

    let unseen = "project_media_transform_epsilon";
    let encoded = codebook.encode_text(unseen).unwrap();
    assert!(encoded.contains(&morphology.code));
    assert!(!encoded.contains(&morphology.expansion));
    assert_eq!(codebook.decode_text(&encoded), unseen);
    assert_eq!(
        crate::codebook::parse(&codebook.to_json()).unwrap(),
        codebook
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn parameterized_ast_pattern_encodes_an_unseen_literal_and_rejects_bad_framing() {
    let root = manifest_path("examples/apps/wav_waveform");
    let codebook = crate::codebook::build(&root, 1024, false).unwrap();
    let template = codebook
        .entries
        .iter()
        .find(|entry| {
            entry.kind == "expression_template" && entry.expansion == "push(image, u8($0))"
        })
        .expect("parameterized image-write expression");
    assert!(template.code.starts_with(&format!("{}!", codebook.sigil)));
    assert!(template.code.ends_with('!'));

    let unseen = "push(image, u8(77))";
    let encoded = codebook.encode_text(unseen).unwrap();
    assert!(encoded.contains(&template.code));
    assert!(encoded.contains("2:77"));
    assert_eq!(codebook.decode_text(&encoded), unseen);

    let payload = codebook.encode_payload(unseen).unwrap();
    assert_eq!(codebook.decode_payload(&payload).unwrap(), unseen);
    let malformed = payload.replacen("2:77", "3:77", 1);
    assert!(codebook
        .decode_payload(&malformed)
        .unwrap_err()
        .contains("malformed template invocation"));
}

#[test]
fn parameterized_statement_pattern_frames_utf8_arguments_by_exact_bytes() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_statement_template_codebook",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"statement-template-codebook\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "fn alpha() -> void:\n    require validate_project_status(\"ready\") == true\n\nfn beta() -> void:\n    require validate_project_status(\"running\") == true\n\nfn gamma() -> void:\n    require validate_project_status(\"done\") == true\n",
    )
    .unwrap();

    let codebook = crate::codebook::build(&root, 4096, false).unwrap();
    let template = codebook
        .entries
        .iter()
        .find(|entry| {
            entry.kind == "statement_template"
                && entry.expansion == "REQ validate_project_status($0) == true"
        })
        .expect("parameterized require statement");
    let unseen = "REQ validate_project_status(\"✓\") == true";
    let encoded = codebook.encode_text(unseen).unwrap();
    assert!(encoded.contains(&template.code));
    assert!(encoded.contains("5:\"✓\""));
    assert_eq!(codebook.decode_text(&encoded), unseen);
    assert_eq!(
        crate::codebook::parse(&codebook.to_json()).unwrap(),
        codebook
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn two_slot_pattern_keeps_arguments_ordered_and_placeholder_text_literal() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_two_slot_template_codebook",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"two-slot-template-codebook\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "fn alpha() -> void:\n    require validate_project_range(\"ready\", 100, stable_context) == true\n\nfn beta() -> void:\n    require validate_project_range(\"running\", 200, stable_context) == true\n\nfn gamma() -> void:\n    require validate_project_range(\"done\", 300, stable_context) == true\n",
    )
    .unwrap();

    let codebook = crate::codebook::build(&root, 4096, false).unwrap();
    let template = codebook
        .entries
        .iter()
        .find(|entry| {
            entry.kind == "statement_template"
                && entry.expansion == "REQ validate_project_range($0, $1, stable_context) == true"
        })
        .expect("two-slot require statement");
    let unseen = "REQ validate_project_range(\"$1\", 777, stable_context) == true";
    let encoded = codebook.encode_text(unseen).unwrap();
    assert!(encoded.contains(&template.code));
    assert!(encoded.contains("4:\"$1\"3:777"));
    assert_eq!(codebook.decode_text(&encoded), unseen);

    let payload = codebook.encode_payload(unseen).unwrap();
    assert_eq!(codebook.decode_payload(&payload).unwrap(), unseen);
    let malformed = payload.replacen("3:777", "4:777", 1);
    assert!(codebook
        .decode_payload(&malformed)
        .unwrap_err()
        .contains("malformed template invocation"));
    assert_eq!(
        crate::codebook::parse(&codebook.to_json()).unwrap(),
        codebook
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn identifier_only_v1_artifact_evolves_without_changing_old_codes() {
    let source_root = manifest_path("examples/apps/wav_waveform");
    let mut legacy = crate::codebook::build(&source_root, 4096, false).unwrap();
    legacy.entries.retain(|entry| entry.kind == "identifier");
    assert!(!legacy.entries.is_empty());
    legacy.tier = "identifiers".to_string();
    let measured = crate::codebook::measure(&source_root, &legacy).unwrap();
    legacy.encoded_tokens = measured.encoded_tokens;
    legacy.gross_savings = measured.gross_savings;
    for _ in 0..4 {
        legacy.codebook_tokens = crate::tokens::estimate(&legacy.to_json());
        legacy.net_first_interaction = legacy.gross_savings as i64 - legacy.codebook_tokens as i64;
        legacy.break_even_interactions = (legacy.gross_savings > 0).then(|| {
            legacy
                .codebook_tokens
                .saturating_add(legacy.gross_savings - 1)
                / legacy.gross_savings
        });
    }
    let serialized = legacy.to_json();
    assert!(!serialized.contains("\"kind\":"));
    let parsed = crate::codebook::parse(&serialized).unwrap();
    assert_eq!(parsed.to_json(), serialized);
    assert_eq!(
        parsed.artifact_hash(),
        crate::crypto::sha256_hex(serialized.as_bytes())
    );

    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_legacy_codebook_evolution",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::copy(
        source_root.join("Forgentex.toml"),
        root.join("Forgentex.toml"),
    )
    .unwrap();
    let changed = fs::read_to_string(source_root.join("src/main.fgx"))
        .unwrap()
        .replace("read_u32_le", "read_word_le");
    fs::write(root.join("src/main.fgx"), changed).unwrap();
    let evolved = crate::codebook::build_with_previous(&root, 1024, false, Some(&parsed)).unwrap();
    assert_eq!(evolved.tier, "semantic");
    assert_eq!(evolved.generation, 2);
    for old in &parsed.entries {
        if let Some(current) = evolved
            .entries
            .iter()
            .find(|entry| entry.expansion == old.expansion)
        {
            assert_eq!(current.code, old.code);
        }
    }
    let _ = fs::remove_dir_all(root);
}
