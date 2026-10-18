use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::ast::CapsuleValue;
use crate::parser::parse_human;
use crate::pipeline::{
    run, run_with_cache_control, run_with_control, run_with_limits, run_with_options,
    PipelineCache, PipelineCancellationToken, PipelineControl, PipelineLimits, PipelineSchedule,
};
use crate::{format::format_agent, parser::parse_agent};

/// Several render/encode fixtures publish to the same fixed artifact path
/// declared in their shared `.fgx` source, so they must not run concurrently:
/// one test's published frame otherwise fails another's `!path.exists()`
/// precondition. Hold this guard for the whole body of any such test.
fn render_artifact_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const PIPELINE: &str = include_str!("../../examples/pipeline/music_preview.fgx");
const ARTIFACT_PIPELINE: &str = include_str!("../../examples/pipeline/artifact_preview.fgx");
const TYPED_BUFFER_PIPELINE: &str = include_str!("../../examples/pipeline/typed_buffers.fgx");
const EXTENDED_BUFFER_PIPELINE: &str =
    include_str!("../../examples/pipeline/extended_integer_buffers.fgx");
const FLOAT_BUFFER_PIPELINE: &str = include_str!("../../examples/pipeline/float_buffers.fgx");
const WIDE_INTEGER_BUFFER_PIPELINE: &str =
    include_str!("../../examples/pipeline/wide_integer_buffers.fgx");
const RENDER_PIPELINE: &str = include_str!("../../examples/pipeline/render_preview.fgx");
const RENDER_ENCODE_PIPELINE: &str =
    include_str!("../../examples/pipeline/render_encode_preview.fgx");
const RENDER_ENCODE_FINAL_PIPELINE: &str =
    include_str!("../../examples/pipeline/render_encode_final.fgx");
const RENDER_ENCODE_PNG_PIPELINE: &str =
    include_str!("../../examples/pipeline/render_encode_png.fgx");
const FAN_IN_PIPELINE: &str = include_str!("../../examples/pipeline/fan_in.fgx");
const SOURCE_GRAPH_PIPELINE: &str = include_str!("../../examples/pipeline/source_graph.fgx");
const MOVE_HANDOFF_PIPELINE: &str = include_str!("../../examples/pipeline/move_handoff.fgx");
const UPDATE_HANDOFF_PIPELINE: &str = include_str!("../../examples/pipeline/update_handoff.fgx");
const VERSIONED_UPDATE_PIPELINE: &str =
    include_str!("../../examples/pipeline/versioned_updates.fgx");

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc ^ 0xffff_ffff
}

fn png_adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

struct ExpectedRgb8Png {
    width: u32,
    height: u32,
    seed: u32,
    glyph_count: u32,
    frame_index: u32,
    frame_count: u32,
    samples_per_pixel: u32,
    shutter_q16: u32,
    adaptive_min_samples: u32,
    adaptive_noise_q16: u32,
}

fn assert_stored_rgb8_png(bytes: &[u8], expected: ExpectedRgb8Png) {
    let ExpectedRgb8Png {
        width,
        height,
        seed,
        glyph_count,
        frame_index,
        frame_count,
        samples_per_pixel,
        shutter_q16,
        adaptive_min_samples,
        adaptive_noise_q16,
    } = expected;
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut offset = 8usize;
    let mut idat = Vec::new();
    let mut saw_ihdr = false;
    let mut saw_srgb = false;
    let mut saw_iend = false;
    let mut chunk_order = Vec::new();
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset + 12 + length;
        assert!(end <= bytes.len());
        let kind = &bytes[offset + 4..offset + 8];
        chunk_order.push(<[u8; 4]>::try_from(kind).unwrap());
        let data = &bytes[offset + 8..offset + 8 + length];
        let expected_crc = u32::from_be_bytes(bytes[offset + 8 + length..end].try_into().unwrap());
        assert_eq!(
            png_crc32(&bytes[offset + 4..offset + 8 + length]),
            expected_crc
        );
        match kind {
            b"IHDR" => {
                assert!(!saw_ihdr);
                assert_eq!(length, 13);
                assert_eq!(u32::from_be_bytes(data[..4].try_into().unwrap()), width);
                assert_eq!(u32::from_be_bytes(data[4..8].try_into().unwrap()), height);
                assert_eq!(&data[8..], &[8, 2, 0, 0, 0]);
                saw_ihdr = true;
            }
            b"sRGB" => {
                assert!(saw_ihdr);
                assert!(!saw_srgb);
                assert!(idat.is_empty());
                assert_eq!(data, &[0], "fixture uses perceptual rendering intent");
                saw_srgb = true;
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => {
                assert_eq!(length, 0);
                saw_iend = true;
                assert_eq!(end, bytes.len());
            }
            other => panic!("unexpected PNG chunk {other:?}"),
        }
        offset = end;
    }
    assert!(saw_ihdr && saw_srgb && saw_iend);
    assert_eq!(
        chunk_order,
        [*b"IHDR", *b"sRGB", *b"IDAT", *b"IEND"],
        "fixture uses one exact ordered PNG chunk sequence"
    );
    assert!(idat.len() >= 11);
    assert_eq!(&idat[..2], &[0x78, 0x01]);
    assert_eq!((u16::from(idat[0]) << 8 | u16::from(idat[1])) % 31, 0);
    let checksum_offset = idat.len() - 4;
    let mut compressed_offset = 2usize;
    let mut raw = Vec::new();
    loop {
        let header = idat[compressed_offset];
        compressed_offset += 1;
        assert!(header <= 1, "expected an uncompressed DEFLATE block");
        let final_block = header == 1;
        let length = u16::from_le_bytes(
            idat[compressed_offset..compressed_offset + 2]
                .try_into()
                .unwrap(),
        );
        let inverse = u16::from_le_bytes(
            idat[compressed_offset + 2..compressed_offset + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(inverse, !length);
        compressed_offset += 4;
        let end = compressed_offset + usize::from(length);
        assert!(end <= checksum_offset);
        raw.extend_from_slice(&idat[compressed_offset..end]);
        compressed_offset = end;
        if final_block {
            break;
        }
    }
    assert_eq!(compressed_offset, checksum_offset);
    assert_eq!(
        png_adler32(&raw),
        u32::from_be_bytes(idat[checksum_offset..].try_into().unwrap())
    );
    let row_bytes = 1 + width as usize * 3;
    assert_eq!(raw.len(), height as usize * row_bytes);
    for (y, row) in raw.chunks_exact(row_bytes).enumerate() {
        assert_eq!(row[0], 0, "fixture uses PNG filter type 0");
        for (x, pixel) in row[1..].as_chunks::<3>().0.iter().enumerate() {
            assert_eq!(
                pixel,
                &[
                    (x as u32 * 3
                        + seed * 5
                        + frame_index
                        + samples_per_pixel / 4
                        + adaptive_min_samples / 8) as u8,
                    (y as u32 * 9
                        + glyph_count * 7
                        + frame_count
                        + shutter_q16 / 256
                        + adaptive_noise_q16 / 256) as u8,
                    ((x + y) as u32 * 11
                        + seed
                        + frame_index
                        + samples_per_pixel / 8
                        + shutter_q16 / 1024
                        + adaptive_min_samples / 16
                        + adaptive_noise_q16 / 512) as u8,
                ]
            );
        }
    }
}

#[test]
fn source_graph_syntax_roundtrips_and_appears_in_canonical_ast() {
    let program = parse_human(SOURCE_GRAPH_PIPELINE).unwrap();
    let pipeline = program.pipelines().next().expect("pipeline declaration");
    assert_eq!(pipeline.name, "preview");
    assert_eq!(
        pipeline.stages,
        vec!["produce_right", "produce_left", "combine"]
    );
    assert_eq!(pipeline.edges.len(), 2);
    let agent = format_agent(&program);
    assert!(agent.contains("PIPELINE preview"), "{agent}");
    assert!(
        agent.contains("EDGE produce_left -> combine VIA left"),
        "{agent}"
    );
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"kind\": \"pipeline\""), "{canonical}");
    assert!(
        canonical.contains("\"id\": \"@pipeline.preview\""),
        "{canonical}"
    );
}

#[test]
fn source_graph_executes_authored_ready_order_and_receipts_origin() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_options(
        "source_graph.fgx",
        &parse_human(SOURCE_GRAPH_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::SourceGraph,
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.mode, "deterministic_source_graph");
    assert_eq!(
        report
            .stages
            .iter()
            .map(|stage| stage.capsule.as_str())
            .collect::<Vec<_>>(),
        vec!["produce_right", "produce_left", "combine"]
    );
    let graph = report.graph.as_ref().unwrap();
    assert_eq!(graph.origin, "source_declaration");
    assert_eq!(graph.declaration.as_deref(), Some("preview"));
    assert_eq!(
        graph.nodes,
        vec!["produce_right", "produce_left", "combine"]
    );
}

#[test]
fn source_graph_rejects_binding_disagreement_before_dispatch() {
    let source = SOURCE_GRAPH_PIPELINE.replace(
        "edge produce_right -> combine via right",
        "edge produce_right -> combine via left",
    );
    let report = run_with_options(
        "invalid_source_graph.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::SourceGraph,
    );
    assert_eq!(report.status, "rejected");
    assert!(report.stages.is_empty());
    assert!(report.graph.is_none());
    assert!(report.diagnostics[0].contains("edge manifest does not match capsule bindings"));
    assert!(report.diagnostics[0].contains("produce_right->combine via right"));
}

#[test]
fn source_graph_requires_exactly_one_nonempty_declaration() {
    let no_declaration = run_with_options(
        "missing_source_graph.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::SourceGraph,
    );
    assert_eq!(no_declaration.status, "rejected");
    assert!(no_declaration.stages.is_empty());
    assert!(no_declaration.diagnostics[0].contains("exactly one pipeline declaration; found 0"));

    let empty = parse_human("pipeline empty:\n").unwrap();
    let empty_report = run_with_options(
        "empty_source_graph.fgx",
        &empty,
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::SourceGraph,
    );
    assert_eq!(empty_report.status, "rejected");
    assert!(empty_report.diagnostics[0].contains("must declare at least one stage"));
}

#[test]
fn move_handoff_consumes_source_only_with_successful_target_commit() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(MOVE_HANDOFF_PIPELINE).unwrap();
    let report = run_with_options(
        "move_handoff.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(report.stages[0].move_sources.is_empty());
    assert!(report.stages[0].handoff_committed);
    assert_eq!(report.stages[1].move_sources, vec!["samples"]);
    assert!(report.stages[1].handoff_committed);
    assert!(report.shared.iter().all(|value| value.name != "samples"));
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "sample_sum")
            .unwrap()
            .value,
        "-200"
    );
    let json = report.to_json();
    assert!(json.contains("\"move_sources\":[\"samples\"]"), "{json}");
    assert!(json.contains("\"handoff_committed\":true"), "{json}");

    let read_program =
        parse_human(&MOVE_HANDOFF_PIPELINE.replace("bind samples move", "bind samples read"))
            .unwrap();
    let read_report = run_with_options(
        "read_handoff.fgx",
        &read_program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(read_report.status, "ok", "report: {read_report:?}");
    assert!(read_report
        .shared
        .iter()
        .any(|value| value.name == "samples"));
    assert_ne!(
        report.stages[1]
            .cache_identity
            .as_ref()
            .unwrap()
            .input_sha256,
        read_report.stages[1]
            .cache_identity
            .as_ref()
            .unwrap()
            .input_sha256
    );
}

