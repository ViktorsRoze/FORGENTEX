use crate::ast::Item;
use crate::capsule::{analyze, run, run_input, validate};
use crate::format::{format_agent, format_human};
use crate::parser::{parse_agent, parse_capsule_values, parse_human};

use super::{fixture, parse_fixture};

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("human capsule should parse")
}

#[test]
fn capsule_parses_contract_and_code_in_human_syntax() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let capsule = program.capsules().next().expect("capsule present");
    assert_eq!(capsule.id, "@capsule.normalize_py");
    assert_eq!(capsule.language, "python");
    assert_eq!(capsule.version, "3.13");
    assert_eq!(capsule.env.as_deref(), Some("cpython-3.13-linux-x86_64"));
    assert_eq!(capsule.effects, vec!["alloc".to_string()]);
    assert_eq!(capsule.inputs.len(), 1);
    assert_eq!(capsule.outputs.len(), 1);
    assert_eq!(capsule.deps.len(), 1);
    assert!(capsule.deps[0].is_pinned());
    assert!(!capsule.code.is_empty());
    // The Python comment line keeps its `#`.
    assert!(capsule.code.iter().any(|line| line.contains("# clamp")));
}

#[test]
fn capsule_roundtrips_between_human_and_agent_views() {
    let from_human = parse_fixture("examples/capsule/normalize_python.fgx");
    let from_agent =
        parse_agent(fixture("examples/capsule/normalize_python.fgai")).expect("agent parse");
    assert!(
        from_human.canonical_eq(&from_agent),
        "human and agent capsule views must share one canonical AST"
    );
}

#[test]
fn capsule_formatting_is_idempotent_in_both_syntaxes() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");

    let human_once = format_human(&program);
    let human_twice = format_human(&human(&human_once));
    assert_eq!(human_once, human_twice);

    let agent_once = format_agent(&program);
    let agent_twice = format_agent(&parse_agent(&agent_once).expect("reparse agent"));
    assert_eq!(agent_once, agent_twice);
}

#[test]
fn dependency_pinning_detection() {
    let pinned = "\
foreign demo python version \"3.13\":
    output v: i32
    dep \"pypi:numpy==2.2.0#sha256:abc\"
    sandbox \"fs.none\"
    code:
        x = 1
";
    let program = human(pinned);
    let capsule = program.capsules().next().unwrap();
    assert!(capsule.deps[0].is_pinned());

    let floating = "\
foreign demo python version \"3.13\":
    output v: i32
    dep \"pypi:numpy\"
    dep \"github:org/repo@main\"
    sandbox \"fs.none\"
    code:
        x = 1
";
    let program = human(floating);
    let capsule = program.capsules().next().unwrap();
    assert!(!capsule.deps[0].is_pinned());
    assert!(!capsule.deps[1].is_pinned());
}

#[test]
fn validation_rejects_incomplete_contracts() {
    // Unpinned dependency, no sandbox, no boundary, empty code.
    let source = "\
foreign bad python version \"3.13\":
    dep \"pypi:requests\"
    code:
";
    let program = human(source);
    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"capsule_unpinned_dependency".to_string()));
    assert!(codes.contains(&"capsule_missing_sandbox".to_string()));
    assert!(codes.contains(&"capsule_missing_typed_boundary".to_string()));
    assert!(codes.contains(&"capsule_empty_code".to_string()));
}

#[test]
fn valid_capsule_has_no_contract_diagnostics() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    assert!(validate(&program).is_empty());
}

#[test]
fn artifact_policy_requires_typed_owned_bytes_and_a_bounded_media_contract() {
    let missing_limit = human(
        "foreign render c version \"c11\":\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    code:\n        int render(void) { return 0; }\n",
    );
    let diagnostics = validate(&missing_limit);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "capsule_invalid_artifact_policy"));

    let wrong_output = human(
        "foreign render c version \"c11\":\n    output value: i32\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 1024\"\n    code:\n        int render(void) { return 0; }\n",
    );
    assert!(validate(&wrong_output).iter().any(|diagnostic| diagnostic
        .message
        .contains("requires exactly one owned Vec[u8]")));

    let unsafe_path = human(
        "foreign render c version \"c11\" effects [fs.write]:\n    output bytes: Vec[u8]\n    sandbox \"fs.write build/artifacts\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 1024\"\n    sandbox \"artifact.path ../escape.png\"\n    code:\n        int render(void) { return 0; }\n",
    );
    assert!(validate(&unsafe_path)
        .iter()
        .any(|diagnostic| diagnostic.message.contains("without parent traversal")));

    let uncovered_path = human(
        "foreign render c version \"c11\":\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 1024\"\n    sandbox \"artifact.path build/render.png\"\n    code:\n        int render(void) { return 0; }\n",
    );
    assert!(validate(&uncovered_path)
        .iter()
        .any(|diagnostic| diagnostic.message.contains("requires the fs.write effect")));
}

