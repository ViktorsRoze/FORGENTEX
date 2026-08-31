//! Foreign code capsule contract validation and review reporting (v2.0).
//!
//! v2.0 is the capsule *specification* milestone: capsules are parsed,
//! their contract is validated, and a machine-readable review report is
//! produced. Capsules are not executed (deferred to v2.3) and are not
//! lowered by the native backends (review-only); the review report states
//! the `not_lowerable` backend stance explicitly rather than letting a
//! backend silently drop the code.
//!
//! Validation enforces the non-negotiable parts of the capsule contract
//! from `docs/foreign_capsule_spec.md`: a pinned dependency set, a declared
//! sandbox policy, a typed boundary, a runtime version, and a code body.

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::diag::Diagnostic;
use crate::json;

pub const MAX_NATIVE_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;

fn is_render_pass(value: &str) -> bool {
    matches!(value, "beauty" | "depth" | "normal" | "object_id")
}

fn is_artifact_token(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn is_core_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
                && part.parse::<u32>().is_ok()
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactPolicy {
    pub media_type: String,
    pub max_bytes: usize,
    pub path: Option<String>,
    pub publisher: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderPolicy {
    pub profile: String,
    pub color_space: Option<String>,
    pub device: Option<String>,
    pub render_pass: Option<String>,
    pub engine: Option<String>,
    pub engine_version: Option<String>,
    pub sequence_id: Option<String>,
    pub seed_field: String,
    pub frame_index_field: Option<String>,
    pub frame_count_field: Option<String>,
    pub frames_per_second: Option<u32>,
    pub samples_field: Option<String>,
    pub shutter_q16_field: Option<String>,
    pub adaptive_min_samples_field: Option<String>,
    pub adaptive_noise_q16_field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodePolicy {
    pub profile: String,
    pub color_space: Option<String>,
    pub input_pass: Option<String>,
    pub engine: Option<String>,
    pub engine_version: Option<String>,
    pub compression: Option<String>,
    pub compression_level: Option<u32>,
    pub pixel_format: Option<String>,
    pub interlace: Option<String>,
    pub scanline_filter: Option<String>,
    pub chunk_checksum: Option<String>,
    pub stream_checksum: Option<String>,
    pub chunk_order: Option<String>,
}

/// Parse the first governed image-encoder policy. Encoders consume an
/// immutable owned-byte handoff and publish one image artifact; the pipeline
/// binds input and output hashes in a separate receipt.
pub fn encode_policy(capsule: &Capsule) -> Result<Option<EncodePolicy>, String> {
    let mut profile = None;
    let mut color_space = None;
    let mut input_pass = None;
    let mut engine = None;
    let mut engine_version = None;
    let mut compression = None;
    let mut compression_level = None;
    let mut pixel_format = None;
    let mut interlace = None;
    let mut scanline_filter = None;
    let mut chunk_checksum = None;
    let mut stream_checksum = None;
    let mut chunk_order = None;
    let mut saw_encode_rule = false;
    for rule in &capsule.sandbox {
        let Some((key, value)) = rule.trim().split_once(char::is_whitespace) else {
            if rule.starts_with("encode.") {
                return Err(format!("encode rule '{rule}' requires a value"));
            }
            continue;
        };
        let value = value.trim();
        match key {
            "encode.profile" => {
                saw_encode_rule = true;
                if profile.replace(value.to_string()).is_some() {
                    return Err("encode.profile may be declared only once".to_string());
                }
            }
            "encode.color_space" => {
                saw_encode_rule = true;
                if color_space.replace(value.to_string()).is_some() {
                    return Err("encode.color_space may be declared only once".to_string());
                }
            }
            "encode.input_pass" => {
                saw_encode_rule = true;
                if input_pass.replace(value.to_string()).is_some() {
                    return Err("encode.input_pass may be declared only once".to_string());
                }
            }
            "encode.engine" => {
                saw_encode_rule = true;
                if engine.replace(value.to_string()).is_some() {
                    return Err("encode.engine may be declared only once".to_string());
                }
            }
            "encode.engine_version" => {
                saw_encode_rule = true;
                if engine_version.replace(value.to_string()).is_some() {
                    return Err("encode.engine_version may be declared only once".to_string());
                }
            }
            "encode.compression" => {
                saw_encode_rule = true;
                if compression.replace(value.to_string()).is_some() {
                    return Err("encode.compression may be declared only once".to_string());
                }
            }
            "encode.compression_level" => {
                saw_encode_rule = true;
                if compression_level.replace(value.to_string()).is_some() {
                    return Err("encode.compression_level may be declared only once".to_string());
                }
            }
            "encode.pixel_format" => {
                saw_encode_rule = true;
                if pixel_format.replace(value.to_string()).is_some() {
                    return Err("encode.pixel_format may be declared only once".to_string());
                }
            }
            "encode.interlace" => {
                saw_encode_rule = true;
                if interlace.replace(value.to_string()).is_some() {
                    return Err("encode.interlace may be declared only once".to_string());
                }
            }
            "encode.scanline_filter" => {
                saw_encode_rule = true;
                if scanline_filter.replace(value.to_string()).is_some() {
                    return Err("encode.scanline_filter may be declared only once".to_string());
                }
            }
            "encode.chunk_checksum" => {
                saw_encode_rule = true;
                if chunk_checksum.replace(value.to_string()).is_some() {
                    return Err("encode.chunk_checksum may be declared only once".to_string());
                }
            }
            "encode.stream_checksum" => {
                saw_encode_rule = true;
                if stream_checksum.replace(value.to_string()).is_some() {
                    return Err("encode.stream_checksum may be declared only once".to_string());
                }
            }
            "encode.chunk_order" => {
                saw_encode_rule = true;
                if chunk_order.replace(value.to_string()).is_some() {
                    return Err("encode.chunk_order may be declared only once".to_string());
                }
            }
            other if other.starts_with("encode.") => {
                return Err(format!("unknown encode policy '{other}'"));
            }
            _ => {}
        }
    }
    if !saw_encode_rule {
        return Ok(None);
    }
    let profile = profile.ok_or_else(|| "encode policy requires encode.profile".to_string())?;
    let expected_media_type = match profile.as_str() {
        "ppm" => "image/x-portable-pixmap",
        "png" => "image/png",
        _ => return Err("encode.profile must be 'ppm' or 'png'".to_string()),
    };
    if color_space.as_deref().is_some_and(|value| value != "srgb") {
        return Err("encode.color_space must be 'srgb'".to_string());
    }
    if input_pass
        .as_deref()
        .is_some_and(|value| !is_render_pass(value))
    {
        return Err(
            "encode.input_pass must be 'beauty', 'depth', 'normal', or 'object_id'".to_string(),
        );
    }
    match (engine.as_deref(), engine_version.as_deref()) {
        (None, None) => {}
        (Some(engine), Some(version)) => {
            if !is_artifact_token(engine) {
                return Err(
                    "encode.engine must be a lowercase 1-64 character artifact token".to_string(),
                );
            }
            if !is_core_version(version) {
                return Err(
                    "encode.engine_version must be an exact major.minor.patch core version"
                        .to_string(),
                );
            }
        }
        _ => {
            return Err(
                "encode engine manifest requires engine and engine_version together".to_string(),
            );
        }
    }
    let compression_mode = compression.as_deref();
    let compression_level_text = compression_level.as_deref();
    let (compression, compression_level) = match (compression_mode, compression_level_text) {
        (None, None) => (None, None),
        (Some(mode), Some(level)) => {
            if profile != "png" {
                return Err("encode compression manifest requires encode.profile 'png'".to_string());
            }
            if mode != "stored_deflate" {
                return Err("encode.compression must be 'stored_deflate'".to_string());
            }
            let parsed = level.parse::<u32>().ok();
            if parsed != Some(0) || level != "0" {
                return Err(
                    "encode.compression_level must be the exact integer 0 for stored_deflate"
                        .to_string(),
                );
            }
            (Some(mode.to_string()), parsed)
        }
        _ => {
            return Err(
                "encode compression manifest requires compression and compression_level together"
                    .to_string(),
            );
        }
    };
    let (pixel_format, interlace) = match (pixel_format.as_deref(), interlace.as_deref()) {
        (None, None) => (None, None),
        (Some(pixel_format), Some(interlace)) => {
            if profile != "png" {
                return Err("encode PNG layout manifest requires encode.profile 'png'".to_string());
            }
            if pixel_format != "rgb8" {
                return Err("encode.pixel_format must be 'rgb8'".to_string());
            }
            if interlace != "none" {
                return Err("encode.interlace must be 'none'".to_string());
            }
            (Some(pixel_format.to_string()), Some(interlace.to_string()))
        }
        _ => {
            return Err(
                "encode PNG layout manifest requires pixel_format and interlace together"
                    .to_string(),
            );
        }
    };
    let scanline_filter = match scanline_filter.as_deref() {
        None => None,
        Some(scanline_filter) => {
            if profile != "png" {
                return Err("encode.scanline_filter requires encode.profile 'png'".to_string());
            }
            if scanline_filter != "none" {
                return Err("encode.scanline_filter must be 'none'".to_string());
            }
            Some(scanline_filter.to_string())
        }
    };
    let (chunk_checksum, stream_checksum) =
        match (chunk_checksum.as_deref(), stream_checksum.as_deref()) {
            (None, None) => (None, None),
            (Some(chunk_checksum), Some(stream_checksum)) => {
                if profile != "png" {
                    return Err(
                        "encode checksum manifest requires encode.profile 'png'".to_string()
                    );
                }
                if chunk_checksum != "crc32" {
                    return Err("encode.chunk_checksum must be 'crc32'".to_string());
                }
                if stream_checksum != "adler32" {
                    return Err("encode.stream_checksum must be 'adler32'".to_string());
                }
                (
                    Some(chunk_checksum.to_string()),
                    Some(stream_checksum.to_string()),
                )
            }
            _ => {
                return Err(
                    "encode checksum manifest requires chunk_checksum and stream_checksum together"
                        .to_string(),
                );
            }
        };
    let chunk_order = match chunk_order.as_deref() {
        None => None,
        Some(chunk_order) => {
            if profile != "png" {
                return Err("encode.chunk_order requires encode.profile 'png'".to_string());
            }
            if chunk_order != "ihdr_srgb_idat_iend" {
                return Err("encode.chunk_order must be 'ihdr_srgb_idat_iend'".to_string());
            }
            Some(chunk_order.to_string())
        }
    };
    let reads_bytes = matches!(
        capsule.inputs.as_slice(),
        [binding]
            if matches!(&binding.ty, crate::ast::Type::Slice { mutable: false, elem }
                if matches!(elem.as_ref(), crate::ast::Type::Primitive(element) if element == "u8"))
    );
    if !reads_bytes {
        return Err("encode policy requires exactly one immutable &[u8] input".to_string());
    }
    let owns_bytes = matches!(
        capsule.outputs.as_slice(),
        [binding]
            if matches!(&binding.ty, crate::ast::Type::Generic { name, args }
                if name == "Vec"
                    && matches!(args.as_slice(), [crate::ast::Type::Primitive(element)] if element == "u8"))
    );
    if !owns_bytes {
        return Err("encode policy requires exactly one owned Vec[u8] output".to_string());
    }
    if capsule.risk_class() != "native" {
        return Err("encode policy requires a native capsule".to_string());
    }
    if capsule
        .sandbox
        .iter()
        .any(|rule| rule.trim().starts_with("audit.redact"))
    {
        return Err("encode policy cannot hash a redacted input".to_string());
    }
    let artifact = artifact_policy(capsule)?
        .ok_or_else(|| "encode policy requires an image artifact policy".to_string())?;
    if artifact.media_type != expected_media_type {
        return Err(format!(
            "encode.profile '{profile}' requires artifact.media_type '{expected_media_type}'"
        ));
    }
    if artifact.path.is_none() && artifact.publisher.as_deref() != Some("host") {
        return Err(
            "encoder requires governed artifact.path persistence or artifact.publisher host"
                .to_string(),
        );
    }
    if !capsule
        .effects
        .iter()
        .any(|effect| effect == "media.encode")
    {
        return Err("encode policy requires the media.encode effect".to_string());
    }
    for required in ["wall_ms", "cpu_ms", "mem_mb"] {
        let expected = format!("limit.{required}");
        let declared = capsule.sandbox.iter().any(|rule| {
            rule.trim()
                .split_once(char::is_whitespace)
                .is_some_and(|(key, value)| key == expected && value.trim().parse::<u64>().is_ok())
        });
        if !declared {
            return Err(format!("encode policy requires limit.{required}"));
        }
    }
    Ok(Some(EncodePolicy {
        profile,
        color_space,
        input_pass,
        engine,
        engine_version,
        compression,
        compression_level,
        pixel_format,
        interlace,
        scanline_filter,
        chunk_checksum,
        stream_checksum,
        chunk_order,
    }))
}

/// Parse the first governed render policy. This slice supports preview images
/// and raw-RGB handoffs produced from one named scene struct. Seed plus optional
/// frame and quality manifest fields are resolved before dispatch, so a receipt
/// cannot invent authored render identity or settings absent from its input.
pub fn render_policy(capsule: &Capsule) -> Result<Option<RenderPolicy>, String> {
    let mut profile = None;
    let mut color_space = None;
    let mut device = None;
    let mut render_pass = None;
    let mut engine = None;
    let mut engine_version = None;
    let mut sequence_id = None;
    let mut seed_field = None;
    let mut frame_index_field = None;
    let mut frame_count_field = None;
    let mut frames_per_second = None;
    let mut samples_field = None;
    let mut shutter_q16_field = None;
    let mut adaptive_min_samples_field = None;
    let mut adaptive_noise_q16_field = None;
    let mut saw_render_rule = false;
    for rule in &capsule.sandbox {
        let Some((key, value)) = rule.trim().split_once(char::is_whitespace) else {
            if rule.starts_with("render.") {
                return Err(format!("render rule '{rule}' requires a value"));
            }
            continue;
        };
        let value = value.trim();
        match key {
            "render.profile" => {
                saw_render_rule = true;
                if profile.replace(value.to_string()).is_some() {
                    return Err("render.profile may be declared only once".to_string());
                }
            }
            "render.color_space" => {
                saw_render_rule = true;
                if color_space.replace(value.to_string()).is_some() {
                    return Err("render.color_space may be declared only once".to_string());
                }
            }
            "render.device" => {
                saw_render_rule = true;
                if device.replace(value.to_string()).is_some() {
                    return Err("render.device may be declared only once".to_string());
                }
            }
            "render.pass" => {
                saw_render_rule = true;
                if render_pass.replace(value.to_string()).is_some() {
                    return Err("render.pass may be declared only once".to_string());
                }
            }
            "render.engine" => {
                saw_render_rule = true;
                if engine.replace(value.to_string()).is_some() {
                    return Err("render.engine may be declared only once".to_string());
                }
            }
            "render.engine_version" => {
                saw_render_rule = true;
                if engine_version.replace(value.to_string()).is_some() {
                    return Err("render.engine_version may be declared only once".to_string());
                }
            }
            "render.sequence_id" => {
                saw_render_rule = true;
                if sequence_id.replace(value.to_string()).is_some() {
                    return Err("render.sequence_id may be declared only once".to_string());
                }
            }
            "render.seed_field" => {
                saw_render_rule = true;
                if seed_field.replace(value.to_string()).is_some() {
                    return Err("render.seed_field may be declared only once".to_string());
                }
            }
            "render.frame_index_field" => {
                saw_render_rule = true;
                if frame_index_field.replace(value.to_string()).is_some() {
                    return Err("render.frame_index_field may be declared only once".to_string());
                }
            }
            "render.frame_count_field" => {
                saw_render_rule = true;
                if frame_count_field.replace(value.to_string()).is_some() {
                    return Err("render.frame_count_field may be declared only once".to_string());
                }
            }
            "render.frames_per_second" => {
                saw_render_rule = true;
                let parsed = value.parse::<u32>().map_err(|_| {
                    "render.frames_per_second must be an integer from 1 through 240".to_string()
                })?;
                if frames_per_second.replace(parsed).is_some() {
                    return Err("render.frames_per_second may be declared only once".to_string());
                }
            }
            "render.samples_field" => {
                saw_render_rule = true;
                if samples_field.replace(value.to_string()).is_some() {
                    return Err("render.samples_field may be declared only once".to_string());
                }
            }
            "render.shutter_q16_field" => {
                saw_render_rule = true;
                if shutter_q16_field.replace(value.to_string()).is_some() {
                    return Err("render.shutter_q16_field may be declared only once".to_string());
                }
            }
            "render.adaptive_min_samples_field" => {
                saw_render_rule = true;
                if adaptive_min_samples_field
                    .replace(value.to_string())
                    .is_some()
                {
                    return Err(
                        "render.adaptive_min_samples_field may be declared only once".to_string(),
                    );
                }
            }
            "render.adaptive_noise_q16_field" => {
                saw_render_rule = true;
                if adaptive_noise_q16_field
                    .replace(value.to_string())
                    .is_some()
                {
                    return Err(
                        "render.adaptive_noise_q16_field may be declared only once".to_string()
                    );
                }
            }
            other if other.starts_with("render.") => {
                return Err(format!("unknown render policy '{other}'"));
            }
            _ => {}
        }
    }
    if !saw_render_rule {
        return Ok(None);
    }
    let profile = profile.ok_or_else(|| "render policy requires render.profile".to_string())?;
    if !matches!(profile.as_str(), "preview" | "final") {
        return Err("render.profile must be 'preview' or 'final'".to_string());
    }
    if color_space.as_deref().is_some_and(|value| value != "srgb") {
        return Err("render.color_space must be 'srgb'".to_string());
    }
    if device
        .as_deref()
        .is_some_and(|value| !matches!(value, "cpu" | "gpu"))
    {
        return Err("render.device must be 'cpu' or 'gpu'".to_string());
    }
    if render_pass
        .as_deref()
        .is_some_and(|value| !is_render_pass(value))
    {
        return Err("render.pass must be 'beauty', 'depth', 'normal', or 'object_id'".to_string());
    }
    match (engine.as_deref(), engine_version.as_deref()) {
        (None, None) => {}
        (Some(engine), Some(version)) => {
            if !is_artifact_token(engine) {
                return Err(
                    "render.engine must be a lowercase 1-64 character artifact token".to_string(),
                );
            }
            if !is_core_version(version) {
                return Err(
                    "render.engine_version must be an exact major.minor.patch core version"
                        .to_string(),
                );
            }
        }
        _ => {
            return Err(
                "render engine manifest requires engine and engine_version together".to_string(),
            );
        }
    }
    if let Some(sequence_id) = sequence_id.as_deref() {
        if !is_artifact_token(sequence_id) {
            return Err(
                "render.sequence_id must be a lowercase 1-64 character artifact token".to_string(),
            );
        }
        if frame_index_field.is_none() {
            return Err("render.sequence_id requires the render frame manifest".to_string());
        }
    }
    let seed_field =
        seed_field.ok_or_else(|| "render policy requires render.seed_field".to_string())?;
    let valid_identifier = |field: &str| {
        !field.is_empty()
            && field.chars().enumerate().all(|(index, ch)| {
                ch == '_' || ch.is_ascii_alphanumeric() && (index > 0 || !ch.is_ascii_digit())
            })
    };
    if !valid_identifier(&seed_field) {
        return Err("render.seed_field must be an identifier".to_string());
    }
    match (
        frame_index_field.as_deref(),
        frame_count_field.as_deref(),
        frames_per_second,
    ) {
        (None, None, None) => {}
        (Some(index), Some(count), Some(fps)) => {
            if !valid_identifier(index) || !valid_identifier(count) {
                return Err("render frame fields must be identifiers".to_string());
            }
            if index == count
                || [index, count]
                    .iter()
                    .any(|field| matches!(*field, "width" | "height") || *field == seed_field)
            {
                return Err(
                    "render frame fields must be distinct from each other, seed, width, and height"
                        .to_string(),
                );
            }
            if !(1..=240).contains(&fps) {
                return Err(
                    "render.frames_per_second must be an integer from 1 through 240".to_string(),
                );
            }
        }
        _ => {
            return Err(
                "render frame manifest requires frame_index_field, frame_count_field, and frames_per_second together"
                    .to_string(),
            );
        }
    }
    match (samples_field.as_deref(), shutter_q16_field.as_deref()) {
        (None, None) => {}
        (Some(samples), Some(shutter)) => {
            if !valid_identifier(samples) || !valid_identifier(shutter) {
                return Err("render quality fields must be identifiers".to_string());
            }
            let collides_with_identity = |field: &str| {
                matches!(field, "width" | "height")
                    || field == seed_field
                    || frame_index_field.as_deref() == Some(field)
                    || frame_count_field.as_deref() == Some(field)
            };
            if samples == shutter
                || collides_with_identity(samples)
                || collides_with_identity(shutter)
            {
                return Err(
                    "render quality fields must be distinct from each other and identity fields"
                        .to_string(),
                );
            }
        }
        _ => {
            return Err(
                "render quality manifest requires samples_field and shutter_q16_field together"
                    .to_string(),
            );
        }
    }
    match (
        adaptive_min_samples_field.as_deref(),
        adaptive_noise_q16_field.as_deref(),
    ) {
        (None, None) => {}
        (Some(min_samples), Some(noise_q16)) => {
            if samples_field.is_none() {
                return Err(
                    "render adaptive manifest requires the render quality manifest".to_string(),
                );
            }
            if !valid_identifier(min_samples) || !valid_identifier(noise_q16) {
                return Err("render adaptive fields must be identifiers".to_string());
            }
            let collides_with_existing = |field: &str| {
                matches!(field, "width" | "height")
                    || field == seed_field
                    || frame_index_field.as_deref() == Some(field)
                    || frame_count_field.as_deref() == Some(field)
                    || samples_field.as_deref() == Some(field)
                    || shutter_q16_field.as_deref() == Some(field)
            };
            if min_samples == noise_q16
                || collides_with_existing(min_samples)
                || collides_with_existing(noise_q16)
            {
                return Err(
                    "render adaptive fields must be distinct from each other and existing render fields"
                        .to_string(),
                );
            }
        }
        _ => {
            return Err(
                "render adaptive manifest requires adaptive_min_samples_field and adaptive_noise_q16_field together"
                    .to_string(),
            );
        }
    }
    if !matches!(capsule.inputs.as_slice(), [binding] if matches!(binding.ty, crate::ast::Type::Named(_)))
    {
        return Err("render policy requires exactly one named scene-struct input".to_string());
    }
    let owns_bytes = matches!(
        capsule.outputs.as_slice(),
        [binding]
            if matches!(&binding.ty, crate::ast::Type::Generic { name, args }
                if name == "Vec"
                    && matches!(args.as_slice(), [crate::ast::Type::Primitive(element)] if element == "u8"))
    );
    if !owns_bytes {
        return Err("render policy requires exactly one owned Vec[u8] output".to_string());
    }
    if capsule.risk_class() != "native" {
        return Err("render policy requires a native capsule".to_string());
    }
    if capsule
        .sandbox
        .iter()
        .any(|rule| rule.trim().starts_with("audit.redact"))
    {
        return Err("render policy cannot disclose a seed from a redacted input".to_string());
    }
    let artifact = artifact_policy(capsule)?
        .ok_or_else(|| "render policy requires an image artifact policy".to_string())?;
    let is_image = artifact.media_type.starts_with("image/");
    let is_raw_frame = artifact.media_type == "application/x-forgentex-rgb";
    if !is_image && !is_raw_frame {
        return Err(
            "render artifact must be an image/* or application/x-forgentex-rgb".to_string(),
        );
    }
    if is_image && artifact.path.is_none() && artifact.publisher.as_deref() != Some("host") {
        return Err(
            "rendered image requires governed artifact.path persistence or artifact.publisher host"
                .to_string(),
        );
    }
    let has_cpu_render = capsule.effects.iter().any(|effect| effect == "cpu.render");
    let has_gpu_render = capsule.effects.iter().any(|effect| effect == "gpu.render");
    if !has_cpu_render && !has_gpu_render {
        return Err("render policy requires the cpu.render or gpu.render effect".to_string());
    }
    if let Some(device) = device.as_deref() {
        let exact_effect = match device {
            "cpu" => has_cpu_render && !has_gpu_render,
            "gpu" => has_gpu_render && !has_cpu_render,
            _ => unreachable!("render device validated above"),
        };
        if !exact_effect {
            return Err(format!(
                "render.device '{device}' requires exactly the matching {device}.render effect"
            ));
        }
    }
    for required in ["wall_ms", "cpu_ms", "mem_mb"] {
        let declared = capsule.sandbox.iter().any(|rule| {
            rule.trim()
                .split_once(char::is_whitespace)
                .is_some_and(|(key, value)| {
                    key == format!("limit.{required}") && value.trim().parse::<u64>().is_ok()
                })
        });
        if !declared {
            return Err(format!("render policy requires limit.{required}"));
        }
    }
    Ok(Some(RenderPolicy {
        profile,
        color_space,
        device,
        render_pass,
        engine,
        engine_version,
        sequence_id,
        seed_field,
        frame_index_field,
        frame_count_field,
        frames_per_second,
        samples_field,
        shutter_q16_field,
        adaptive_min_samples_field,
        adaptive_noise_q16_field,
    }))
}

/// Parse the optional artifact contract carried by a capsule's sandbox rules.
/// Artifact metadata is meaningful only for the single owned `Vec[u8]`
/// boundary; requiring both fields prevents a receipt from claiming an
/// unbounded or untyped artifact after the native call has run.
pub fn artifact_policy(capsule: &Capsule) -> Result<Option<ArtifactPolicy>, String> {
    let mut media_type = None;
    let mut max_bytes = None;
    let mut path = None;
    let mut publisher = None;
    let mut saw_artifact_rule = false;
    for rule in &capsule.sandbox {
        let Some((key, value)) = rule.trim().split_once(char::is_whitespace) else {
            if rule.starts_with("artifact.") {
                return Err(format!("artifact rule '{rule}' requires a value"));
            }
            continue;
        };
        let value = value.trim();
        match key {
            "artifact.media_type" => {
                saw_artifact_rule = true;
                if media_type.replace(value.to_string()).is_some() {
                    return Err("artifact.media_type may be declared only once".to_string());
                }
            }
            "artifact.max_bytes" => {
                saw_artifact_rule = true;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| "artifact.max_bytes must be a positive integer".to_string())?;
                if max_bytes.replace(parsed).is_some() {
                    return Err("artifact.max_bytes may be declared only once".to_string());
                }
            }
            "artifact.path" => {
                saw_artifact_rule = true;
                if path.replace(value.to_string()).is_some() {
                    return Err("artifact.path may be declared only once".to_string());
                }
            }
            "artifact.publisher" => {
                saw_artifact_rule = true;
                if publisher.replace(value.to_string()).is_some() {
                    return Err("artifact.publisher may be declared only once".to_string());
                }
            }
            other if other.starts_with("artifact.") => {
                return Err(format!("unknown artifact policy '{other}'"));
            }
            _ => {}
        }
    }
    if !saw_artifact_rule {
        return Ok(None);
    }
    let media_type =
        media_type.ok_or_else(|| "artifact policy requires artifact.media_type".to_string())?;
    let valid_media_type = media_type.split_once('/').is_some_and(|(kind, subtype)| {
        !kind.is_empty()
            && !subtype.is_empty()
            && !subtype.contains('/')
            && media_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-/".contains(&byte))
    });
    if !valid_media_type {
        return Err("artifact.media_type must be a valid type/subtype token".to_string());
    }
    let max_bytes =
        max_bytes.ok_or_else(|| "artifact policy requires artifact.max_bytes".to_string())?;
    if max_bytes == 0 || max_bytes > MAX_NATIVE_ARTIFACT_BYTES {
        return Err(format!(
            "artifact.max_bytes must be between 1 and {MAX_NATIVE_ARTIFACT_BYTES}"
        ));
    }
    let owns_bytes = matches!(
        capsule.outputs.as_slice(),
        [binding]
            if matches!(&binding.ty, crate::ast::Type::Generic { name, args }
                if name == "Vec"
                    && matches!(args.as_slice(), [crate::ast::Type::Primitive(element)] if element == "u8"))
    );
    if !owns_bytes {
        return Err(
            "artifact policy requires exactly one owned Vec[u8] capsule output".to_string(),
        );
    }
    if let Some(publisher) = publisher.as_deref() {
        if !matches!(publisher, "native" | "host") {
            return Err("artifact.publisher must be 'native' or 'host'".to_string());
        }
        if publisher == "native" && path.is_none() {
            return Err("artifact.publisher native requires artifact.path".to_string());
        }
        if publisher == "host" {
            if path.is_some() {
                return Err("artifact.publisher host cannot declare artifact.path".to_string());
            }
            if capsule.effects.iter().any(|effect| effect == "fs.write")
                || capsule.sandbox.iter().any(|rule| {
                    rule.trim()
                        .split_once(char::is_whitespace)
                        .is_some_and(|(capability, _)| capability == "fs.write")
                })
            {
                return Err(
                    "artifact.publisher host cannot grant the capsule fs.write authority"
                        .to_string(),
                );
            }
        }
    }
    if let Some(path) = &path {
        let artifact_path = std::path::Path::new(path);
        let safe_relative_path = !path.is_empty()
            && !artifact_path.is_absolute()
            && artifact_path.components().all(|component| {
                matches!(
                    component,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            });
        if !safe_relative_path {
            return Err(
                "artifact.path must be a non-empty relative path without parent traversal"
                    .to_string(),
            );
        }
        if !capsule.effects.iter().any(|effect| effect == "fs.write") {
            return Err("artifact.path persistence requires the fs.write effect".to_string());
        }
        let covered = capsule.sandbox.iter().any(|rule| {
            rule.trim()
                .split_once(char::is_whitespace)
                .filter(|(capability, _)| *capability == "fs.write")
                .is_some_and(|(_, scope)| {
                    let scope = std::path::Path::new(scope.trim());
                    !scope.as_os_str().is_empty()
                        && !scope.is_absolute()
                        && scope.components().all(|component| {
                            matches!(
                                component,
                                std::path::Component::Normal(_) | std::path::Component::CurDir
                            )
                        })
                        && artifact_path.starts_with(scope)
                })
        });
        if !covered {
            return Err(
                "artifact.path must be contained by a relative fs.write sandbox scope".to_string(),
            );
        }
    }
    Ok(Some(ArtifactPolicy {
        media_type,
        max_bytes,
        path,
        publisher,
    }))
}

/// Contract diagnostics for every capsule in the program. These are
/// blocking: a project that contains an invalid capsule fails `fgx check`.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        validate_capsule(capsule, &mut diagnostics);
    }
    diagnostics
}

fn validate_capsule(capsule: &Capsule, out: &mut Vec<Diagnostic>) {
    if let Err(message) = artifact_policy(capsule) {
        out.push(
            Diagnostic::new(
                "capsule_invalid_artifact_policy",
                &capsule.id,
                format!("capsule '{}' has invalid artifact policy: {message}", capsule.name),
            )
            .suggest(
                "declare media type and size rules on one Vec[u8] output; persistence additionally needs artifact.path, an fs.write effect, and a covering fs.write scope",
            ),
        );
    }
    if let Err(message) = render_policy(capsule) {
        out.push(
            Diagnostic::new(
                "capsule_invalid_render_policy",
                &capsule.id,
                format!("capsule '{}' has invalid render policy: {message}", capsule.name),
            )
            .suggest(
                "declare a preview or final profile, seed field, image artifact path, render effect, and wall/CPU/memory limits",
            ),
        );
    }
    if let Err(message) = encode_policy(capsule) {
        out.push(
            Diagnostic::new(
                "capsule_invalid_encode_policy",
                &capsule.id,
                format!("capsule '{}' has invalid encode policy: {message}", capsule.name),
            )
            .suggest(
                "declare a PPM or PNG profile, matching image media type, immutable byte input, image artifact path, media.encode effect, and wall/CPU/memory limits",
            ),
        );
    }
    if capsule.version.trim().is_empty() {
        out.push(
            Diagnostic::new(
                "capsule_missing_version",
                &capsule.id,
                format!("capsule '{}' must declare a runtime version", capsule.name),
            )
            .suggest("add a version, for example version \"3.13\""),
        );
    }
    if capsule.inputs.is_empty() && capsule.outputs.is_empty() {
        out.push(
            Diagnostic::new(
                "capsule_missing_typed_boundary",
                &capsule.id,
                format!(
                    "capsule '{}' must declare at least one typed input or output",
                    capsule.name
                ),
            )
            .suggest("add an input or output binding at the Forgentex boundary"),
        );
    }
    if capsule.sandbox.is_empty() {
        out.push(
            Diagnostic::new(
                "capsule_missing_sandbox",
                &capsule.id,
                format!("capsule '{}' must declare a sandbox policy", capsule.name),
            )
            .suggest("add a sandbox rule, for example sandbox \"fs.read /data\""),
        );
    }
    if capsule.code.is_empty() {
        out.push(Diagnostic::new(
            "capsule_empty_code",
            &capsule.id,
            format!("capsule '{}' has an empty code body", capsule.name),
        ));
    }
    for dep in &capsule.deps {
        if !dep.is_pinned() {
            out.push(
                Diagnostic::new(
                    "capsule_unpinned_dependency",
                    &capsule.id,
                    format!(
                        "capsule '{}' dependency '{}' is not pinned",
                        capsule.name, dep.source
                    ),
                )
                .suggest("pin with an exact version, commit, or checksum"),
            );
        }
    }
    for (index, fixture) in capsule.fixtures.iter().enumerate() {
        if fixture.inputs.len() != capsule.inputs.len()
            || fixture.outputs.len() != capsule.outputs.len()
        {
            out.push(
                Diagnostic::new(
                    "capsule_fixture_arity",
                    &capsule.id,
                    format!(
                        "capsule '{}' mock {} has {} input(s) and {} output(s) but the boundary declares {} and {}",
                        capsule.name,
                        index,
                        fixture.inputs.len(),
                        fixture.outputs.len(),
                        capsule.inputs.len(),
                        capsule.outputs.len()
                    ),
                )
                .suggest("match the mock arity to the declared inputs and outputs"),
            );
            continue;
        }
        for (value, binding) in fixture.inputs.iter().zip(&capsule.inputs) {
            if !value.matches_type(&binding.ty) {
                out.push(fixture_type_diagnostic(
                    capsule,
                    index,
                    "input",
                    &binding.name,
                    value,
                    &binding.ty,
                ));
            }
        }
        for (value, binding) in fixture.outputs.iter().zip(&capsule.outputs) {
            if !value.matches_type(&binding.ty) {
                out.push(fixture_type_diagnostic(
                    capsule,
                    index,
                    "output",
                    &binding.name,
                    value,
                    &binding.ty,
                ));
            }
        }
    }
}

fn fixture_type_diagnostic(
    capsule: &Capsule,
    index: usize,
    role: &str,
    binding: &str,
    value: &crate::ast::CapsuleValue,
    ty: &crate::ast::Type,
) -> Diagnostic {
    Diagnostic::new(
        "capsule_fixture_type",
        &capsule.id,
        format!(
            "capsule '{}' mock {} {} '{}' is {} but the boundary type is {}",
            capsule.name,
            index,
            role,
            binding,
            value.type_name(),
            ty.to_agent()
        ),
    )
    .suggest("use a literal whose type matches the boundary binding")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub capsules: Vec<CapsuleReviewEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleReviewEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub version: String,
    pub env: Option<String>,
    pub risk_class: String,
    pub high_risk: bool,
    pub effects: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub dependencies: Vec<(String, bool)>,
    pub sandbox: Vec<String>,
    pub fixtures: usize,
    pub code_lines: usize,
    pub native_backend: String,
    pub status: String,
    pub diagnostics: Vec<String>,
    pub notes: Vec<String>,
}

pub fn analyze(file: &str, program: &Program) -> CapsuleReport {
    let mut capsules = Vec::new();
    for capsule in program.capsules() {
        let mut diagnostics = Vec::new();
        validate_capsule(capsule, &mut diagnostics);
        let codes: Vec<String> = diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect();
        let high_risk = capsule.risk_class() != "managed";
        let status = if !codes.is_empty() {
            "rejected"
        } else if high_risk {
            "review_required"
        } else {
            "pass"
        };
        let mut notes = Vec::new();
        if high_risk {
            notes.push(format!(
                "high-risk {} capsule: stricter review required",
                capsule.risk_class()
            ));
        }
        if matches!(capsule.risk_class(), "smart_contract") {
            notes.push(
                "smart-contract capsule: wallet/secret-key access denied by default".to_string(),
            );
        }
        notes.push("native backends: not lowerable (review-only)".to_string());
        capsules.push(CapsuleReviewEntry {
            id: capsule.id.clone(),
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            version: capsule.version.clone(),
            env: capsule.env.clone(),
            risk_class: capsule.risk_class().to_string(),
            high_risk,
            effects: capsule.effects.clone(),
            inputs: capsule
                .inputs
                .iter()
                .map(|binding| format!("{}: {}", binding.name, binding.ty.to_agent()))
                .collect(),
            outputs: capsule
                .outputs
                .iter()
                .map(|binding| format!("{}: {}", binding.name, binding.ty.to_agent()))
                .collect(),
            dependencies: capsule
                .deps
                .iter()
                .map(|dep| (dep.source.clone(), dep.is_pinned()))
                .collect(),
            sandbox: capsule.sandbox.clone(),
            fixtures: capsule.fixtures.len(),
            code_lines: capsule.code.len(),
            native_backend: "not_lowerable".to_string(),
            status: status.to_string(),
            diagnostics: codes,
            notes,
        });
    }
    let status = if capsules.iter().any(|c| c.status == "rejected") {
        "rejected"
    } else if capsules.iter().any(|c| c.status == "review_required") {
        "review_required"
    } else {
        "pass"
    };
    CapsuleReport {
        schema: "forgentex.capsule_review.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        capsules,
    }
}

impl CapsuleReport {
    pub fn is_rejected(&self) -> bool {
        self.status == "rejected"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule review\n");
        out.push_str(&format!(
            "schema={} file={} status={} capsules={}\n",
            self.schema,
            self.file,
            self.status,
            self.capsules.len()
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  - {} {} lang={} version={} risk={} status={} backend={}\n",
                capsule.name,
                capsule.id,
                capsule.language,
                capsule.version,
                capsule.risk_class,
                capsule.status,
                capsule.native_backend
            ));
            out.push_str(&format!(
                "      effects=[{}] inputs=[{}] outputs=[{}]\n",
                capsule.effects.join(","),
                capsule.inputs.join(","),
                capsule.outputs.join(",")
            ));
            for (source, pinned) in &capsule.dependencies {
                out.push_str(&format!("      dep {source} pinned={pinned}\n"));
            }
            for rule in &capsule.sandbox {
                out.push_str(&format!("      sandbox {rule}\n"));
            }
            if !capsule.diagnostics.is_empty() {
                out.push_str(&format!(
                    "      diagnostics=[{}]\n",
                    capsule.diagnostics.join(",")
                ));
            }
            for note in &capsule.notes {
                out.push_str(&format!("      note: {note}\n"));
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str(&format!("  \"status\": {},\n", json::string(&self.status)));
        out.push_str("  \"capsules\": [");
        for (idx, capsule) in self.capsules.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {\n");
            out.push_str(&format!("      \"id\": {},\n", json::string(&capsule.id)));
            out.push_str(&format!(
                "      \"name\": {},\n",
                json::string(&capsule.name)
            ));
            out.push_str(&format!(
                "      \"language\": {},\n",
                json::string(&capsule.language)
            ));
            out.push_str(&format!(
                "      \"version\": {},\n",
                json::string(&capsule.version)
            ));
            out.push_str(&format!(
                "      \"env\": {},\n",
                json::opt_string(capsule.env.as_deref())
            ));
            out.push_str(&format!(
                "      \"risk_class\": {},\n",
                json::string(&capsule.risk_class)
            ));
            out.push_str(&format!("      \"high_risk\": {},\n", capsule.high_risk));
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json::string_array(&capsule.effects)
            ));
            out.push_str(&format!(
                "      \"inputs\": {},\n",
                json::string_array(&capsule.inputs)
            ));
            out.push_str(&format!(
                "      \"outputs\": {},\n",
                json::string_array(&capsule.outputs)
            ));
            out.push_str("      \"dependencies\": [");
            for (dep_idx, (source, pinned)) in capsule.dependencies.iter().enumerate() {
                if dep_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"source\": {}, \"pinned\": {}}}",
                    json::string(source),
                    pinned
                ));
            }
            out.push_str("],\n");
            out.push_str(&format!(
                "      \"sandbox\": {},\n",
                json::string_array(&capsule.sandbox)
            ));
            out.push_str(&format!("      \"fixtures\": {},\n", capsule.fixtures));
            out.push_str(&format!("      \"code_lines\": {},\n", capsule.code_lines));
            out.push_str(&format!(
                "      \"native_backend\": {},\n",
                json::string(&capsule.native_backend)
            ));
            out.push_str(&format!(
                "      \"status\": {},\n",
                json::string(&capsule.status)
            ));
            out.push_str(&format!(
                "      \"diagnostics\": {},\n",
                json::string_array(&capsule.diagnostics)
            ));
            out.push_str(&format!(
                "      \"notes\": {}\n",
                json::string_array(&capsule.notes)
            ));
            out.push_str("    }");
        }
        if !self.capsules.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