#[test]
fn move_handoff_retains_source_on_denial_cancellation_and_budget_exhaustion() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(MOVE_HANDOFF_PIPELINE).unwrap();
    let denied = run(
        "move_denied.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string()]),
    );
    assert_eq!(denied.status, "denied", "report: {denied:?}");
    assert_eq!(denied.stages[1].move_sources, vec!["samples"]);
    assert!(!denied.stages[1].handoff_committed);
    assert!(denied.shared.iter().any(|value| value.name == "samples"));

    let cancelled = run_with_control(
        "move_cancelled.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::Declaration,
        PipelineControl {
            cancellation: None,
            stop_after_stages: Some(1),
        },
    );
    assert_eq!(cancelled.status, "cancelled", "report: {cancelled:?}");
    assert_eq!(cancelled.stages.len(), 1);
    assert!(cancelled.shared.iter().any(|value| value.name == "samples"));

    let budget_program = parse_human(
        &MOVE_HANDOFF_PIPELINE.replace("bind sample_count read", "bind sample_count move"),
    )
    .unwrap();
    let exhausted = run_with_limits(
        "move_budget_exhausted.fgx",
        &budget_program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits {
            max_owned_bytes: Some(1),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(
        exhausted.status, "budget_exhausted",
        "report: {exhausted:?}"
    );
    assert_eq!(exhausted.stages.len(), 1);
    assert_eq!(exhausted.stages[0].move_sources, vec!["sample_count"]);
    assert!(!exhausted.stages[0].handoff_committed);
    assert!(exhausted
        .shared
        .iter()
        .any(|value| value.name == "sample_count" && value.value == "4"));
    assert!(exhausted.shared.iter().all(|value| value.name != "samples"));
}

#[test]
fn move_handoff_requires_one_exclusive_consumer_before_dispatch() {
    let source = format!(
        "{}\nshare observed_sum: i32 = 0\n\nforeign observe_moved_samples c version \"c11\" effects [cpu]:\n    input samples: &[i16]\n    output sum: i32\n    sandbox \"fs.none\"\n    bind samples read\n    bind observed_sum rw\n    code:\n        int observe_moved_samples(const short *samples, unsigned long long len) {{ return len ? samples[0] : 0; }}\n",
        MOVE_HANDOFF_PIPELINE
    );
    let report = run_with_options(
        "move_fan_out.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "rejected");
    assert!(report.stages.is_empty());
    assert!(report.diagnostics[0].contains("must have exactly one exclusive consumer"));
    assert!(report.diagnostics[0].contains("sum_moved_samples:move"));
    assert!(report.diagnostics[0].contains("observe_moved_samples:read"));
}

#[test]
fn update_handoff_commits_mutated_buffer_and_target_atomically() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_options(
        "update_handoff.fgx",
        &parse_human(UPDATE_HANDOFF_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    let update = &report.stages[1];
    assert_eq!(update.update_sources, vec!["samples"]);
    assert!(update.handoff_committed);
    assert_eq!(update.updates.len(), 1);
    assert_eq!(update.updates[0].source, "samples");
    assert_eq!(update.updates[0].elements, 4);
    assert_eq!(update.updates[0].bytes, 8);
    assert_ne!(
        update.updates[0].before_sha256,
        update.updates[0].after_sha256
    );
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "samples")
            .unwrap()
            .value,
        "[-400, -200, 0, 200]"
    );
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "sample_sum")
            .unwrap()
            .value,
        "-400"
    );
    let json = report.to_json();
    assert!(json.contains("\"update_sources\":[\"samples\"]"), "{json}");
    assert!(json.contains("\"before_sha256\""), "{json}");
    assert!(json.contains("\"after_sha256\""), "{json}");
}

#[test]
fn update_handoff_rolls_back_denial_cancellation_and_budget_exhaustion() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(UPDATE_HANDOFF_PIPELINE).unwrap();
    let original = "[-200, -100, 0, 100]";

    let denied = run(
        "update_denied.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string()]),
    );
    assert_eq!(denied.status, "denied", "report: {denied:?}");
    assert_eq!(denied.stages[1].update_sources, vec!["samples"]);
    assert!(!denied.stages[1].handoff_committed);
    assert_eq!(
        denied
            .shared
            .iter()
            .find(|value| value.name == "samples")
            .unwrap()
            .value,
        original
    );
    assert_eq!(
        denied
            .shared
            .iter()
            .find(|value| value.name == "sample_sum")
            .unwrap()
            .value,
        "0"
    );

    let cancelled = run_with_control(
        "update_cancelled.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::Declaration,
        PipelineControl {
            cancellation: None,
            stop_after_stages: Some(1),
        },
    );
    assert_eq!(cancelled.status, "cancelled", "report: {cancelled:?}");
    assert_eq!(
        cancelled
            .shared
            .iter()
            .find(|value| value.name == "samples")
            .unwrap()
            .value,
        original
    );

    let exhausted = run_with_limits(
        "update_budget_exhausted.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits {
            max_owned_bytes: Some(8),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(
        exhausted.status, "budget_exhausted",
        "report: {exhausted:?}"
    );
    assert_eq!(exhausted.stages.len(), 2);
    assert_eq!(exhausted.stages[1].updates.len(), 1);
    assert!(!exhausted.stages[1].handoff_committed);
    assert_eq!(exhausted.budget.used_owned_bytes, 16);
    assert_eq!(
        exhausted
            .shared
            .iter()
            .find(|value| value.name == "samples")
            .unwrap()
            .value,
        original
    );
    assert_eq!(
        exhausted
            .shared
            .iter()
            .find(|value| value.name == "sample_sum")
            .unwrap()
            .value,
        "0"
    );
}

#[test]
fn explicit_versions_order_sequential_updates_and_retain_prior_snapshots() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(VERSIONED_UPDATE_PIPELINE).unwrap();
    let report = run_with_options(
        "versioned_updates.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(
        report
            .stages
            .iter()
            .map(|stage| stage.capsule.as_str())
            .collect::<Vec<_>>(),
        vec![
            "make_versioned_samples",
            "scale_version_zero",
            "sum_original_snapshot",
            "shift_version_one"
        ]
    );
    let updates = report
        .stages
        .iter()
        .flat_map(|stage| &stage.updates)
        .collect::<Vec<_>>();
    assert_eq!(updates.len(), 2);
    assert_eq!(
        (updates[0].source_version, updates[0].target_version),
        (0, 1)
    );
    assert_eq!(
        (updates[1].source_version, updates[1].target_version),
        (1, 2)
    );
    let shared_value = |name: &str| {
        report
            .shared
            .iter()
            .find(|value| value.name == name)
            .map(|value| value.value.as_str())
            .unwrap()
    };
    assert_eq!(shared_value("samples"), "[-200, -100, 0, 100]");
    assert_eq!(shared_value("samples@1"), "[-400, -200, 0, 200]");
    assert_eq!(shared_value("samples@2"), "[-390, -190, 10, 210]");
    assert_eq!(shared_value("original_sum"), "-200");
    assert_eq!(shared_value("scaled_sum"), "-400");
    assert_eq!(shared_value("shifted_sum"), "-360");
    let graph = report.graph.as_ref().unwrap();
    assert!(graph.edges.iter().any(|edge| {
        edge.from == "scale_version_zero"
            && edge.to == "shift_version_one"
            && edge.share == "samples@1"
    }));
    let json = report.to_json();
    assert!(
        json.contains("\"source_version\":0,\"target_version\":1"),
        "{json}"
    );
    assert!(json.contains("\"name\":\"samples@2\""), "{json}");
}