#[test]
fn preview_render_policy_requires_seed_artifact_effect_and_budgets() {
    let source = include_str!("../../examples/pipeline/render_preview.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_ppm_preview")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.profile, "preview");
    assert_eq!(policy.seed_field, "seed");

    let missing_budget = human(&source.replacen("    sandbox \"limit.mem_mb 64\"\n", "", 1));
    assert!(validate(&missing_budget).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("requires limit.mem_mb")
    }));

    let wrong_media =
        human(&source.replacen("image/x-portable-pixmap", "application/octet-stream", 1));
    assert!(validate(&wrong_media).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("must be an image/* or application/x-forgentex-rgb")
    }));
}

#[test]
fn render_policy_distinguishes_preview_and_final_profiles() {
    let final_source = include_str!("../../examples/pipeline/render_encode_final.fgx");
    let final_program = human(final_source);
    assert!(validate(&final_program).is_empty());
    let renderer = final_program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_final")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.profile, "final");
    assert_eq!(policy.color_space, None);
    assert_eq!(policy.device, None);
    assert_eq!(policy.render_pass, None);
    assert_eq!(policy.engine, None);
    assert_eq!(policy.engine_version, None);
    assert_eq!(policy.sequence_id, None);
    assert_eq!(policy.seed_field, "seed");
    assert_eq!(policy.frame_index_field, None);
    assert_eq!(policy.frame_count_field, None);
    assert_eq!(policy.frames_per_second, None);
    assert_eq!(policy.samples_field, None);
    assert_eq!(policy.shutter_q16_field, None);
    assert_eq!(policy.adaptive_min_samples_field, None);
    assert_eq!(policy.adaptive_noise_q16_field, None);

    let unsupported =
        human(&final_source.replacen("render.profile final", "render.profile cinematic", 1));
    assert!(validate(&unsupported).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("must be 'preview' or 'final'")
    }));
}

#[test]
fn render_quality_manifest_policy_is_complete_and_identity_distinct() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.samples_field.as_deref(), Some("samples_per_pixel"));
    assert_eq!(policy.shutter_q16_field.as_deref(), Some("shutter_q16"));

    let partial = human(&source.replacen(
        "    sandbox \"render.shutter_q16_field shutter_q16\"\n",
        "",
        1,
    ));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("requires samples_field and shutter_q16_field together")
    }));

    let collision = human(&source.replacen(
        "render.shutter_q16_field shutter_q16",
        "render.shutter_q16_field frame_index",
        1,
    ));
    assert!(validate(&collision).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("must be distinct from each other and identity fields")
    }));
}

#[test]
fn render_adaptive_manifest_policy_is_complete_quality_bound_and_distinct() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(
        policy.adaptive_min_samples_field.as_deref(),
        Some("adaptive_min_samples")
    );
    assert_eq!(
        policy.adaptive_noise_q16_field.as_deref(),
        Some("adaptive_noise_q16")
    );

    let partial = human(&source.replacen(
        "    sandbox \"render.adaptive_noise_q16_field adaptive_noise_q16\"\n",
        "",
        1,
    ));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains(
                "requires adaptive_min_samples_field and adaptive_noise_q16_field together",
            )
    }));

    let without_quality = human(
        &source
            .replacen(
                "    sandbox \"render.samples_field samples_per_pixel\"\n",
                "",
                1,
            )
            .replacen(
                "    sandbox \"render.shutter_q16_field shutter_q16\"\n",
                "",
                1,
            ),
    );
    assert!(validate(&without_quality).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("adaptive manifest requires the render quality manifest")
    }));

    let collision = human(&source.replacen(
        "render.adaptive_noise_q16_field adaptive_noise_q16",
        "render.adaptive_noise_q16_field samples_per_pixel",
        1,
    ));
    assert!(validate(&collision).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("must be distinct from each other and existing render fields")
    }));
}

