use crate::parser::parse_human;
use crate::pipeline::run;

const PIPELINE: &str = include_str!("../../examples/pipeline/music_preview.fgx");
const ARTIFACT_PIPELINE: &str = include_str!("../../examples/pipeline/artifact_preview.fgx");
const TYPED_BUFFER_PIPELINE: &str = include_str!("../../examples/pipeline/typed_buffers.fgx");
const RENDER_PIPELINE: &str = include_str!("../../examples/pipeline/render_preview.fgx");
const RENDER_ENCODE_PIPELINE: &str =
    include_str!("../../examples/pipeline/render_encode_preview.fgx");

#[test]
fn deterministic_pipeline_hands_one_shared_value_between_live_c_stages() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run(
        "music_preview.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.mode, "deterministic_single_threaded");
    assert_eq!(report.stages.len(), 2);
    assert_eq!(report.stages[0].source, "music_energy");
    assert_eq!(report.stages[0].target, "scene_metrics");
    assert_eq!(report.stages[1].source, "scene_metrics");
    assert_eq!(report.stages[1].target, "preview_score");
    assert!(report.stages.iter().all(|stage| stage.native));
    assert!(report.to_json().contains("\"preview_score\""));
    assert!(report.to_json().contains("\"value\":194"));
    assert!(report.to_json().contains("\"$struct\": \"SceneMetrics\""));
}

#[test]
fn deterministic_pipeline_hands_owned_artifact_to_an_immutable_slice_stage() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run(
        "artifact_preview.fgx",
        &parse_human(ARTIFACT_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    let artifact = report.stages[0]
        .artifact
        .as_ref()
        .expect("producer artifact receipt");
    let buffer = report.stages[0]
        .buffer
        .as_ref()
        .expect("producer buffer receipt");
    assert_eq!(artifact.media_type, "application/octet-stream");
    assert_eq!(artifact.bytes, 4);
    assert_eq!(artifact.sha256.len(), 64);
    assert_eq!(buffer.element_type, "u8");
    assert_eq!(buffer.elements, 4);
    assert_eq!(buffer.bytes, 4);
    assert_eq!(buffer.sha256, artifact.sha256);
    assert!(report.stages[1].artifact.is_none());
    assert_eq!(report.stages[1].input, "[1, 2, 3, 4]");
    let json = report.to_json();
    assert!(json.contains("\"type\":\"Vec[u8]\",\"value\":[1, 2, 3, 4]"));
    assert!(json.contains("\"preview_checksum\""));
    assert!(json.contains("\"value\":10"));
    assert!(json.contains("forgentex.native_artifact.v1"));
}

#[test]
fn deterministic_pipeline_hands_reviewed_typed_buffers_to_immutable_slices() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(TYPED_BUFFER_PIPELINE).unwrap();
    let allow = Some(vec!["alloc".to_string(), "cpu".to_string()]);
    let report = run("typed_buffers.fgx", &program, Vec::new(), allow.clone());
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 4);
    assert!(report.stages.iter().all(|stage| stage.native));

    let samples = report.stages[0].buffer.as_ref().expect("i16 receipt");
    assert_eq!(samples.schema, "forgentex.pipeline_buffer.v1");
    assert_eq!(samples.element_type, "i16");
    assert_eq!(samples.elements, 4);
    assert_eq!(samples.bytes, 8);
    assert_eq!(samples.encoding, "little_endian_integer_v1");
    assert_eq!(samples.sha256.len(), 64);
    assert!(report.stages[1].buffer.is_none());
    assert_eq!(report.stages[1].input, "[-200, -100, 0, 100]");

    let values = report.stages[2].buffer.as_ref().expect("i32 receipt");
    assert_eq!(values.element_type, "i32");
    assert_eq!(values.elements, 3);
    assert_eq!(values.bytes, 12);
    assert_eq!(values.sha256.len(), 64);
    assert!(report.stages[3].buffer.is_none());
    assert_eq!(report.stages[3].input, "[-200, 99800, 199800]");

    let json = report.to_json();
    assert!(json.contains("forgentex.pipeline_buffer.v1"));
    assert!(json.contains("\"type\":\"Vec[i16]\",\"value\":[-200, -100, 0, 100]"));
    assert!(json.contains("\"type\":\"Vec[i32]\",\"value\":[-200, 99800, 199800]"));
    assert!(json.contains("\"name\":\"value_sum\",\"type\":\"i64\",\"value\":299400"));

    let repeated = run("typed_buffers.fgx", &program, Vec::new(), allow);
    assert_eq!(repeated.status, "ok", "report: {repeated:?}");
    assert_eq!(repeated.shared, report.shared);
    assert_eq!(repeated.stages[0].buffer, report.stages[0].buffer);
    assert_eq!(repeated.stages[2].buffer, report.stages[2].buffer);
}