#[test]
fn versioned_update_budget_failure_withholds_only_the_new_version_and_target() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_limits(
        "versioned_update_budget.fgx",
        &parse_human(VERSIONED_UPDATE_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits {
            max_owned_bytes: Some(16),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(report.status, "budget_exhausted", "report: {report:?}");
    assert_eq!(report.stages.len(), 4);
    assert!(!report.stages[3].handoff_committed);
    assert_eq!(report.stages[3].updates[0].target_version, 2);
    assert!(report.shared.iter().any(|value| value.name == "samples@1"));
    assert!(report.shared.iter().all(|value| value.name != "samples@2"));
    assert!(report
        .shared
        .iter()
        .any(|value| value.name == "shifted_sum" && value.value == "0"));
}

#[test]
fn versioned_update_graph_rejects_missing_mixed_and_duplicate_producers() {
    let missing =
        parse_human(&VERSIONED_UPDATE_PIPELINE.replace("samples@1 update", "samples@2 update"))
            .unwrap();
    let missing_report = run_with_options(
        "missing_version.fgx",
        &missing,
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(missing_report.status, "rejected");
    assert!(missing_report.diagnostics[0].contains("samples@2"));
    assert!(missing_report.diagnostics[0].contains("no initializer or writer"));

    let mixed =
        parse_human(&VERSIONED_UPDATE_PIPELINE.replace("samples@0 read", "samples read")).unwrap();
    let mixed_report = run_with_options(
        "mixed_versions.fgx",
        &mixed,
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(mixed_report.status, "rejected");
    assert!(mixed_report.diagnostics[0].contains("mixes explicit and implicit"));

    let duplicate =
        VERSIONED_UPDATE_PIPELINE.replace("bind samples@1 update", "bind samples@0 update");
    let duplicate_report = run_with_options(
        "duplicate_version.fgx",
        &parse_human(&duplicate).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(duplicate_report.status, "rejected");
    assert!(duplicate_report.diagnostics[0].contains("only one version producer"));
}

#[test]
fn update_handoff_requires_a_reviewed_vector_and_exclusive_consumer() {
    let source = format!(
        "{}\nshare observed_sum: i32 = 0\n\nforeign observe_update_samples c version \"c11\" effects [cpu]:\n    input samples: &[i16]\n    output sum: i32\n    sandbox \"fs.none\"\n    bind samples read\n    bind observed_sum rw\n    code:\n        int observe_update_samples(const short *samples, unsigned long long len) {{ return len ? samples[0] : 0; }}\n",
        UPDATE_HANDOFF_PIPELINE
    );
    let report = run_with_options(
        "update_fan_out.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "rejected");
    assert!(report.stages.is_empty());
    assert!(report.diagnostics[0].contains("update source 'samples'"));
    assert!(report.diagnostics[0].contains("must have exactly one exclusive consumer"));

    let scalar = "share value: i32 = 1\nshare result: i32 = 0\n\nforeign invalid_update c version \"c11\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind value update\n    bind result rw\n    code:\n        int invalid_update(int value) { return value; }\n";
    let scalar_report = run(
        "scalar_update.fgx",
        &parse_human(scalar).unwrap(),
        Vec::new(),
        None,
    );
    assert_eq!(scalar_report.status, "rejected");
    assert!(scalar_report.stages[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("boundary must match")));
}

#[test]
fn budgeted_pipeline_reports_planned_and_consumed_limits() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_limits(
        "music_preview.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits {
            max_stages: Some(2),
            max_wall_ms: Some(2000),
            max_cpu_ms: None,
            max_memory_bytes: None,
            max_owned_bytes: Some(1024),
            max_fs_bytes: None,
            max_net_bytes: None,
        },
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.schema, crate::pipeline::PIPELINE_RUN_SCHEMA);
    assert_eq!(report.budget.schema, "forgentex.pipeline_budget.v1");
    assert_eq!(report.budget.decision, "within_budget");
    assert_eq!(report.budget.planned_stages, 2);
    assert_eq!(report.budget.planned_wall_ms, Some(2000));
    assert_eq!(report.budget.used_stages, 2);
    assert_eq!(report.budget.used_owned_bytes, 0);
    assert!(report
        .stages
        .iter()
        .all(|stage| stage.duration_ms.is_some()));
    let json = report.to_json();
    assert!(json.contains("\"max_wall_ms\":2000"), "{json}");
    assert!(json.contains("\"decision\":\"within_budget\""), "{json}");
}

#[test]
fn dependency_graph_reorders_producer_and_supports_deterministic_fan_out() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = "share seed: i32 = 5\nshare middle: i32\nshare left: i32 = 0\nshare right: i32 = 0\n\nforeign add_one c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind middle read\n    bind left rw\n    code:\n        int add_one(int value) { return value + 1; }\n\nforeign add_two c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind middle read\n    bind right rw\n    code:\n        int add_two(int value) { return value + 2; }\n\nforeign produce c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind seed read\n    bind middle rw\n    code:\n        int produce(int value) { return value * 2; }\n";
    let program = parse_human(source).unwrap();
    let declaration = run("fan_out.fgx", &program, Vec::new(), None);
    assert_eq!(declaration.status, "rejected");
    assert!(declaration.stages[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("no default")));

    let report = run_with_options(
        "fan_out.fgx",
        &program,
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.mode, "deterministic_dependency_graph");
    assert_eq!(
        report
            .stages
            .iter()
            .map(|stage| stage.capsule.as_str())
            .collect::<Vec<_>>(),
        vec!["produce", "add_one", "add_two"]
    );
    let graph = report.graph.as_ref().expect("graph receipt");
    assert_eq!(graph.schema, "forgentex.pipeline_graph.v1");
    assert_eq!(graph.roots, vec!["seed"]);
    assert_eq!(graph.fan_out_shares, vec!["middle"]);
    assert_eq!(graph.edges.len(), 2);
    assert_eq!(graph.execution_order, vec!["produce", "add_one", "add_two"]);
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "left")
            .unwrap()
            .value,
        "11"
    );
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "right")
            .unwrap()
            .value,
        "12"
    );
    let json = report.to_json();
    assert!(json.contains("forgentex.pipeline_graph.v1"), "{json}");
    assert!(json.contains("\"fan_out_shares\":[\"middle\"]"), "{json}");
}

#[test]
fn dependency_graph_executes_ordered_multi_input_fan_in() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_options(
        "fan_in.fgx",
        &parse_human(FAN_IN_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(
        report
            .stages
            .iter()
            .map(|stage| stage.capsule.as_str())
            .collect::<Vec<_>>(),
        vec!["produce_left", "produce_right", "combine"]
    );
    let combine = &report.stages[2];
    assert_eq!(combine.sources, vec!["left", "right"]);
    assert_eq!(combine.source, "left,right");
    assert_eq!(combine.input, "[6,12]");
    assert_eq!(combine.output.as_deref(), Some("18"));
    let graph = report.graph.as_ref().unwrap();
    assert_eq!(graph.roots, vec!["left_seed", "right_seed"]);
    assert_eq!(
        graph
            .edges
            .iter()
            .map(|edge| edge.share.as_str())
            .collect::<Vec<_>>(),
        vec!["left", "right"]
    );
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "total")
            .unwrap()
            .value,
        "18"
    );
    let json = report.to_json();
    assert!(json.contains("\"sources\":[\"left\", \"right\"]"), "{json}");
}

#[test]
fn pipeline_multi_input_contract_requires_one_ordered_read_per_input() {
    let source = "share left: i32 = 1\nshare total: i32 = 0\n\nforeign incomplete_fan_in c version \"c11\" effects [cpu]:\n    input left_value: i32\n    input right_value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind left read\n    bind total rw\n    code:\n        int incomplete_fan_in(int left_value, int right_value) { return left_value + right_value; }\n";
    let report = run(
        "incomplete_fan_in.fgx",
        &parse_human(source).unwrap(),
        Vec::new(),
        None,
    );
    assert_eq!(report.status, "rejected");
    assert_eq!(report.stages.len(), 1);
    assert!(report.stages[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("expected 2, found 1")));
    assert!(!report.stages[0].native);
}

#[test]
fn dependency_graph_rejects_a_missing_fan_in_root_before_dispatch() {
    let source = "share left: i32 = 1\nshare right: i32\nshare total: i32 = 0\n\nforeign combine c version \"c11\" effects [cpu]:\n    input left_value: i32\n    input right_value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind left read\n    bind right read\n    bind total rw\n    code:\n        int combine(int left_value, int right_value) { return left_value + right_value; }\n";
    let report = run_with_options(
        "missing_fan_in_root.fgx",
        &parse_human(source).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "rejected");
    assert!(report.stages.is_empty());
    assert!(report.graph.is_none());
    assert!(report.diagnostics[0]
        .contains("source 'right' for stage 'combine' has no initializer or writer"));
}

#[test]
fn dependency_graph_rejects_cycles_before_dispatch() {
    let source = "share x: i32\nshare y: i32\n\nforeign stage_a c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind y read\n    bind x rw\n    code:\n        int stage_a(int value) { return value; }\n\nforeign stage_b c version \"c11\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind x read\n    bind y rw\n    code:\n        int stage_b(int value) { return value; }\n";
    let report = run_with_options(
        "cycle.fgx",
        &parse_human(source).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
    );
    assert_eq!(report.status, "rejected");
    assert_eq!(report.mode, "deterministic_dependency_graph");
    assert!(report.stages.is_empty());
    assert!(report.graph.is_none());
    assert!(report.diagnostics[0].contains("cycle involving [stage_a,stage_b]"));
}

#[test]
fn pipeline_stage_and_wall_budgets_reject_before_dispatch() {
    let program = parse_human(PIPELINE).unwrap();
    let stages = run_with_limits(
        "music_preview.fgx",
        &program,
        Vec::new(),
        None,
        PipelineLimits {
            max_stages: Some(1),
            max_wall_ms: None,
            max_cpu_ms: None,
            max_memory_bytes: None,
            max_owned_bytes: None,
            max_fs_bytes: None,
            max_net_bytes: None,
        },
    );
    assert_eq!(stages.status, "budget_exhausted");
    assert!(stages.stages.is_empty());
    assert_eq!(stages.budget.decision, "exhausted");
    assert!(stages.diagnostics[0].contains("planned stage count 2"));

    let wall = run_with_limits(
        "music_preview.fgx",
        &program,
        Vec::new(),
        None,
        PipelineLimits {
            max_stages: None,
            max_wall_ms: Some(1999),
            max_cpu_ms: None,
            max_memory_bytes: None,
            max_owned_bytes: None,
            max_fs_bytes: None,
            max_net_bytes: None,
        },
    );
    assert_eq!(wall.status, "budget_exhausted");
    assert!(wall.stages.is_empty());
    assert!(wall.diagnostics[0].contains("declared stage total 2000"));

    let unbounded_stage = PIPELINE.replacen("    sandbox \"limit.wall_ms 1000\"\n", "", 1);
    let missing = run_with_limits(
        "missing_wall.fgx",
        &parse_human(&unbounded_stage).unwrap(),
        Vec::new(),
        None,
        PipelineLimits {
            max_stages: None,
            max_wall_ms: Some(2000),
            max_cpu_ms: None,
            max_memory_bytes: None,
            max_owned_bytes: None,
            max_fs_bytes: None,
            max_net_bytes: None,
        },
    );
    assert_eq!(missing.status, "rejected");
    assert!(missing.stages.is_empty());
    assert!(missing.diagnostics[0].contains("missing [analyze_music]"));
}

#[test]
fn pipeline_cpu_and_peak_memory_budgets_are_receipted_and_enforced() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = PIPELINE.replace(
        "    sandbox \"limit.wall_ms 1000\"\n",
        "    sandbox \"limit.wall_ms 1000\"\n    sandbox \"limit.cpu_ms 1000\"\n    sandbox \"limit.mem_mb 256\"\n",
    );
    let report = run_with_limits(
        "resource_budget.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits {
            max_stages: Some(2),
            max_wall_ms: Some(2000),
            max_cpu_ms: Some(2000),
            max_memory_bytes: Some(256 * 1024 * 1024),
            max_owned_bytes: None,
            max_fs_bytes: None,
            max_net_bytes: None,
        },
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.budget.planned_cpu_ms, Some(2000));
    assert_eq!(
        report.budget.planned_peak_memory_bytes,
        Some(256 * 1024 * 1024)
    );
    assert_eq!(report.budget.max_cpu_ms, Some(2000));
    assert_eq!(report.budget.max_memory_bytes, Some(256 * 1024 * 1024));
    assert_eq!(report.budget.decision, "within_budget");
    let json = report.to_json();
    assert!(json.contains("\"planned_cpu_ms\":2000"), "{json}");
    assert!(
        json.contains("\"planned_peak_memory_bytes\":268435456"),
        "{json}"
    );
}