#[test]
fn render_and_encode_color_space_policies_are_exact_srgb() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    assert_eq!(
        crate::capsule::render_policy(renderer)
            .unwrap()
            .unwrap()
            .color_space
            .as_deref(),
        Some("srgb")
    );
    assert_eq!(
        crate::capsule::encode_policy(encoder)
            .unwrap()
            .unwrap()
            .color_space
            .as_deref(),
        Some("srgb")
    );

    let bad_render = human(&source.replacen(
        "render.color_space srgb",
        "render.color_space display-p3",
        1,
    ));
    assert!(validate(&bad_render).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("color_space must be 'srgb'")
    }));

    let bad_encode = human(&source.replacen(
        "encode.color_space srgb",
        "encode.color_space linear-srgb",
        1,
    ));
    assert!(validate(&bad_encode).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic.message.contains("color_space must be 'srgb'")
    }));
}

#[test]
fn render_device_policy_requires_one_exact_matching_effect() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    assert_eq!(
        crate::capsule::render_policy(renderer)
            .unwrap()
            .unwrap()
            .device
            .as_deref(),
        Some("cpu")
    );

    let gpu = human(
        &source
            .replacen("render.device cpu", "render.device gpu", 1)
            .replacen(
                "effects [alloc, cpu.render]",
                "effects [alloc, gpu.render]",
                1,
            ),
    );
    assert!(validate(&gpu).is_empty());

    let unsupported = human(&source.replacen("render.device cpu", "render.device tpu", 1));
    assert!(validate(&unsupported).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("must be 'cpu' or 'gpu'")
    }));

    let mismatched = human(&source.replacen(
        "effects [alloc, cpu.render]",
        "effects [alloc, gpu.render]",
        1,
    ));
    assert!(validate(&mismatched).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("render.device 'cpu' requires exactly the matching cpu.render effect")
    }));

    let ambiguous = human(&source.replacen(
        "effects [alloc, cpu.render]",
        "effects [alloc, cpu.render, gpu.render]",
        1,
    ));
    assert!(validate(&ambiguous).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("render.device 'cpu' requires exactly the matching cpu.render effect")
    }));
}

#[test]
fn render_engine_manifest_is_atomic_and_core_versioned() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.engine.as_deref(), Some("forgentex_software_rgb8"));
    assert_eq!(policy.engine_version.as_deref(), Some("1.0.0"));

    let partial = human(&source.replacen("    sandbox \"render.engine_version 1.0.0\"\n", "", 1));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("requires engine and engine_version together")
    }));

    let bad_name = human(&source.replacen(
        "render.engine forgentex_software_rgb8",
        "render.engine Forgentex",
        1,
    ));
    assert!(validate(&bad_name).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("lowercase 1-64 character artifact token")
    }));

    for version in ["1.0", "01.0.0", "4294967296.0.0"] {
        let invalid = human(&source.replacen(
            "render.engine_version 1.0.0",
            &format!("render.engine_version {version}"),
            1,
        ));
        assert!(validate(&invalid).iter().any(|diagnostic| {
            diagnostic.code == "capsule_invalid_render_policy"
                && diagnostic
                    .message
                    .contains("exact major.minor.patch core version")
        }));
    }
}

#[test]
fn encode_engine_manifest_is_atomic_and_core_versioned() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.engine.as_deref(), Some("forgentex_png_rgb8"));
    assert_eq!(policy.engine_version.as_deref(), Some("1.0.0"));

    let partial = human(&source.replacen("    sandbox \"encode.engine_version 1.0.0\"\n", "", 1));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("requires engine and engine_version together")
    }));

    let bad_name = human(&source.replacen(
        "encode.engine forgentex_png_rgb8",
        "encode.engine ForgentexPNG",
        1,
    ));
    assert!(validate(&bad_name).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("lowercase 1-64 character artifact token")
    }));

    for version in ["1.0", "01.0.0", "4294967296.0.0"] {
        let invalid = human(&source.replacen(
            "encode.engine_version 1.0.0",
            &format!("encode.engine_version {version}"),
            1,
        ));
        assert!(validate(&invalid).iter().any(|diagnostic| {
            diagnostic.code == "capsule_invalid_encode_policy"
                && diagnostic
                    .message
                    .contains("exact major.minor.patch core version")
        }));
    }
}