#[test]
fn typed_buffer_effect_denial_preserves_the_last_committed_shared_state() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run(
        "typed_buffers.fgx",
        &parse_human(TYPED_BUFFER_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["alloc".to_string()]),
    );
    assert_eq!(report.status, "denied", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(report.stages[0].buffer.is_some());
    assert_eq!(report.stages[1].decision, "denied");
    assert!(!report.stages[1].native);
    assert!(report.stages[1].buffer.is_none());
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "sample_sum")
            .unwrap()
            .value,
        "0"
    );
    assert!(report.shared.iter().all(|value| value.name != "values"));
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "value_sum")
            .unwrap()
            .value,
        "0"
    );
}

#[test]
fn governed_preview_pipeline_persists_image_and_emits_render_receipt() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let path = std::path::Path::new("target/render-previews/visualizer-preview.ppm");
    let _ = std::fs::remove_file(path);
    let program = parse_human(RENDER_PIPELINE).unwrap();
    let allow = Some(vec![
        "alloc".to_string(),
        "cpu".to_string(),
        "cpu.render".to_string(),
        "fs.write".to_string(),
    ]);
    let report = run("render_preview.fgx", &program, Vec::new(), allow.clone());
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(report.stages[0].render.is_none());
    let receipt = report.stages[1].render.as_ref().expect("render receipt");
    assert_eq!(receipt.schema, "forgentex.render_receipt.v1");
    assert_eq!(receipt.profile, "preview");
    assert_eq!(receipt.scene_type, "ScenePlan");
    assert_eq!(receipt.seed, 7);
    assert_eq!((receipt.width, receipt.height), (16, 16));
    assert_eq!(receipt.wall_limit_ms, Some(2000));
    assert_eq!(receipt.cpu_limit_ms, Some(1000));
    assert_eq!(receipt.memory_limit_bytes, Some(64 * 1024 * 1024));
    assert_eq!(receipt.scene_sha256.len(), 64);
    assert_eq!(receipt.artifact.bytes, 781);
    assert_eq!(
        receipt.artifact.path.as_deref(),
        Some(path.to_str().unwrap())
    );
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.starts_with(b"P6\n16 16\n255\n"));
    assert_eq!(crate::crypto::sha256_hex(&bytes), receipt.artifact.sha256);
    let json = report.to_json();
    assert!(json.contains("forgentex.render_receipt.v1"));
    assert!(json.contains("\"scene_sha256\""));
    assert!(json.contains("\"boundary_hash\""));

    // Re-running an identical approved scene is deterministic and immutable
    // publication treats identical content as idempotent.
    let repeated = run("render_preview.fgx", &program, Vec::new(), allow);
    let repeated_receipt = repeated.stages[1].render.as_ref().unwrap();
    assert_eq!(repeated_receipt.scene_sha256, receipt.scene_sha256);
    assert_eq!(repeated_receipt.artifact.sha256, receipt.artifact.sha256);
    let _ = std::fs::remove_file(path);
}