#[test]
fn pipeline_cpu_and_peak_memory_overruns_reject_before_dispatch() {
    let source = PIPELINE.replace(
        "    sandbox \"limit.wall_ms 1000\"\n",
        "    sandbox \"limit.wall_ms 1000\"\n    sandbox \"limit.cpu_ms 1000\"\n    sandbox \"limit.mem_mb 256\"\n",
    );
    let program = parse_human(&source).unwrap();
    let cpu = run_with_limits(
        "cpu_budget.fgx",
        &program,
        Vec::new(),
        None,
        PipelineLimits {
            max_cpu_ms: Some(1999),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(cpu.status, "budget_exhausted");
    assert!(cpu.stages.is_empty());
    assert_eq!(cpu.budget.planned_cpu_ms, Some(2000));
    assert!(cpu.diagnostics[0].contains("declared stage total 2000"));

    let memory = run_with_limits(
        "memory_budget.fgx",
        &program,
        Vec::new(),
        None,
        PipelineLimits {
            max_memory_bytes: Some(256 * 1024 * 1024 - 1),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(memory.status, "budget_exhausted");
    assert!(memory.stages.is_empty());
    assert_eq!(
        memory.budget.planned_peak_memory_bytes,
        Some(256 * 1024 * 1024)
    );
    assert!(memory.diagnostics[0].contains("declared sequential peak 268435456"));

    let missing = run_with_limits(
        "missing_cpu_budget.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        None,
        PipelineLimits {
            max_cpu_ms: Some(2000),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(missing.status, "rejected");
    assert!(missing.stages.is_empty());
    assert!(missing.diagnostics[0].contains("limit.cpu_ms"));
}

#[test]
fn pipeline_owned_byte_budget_prevents_oversized_share_commit() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_limits(
        "artifact_preview.fgx",
        &parse_human(ARTIFACT_PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits {
            max_stages: None,
            max_wall_ms: None,
            max_cpu_ms: None,
            max_memory_bytes: None,
            max_owned_bytes: Some(3),
            max_fs_bytes: None,
            max_net_bytes: None,
        },
    );
    assert_eq!(report.status, "budget_exhausted", "report: {report:?}");
    assert_eq!(report.stages.len(), 1);
    assert!(report.stages[0].native);
    assert!(report.stages[0].output.is_none());
    assert_eq!(report.stages[0].buffer.as_ref().unwrap().bytes, 4);
    assert!(report.stages[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("max_owned_bytes 3 exceeded by 4 bytes")));
    assert_eq!(report.budget.decision, "exhausted");
    assert_eq!(report.budget.used_owned_bytes, 4);
    assert!(report
        .shared
        .iter()
        .all(|value| value.name != "preview_bytes"));
}

#[test]
fn pipeline_filesystem_transfer_budget_is_cumulative_and_atomic_on_windows() {
    if !cfg!(windows) || crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(PIPELINE).unwrap();
    let admitted = run_with_limits(
        "music_preview.fgx",
        &program,
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits {
            max_fs_bytes: Some(1024 * 1024 * 1024),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(admitted.status, "ok", "report: {admitted:?}");
    assert!(admitted.budget.used_fs_bytes.is_some_and(|bytes| bytes > 0));
    assert!(admitted.stages.iter().all(|stage| stage
        .io
        .as_ref()
        .is_some_and(|io| io.source == "windows_job_object_accounting")));
    let admitted_json = admitted.to_json();
    assert!(admitted_json.contains("forgentex.pipeline_stage_io.v1"));
    assert!(admitted_json.contains("\"max_fs_bytes\":1073741824"));
    assert!(admitted_json.contains("\"used_fs_bytes\":"));

    let exhausted = run_with_limits(
        "music_preview.fgx",
        &program,
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits {
            max_fs_bytes: Some(1),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(
        exhausted.status, "budget_exhausted",
        "report: {exhausted:?}"
    );
    assert_eq!(exhausted.stages.len(), 1);
    assert!(exhausted.stages[0].output.is_none());
    assert!(exhausted.stages[0].cache_identity.is_none());
    assert!(exhausted.stages[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("max_fs_bytes 1 exceeded")));
    assert!(exhausted
        .shared
        .iter()
        .all(|value| value.name != "scene_metrics"));
    assert_eq!(exhausted.budget.decision, "exhausted");
    assert!(exhausted
        .budget
        .used_fs_bytes
        .is_some_and(|bytes| bytes > 1));
}

#[test]
fn pipeline_unavailable_io_budgets_fail_before_dispatch() {
    let program = parse_human(PIPELINE).unwrap();
    let network = run_with_limits(
        "music_preview.fgx",
        &program,
        Vec::new(),
        None,
        PipelineLimits {
            max_net_bytes: Some(1),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(network.status, "rejected");
    assert!(network.stages.is_empty());
    assert!(network.diagnostics[0].contains("aggregate network accounting"));

    if !cfg!(windows) {
        let filesystem = run_with_limits(
            "music_preview.fgx",
            &program,
            Vec::new(),
            None,
            PipelineLimits {
                max_fs_bytes: Some(1),
                ..PipelineLimits::default()
            },
        );
        assert_eq!(filesystem.status, "rejected");
        assert!(filesystem.stages.is_empty());
        assert!(filesystem.diagnostics[0].contains("aggregate OS accounting"));
    } else {
        let managed = parse_human(
            "share source: i32 = 1\nshare target: i32 = 0\nforeign managed python version \"3.12\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    bind source read\n    bind target rw\n    code:\n        def managed(value):\n            return value\n",
        )
        .unwrap();
        let filesystem = run_with_limits(
            "managed_pipeline.fgx",
            &managed,
            Vec::new(),
            None,
            PipelineLimits {
                max_fs_bytes: Some(1),
                ..PipelineLimits::default()
            },
        );
        assert_eq!(filesystem.status, "rejected");
        assert!(filesystem.stages.is_empty());
        assert!(filesystem.diagnostics[0].contains("unsupported [managed]"));
    }
}

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
fn pipeline_can_cancel_deterministically_after_a_committed_stage() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let report = run_with_control(
        "music_preview.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        Some(vec!["cpu".to_string()]),
        PipelineLimits::default(),
        PipelineSchedule::Declaration,
        PipelineControl {
            cancellation: None,
            stop_after_stages: Some(1),
        },
    );
    assert_eq!(report.status, "cancelled", "report: {report:?}");
    assert_eq!(report.stages.len(), 1);
    assert_eq!(report.stages[0].capsule, "analyze_music");
    assert!(report.stages[0].output.is_some());
    assert!(report
        .shared
        .iter()
        .any(|value| value.name == "scene_metrics"));
    assert_eq!(
        report
            .shared
            .iter()
            .find(|value| value.name == "preview_score")
            .unwrap()
            .value,
        "0"
    );
    let cancellation = report.cancellation.as_ref().expect("cancellation receipt");
    assert_eq!(cancellation.schema, "forgentex.pipeline_cancellation.v1");
    assert_eq!(cancellation.source, "stage_limit");
    assert_eq!(cancellation.decision, "cancelled");
    assert_eq!(cancellation.committed_stages, 1);
    assert_eq!(cancellation.after_stage.as_deref(), Some("analyze_music"));
    let json = report.to_json();
    assert!(json.contains("\"cancellation\":{\"schema\":\"forgentex.pipeline_cancellation.v1\""));
    assert!(json.contains("\"committed_stages\":1"));
}

#[test]
fn pre_cancelled_pipeline_token_prevents_stage_dispatch() {
    let token = PipelineCancellationToken::default();
    token.cancel();
    let report = run_with_control(
        "music_preview.fgx",
        &parse_human(PIPELINE).unwrap(),
        Vec::new(),
        None,
        PipelineLimits::default(),
        PipelineSchedule::Declaration,
        PipelineControl {
            cancellation: Some(token),
            stop_after_stages: None,
        },
    );
    assert_eq!(report.status, "cancelled", "report: {report:?}");
    assert!(report.stages.is_empty());
    let cancellation = report.cancellation.as_ref().expect("cancellation receipt");
    assert_eq!(cancellation.source, "external_token");
    assert_eq!(cancellation.decision, "cancelled");
    assert_eq!(cancellation.committed_stages, 0);
    assert_eq!(cancellation.after_stage, None);
    assert!(report.to_json().contains("\"after_stage\":null"));
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
fn process_local_cache_restores_validated_read_only_native_stages() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(PIPELINE).unwrap();
    let allow = Some(vec!["cpu".to_string()]);
    let mut cache = PipelineCache::default();
    let first = run_with_cache_control(
        "music_preview.fgx",
        &program,
        Vec::new(),
        allow.clone(),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
        PipelineControl::default(),
        &mut cache,
    );
    assert_eq!(first.status, "ok", "report: {first:?}");
    assert_eq!(cache.len(), 2);
    assert!(first.stages.iter().all(|stage| stage.native));
    assert!(first
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));
    assert!(first.stages.iter().all(|stage| {
        stage
            .cache_identity
            .as_ref()
            .is_some_and(|identity| identity.reuse == "stored_process_local")
    }));

    let second = run_with_cache_control(
        "music_preview.fgx",
        &program,
        Vec::new(),
        allow,
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
        PipelineControl::default(),
        &mut cache,
    );
    assert_eq!(second.status, "ok", "report: {second:?}");
    assert_eq!(second.shared, first.shared);
    assert!(second.stages.iter().all(|stage| !stage.native));
    assert!(second
        .stages
        .iter()
        .all(|stage| stage.decision == "cache_hit" && stage.duration_ms == Some(0)));
    assert!(second.stages.iter().all(|stage| {
        stage.cache_restore.as_ref().is_some_and(|restore| {
            restore.schema == "forgentex.pipeline_cache_restore.v1"
                && restore.decision == "verified_hit"
                && restore.request_sha256.len() == 64
                && restore.output_sha256.len() == 64
        })
    }));
    assert!(second.stages.iter().all(|stage| {
        stage
            .cache_identity
            .as_ref()
            .is_some_and(|identity| identity.reuse == "verified_process_local_hit")
    }));
    assert_eq!(second.budget.used_wall_ms, 0);
    let json = second.to_json();
    assert!(
        json.contains("\"cache_restore\":{\"schema\":\"forgentex.pipeline_cache_restore.v1\""),
        "{json}"
    );
    assert!(
        json.contains("\"storage\":\"bounded_process_local_memory\""),
        "{json}"
    );

    let denied = run_with_cache_control(
        "music_preview.fgx",
        &program,
        Vec::new(),
        Some(Vec::new()),
        PipelineLimits::default(),
        PipelineSchedule::DependencyGraph,
        PipelineControl::default(),
        &mut cache,
    );
    assert_eq!(denied.status, "denied", "report: {denied:?}");
    assert!(denied.stages[0].cache_restore.is_none());
}

#[test]
fn process_local_cache_misses_on_input_limit_or_integrity_change() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(PIPELINE).unwrap();
    let allow = Some(vec!["cpu".to_string()]);
    let mut cache = PipelineCache::default();
    let populate = |cache: &mut PipelineCache, overrides, limits| {
        run_with_cache_control(
            "music_preview.fgx",
            &program,
            overrides,
            allow.clone(),
            limits,
            PipelineSchedule::DependencyGraph,
            PipelineControl::default(),
            cache,
        )
    };
    assert_eq!(
        populate(&mut cache, Vec::new(), PipelineLimits::default()).status,
        "ok"
    );

    let changed_input = populate(
        &mut cache,
        vec![("music_energy".to_string(), CapsuleValue::Int(5))],
        PipelineLimits::default(),
    );
    assert!(changed_input.stages.iter().all(|stage| stage.native));
    assert!(changed_input
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));

    let changed_limits = populate(
        &mut cache,
        Vec::new(),
        PipelineLimits {
            max_stages: Some(2),
            ..PipelineLimits::default()
        },
    );
    assert!(changed_limits.stages.iter().all(|stage| stage.native));

    let mut tamper_cache = PipelineCache::default();
    assert_eq!(
        populate(&mut tamper_cache, Vec::new(), PipelineLimits::default()).status,
        "ok"
    );
    tamper_cache.corrupt_all_entries_for_test();
    let after_corruption = populate(&mut tamper_cache, Vec::new(), PipelineLimits::default());
    assert_eq!(
        after_corruption.status, "ok",
        "report: {after_corruption:?}"
    );
    assert_eq!(
        after_corruption
            .stages
            .iter()
            .filter(|stage| stage.native)
            .count(),
        2
    );
    assert_eq!(
        after_corruption
            .stages
            .iter()
            .filter(|stage| stage.cache_restore.is_some())
            .count(),
        0
    );
}

#[test]
fn persistent_cache_restores_across_instances_and_recovers_without_clobbering() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "forgentex_pipeline_cache_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let program = parse_human(PIPELINE).unwrap();
    let execute = |cache: &mut PipelineCache| {
        run_with_cache_control(
            "music_preview.fgx",
            &program,
            Vec::new(),
            Some(vec!["cpu".to_string()]),
            PipelineLimits::default(),
            PipelineSchedule::DependencyGraph,
            PipelineControl::default(),
            cache,
        )
    };

    let mut first_cache = PipelineCache::persistent(&root).unwrap();
    let first = execute(&mut first_cache);
    assert_eq!(first.status, "ok", "report: {first:?}");
    assert!(first.stages.iter().all(|stage| {
        stage.cache_identity.as_ref().is_some_and(|identity| {
            identity.reuse == "stored_process_local_and_integrity_checked_file"
        })
    }));
    let original_files = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(original_files.len(), 2);

    let mut second_cache = PipelineCache::persistent(&root).unwrap();
    let second = execute(&mut second_cache);
    assert_eq!(second.status, "ok", "report: {second:?}");
    assert!(second.stages.iter().all(|stage| {
        !stage.native
            && stage.decision == "cache_hit"
            && stage.cache_restore.as_ref().is_some_and(|restore| {
                restore.storage == "integrity_checked_content_addressed_file"
            })
            && stage
                .cache_identity
                .as_ref()
                .is_some_and(|identity| identity.reuse == "verified_integrity_checked_file_hit")
    }));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);

    let mut corrupted = Vec::new();
    for path in &original_files {
        let mut bytes = std::fs::read(path).unwrap();
        let last = bytes.last_mut().expect("cache file is non-empty");
        *last ^= 1;
        std::fs::write(path, &bytes).unwrap();
        corrupted.push((path.clone(), bytes));
    }
    let mut recovery_cache = PipelineCache::persistent(&root).unwrap();
    let recovered = execute(&mut recovery_cache);
    assert_eq!(recovered.status, "ok", "report: {recovered:?}");
    assert!(recovered.stages.iter().all(|stage| stage.native));
    assert!(recovered
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));
    assert!(std::fs::read_dir(&root).unwrap().count() > 2);
    for (path, bytes) in corrupted {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    let mut healed_cache = PipelineCache::persistent(&root).unwrap();
    let healed = execute(&mut healed_cache);
    assert_eq!(healed.status, "ok", "report: {healed:?}");
    assert!(healed
        .stages
        .iter()
        .all(|stage| stage.decision == "cache_hit"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn persistent_cache_requires_an_existing_real_directory_and_excludes_secrets() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_pipeline_cache_policy_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    assert!(PipelineCache::persistent(&root).is_err());
    std::fs::write(&root, b"not a directory").unwrap();
    assert!(PipelineCache::persistent(&root).is_err());
    std::fs::remove_file(&root).unwrap();
    std::fs::create_dir(&root).unwrap();

    let link = root.with_extension("link");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(PipelineCache::persistent(&link).is_err());
        std::fs::remove_file(&link).unwrap();
    }
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(&root, &link).is_ok() {
        assert!(PipelineCache::persistent(&link).is_err());
        std::fs::remove_dir(&link).unwrap();
    }

    if crate::native_build::detect_toolchain().is_some() {
        let sensitive = PIPELINE.replacen(
            "sandbox \"fs.none\"",
            "sandbox \"fs.none\"\n    sandbox \"audit.redact energy\"",
            1,
        );
        let program = parse_human(&sensitive).unwrap();
        let mut cache = PipelineCache::persistent(&root).unwrap();
        let report = run_with_cache_control(
            "sensitive.fgx",
            &program,
            Vec::new(),
            Some(vec!["cpu".to_string()]),
            PipelineLimits::default(),
            PipelineSchedule::DependencyGraph,
            PipelineControl::default(),
            &mut cache,
        );
        assert_eq!(report.status, "ok", "report: {report:?}");
        assert!(cache.is_empty());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn authenticated_persistent_cache_requires_the_matching_key_without_downgrade() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "forgentex_pipeline_authenticated_cache_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let key = b"forgentex-authenticated-cache-key-01";
    let wrong_key = b"forgentex-authenticated-cache-key-02";
    let program = parse_human(PIPELINE).unwrap();
    let execute = |cache: &mut PipelineCache| {
        run_with_cache_control(
            "music_preview.fgx",
            &program,
            Vec::new(),
            Some(vec!["cpu".to_string()]),
            PipelineLimits::default(),
            PipelineSchedule::DependencyGraph,
            PipelineControl::default(),
            cache,
        )
    };

    assert!(PipelineCache::authenticated_persistent(&root, b"short").is_err());
    let mut writer = PipelineCache::authenticated_persistent(&root, key).unwrap();
    assert!(!format!("{writer:?}").contains("forgentex-authenticated-cache-key"));
    let first = execute(&mut writer);
    assert_eq!(first.status, "ok", "report: {first:?}");
    assert!(first.stages.iter().all(|stage| {
        stage
            .cache_identity
            .as_ref()
            .is_some_and(|identity| identity.reuse == "stored_process_local_and_authenticated_file")
    }));
    for item in std::fs::read_dir(&root).unwrap() {
        let bytes = std::fs::read(item.unwrap().path()).unwrap();
        assert!(!bytes.windows(key.len()).any(|window| window == key));
    }

    let mut reader = PipelineCache::authenticated_persistent(&root, key).unwrap();
    let restored = execute(&mut reader);
    assert_eq!(restored.status, "ok", "report: {restored:?}");
    assert!(restored.stages.iter().all(|stage| {
        stage.decision == "cache_hit"
            && !stage.native
            && stage
                .cache_identity
                .as_ref()
                .is_some_and(|identity| identity.reuse == "verified_authenticated_file_hit")
            && stage.cache_restore.as_ref().is_some_and(|restore| {
                restore.storage == "authenticated_hmac_sha256_content_addressed_file"
            })
    }));

    let mut wrong_reader = PipelineCache::authenticated_persistent(&root, wrong_key).unwrap();
    let wrong = execute(&mut wrong_reader);
    assert_eq!(wrong.status, "ok", "report: {wrong:?}");
    assert!(wrong.stages.iter().all(|stage| stage.native));
    assert!(wrong
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));

    let mut integrity_reader = PipelineCache::persistent(&root).unwrap();
    let no_downgrade = execute(&mut integrity_reader);
    assert_eq!(no_downgrade.status, "ok", "report: {no_downgrade:?}");
    assert!(no_downgrade.stages.iter().all(|stage| stage.native));
    assert!(no_downgrade
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));

    let mut reader_again = PipelineCache::authenticated_persistent(&root, key).unwrap();
    let restored_again = execute(&mut reader_again);
    assert!(restored_again
        .stages
        .iter()
        .all(|stage| stage.decision == "cache_hit"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn authenticated_persistent_cache_rejects_tampered_tags() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "forgentex_pipeline_authenticated_tamper_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let key = b"forgentex-authenticated-cache-key-01";
    let program = parse_human(PIPELINE).unwrap();
    let execute = |cache: &mut PipelineCache| {
        run_with_cache_control(
            "music_preview.fgx",
            &program,
            Vec::new(),
            Some(vec!["cpu".to_string()]),
            PipelineLimits::default(),
            PipelineSchedule::DependencyGraph,
            PipelineControl::default(),
            cache,
        )
    };
    let mut writer = PipelineCache::authenticated_persistent(&root, key).unwrap();
    assert_eq!(execute(&mut writer).status, "ok");
    for item in std::fs::read_dir(&root).unwrap() {
        let path = item.unwrap().path();
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().expect("authenticated tag") ^= 1;
        std::fs::write(path, bytes).unwrap();
    }
    let mut reader = PipelineCache::authenticated_persistent(&root, key).unwrap();
    let report = execute(&mut reader);
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert!(report.stages.iter().all(|stage| stage.native));
    assert!(report
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn process_local_cache_restores_pathless_artifacts_and_excludes_side_effectful_stages() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let mut cache = PipelineCache::default();
    let mut run_twice = |source: &str, allow: Option<Vec<String>>| {
        let program = parse_human(source).unwrap();
        let first = run_with_cache_control(
            "ineligible.fgx",
            &program,
            Vec::new(),
            allow.clone(),
            PipelineLimits::default(),
            PipelineSchedule::Declaration,
            PipelineControl::default(),
            &mut cache,
        );
        let second = run_with_cache_control(
            "ineligible.fgx",
            &program,
            Vec::new(),
            allow,
            PipelineLimits::default(),
            PipelineSchedule::Declaration,
            PipelineControl::default(),
            &mut cache,
        );
        (first, second)
    };

    let sensitive = PIPELINE.replacen(
        "sandbox \"fs.none\"",
        "sandbox \"fs.none\"\n    sandbox \"audit.redact energy\"",
        1,
    );
    let (_, sensitive_second) = run_twice(&sensitive, Some(vec!["cpu".to_string()]));
    assert!(sensitive_second
        .stages
        .iter()
        .all(|stage| stage.cache_restore.is_none()));
    assert!(sensitive_second
        .stages
        .iter()
        .all(|stage| stage.cache_identity.is_none()));

    let (_, artifact_second) = run_twice(
        ARTIFACT_PIPELINE,
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
    );
    assert!(artifact_second
        .stages
        .iter()
        .all(|stage| stage.decision == "cache_hit"));
    let restored_artifact = artifact_second.stages[0]
        .artifact
        .as_ref()
        .expect("restored artifact receipt");
    assert_eq!(restored_artifact.schema, "forgentex.native_artifact.v1");
    assert_eq!(restored_artifact.media_type, "application/octet-stream");
    assert_eq!(restored_artifact.bytes, 4);
    assert_eq!(restored_artifact.path, None);
    assert_eq!(
        restored_artifact.sha256,
        artifact_second.stages[0].buffer.as_ref().unwrap().sha256
    );

    let (_, update_second) = run_twice(
        UPDATE_HANDOFF_PIPELINE,
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
    );
    assert!(update_second.stages[1].cache_restore.is_none());

    let render_path = std::path::Path::new("target/render-previews/visualizer-preview.ppm");
    let _ = std::fs::remove_file(render_path);
    let (_, render_second) = run_twice(
        RENDER_PIPELINE,
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert!(render_second.stages[1].cache_restore.is_none());
    assert!(render_second.stages[1].native);
    let _ = std::fs::remove_file(render_path);
}

#[test]
fn authenticated_persistent_cache_restores_pathless_artifact_receipts() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "forgentex_pipeline_artifact_cache_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let key = b"forgentex-authenticated-cache-key-01";
    let program = parse_human(ARTIFACT_PIPELINE).unwrap();
    let execute = |cache: &mut PipelineCache, program| {
        run_with_cache_control(
            "artifact_preview.fgx",
            program,
            Vec::new(),
            Some(vec!["alloc".to_string(), "cpu".to_string()]),
            PipelineLimits::default(),
            PipelineSchedule::Declaration,
            PipelineControl::default(),
            cache,
        )
    };

    let mut writer = PipelineCache::authenticated_persistent(&root, key).unwrap();
    let first = execute(&mut writer, &program);
    assert_eq!(first.status, "ok", "report: {first:?}");
    let first_artifact = first.stages[0].artifact.clone().expect("artifact receipt");

    let mut reader = PipelineCache::authenticated_persistent(&root, key).unwrap();
    let restored = execute(&mut reader, &program);
    assert_eq!(restored.status, "ok", "report: {restored:?}");
    assert!(restored
        .stages
        .iter()
        .all(|stage| stage.decision == "cache_hit" && !stage.native));
    assert_eq!(restored.stages[0].artifact, Some(first_artifact));
    assert!(restored.stages[0]
        .cache_restore
        .as_ref()
        .is_some_and(|receipt| {
            receipt.storage == "authenticated_hmac_sha256_content_addressed_file"
        }));

    let changed_program = parse_human(&ARTIFACT_PIPELINE.replace(
        "artifact.media_type application/octet-stream",
        "artifact.media_type application/x-forgentex-changed",
    ))
    .unwrap();
    let mut policy_reader = PipelineCache::authenticated_persistent(&root, key).unwrap();
    let changed = execute(&mut policy_reader, &changed_program);
    assert_eq!(changed.status, "ok", "report: {changed:?}");
    assert!(changed.stages.iter().all(|stage| stage.native));
    assert_eq!(
        changed.stages[0].artifact.as_ref().unwrap().media_type,
        "application/x-forgentex-changed"
    );
    std::fs::remove_dir_all(root).unwrap();
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
    assert!(report
        .stages
        .iter()
        .all(|stage| stage.cache_identity.is_some()));

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
    assert_eq!(
        repeated
            .stages
            .iter()
            .map(|stage| stage.cache_identity.as_ref())
            .collect::<Vec<_>>(),
        report
            .stages
            .iter()
            .map(|stage| stage.cache_identity.as_ref())
            .collect::<Vec<_>>()
    );

    let changed_input = run(
        "typed_buffers.fgx",
        &program,
        vec![("sample_count".to_string(), CapsuleValue::Int(3))],
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
    );
    assert_eq!(changed_input.status, "ok", "report: {changed_input:?}");
    let original_identity = report.stages[0]
        .cache_identity
        .as_ref()
        .expect("original cache identity");
    let changed_identity = changed_input.stages[0]
        .cache_identity
        .as_ref()
        .expect("changed-input cache identity");
    assert_ne!(
        changed_identity.input_sha256,
        original_identity.input_sha256
    );
    assert_ne!(changed_identity.key_sha256, original_identity.key_sha256);
    assert_eq!(changed_identity.code_sha256, original_identity.code_sha256);
    assert_eq!(
        changed_identity.policy_sha256,
        original_identity.policy_sha256
    );

    let changed_limits = run_with_limits(
        "typed_buffers.fgx",
        &program,
        Vec::new(),
        Some(vec!["alloc".to_string(), "cpu".to_string()]),
        PipelineLimits {
            max_stages: Some(4),
            ..PipelineLimits::default()
        },
    );
    assert_eq!(changed_limits.status, "ok", "report: {changed_limits:?}");
    let changed_limit_identity = changed_limits.stages[0]
        .cache_identity
        .as_ref()
        .expect("changed-limit cache identity");
    assert_ne!(
        changed_limit_identity.limits_sha256,
        original_identity.limits_sha256
    );
    assert_ne!(
        changed_limit_identity.key_sha256,
        original_identity.key_sha256
    );
    assert_eq!(
        changed_limit_identity.input_sha256,
        original_identity.input_sha256
    );
    assert_eq!(
        changed_limit_identity.code_sha256,
        original_identity.code_sha256
    );
}

#[test]
fn deterministic_pipeline_hands_extended_owned_buffer_types() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(EXTENDED_BUFFER_PIPELINE).unwrap();
    let allow = Some(vec!["alloc".to_string(), "cpu".to_string()]);
    let report = run(
        "extended_integer_buffers.fgx",
        &program,
        Vec::new(),
        allow.clone(),
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(report.stages.iter().all(|stage| stage.native));

    let u32_values = report.stages[0].buffer.as_ref().expect("u32 receipt");
    assert_eq!(u32_values.element_type, "u32");
    assert_eq!(u32_values.elements, 3);
    assert_eq!(u32_values.bytes, 12);

    assert!(report.stages[1].buffer.is_none());

    let json = report.to_json();
    assert!(json.contains("\"name\":\"i64_sum\",\"type\":\"i64\",\"value\":6000000000"));
}

#[test]
fn deterministic_pipeline_hands_reviewed_float_buffers_to_immutable_slices() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(FLOAT_BUFFER_PIPELINE).unwrap();
    let allow = Some(vec!["alloc".to_string(), "cpu".to_string()]);
    let report = run("float_buffers.fgx", &program, Vec::new(), allow.clone());
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 4);
    assert!(report.stages.iter().all(|stage| stage.native));

    let gains = report.stages[0].buffer.as_ref().expect("f32 receipt");
    assert_eq!(gains.schema, "forgentex.pipeline_buffer.v1");
    assert_eq!(gains.element_type, "f32");
    assert_eq!(gains.elements, 4);
    assert_eq!(gains.bytes, 16);
    assert_eq!(gains.encoding, "little_endian_ieee754_v1");
    assert_eq!(gains.sha256.len(), 64);
    assert!(report.stages[1].buffer.is_none());
    assert_eq!(report.stages[1].input, "[0.5, -1.25, 2.0, 4.75]");

    let precise = report.stages[2].buffer.as_ref().expect("f64 receipt");
    assert_eq!(precise.element_type, "f64");
    assert_eq!(precise.elements, 4);
    assert_eq!(precise.bytes, 32);
    assert_eq!(precise.encoding, "little_endian_ieee754_v1");
    assert_eq!(precise.sha256.len(), 64);
    assert!(report.stages[3].buffer.is_none());
    assert_eq!(report.stages[3].input, "[0.5, -2.25, -0.0, 4.0]");

    let json = report.to_json();
    assert!(json.contains("\"name\":\"gain_sum\",\"type\":\"f32\",\"value\":6"));
    assert!(json.contains("\"name\":\"precise_sum\",\"type\":\"f64\",\"value\":2.25"));

    let repeated = run("float_buffers.fgx", &program, Vec::new(), allow);
    assert_eq!(repeated.status, "ok", "report: {repeated:?}");
    assert_eq!(repeated.shared, report.shared);
    assert_eq!(repeated.stages[0].buffer, report.stages[0].buffer);
    assert_eq!(repeated.stages[2].buffer, report.stages[2].buffer);
}

#[test]
fn deterministic_pipeline_hands_reviewed_wide_integer_buffers_to_immutable_slices() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(WIDE_INTEGER_BUFFER_PIPELINE).unwrap();
    let allow = Some(vec!["alloc".to_string(), "cpu".to_string()]);
    let report = run(
        "wide_integer_buffers.fgx",
        &program,
        Vec::new(),
        allow.clone(),
    );
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 4);
    assert!(report.stages.iter().all(|stage| stage.native));

    let ids = report.stages[0].buffer.as_ref().expect("u64 receipt");
    assert_eq!(ids.element_type, "u64");
    assert_eq!(ids.elements, 3);
    assert_eq!(ids.bytes, 24);
    assert_eq!(ids.encoding, "little_endian_integer_v1");
    assert_eq!(
        report.stages[1].input,
        "[5000000000, 6000000000, 7000000000]"
    );

    let offsets = report.stages[2].buffer.as_ref().expect("isize receipt");
    assert_eq!(offsets.element_type, "isize");
    assert_eq!(offsets.elements, 3);
    assert_eq!(
        offsets.bytes,
        3_usize.saturating_mul(std::mem::size_of::<isize>())
    );
    assert_eq!(offsets.encoding, "little_endian_integer_v1");
    assert_eq!(
        report.stages[3].input,
        "[-2000000000, 500000000, 2000000000]"
    );

    let json = report.to_json();
    assert!(json.contains("\"name\":\"id_sum\",\"type\":\"i64\",\"value\":18000000000"));
    assert!(json.contains("\"name\":\"offset_sum\",\"type\":\"i64\",\"value\":500000000"));

    let repeated = run("wide_integer_buffers.fgx", &program, Vec::new(), allow);
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
    let _artifact_guard = render_artifact_guard();
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
    assert_eq!(receipt.color_space, None);
    assert_eq!(receipt.device, None);
    assert_eq!(receipt.render_pass, None);
    assert_eq!(receipt.engine, None);
    assert_eq!(receipt.engine_version, None);
    assert_eq!(receipt.sequence_id, None);
    assert_eq!(receipt.frame_cache_key_sha256, None);
    assert_eq!(receipt.renderer_binary_sha256.len(), 64);
    assert_eq!(receipt.scene_type, "ScenePlan");
    assert_eq!(receipt.seed, 7);
    assert_eq!((receipt.width, receipt.height), (16, 16));
    assert_eq!((receipt.frame_index, receipt.frame_count), (0, 1));
    assert_eq!(receipt.frames_per_second, None);
    assert_eq!(receipt.samples_per_pixel, None);
    assert_eq!(receipt.shutter_q16, None);
    assert_eq!(receipt.adaptive_min_samples, None);
    assert_eq!(receipt.adaptive_noise_q16, None);
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
    let _artifact_guard = render_artifact_guard();
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
    assert_eq!(render.color_space, None);
    assert_eq!(render.device, None);
    assert_eq!(render.render_pass, None);
    assert_eq!(render.engine, None);
    assert_eq!(render.engine_version, None);
    assert_eq!(render.sequence_id, None);
    assert_eq!(render.frame_cache_key_sha256, None);
    assert_eq!(render.renderer_binary_sha256.len(), 64);
    assert_eq!(render.artifact.bytes, 776);
    assert!(render.artifact.path.is_none());
    assert!(report.stages[1].encode.is_none());

    let encode = report.stages[2].encode.as_ref().expect("encode receipt");
    assert_eq!(encode.schema, "forgentex.encode_receipt.v1");
    assert_eq!(encode.profile, "ppm");
    assert_eq!(encode.color_space, None);
    assert_eq!(encode.input_pass, None);
    assert_eq!(encode.engine, None);
    assert_eq!(encode.engine_version, None);
    assert_eq!(encode.compression, None);
    assert_eq!(encode.compression_level, None);
    assert_eq!(encode.pixel_format, None);
    assert_eq!(encode.interlace, None);
    assert_eq!(encode.scanline_filter, None);
    assert_eq!(encode.chunk_checksum, None);
    assert_eq!(encode.stream_checksum, None);
    assert_eq!(encode.chunk_order, None);
    assert_eq!(encode.encoder, "encode_ppm_preview");
    assert_eq!(encode.encoder_binary_sha256.len(), 64);
    assert_eq!(encode.input_bytes, 776);
    assert_eq!((encode.input_width, encode.input_height), (16, 16));
    assert_eq!(encode.input_format, "rgb8");
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
    assert!(json.contains("\"input_width\":16"));
    assert!(json.contains("\"input_height\":16"));
    assert!(json.contains("\"input_format\":\"rgb8\""));

    let repeated = run("render_encode_preview.fgx", &program, Vec::new(), allow);
    let repeated_render = repeated.stages[1].render.as_ref().unwrap();
    let repeated_encode = repeated.stages[2].encode.as_ref().unwrap();
    assert_eq!(repeated_render.artifact.sha256, render.artifact.sha256);
    assert_eq!(repeated_encode.input_sha256, encode.input_sha256);
    assert_eq!(repeated_encode.artifact.sha256, encode.artifact.sha256);
    let _ = std::fs::remove_file(path);
}

#[test]
fn governed_final_profile_runs_with_4k_bounds_and_linked_receipts() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.ppm");
    let _ = std::fs::remove_file(path);
    let program = parse_human(RENDER_ENCODE_FINAL_PIPELINE).unwrap();
    let allow = Some(vec![
        "alloc".to_string(),
        "cpu".to_string(),
        "cpu.render".to_string(),
        "media.encode".to_string(),
        "fs.write".to_string(),
    ]);
    let report = run("render_encode_final.fgx", &program, Vec::new(), allow);
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);

    let render = report.stages[1].render.as_ref().expect("render receipt");
    assert_eq!(render.profile, "final");
    assert_eq!(render.color_space, None);
    assert_eq!(render.device, None);
    assert_eq!(render.render_pass, None);
    assert_eq!(render.engine, None);
    assert_eq!(render.engine_version, None);
    assert_eq!(render.sequence_id, None);
    assert_eq!(render.frame_cache_key_sha256, None);
    assert_eq!(render.renderer_binary_sha256.len(), 64);
    assert_eq!(render.renderer, "render_rgb_final");
    assert_eq!(render.seed, 29);
    assert_eq!((render.width, render.height), (64, 36));
    assert_eq!((render.frame_index, render.frame_count), (0, 1));
    assert_eq!(render.frames_per_second, None);
    assert_eq!(render.samples_per_pixel, None);
    assert_eq!(render.shutter_q16, None);
    assert_eq!(render.adaptive_min_samples, None);
    assert_eq!(render.adaptive_noise_q16, None);
    assert_eq!(render.artifact.media_type, "application/x-forgentex-rgb");
    assert_eq!(render.artifact.bytes, 6920);

    let encode = report.stages[2].encode.as_ref().expect("encode receipt");
    assert_eq!(encode.profile, "ppm");
    assert_eq!(encode.color_space, None);
    assert_eq!(encode.input_pass, None);
    assert_eq!(encode.engine, None);
    assert_eq!(encode.engine_version, None);
    assert_eq!(encode.compression, None);
    assert_eq!(encode.compression_level, None);
    assert_eq!(encode.pixel_format, None);
    assert_eq!(encode.interlace, None);
    assert_eq!(encode.scanline_filter, None);
    assert_eq!(encode.chunk_checksum, None);
    assert_eq!(encode.stream_checksum, None);
    assert_eq!(encode.chunk_order, None);
    assert_eq!(encode.encoder, "encode_ppm_final");
    assert_eq!(encode.encoder_binary_sha256.len(), 64);
    assert_eq!(encode.input_bytes, 6920);
    assert_eq!((encode.input_width, encode.input_height), (64, 36));
    assert_eq!(encode.input_format, "rgb8");
    assert_eq!(encode.input_sha256, render.artifact.sha256);
    assert_eq!(encode.artifact.bytes, 6925);
    assert_eq!(encode.artifact.path.as_deref(), path.to_str());

    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.starts_with(b"P6\n64 36\n255\n"));
    assert_eq!(crate::crypto::sha256_hex(&bytes), encode.artifact.sha256);
    let _ = std::fs::remove_file(path);
}

#[test]
fn governed_png_encoder_emits_a_valid_deterministic_rgb8_image() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let program = parse_human(RENDER_ENCODE_PNG_PIPELINE).unwrap();
    let allow = Some(vec![
        "alloc".to_string(),
        "cpu".to_string(),
        "cpu.render".to_string(),
        "media.encode".to_string(),
        "fs.write".to_string(),
    ]);
    let report = run("render_encode_png.fgx", &program, Vec::new(), allow.clone());
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    let render = report.stages[1].render.as_ref().expect("render receipt");
    assert_eq!(render.profile, "final");
    assert_eq!(render.color_space.as_deref(), Some("srgb"));
    assert_eq!(render.device.as_deref(), Some("cpu"));
    assert_eq!(render.render_pass.as_deref(), Some("beauty"));
    assert_eq!(render.engine.as_deref(), Some("forgentex_software_rgb8"));
    assert_eq!(render.engine_version.as_deref(), Some("1.0.0"));
    assert_eq!(render.sequence_id.as_deref(), Some("visualizer_final_v1"));
    assert_eq!(
        render.frame_cache_key_sha256.as_deref().map(str::len),
        Some(64)
    );
    assert_eq!(render.renderer_binary_sha256.len(), 64);
    assert_eq!((render.width, render.height), (64, 36));
    assert_eq!((render.frame_index, render.frame_count), (12, 240));
    assert_eq!(render.frames_per_second, Some(30));
    assert_eq!(render.samples_per_pixel, Some(256));
    assert_eq!(render.shutter_q16, Some(32_768));
    assert_eq!(render.adaptive_min_samples, Some(32));
    assert_eq!(render.adaptive_noise_q16, Some(655));
    assert_eq!(render.artifact.bytes, 6920);

    let encode = report.stages[2].encode.as_ref().expect("encode receipt");
    assert_eq!(encode.profile, "png");
    assert_eq!(encode.color_space.as_deref(), Some("srgb"));
    assert_eq!(encode.input_pass.as_deref(), Some("beauty"));
    assert_eq!(encode.engine.as_deref(), Some("forgentex_png_rgb8"));
    assert_eq!(encode.engine_version.as_deref(), Some("1.0.0"));
    assert_eq!(encode.compression.as_deref(), Some("stored_deflate"));
    assert_eq!(encode.compression_level, Some(0));
    assert_eq!(encode.pixel_format.as_deref(), Some("rgb8"));
    assert_eq!(encode.interlace.as_deref(), Some("none"));
    assert_eq!(encode.scanline_filter.as_deref(), Some("none"));
    assert_eq!(encode.chunk_checksum.as_deref(), Some("crc32"));
    assert_eq!(encode.stream_checksum.as_deref(), Some("adler32"));
    assert_eq!(encode.chunk_order.as_deref(), Some("ihdr_srgb_idat_iend"));
    assert_eq!(encode.encoder, "encode_png_final");
    assert_eq!(encode.encoder_binary_sha256.len(), 64);
    assert_ne!(encode.encoder_binary_sha256, render.renderer_binary_sha256);
    assert_eq!(encode.input_sha256, render.artifact.sha256);
    assert_eq!(encode.input_bytes, 6920);
    assert_eq!((encode.input_width, encode.input_height), (64, 36));
    assert_eq!(encode.input_format, "rgb8");
    assert_eq!(encode.artifact.media_type, "image/png");
    assert_eq!(encode.artifact.bytes, 7029);
    assert_eq!(encode.artifact.path.as_deref(), path.to_str());

    let bytes = std::fs::read(path).unwrap();
    assert_stored_rgb8_png(
        &bytes,
        ExpectedRgb8Png {
            width: 64,
            height: 36,
            seed: 41,
            glyph_count: 32,
            frame_index: 12,
            frame_count: 240,
            samples_per_pixel: 256,
            shutter_q16: 32_768,
            adaptive_min_samples: 32,
            adaptive_noise_q16: 655,
        },
    );
    assert_eq!(crate::crypto::sha256_hex(&bytes), encode.artifact.sha256);
    let json = report.to_json();
    assert!(json.contains("\"frame_index\":12"));
    assert!(json.contains("\"frame_count\":240"));
    assert!(json.contains("\"frames_per_second\":30"));
    assert!(json.contains("\"samples_per_pixel\":256"));
    assert!(json.contains("\"shutter_q16\":32768"));
    assert!(json.contains("\"adaptive_min_samples\":32"));
    assert!(json.contains("\"adaptive_noise_q16\":655"));
    assert!(json.contains("\"color_space\":\"srgb\""));
    assert!(json.contains("\"device\":\"cpu\""));
    assert!(json.contains("\"render_pass\":\"beauty\""));
    assert!(json.contains("\"engine\":\"forgentex_software_rgb8\""));
    assert!(json.contains("\"engine\":\"forgentex_png_rgb8\""));
    assert!(json.contains("\"compression\":\"stored_deflate\""));
    assert!(json.contains("\"compression_level\":0"));
    assert!(json.contains("\"pixel_format\":\"rgb8\""));
    assert!(json.contains("\"interlace\":\"none\""));
    assert!(json.contains("\"scanline_filter\":\"none\""));
    assert!(json.contains("\"chunk_checksum\":\"crc32\""));
    assert!(json.contains("\"stream_checksum\":\"adler32\""));
    assert!(json.contains("\"chunk_order\":\"ihdr_srgb_idat_iend\""));
    assert!(json.contains("\"engine_version\":\"1.0.0\""));
    assert!(json.contains("\"sequence_id\":\"visualizer_final_v1\""));
    assert!(json.contains("\"frame_cache_key_sha256\":"));
    assert!(json.contains(&format!(
        "\"renderer_binary_sha256\":\"{}\"",
        render.renderer_binary_sha256
    )));
    assert!(json.contains(&format!(
        "\"encoder_binary_sha256\":\"{}\"",
        encode.encoder_binary_sha256
    )));
    assert!(json.contains("\"input_pass\":\"beauty\""));

    let repeated = run("render_encode_png.fgx", &program, Vec::new(), allow);
    let repeated_render = repeated.stages[1].render.as_ref().unwrap();
    let repeated_encode = repeated.stages[2].encode.as_ref().unwrap();
    assert_eq!(
        repeated_render.frame_cache_key_sha256,
        render.frame_cache_key_sha256
    );
    assert_eq!(
        repeated_render.renderer_binary_sha256,
        render.renderer_binary_sha256
    );
    assert_eq!(
        repeated_encode.encoder_binary_sha256,
        encode.encoder_binary_sha256
    );
    assert_eq!(repeated_encode.artifact.sha256, encode.artifact.sha256);
    let _ = std::fs::remove_file(path);
}

#[test]
fn render_frame_cache_key_changes_with_exact_frame_identity() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let allow = Some(vec![
        "alloc".to_string(),
        "cpu".to_string(),
        "cpu.render".to_string(),
        "media.encode".to_string(),
        "fs.write".to_string(),
    ]);
    let first = run(
        "render_encode_png.fgx",
        &parse_human(RENDER_ENCODE_PNG_PIPELINE).unwrap(),
        Vec::new(),
        allow.clone(),
    );
    assert_eq!(first.status, "ok", "report: {first:?}");
    let first_render = first.stages[1].render.as_ref().unwrap();
    let first_key = first_render.frame_cache_key_sha256.as_deref().unwrap();

    let _ = std::fs::remove_file(path);
    let next_source =
        RENDER_ENCODE_PNG_PIPELINE.replacen("scene.frame_index = 12", "scene.frame_index = 13", 1);
    let next = run(
        "render_encode_png_next_frame.fgx",
        &parse_human(&next_source).unwrap(),
        Vec::new(),
        allow,
    );
    assert_eq!(next.status, "ok", "report: {next:?}");
    let next_render = next.stages[1].render.as_ref().unwrap();
    let next_key = next_render.frame_cache_key_sha256.as_deref().unwrap();
    assert_ne!(next_render.scene_sha256, first_render.scene_sha256);
    assert_ne!(next_key, first_key);
    let _ = std::fs::remove_file(path);
}