#[test]
fn png_compression_manifest_is_atomic_exact_and_profile_bound() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.compression.as_deref(), Some("stored_deflate"));
    assert_eq!(policy.compression_level, Some(0));

    let partial = human(&source.replacen("    sandbox \"encode.compression_level 0\"\n", "", 1));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("requires compression and compression_level together")
    }));

    let bad_mode = human(&source.replacen(
        "encode.compression stored_deflate",
        "encode.compression dynamic_deflate",
        1,
    ));
    assert!(validate(&bad_mode).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("compression must be 'stored_deflate'")
    }));

    for level in ["1", "00", "4294967296"] {
        let invalid = human(&source.replacen(
            "encode.compression_level 0",
            &format!("encode.compression_level {level}"),
            1,
        ));
        assert!(validate(&invalid).iter().any(|diagnostic| {
            diagnostic.code == "capsule_invalid_encode_policy"
                && diagnostic
                    .message
                    .contains("exact integer 0 for stored_deflate")
        }));
    }

    let ppm = human(&source.replacen("encode.profile png", "encode.profile ppm", 1));
    assert!(validate(&ppm).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("compression manifest requires encode.profile 'png'")
    }));
}

#[test]
fn png_layout_manifest_is_atomic_exact_and_profile_bound() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.pixel_format.as_deref(), Some("rgb8"));
    assert_eq!(policy.interlace.as_deref(), Some("none"));
    assert_eq!(policy.scanline_filter.as_deref(), Some("none"));

    let partial = human(&source.replacen("    sandbox \"encode.interlace none\"\n", "", 1));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("requires pixel_format and interlace together")
    }));

    let bad_format =
        human(&source.replacen("encode.pixel_format rgb8", "encode.pixel_format rgba8", 1));
    assert!(validate(&bad_format).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic.message.contains("pixel_format must be 'rgb8'")
    }));

    let interlaced = human(&source.replacen("encode.interlace none", "encode.interlace adam7", 1));
    assert!(validate(&interlaced).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic.message.contains("interlace must be 'none'")
    }));

    let ppm = human(
        &source
            .replacen("encode.profile png", "encode.profile ppm", 1)
            .replacen("    sandbox \"encode.compression stored_deflate\"\n", "", 1)
            .replacen("    sandbox \"encode.compression_level 0\"\n", "", 1),
    );
    assert!(validate(&ppm).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("PNG layout manifest requires encode.profile 'png'")
    }));
}

#[test]
fn png_scanline_filter_is_exact_and_profile_bound() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.scanline_filter.as_deref(), Some("none"));

    let adaptive = human(&source.replacen(
        "encode.scanline_filter none",
        "encode.scanline_filter adaptive",
        1,
    ));
    assert!(validate(&adaptive).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("scanline_filter must be 'none'")
    }));

    let duplicate = human(&source.replacen(
        "    sandbox \"encode.scanline_filter none\"\n",
        "    sandbox \"encode.scanline_filter none\"\n    sandbox \"encode.scanline_filter none\"\n",
        1,
    ));
    assert!(validate(&duplicate).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("scanline_filter may be declared only once")
    }));

    let ppm = human(
        &source
            .replacen("encode.profile png", "encode.profile ppm", 1)
            .replacen("    sandbox \"encode.compression stored_deflate\"\n", "", 1)
            .replacen("    sandbox \"encode.compression_level 0\"\n", "", 1)
            .replacen("    sandbox \"encode.pixel_format rgb8\"\n", "", 1)
            .replacen("    sandbox \"encode.interlace none\"\n", "", 1),
    );
    assert!(validate(&ppm).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("scanline_filter requires encode.profile 'png'")
    }));
}