#[test]
fn governed_renderer_hands_raw_frame_to_encoder_with_linked_receipts() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let path = std::path::Path::new("target/render-previews/visualizer-encoded.ppm");
    let _ = std::fs::remove_file(path);
    let program = parse_human(RENDER_ENCODE_PIPELINE).unwrap();
    let allow = Some(vec![
        "alloc".to_string(),
        "cpu".to_string(),
        "cpu.render".to_string(),
        "media.encode".to_string(),
        "fs.write".to_string(),
    ]);
    let report = run(
        "render_encode_preview.fgx",
        &program,
        Vec::new(),
        allow.clone(),
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    assert!(report.stages[0].render.is_none());
    assert!(report.stages[0].encode.is_none());

    let render = report.stages[1].render.as_ref().expect("render receipt");
    assert_eq!(render.artifact.media_type, "application/x-forgentex-rgb");
    assert_eq!(render.artifact.bytes, 776);
    assert!(render.artifact.path.is_none());
    assert!(report.stages[1].encode.is_none());

    let encode = report.stages[2].encode.as_ref().expect("encode receipt");
    assert_eq!(encode.schema, "forgentex.encode_receipt.v1");
    assert_eq!(encode.profile, "ppm");
    assert_eq!(encode.encoder, "encode_ppm_preview");
    assert_eq!(encode.input_bytes, 776);
    assert_eq!(encode.input_sha256, render.artifact.sha256);
    assert_eq!(encode.wall_limit_ms, Some(2000));
    assert_eq!(encode.cpu_limit_ms, Some(1000));
    assert_eq!(encode.memory_limit_bytes, Some(64 * 1024 * 1024));
    assert_eq!(encode.artifact.bytes, 781);
    assert_eq!(
        encode.artifact.path.as_deref(),
        Some(path.to_str().unwrap())
    );
    assert!(report.stages[2].render.is_none());

    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.starts_with(b"P6\n16 16\n255\n"));
    assert_eq!(crate::crypto::sha256_hex(&bytes), encode.artifact.sha256);
    let json = report.to_json();
    assert!(json.contains("forgentex.encode_receipt.v1"));
    assert!(json.contains("\"input_sha256\""));
    assert!(json.contains("\"input_bytes\":776"));

    let repeated = run("render_encode_preview.fgx", &program, Vec::new(), allow);
    let repeated_render = repeated.stages[1].render.as_ref().unwrap();
    let repeated_encode = repeated.stages[2].encode.as_ref().unwrap();
    assert_eq!(repeated_render.artifact.sha256, render.artifact.sha256);
    assert_eq!(repeated_encode.input_sha256, encode.input_sha256);
    assert_eq!(repeated_encode.artifact.sha256, encode.artifact.sha256);
    let _ = std::fs::remove_file(path);
}

#[test]
fn encoder_effect_denial_persists_nothing_and_issues_no_encode_receipt() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let path = std::path::Path::new("target/render-previews/visualizer-encoded-denied.ppm");
    let _ = std::fs::remove_file(path);
    let source = RENDER_ENCODE_PIPELINE.replace(
        "target/render-previews/visualizer-encoded.ppm",
        "target/render-previews/visualizer-encoded-denied.ppm",
    );
    let report = run(
        "render_encode_preview.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "denied", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    assert!(report.stages[1].render.is_some());
    assert_eq!(report.stages[2].decision, "denied");
    assert!(!report.stages[2].native);
    assert!(report.stages[2].encode.is_none());
    assert!(!path.exists());
}

#[test]
fn encoder_statically_rejects_a_non_raw_frame_producer() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let path = std::path::Path::new("target/render-previews/bad-encoded-source.ppm");
    let _ = std::fs::remove_file(path);
    let source = RENDER_ENCODE_PIPELINE
        .replacen(
            "foreign render_rgb_preview c version \"c11\" effects [alloc, cpu.render]:\n    input scene: ScenePlan\n    output frame: Vec[u8]\n    sandbox \"fs.none\"",
            "foreign render_rgb_preview c version \"c11\" effects [alloc, cpu.render, fs.write]:\n    input scene: ScenePlan\n    output frame: Vec[u8]\n    sandbox \"fs.write target/render-previews\"",
            1,
        )
        .replacen(
            "artifact.media_type application/x-forgentex-rgb",
            "artifact.media_type image/x-portable-pixmap",
            1,
        )
        .replacen(
            "    sandbox \"artifact.max_bytes 1048576\"\n    sandbox \"render.profile preview\"",
            "    sandbox \"artifact.max_bytes 1048576\"\n    sandbox \"artifact.path target/render-previews/bad-encoded-source.ppm\"\n    sandbox \"render.profile preview\"",
            1,
        );
    let report = run(
        "bad_render_encode_preview.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        None,
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    assert!(report.stages[2]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("must publish application/x-forgentex-rgb")));
    assert!(!report.stages[2].native);
    let _ = std::fs::remove_file(path);
}

#[test]
fn preview_dimensions_are_rejected_before_renderer_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = RENDER_PIPELINE.replacen("scene.width = 16", "scene.width = 4000", 1);
    let report = run(
        "oversized_render_preview.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(!report.stages[1].native);
    assert!(report.stages[1]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("exceed the 1920x1080 preview profile")));
}

#[test]
fn preview_render_effect_denial_writes_nothing_and_issues_no_receipt() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let path = std::path::Path::new("target/render-previews/visualizer-preview-denied.ppm");
    let _ = std::fs::remove_file(path);
    let source = RENDER_PIPELINE.replace(
        "target/render-previews/visualizer-preview.ppm",
        "target/render-previews/visualizer-preview-denied.ppm",
    );
    let report = run(
        "render_preview.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "denied", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert_eq!(report.stages[1].decision, "denied");
    assert!(!report.stages[1].native);
    assert!(report.stages[1].render.is_none());
    assert!(!path.exists());
}