#[test]
fn render_frame_manifest_rejects_an_out_of_range_index_before_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let source =
        RENDER_ENCODE_PNG_PIPELINE.replacen("scene.frame_index = 12", "scene.frame_index = 240", 1);
    let report = run(
        "invalid_png_frame_index.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "media.encode".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert_eq!(report.stages[1].decision, "rejected");
    assert!(!report.stages[1].native);
    assert!(report.stages[1].render.is_none());
    assert!(report.stages[1]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("render frame index 240 must be from 0 through 239")));
    assert!(!path.exists());
}

#[test]
fn render_quality_manifest_enforces_profile_samples_and_q16_shutter() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let preview_samples = RENDER_ENCODE_PNG_PIPELINE
        .replacen("render.profile final", "render.profile preview", 1)
        .replacen(
            "scene.samples_per_pixel = 256",
            "scene.samples_per_pixel = 65",
            1,
        );
    let final_samples = RENDER_ENCODE_PNG_PIPELINE.replacen(
        "scene.samples_per_pixel = 256",
        "scene.samples_per_pixel = 4097",
        1,
    );
    let bad_shutter = RENDER_ENCODE_PNG_PIPELINE.replacen(
        "scene.shutter_q16 = 32768",
        "scene.shutter_q16 = 65537",
        1,
    );
    let cases = [
        (
            "preview_samples.fgx",
            preview_samples,
            "render preview samples per pixel 65 must be from 1 through 64",
        ),
        (
            "final_samples.fgx",
            final_samples,
            "render final samples per pixel 4097 must be from 1 through 4096",
        ),
        (
            "bad_shutter.fgx",
            bad_shutter,
            "render shutter Q16 value 65537 must be from 0 through 65536",
        ),
    ];
    for (file, source, expected) in cases {
        let report = run(
            file,
            &parse_human(&source).unwrap(),
            Vec::new(),
            Some(vec![
                "alloc".to_string(),
                "cpu".to_string(),
                "cpu.render".to_string(),
                "media.encode".to_string(),
                "fs.write".to_string(),
            ]),
        );
        assert_eq!(report.status, "rejected", "report: {report:?}");
        assert_eq!(report.stages.len(), 2);
        assert!(!report.stages[1].native);
        assert!(report.stages[1].render.is_none());
        assert!(report.stages[1]
            .error
            .as_deref()
            .is_some_and(|error| error.contains(expected)));
    }
    assert!(!path.exists());
}