#[test]
fn png_checksum_manifest_is_atomic_exact_and_profile_bound() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.chunk_checksum.as_deref(), Some("crc32"));
    assert_eq!(policy.stream_checksum.as_deref(), Some("adler32"));

    let partial =
        human(&source.replacen("    sandbox \"encode.stream_checksum adler32\"\n", "", 1));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("requires chunk_checksum and stream_checksum together")
    }));

    for (declared, invalid, expected) in [
        (
            "chunk_checksum crc32",
            "chunk_checksum crc64",
            "chunk_checksum must be 'crc32'",
        ),
        (
            "stream_checksum adler32",
            "stream_checksum sha256",
            "stream_checksum must be 'adler32'",
        ),
    ] {
        let invalid = human(&source.replacen(declared, invalid, 1));
        assert!(validate(&invalid).iter().any(|diagnostic| {
            diagnostic.code == "capsule_invalid_encode_policy"
                && diagnostic.message.contains(expected)
        }));
    }

    let duplicate = human(&source.replacen(
        "    sandbox \"encode.chunk_checksum crc32\"\n",
        "    sandbox \"encode.chunk_checksum crc32\"\n    sandbox \"encode.chunk_checksum crc32\"\n",
        1,
    ));
    assert!(validate(&duplicate).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("chunk_checksum may be declared only once")
    }));

    let ppm = human(
        &source
            .replacen("encode.profile png", "encode.profile ppm", 1)
            .replacen("    sandbox \"encode.compression stored_deflate\"\n", "", 1)
            .replacen("    sandbox \"encode.compression_level 0\"\n", "", 1)
            .replacen("    sandbox \"encode.pixel_format rgb8\"\n", "", 1)
            .replacen("    sandbox \"encode.interlace none\"\n", "", 1)
            .replacen("    sandbox \"encode.scanline_filter none\"\n", "", 1),
    );
    assert!(validate(&ppm).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("checksum manifest requires encode.profile 'png'")
    }));
}

#[test]
fn png_chunk_order_is_exact_and_profile_bound() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.chunk_order.as_deref(), Some("ihdr_srgb_idat_iend"));

    let reordered = human(&source.replacen(
        "encode.chunk_order ihdr_srgb_idat_iend",
        "encode.chunk_order ihdr_idat_srgb_iend",
        1,
    ));
    assert!(validate(&reordered).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("chunk_order must be 'ihdr_srgb_idat_iend'")
    }));

    let duplicate = human(&source.replacen(
        "    sandbox \"encode.chunk_order ihdr_srgb_idat_iend\"\n",
        "    sandbox \"encode.chunk_order ihdr_srgb_idat_iend\"\n    sandbox \"encode.chunk_order ihdr_srgb_idat_iend\"\n",
        1,
    ));
    assert!(validate(&duplicate).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("chunk_order may be declared only once")
    }));

    let ppm = human(
        &source
            .replacen("encode.profile png", "encode.profile ppm", 1)
            .replacen("    sandbox \"encode.compression stored_deflate\"\n", "", 1)
            .replacen("    sandbox \"encode.compression_level 0\"\n", "", 1)
            .replacen("    sandbox \"encode.pixel_format rgb8\"\n", "", 1)
            .replacen("    sandbox \"encode.interlace none\"\n", "", 1)
            .replacen("    sandbox \"encode.scanline_filter none\"\n", "", 1)
            .replacen("    sandbox \"encode.chunk_checksum crc32\"\n", "", 1)
            .replacen("    sandbox \"encode.stream_checksum adler32\"\n", "", 1),
    );
    assert!(validate(&ppm).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("chunk_order requires encode.profile 'png'")
    }));
}

#[test]
fn render_sequence_manifest_requires_frame_identity_and_safe_token() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.sequence_id.as_deref(), Some("visualizer_final_v1"));

    let bad_name = human(&source.replacen(
        "render.sequence_id visualizer_final_v1",
        "render.sequence_id VisualizerFinal",
        1,
    ));
    assert!(validate(&bad_name).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("lowercase 1-64 character artifact token")
    }));

    let without_frame_manifest = human(
        &source
            .replacen(
                "    sandbox \"render.frame_index_field frame_index\"\n",
                "",
                1,
            )
            .replacen(
                "    sandbox \"render.frame_count_field frame_count\"\n",
                "",
                1,
            )
            .replacen("    sandbox \"render.frames_per_second 30\"\n", "", 1),
    );
    assert!(validate(&without_frame_manifest).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("sequence_id requires the render frame manifest")
    }));
}