// ---- Deterministic mock execution (v2.1) --------------------------------
//
// Capsules are not executed for real in v2; the mock executor exercises the
// declared fixtures against the typed boundary so the contract is testable
// and reproducible without a foreign runtime.

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleRunReport {
    pub schema: String,
    pub file: String,
    pub capsule: String,
    pub status: String,
    pub passed: usize,
    pub failed: usize,
    pub cases: Vec<CapsuleRunCase>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleRunCase {
    pub index: usize,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub status: String,
    pub detail: String,
}

fn select_capsule<'a>(program: &'a Program, name: Option<&str>) -> Result<&'a Capsule, String> {
    let mut capsules = program.capsules();
    match name {
        Some(name) => program
            .capsules()
            .find(|capsule| capsule.name == name)
            .ok_or_else(|| format!("no capsule named '{name}'")),
        None => {
            let first = capsules
                .next()
                .ok_or_else(|| "no capsule in file".to_string())?;
            if capsules.next().is_some() {
                return Err(
                    "file has multiple capsules; pass --capsule <name> to choose one".to_string(),
                );
            }
            Ok(first)
        }
    }
}

/// Run every declared fixture through the mock boundary check.
pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<CapsuleRunReport, String> {
    let capsule = select_capsule(program, name)?;
    let mut cases = Vec::new();
    for (index, fixture) in capsule.fixtures.iter().enumerate() {
        let inputs: Vec<String> = fixture
            .inputs
            .iter()
            .map(CapsuleValue::to_literal)
            .collect();
        let outputs: Vec<String> = fixture
            .outputs
            .iter()
            .map(CapsuleValue::to_literal)
            .collect();
        let (status, detail) = evaluate_fixture(capsule, fixture);
        cases.push(CapsuleRunCase {
            index,
            inputs,
            outputs,
            status,
            detail,
        });
    }
    let passed = cases.iter().filter(|case| case.status == "pass").count();
    let failed = cases.len() - passed;
    let status = if capsule.fixtures.is_empty() {
        "no_fixtures"
    } else if failed == 0 {
        "pass"
    } else {
        "fail"
    };
    Ok(CapsuleRunReport {
        schema: "forgentex.capsule_run.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.id.clone(),
        status: status.to_string(),
        passed,
        failed,
        cases,
    })
}