#[test]
fn render_adaptive_manifest_enforces_minimum_samples_and_q16_noise() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let cases = [
        (
            "adaptive_min_zero.fgx",
            RENDER_ENCODE_PNG_PIPELINE.replacen(
                "scene.adaptive_min_samples = 32",
                "scene.adaptive_min_samples = 0",
                1,
            ),
            "render adaptive minimum samples 0 must be from 1 through 256",
        ),
        (
            "adaptive_min_above_max.fgx",
            RENDER_ENCODE_PNG_PIPELINE.replacen(
                "scene.adaptive_min_samples = 32",
                "scene.adaptive_min_samples = 257",
                1,
            ),
            "render adaptive minimum samples 257 must be from 1 through 256",
        ),
        (
            "adaptive_noise_zero.fgx",
            RENDER_ENCODE_PNG_PIPELINE.replacen(
                "scene.adaptive_noise_q16 = 655",
                "scene.adaptive_noise_q16 = 0",
                1,
            ),
            "render adaptive noise Q16 value 0 must be from 1 through 65536",
        ),
        (
            "adaptive_noise_above_max.fgx",
            RENDER_ENCODE_PNG_PIPELINE.replacen(
                "scene.adaptive_noise_q16 = 655",
                "scene.adaptive_noise_q16 = 65537",
                1,
            ),
            "render adaptive noise Q16 value 65537 must be from 1 through 65536",
        ),
    ];
    for (file, source, expected) in cases {
        let report = run(
            file,
            &parse_human(&source).unwrap(),
            Vec::new(),
            Some(vec![
                "alloc".to_string(),
                "cpu".to_string(),
                "cpu.render".to_string(),
                "media.encode".to_string(),
                "fs.write".to_string(),
            ]),
        );
        assert_eq!(report.status, "rejected", "report: {report:?}");
        assert_eq!(report.stages.len(), 2);
        assert!(!report.stages[1].native);
        assert!(report.stages[1].render.is_none());
        assert!(report.stages[1]
            .error
            .as_deref()
            .is_some_and(|error| error.contains(expected)));
    }
    assert!(!path.exists());
}