#[test]
fn render_and_encode_pass_policies_use_bounded_rgb8_visualizations() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    assert_eq!(
        crate::capsule::render_policy(renderer)
            .unwrap()
            .unwrap()
            .render_pass
            .as_deref(),
        Some("beauty")
    );
    assert_eq!(
        crate::capsule::encode_policy(encoder)
            .unwrap()
            .unwrap()
            .input_pass
            .as_deref(),
        Some("beauty")
    );

    let depth = human(&source.replacen("pass beauty", "pass depth", 2));
    assert!(validate(&depth).is_empty());

    let bad_render = human(&source.replacen("render.pass beauty", "render.pass velocity", 1));
    assert!(validate(&bad_render).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("render.pass must be 'beauty'")
    }));

    let bad_encode = human(&source.replacen(
        "encode.input_pass beauty",
        "encode.input_pass cryptomatte",
        1,
    ));
    assert!(validate(&bad_encode).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("encode.input_pass must be 'beauty'")
    }));
}

#[test]
fn render_frame_manifest_policy_is_bounded_and_all_or_nothing() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_rgb_png")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.frame_index_field.as_deref(), Some("frame_index"));
    assert_eq!(policy.frame_count_field.as_deref(), Some("frame_count"));
    assert_eq!(policy.frames_per_second, Some(30));

    let partial = human(&source.replacen(
        "    sandbox \"render.frame_count_field frame_count\"\n",
        "",
        1,
    ));
    assert!(validate(&partial).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains(
                "requires frame_index_field, frame_count_field, and frames_per_second together",
            )
    }));

    let excessive_fps = human(&source.replacen(
        "render.frames_per_second 30",
        "render.frames_per_second 241",
        1,
    ));
    assert!(validate(&excessive_fps).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("from 1 through 240")
    }));
}

#[test]
fn image_encode_policy_requires_raw_input_image_output_effect_and_budgets() {
    let source = include_str!("../../examples/pipeline/render_encode_preview.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_ppm_preview")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.profile, "ppm");
    assert_eq!(policy.color_space, None);
    assert_eq!(policy.input_pass, None);
    assert_eq!(policy.engine, None);
    assert_eq!(policy.engine_version, None);
    assert_eq!(policy.compression, None);
    assert_eq!(policy.compression_level, None);
    assert_eq!(policy.pixel_format, None);
    assert_eq!(policy.interlace, None);
    assert_eq!(policy.scanline_filter, None);
    assert_eq!(policy.chunk_checksum, None);
    assert_eq!(policy.stream_checksum, None);
    assert_eq!(policy.chunk_order, None);

    let missing_budget = human(&source.replacen("    sandbox \"limit.mem_mb 64\"\n", "", 2));
    assert!(validate(&missing_budget).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic.message.contains("requires limit.mem_mb")
    }));

    let wrong_input =
        human(&source.replacen("    input frame: &[u8]", "    input frame: &mut [u8]", 1));
    assert!(validate(&wrong_input).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("exactly one immutable &[u8] input")
    }));

    let wrong_output = human(&source.replacen(
        "artifact.media_type image/x-portable-pixmap",
        "artifact.media_type application/octet-stream",
        1,
    ));
    assert!(validate(&wrong_output).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("requires artifact.media_type 'image/x-portable-pixmap'")
    }));
}

#[test]
fn image_encode_policy_allows_explicit_host_publication_without_filesystem_authority() {
    let source = include_str!("../../examples/pipeline/render_encode_preview.fgx");
    let source = source
        .replace(
            "effects [alloc, media.encode, fs.write]",
            "effects [alloc, media.encode]",
        )
        .replace(
            "    sandbox \"fs.write target/render-previews\"\n",
            "    sandbox \"fs.none\"\n",
        )
        .replace(
            "    sandbox \"artifact.path target/render-previews/visualizer-encoded.ppm\"\n",
            "    sandbox \"artifact.publisher host\"\n",
        );
    let program = human(&source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_ppm_preview")
        .unwrap();
    let artifact = crate::capsule::artifact_policy(encoder)
        .unwrap()
        .expect("artifact policy");
    assert_eq!(artifact.path, None);
    assert_eq!(artifact.publisher.as_deref(), Some("host"));

    let conflicting = human(&source.replace(
        "    sandbox \"artifact.publisher host\"\n",
        "    sandbox \"artifact.publisher host\"\n    sandbox \"artifact.path preview.ppm\"\n",
    ));
    assert!(validate(&conflicting).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_artifact_policy"
            && diagnostic.message.contains("cannot declare artifact.path")
    }));
}