/// Execution hook: given input literals, deterministically return the
/// outputs of the matching fixture, or an error trap.
pub fn run_input(
    program: &Program,
    name: Option<&str>,
    inputs: &[CapsuleValue],
) -> Result<Vec<String>, String> {
    let capsule = select_capsule(program, name)?;
    if inputs.len() != capsule.inputs.len() {
        return Err(format!(
            "input arity {} does not match boundary arity {}",
            inputs.len(),
            capsule.inputs.len()
        ));
    }
    for fixture in &capsule.fixtures {
        if fixture.inputs == inputs {
            return Ok(fixture
                .outputs
                .iter()
                .map(CapsuleValue::to_literal)
                .collect());
        }
    }
    Err("no_matching_fixture: no declared mock matches these inputs".to_string())
}

fn evaluate_fixture(capsule: &Capsule, fixture: &crate::ast::CapsuleFixture) -> (String, String) {
    if fixture.inputs.len() != capsule.inputs.len()
        || fixture.outputs.len() != capsule.outputs.len()
    {
        return ("arity".to_string(), "fixture arity mismatch".to_string());
    }
    for (value, binding) in fixture.inputs.iter().zip(&capsule.inputs) {
        if !value.matches_type(&binding.ty) {
            return (
                "type_mismatch".to_string(),
                format!("input '{}' type mismatch", binding.name),
            );
        }
    }
    for (value, binding) in fixture.outputs.iter().zip(&capsule.outputs) {
        if !value.matches_type(&binding.ty) {
            return (
                "type_mismatch".to_string(),
                format!("output '{}' type mismatch", binding.name),
            );
        }
    }
    ("pass".to_string(), String::new())
}