#[test]
fn encoder_rejects_a_missing_raw_frame_color_space_contract_before_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let source =
        RENDER_ENCODE_PNG_PIPELINE.replacen("    sandbox \"encode.color_space srgb\"\n", "", 1);
    let report = run(
        "missing_encode_color_space.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "media.encode".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    let encode = &report.stages[2];
    assert_eq!(encode.decision, "rejected");
    assert!(!encode.native);
    assert!(encode.encode.is_none());
    assert!(encode.error.as_deref().is_some_and(|error| {
        error.contains("encode color space None")
            && error.contains("producer 'render_rgb_png' color space Some(\"srgb\")")
    }));
    assert!(!path.exists());
}

#[test]
fn encoder_rejects_a_missing_raw_frame_pass_contract_before_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-final/visualizer-final.png");
    let _ = std::fs::remove_file(path);
    let source =
        RENDER_ENCODE_PNG_PIPELINE.replacen("    sandbox \"encode.input_pass beauty\"\n", "", 1);
    let report = run(
        "missing_encode_input_pass.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "media.encode".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    let encode = &report.stages[2];
    assert_eq!(encode.decision, "rejected");
    assert!(!encode.native);
    assert!(encode.encode.is_none());
    assert!(encode.error.as_deref().is_some_and(|error| {
        error.contains("encode input pass None")
            && error.contains("producer 'render_rgb_png' render pass Some(\"beauty\")")
    }));
    assert!(!path.exists());
}