#[test]
fn png_encode_policy_requires_the_png_media_type() {
    let source = include_str!("../../examples/pipeline/render_encode_png.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_png_final")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.profile, "png");
    assert_eq!(policy.color_space.as_deref(), Some("srgb"));
    assert_eq!(policy.input_pass.as_deref(), Some("beauty"));

    let mismatched = human(&source.replacen(
        "artifact.media_type image/png",
        "artifact.media_type image/x-portable-pixmap",
        1,
    ));
    assert!(validate(&mismatched).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("encode.profile 'png' requires artifact.media_type 'image/png'")
    }));
}

#[test]
fn review_report_classifies_managed_and_high_risk() {
    let managed = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = analyze("normalize_python.fgx", &managed);
    assert_eq!(report.status, "pass");
    assert_eq!(report.capsules[0].risk_class, "managed");
    assert!(!report.capsules[0].high_risk);
    assert_eq!(report.capsules[0].native_backend, "not_lowerable");

    let contract = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    let report = analyze("price_oracle_solidity.fgx", &contract);
    assert_eq!(report.status, "review_required");
    assert_eq!(report.capsules[0].risk_class, "smart_contract");
    assert!(report.capsules[0].high_risk);
    assert!(report
        .capsules
        .iter()
        .any(|c| c.notes.iter().any(|n| n.contains("secret-key"))));
}

#[test]
fn review_report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let json = analyze("normalize_python.fgx", &program).to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_review.v1\""));
    assert!(json.contains("\"risk_class\": \"managed\""));
    assert!(json.contains("\"native_backend\": \"not_lowerable\""));
    assert!(json.contains("\"pinned\": true"));
}

#[test]
fn fixtures_parse_and_validate_against_boundary() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let capsule = program.capsules().next().unwrap();
    assert_eq!(capsule.fixtures.len(), 3);
    assert!(validate(&program).is_empty());
}

#[test]
fn fixture_type_and_arity_are_rejected() {
    let source = "\
foreign t python version \"3.13\":
    input value: i32
    output normalized: i32
    sandbox \"fs.none\"
    mock 300 -> \"oops\"
    mock 1, 2 -> 3
    code:
        x = 1
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"capsule_fixture_type".to_string()));
    assert!(codes.contains(&"capsule_fixture_arity".to_string()));
}

#[test]
fn mock_runner_executes_all_fixtures() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = run("normalize_python.fgx", &program, None).expect("run");
    assert_eq!(report.status, "pass");
    assert_eq!(report.passed, 3);
    assert_eq!(report.failed, 0);
    assert!(report.to_json().contains("forgentex.capsule_run.v1"));
}

#[test]
fn mock_execution_hook_returns_matching_outputs() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let inputs = parse_capsule_values("300").unwrap();
    let outputs = run_input(&program, None, &inputs).expect("matched fixture");
    assert_eq!(outputs, vec!["255".to_string()]);

    let missing = parse_capsule_values("7").unwrap();
    assert!(run_input(&program, None, &missing).is_err());
}

#[test]
fn fixtures_survive_roundtrip() {
    let from_human = parse_fixture("examples/capsule/normalize_python.fgx");
    let agent = format_agent(&from_human);
    let from_agent = parse_agent(&agent).expect("agent parse");
    assert!(from_human.canonical_eq(&from_agent));
    let capsule = from_agent.capsules().next().unwrap();
    assert_eq!(capsule.fixtures.len(), 3);
}

#[test]
fn capsule_appears_in_canonical_ast() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"kind\": \"capsule\""));
    assert!(canonical.contains("\"id\": \"@capsule.normalize_py\""));
    assert!(matches!(program.items.last(), Some(Item::Capsule(_))));
}