#[test]
fn pipeline_owned_artifact_cannot_feed_a_mutable_slice_view() {
    let source = "share bytes: Vec[u8]\nshare checksum: i32 = 0\n\nforeign mutate c version \"c11\" effects [cpu]:\n    input bytes: &mut [u8]\n    output checksum: i32\n    sandbox \"fs.none\"\n    bind bytes read\n    bind checksum rw\n    code:\n        #include <stddef.h>\n        #include <stdint.h>\n        int32_t mutate(uint8_t *bytes, size_t bytes_len) { (void)bytes; return (int32_t)bytes_len; }\n";
    let report = run(
        "mutable_artifact.fgx",
        &parse_human(source).unwrap(),
        Vec::new(),
        None,
    );
    assert_eq!(report.status, "rejected");
    assert!(report.stages[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("boundary must match"));
}

#[test]
fn pipeline_typed_owned_buffer_cannot_feed_a_mutable_slice_view() {
    let source = "share samples: Vec[i16]\nshare sum: i32 = 0\n\nforeign mutate c version \"c11\" effects [cpu]:\n    input samples: &mut [i16]\n    output sum: i32\n    sandbox \"fs.none\"\n    bind samples read\n    bind sum rw\n    code:\n        #include <stddef.h>\n        #include <stdint.h>\n        int32_t mutate(int16_t *samples, size_t samples_len) { (void)samples; return (int32_t)samples_len; }\n";
    let report = run(
        "mutable_typed_buffer.fgx",
        &parse_human(source).unwrap(),
        Vec::new(),
        None,
    );
    assert_eq!(report.status, "rejected");
    assert_eq!(report.stages.len(), 1);
    assert!(report.stages[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("boundary must match"));
    assert!(!report.stages[0].native);
}

#[test]
fn pipeline_effect_denial_happens_before_the_native_stage_runs() {
    let report = run(
        "music_preview.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["alloc".to_string()]),
    );
    assert_eq!(report.status, "denied", "report: {report:?}");
    assert_eq!(report.stages.len(), 1);
    assert_eq!(report.stages[0].decision, "denied");
    assert!(!report.stages[0].native);
}

#[test]
fn pipeline_redacts_sensitive_stage_and_shared_input_values() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = "share secret_energy: i32 = 42\nshare doubled: i32 = 0\n\nforeign double_secret c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    sandbox \"audit.redact value\"\n    bind secret_energy read\n    bind doubled rw\n    code:\n        int double_secret(int value) { return value * 2; }\n";
    let report = run(
        "redacted_pipeline.fgx",
        &parse_human(source).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages[0].input, "{\"redacted\":true}");
    assert_eq!(report.stages[0].redacted_inputs, vec!["value".to_string()]);
    let secret = report
        .shared
        .iter()
        .find(|value| value.name == "secret_energy")
        .unwrap();
    assert_eq!(secret.value, "{\"redacted\":true}");
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "doubled")
            .unwrap()
            .value,
        "84"
    );
    let json = report.to_json();
    assert!(!json.contains("\"value\":42"), "secret leaked: {json}");
    assert!(json.contains("\"redacted_inputs\":[\"value\"]"));
}

#[test]
fn pipeline_contract_rejects_a_stage_without_an_explicit_output_share() {
    let program = parse_human(
        "share input: i32 = 1\n\nforeign incomplete c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind input read\n    code:\n        int incomplete(int value) { return value; }\n",
    )
    .unwrap();
    let report = run("incomplete.fgx", &program, Vec::new(), None);
    assert_eq!(report.status, "rejected");
    assert_eq!(report.stages.len(), 1);
    assert!(report.stages[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("one rw binding"));
}

#[test]
fn pipeline_rejects_a_struct_source_before_any_stage_has_written_it() {
    let program = parse_human(
        "@repr(C)\nstruct Metrics:\n    frames: i32\n\nshare metrics: Metrics\nshare score: i32 = 0\n\nforeign render c version \"c11\" effects [cpu]:\n    input value: Metrics\n    output result: i32\n    sandbox \"fs.none\"\n    bind metrics read\n    bind score rw\n    code:\n        #include \"types.h\"\n        int render(Metrics value) { return value.frames; }\n",
    )
    .unwrap();
    let report = run("uninitialized_struct.fgx", &program, Vec::new(), None);
    assert_eq!(report.status, "rejected");
    assert_eq!(report.stages.len(), 1);
    assert!(report.stages[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("no default, override, or preceding stage output"));
}