#[test]
fn renderer_cannot_commit_raw_rgb_dimensions_that_disagree_with_the_scene() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-previews/visualizer-encoded.ppm");
    let _ = std::fs::remove_file(path);
    let source = RENDER_ENCODE_PIPELINE.replacen(
        "uint32_t width = (uint32_t)scene.width;\n            uint32_t height = (uint32_t)scene.height;",
        "uint32_t width = 8;\n            uint32_t height = 32;",
        1,
    );
    let report = run(
        "mismatched_render_frame.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "media.encode".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "error", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(report.stages[1].native);
    assert!(!report.stages[1].handoff_committed);
    assert!(report.stages[1].render.is_none());
    assert!(report.stages[1].error.as_deref().is_some_and(|error| error
        .contains("raw RGB frame dimensions 8x32 do not match render scene dimensions 16x16")));
    assert!(report.shared.iter().all(|value| value.name != "raw_frame"));
    assert!(!path.exists());
}

#[test]
fn encoder_rejects_a_malformed_declared_raw_rgb_frame_before_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let path = std::path::Path::new("target/render-previews/visualizer-encoded.ppm");
    let _ = std::fs::remove_file(path);
    let source = RENDER_ENCODE_PIPELINE
        .replace(
            "    sandbox \"render.profile preview\"\n    sandbox \"render.seed_field seed\"\n",
            "",
        )
        .replacen(
            "uint32_t width = (uint32_t)scene.width;",
            "uint32_t width = 15;",
            1,
        );
    let report = run(
        "malformed_raw_frame.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "media.encode".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 3);
    assert!(report.stages[1].handoff_committed);
    assert_eq!(report.stages[2].decision, "rejected");
    assert!(!report.stages[2].native);
    assert!(report.stages[2].encode.is_none());
    assert!(report.stages[2]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("raw RGB frame 15x16 requires 728 bytes, found 776")));
    assert!(!path.exists());
}

#[test]
fn encoder_effect_denial_persists_nothing_and_issues_no_encode_receipt() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
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
    let _artifact_guard = render_artifact_guard();
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
    let _artifact_guard = render_artifact_guard();
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
fn final_dimensions_are_rejected_beyond_4k_before_renderer_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
    let source = RENDER_ENCODE_FINAL_PIPELINE.replacen("scene.width = 64", "scene.width = 3841", 1);
    let report = run(
        "oversized_render_final.fgx",
        &parse_human(&source).unwrap(),
        Vec::new(),
        Some(vec![
            "alloc".to_string(),
            "cpu".to_string(),
            "cpu.render".to_string(),
            "media.encode".to_string(),
            "fs.write".to_string(),
        ]),
    );
    assert_eq!(report.status, "rejected", "report: {report:?}");
    assert_eq!(report.stages.len(), 2);
    assert!(!report.stages[1].native);
    assert!(report.stages[1]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("exceed the 3840x2160 final profile")));
}

#[test]
fn preview_render_effect_denial_writes_nothing_and_issues_no_receipt() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let _artifact_guard = render_artifact_guard();
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
    assert!(report.stages[0].cache_identity.is_none());
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
    assert!(json.contains("\"cache_identity\":null"));
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