impl CapsuleRunReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule run\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} status={} passed={} failed={}\n",
            self.schema, self.file, self.capsule, self.status, self.passed, self.failed
        ));
        for case in &self.cases {
            out.push_str(&format!(
                "  - mock {} [{}] -> [{}] {}{}\n",
                case.index,
                case.inputs.join(", "),
                case.outputs.join(", "),
                case.status,
                if case.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", case.detail)
                }
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str(&format!(
            "  \"capsule\": {},\n",
            json::string(&self.capsule)
        ));
        out.push_str(&format!("  \"status\": {},\n", json::string(&self.status)));
        out.push_str(&format!("  \"passed\": {},\n", self.passed));
        out.push_str(&format!("  \"failed\": {},\n", self.failed));
        out.push_str("  \"cases\": [");
        for (idx, case) in self.cases.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {");
            out.push_str(&format!("\"index\": {}, ", case.index));
            out.push_str(&format!(
                "\"inputs\": {}, ",
                json::string_array(&case.inputs)
            ));
            out.push_str(&format!(
                "\"outputs\": {}, ",
                json::string_array(&case.outputs)
            ));
            out.push_str(&format!("\"status\": {}, ", json::string(&case.status)));
            out.push_str(&format!("\"detail\": {}", json::string(&case.detail)));
            out.push('}');
        }
        if !self.cases.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}
