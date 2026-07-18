//! Deterministic runtime pipeline (Milestone C, first slice).
//!
//! A stage is a checked capsule with exactly one `bind <source> read` and one
//! `bind <target> rw`. Its one scalar, reviewed `@repr(C)` struct, or reviewed
//! owned vector input is read from the Forgentex-owned shared store and its
//! output is atomically written to the target. Stages run in source-declaration
//! order, one at a time. Owned `Vec[u8]`, `Vec[i16]`, and `Vec[i32]` values may
//! feed later matching immutable slices. This deliberately excludes fan-in/out,
//! mutable buffers, implicit globals, and parallel scheduling.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Capsule, CapsuleValue, Program, ShareMode, Type};
use crate::interp::{invoke_capsule_live, value_to_json, CapsuleCall, Value};
use crate::json;

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineValue {
    pub name: String,
    pub ty: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineStage {
    pub capsule: String,
    pub source: String,
    pub target: String,
    pub decision: String,
    pub input: String,
    pub redacted_inputs: Vec<String>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub native: bool,
    pub buffer: Option<PipelineBufferReceipt>,
    pub artifact: Option<crate::ffi::NativeArtifactReceipt>,
    pub render: Option<RenderReceipt>,
    pub encode: Option<EncodeReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineBufferReceipt {
    pub schema: String,
    pub element_type: String,
    pub elements: usize,
    pub bytes: usize,
    pub encoding: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderReceipt {
    pub schema: String,
    pub profile: String,
    pub renderer: String,
    pub scene_type: String,
    pub scene_sha256: String,
    pub seed: i64,
    pub width: u32,
    pub height: u32,
    pub effects: Vec<String>,
    pub boundary_hash: String,
    pub toolchain: String,
    pub wall_limit_ms: Option<u128>,
    pub cpu_limit_ms: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub artifact: crate::ffi::NativeArtifactReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeReceipt {
    pub schema: String,
    pub profile: String,
    pub encoder: String,
    pub input_sha256: String,
    pub input_bytes: u64,
    pub effects: Vec<String>,
    pub boundary_hash: String,
    pub toolchain: String,
    pub wall_limit_ms: Option<u128>,
    pub cpu_limit_ms: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub artifact: crate::ffi::NativeArtifactReceipt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineReport {
    pub schema: String,
    pub file: String,
    pub mode: String,
    pub status: String,
    pub diagnostics: Vec<String>,
    pub stages: Vec<PipelineStage>,
    pub shared: Vec<PipelineValue>,
}

/// Execute the bounded single-threaded pipeline. `overrides` replaces declared
/// share defaults for one run; it never mutates the source program.
pub fn run(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
) -> PipelineReport {
    let diagnostics = crate::typecheck::check(program);
    if !diagnostics.is_empty() {
        return report(
            file,
            "rejected",
            diagnostics.into_iter().map(|item| item.code).collect(),
            Vec::new(),
            BTreeMap::new(),
            program,
        );
    }

    let mut shared = match initial_shared_values(program, overrides) {
        Ok(values) => values,
        Err(error) => {
            return report(
                file,
                "rejected",
                vec![error],
                Vec::new(),
                BTreeMap::new(),
                program,
            )
        }
    };
    let mut stages = Vec::new();
    for capsule in program
        .capsules()
        .filter(|capsule| !capsule.bindings.is_empty())
    {
        let (source, target) = match stage_contract(capsule, program) {
            Ok(contract) => contract,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    source: String::new(),
                    target: String::new(),
                    decision: "rejected".to_string(),
                    input: "null".to_string(),
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    buffer: None,
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(file, "rejected", Vec::new(), stages, shared, program);
            }
        };
        let input = match shared.get(&source) {
            Some(value) => value.clone(),
            None => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    source,
                    target,
                    decision: "rejected".to_string(),
                    input: "null".to_string(),
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(
                        "pipeline source has no default, override, or preceding stage output"
                            .to_string(),
                    ),
                    native: false,
                    buffer: None,
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(file, "rejected", Vec::new(), stages, shared, program);
            }
        };
        let input_json = value_to_json(&input);
        let render_context = match render_context(capsule, &input, &input_json) {
            Ok(context) => context,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    source,
                    target,
                    decision: "rejected".to_string(),
                    input: input_json,
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    buffer: None,
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(file, "rejected", Vec::new(), stages, shared, program);
            }
        };
        let encode_context = match encode_context(capsule, &input) {
            Ok(context) => context,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    source,
                    target,
                    decision: "rejected".to_string(),
                    input: input_json,
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    buffer: None,
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(file, "rejected", Vec::new(), stages, shared, program);
            }
        };
        let (outcome, calls) = invoke_capsule_live(program, &capsule.name, input, allow.clone());
        let call = calls.last();
        match outcome {
            Ok(output)
                if value_matches_type(program, &output, share_type(program, &target).unwrap()) =>
            {
                let output_json = value_to_json(&output);
                let mut event = stage_event(
                    capsule,
                    source,
                    target.clone(),
                    call,
                    input_json,
                    Some(output_json),
                    None,
                    render_context.as_ref(),
                    encode_context.as_ref(),
                );
                event.buffer = match buffer_receipt(
                    share_type(program, &target).unwrap(),
                    &output,
                    call.and_then(|item| item.native.as_ref()),
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        event.output = None;
                        event.error = Some(error);
                        stages.push(event);
                        return report(file, "error", Vec::new(), stages, shared, program);
                    }
                };
                if render_context.is_some() && event.render.is_none() {
                    event.output = None;
                    event.error =
                        Some("render stage completed without native artifact evidence".to_string());
                    stages.push(event);
                    return report(file, "error", Vec::new(), stages, shared, program);
                }
                if encode_context.is_some() && event.encode.is_none() {
                    event.output = None;
                    event.error =
                        Some("encode stage completed without native artifact evidence".to_string());
                    stages.push(event);
                    return report(file, "error", Vec::new(), stages, shared, program);
                }
                shared.insert(target.clone(), output);
                stages.push(event);
            }
            Ok(output) => {
                stages.push(stage_event(
                    capsule,
                    source,
                    target.clone(),
                    call,
                    input_json,
                    None,
                    Some(format!(
                        "stage output does not match shared '{}' type {}; got {}",
                        target,
                        share_type(program, &target).unwrap().to_agent(),
                        value_to_json(&output)
                    )),
                    render_context.as_ref(),
                    encode_context.as_ref(),
                ));
                return report(file, "error", Vec::new(), stages, shared, program);
            }
            Err(error) => {
                let status = if call.map(|item| item.decision.as_str()) == Some("denied") {
                    "denied"
                } else {
                    "error"
                };
                stages.push(stage_event(
                    capsule,
                    source,
                    target,
                    call,
                    input_json,
                    None,
                    Some(error),
                    render_context.as_ref(),
                    encode_context.as_ref(),
                ));
                return report(file, status, Vec::new(), stages, shared, program);
            }
        }
    }
    report(file, "ok", Vec::new(), stages, shared, program)
}

fn initial_shared_values(
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut values = BTreeMap::new();
    for share in program.shares() {
        if let Some(value) = &share.default {
            values.insert(share.name.clone(), capsule_value_to_value(value));
        }
    }
    for (name, value) in overrides {
        let share = program
            .shares()
            .find(|share| share.name == name)
            .ok_or_else(|| format!("pipeline override names unknown shared value '{name}'"))?;
        if !value.matches_type(&share.ty) {
            return Err(format!(
                "pipeline override for '{}' does not match {}",
                name,
                share.ty.to_agent()
            ));
        }
        values.insert(name, capsule_value_to_value(&value));
    }
    Ok(values)
}

fn stage_contract(capsule: &Capsule, program: &Program) -> Result<(String, String), String> {
    if capsule.bindings.len() != 2 || capsule.inputs.len() != 1 || capsule.outputs.len() != 1 {
        return Err(format!(
            "pipeline stage '{}' requires exactly one input, one output, one read binding, and one rw binding",
            capsule.name
        ));
    }
    let source = capsule
        .bindings
        .iter()
        .find(|binding| binding.mode == ShareMode::Read)
        .map(|binding| binding.name.clone())
        .ok_or_else(|| {
            format!(
                "pipeline stage '{}' is missing a read binding",
                capsule.name
            )
        })?;
    let target = capsule
        .bindings
        .iter()
        .find(|binding| binding.mode == ShareMode::Rw)
        .map(|binding| binding.name.clone())
        .ok_or_else(|| format!("pipeline stage '{}' is missing an rw binding", capsule.name))?;
    if capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode == ShareMode::Read)
        .count()
        != 1
        || capsule
            .bindings
            .iter()
            .filter(|binding| binding.mode == ShareMode::Rw)
            .count()
            != 1
    {
        return Err(format!(
            "pipeline stage '{}' has ambiguous shared bindings",
            capsule.name
        ));
    }
    let source_ty = share_type(program, &source).ok_or_else(|| {
        format!(
            "pipeline stage '{}' reads unknown shared '{}",
            capsule.name, source
        )
    })?;
    let target_ty = share_type(program, &target).ok_or_else(|| {
        format!(
            "pipeline stage '{}' writes unknown shared '{}",
            capsule.name, target
        )
    })?;
    if !is_pipeline_value(program, source_ty) || !is_pipeline_value(program, target_ty) {
        return Err(format!(
            "pipeline stage '{}' only supports scalar, restricted @repr(C) struct, or reviewed owned-vector shared values",
            capsule.name
        ));
    }
    if !pipeline_input_compatible(source_ty, &capsule.inputs[0].ty)
        || &capsule.outputs[0].ty != target_ty
    {
        return Err(format!(
            "pipeline stage '{}' boundary must match {} -> {} shared types",
            capsule.name,
            source_ty.to_agent(),
            target_ty.to_agent()
        ));
    }
    validate_render_stage(capsule, program)?;
    validate_encode_stage(capsule, program, &source)?;
    Ok((source, target))
}

fn validate_encode_stage(capsule: &Capsule, program: &Program, source: &str) -> Result<(), String> {
    if crate::capsule::encode_policy(capsule)?.is_none() {
        return Ok(());
    }
    let producer = program
        .capsules()
        .take_while(|candidate| candidate.name != capsule.name)
        .filter(|candidate| {
            candidate
                .bindings
                .iter()
                .any(|binding| binding.mode == ShareMode::Rw && binding.name == source)
        })
        .last()
        .ok_or_else(|| {
            format!(
                "encode stage '{}' requires a preceding producer for shared raw frame '{}'",
                capsule.name, source
            )
        })?;
    let artifact = crate::capsule::artifact_policy(producer)?
        .ok_or_else(|| "encode input producer requires an artifact policy".to_string())?;
    if artifact.media_type != "application/x-forgentex-rgb" {
        return Err(format!(
            "encode input producer '{}' must publish application/x-forgentex-rgb, found '{}'",
            producer.name, artifact.media_type
        ));
    }
    Ok(())
}

fn validate_render_stage(capsule: &Capsule, program: &Program) -> Result<(), String> {
    let Some(policy) = crate::capsule::render_policy(capsule)? else {
        return Ok(());
    };
    let Type::Named(scene_type) = &capsule.inputs[0].ty else {
        return Err("render stage requires one named scene struct".to_string());
    };
    let scene = program
        .structs()
        .find(|decl| decl.name == *scene_type)
        .ok_or_else(|| format!("render scene struct '{scene_type}' is not declared"))?;
    for field_name in [&policy.seed_field, "width", "height"] {
        let field = scene
            .fields
            .iter()
            .find(|field| field.name == *field_name)
            .ok_or_else(|| {
                format!(
                    "render scene struct '{scene_type}' is missing integer field '{field_name}'"
                )
            })?;
        if !matches!(&field.ty, Type::Primitive(name) if crate::ast::is_integer_primitive(name)) {
            return Err(format!(
                "render scene field '{scene_type}.{field_name}' must be an integer"
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct RenderContext {
    profile: String,
    scene_type: String,
    scene_sha256: String,
    seed: i64,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug)]
struct EncodeContext {
    profile: String,
    input_sha256: String,
    input_bytes: u64,
}

fn render_context(
    capsule: &Capsule,
    input: &Value,
    input_json: &str,
) -> Result<Option<RenderContext>, String> {
    let Some(policy) = crate::capsule::render_policy(capsule)? else {
        return Ok(None);
    };
    let Value::Struct(scene_type, fields) = input else {
        return Err("render stage input is not a scene struct".to_string());
    };
    let integer = |name: &str| match fields.get(name) {
        Some(Value::Int(value)) => Ok(*value),
        _ => Err(format!(
            "render scene field '{scene_type}.{name}' is not an integer"
        )),
    };
    let seed = integer(&policy.seed_field)?;
    let width = integer("width")?;
    let height = integer("height")?;
    if !(1..=1920).contains(&width) || !(1..=1080).contains(&height) {
        return Err(format!(
            "render preview dimensions {width}x{height} exceed the 1920x1080 preview profile"
        ));
    }
    Ok(Some(RenderContext {
        profile: policy.profile,
        scene_type: scene_type.clone(),
        scene_sha256: crate::crypto::sha256_hex(input_json.as_bytes()),
        seed,
        width: width as u32,
        height: height as u32,
    }))
}

fn encode_context(capsule: &Capsule, input: &Value) -> Result<Option<EncodeContext>, String> {
    let Some(policy) = crate::capsule::encode_policy(capsule)? else {
        return Ok(None);
    };
    let Value::Array(values) = input else {
        return Err("encode stage input is not an owned byte handoff".to_string());
    };
    let bytes = values
        .iter()
        .map(|value| match value {
            Value::Int(byte) if (0..=255).contains(byte) => Ok(*byte as u8),
            _ => Err("encode stage input contains a non-byte value".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(EncodeContext {
        profile: policy.profile,
        input_sha256: crate::crypto::sha256_hex(&bytes),
        input_bytes: bytes.len() as u64,
    }))
}

fn reviewed_owned_element(ty: &Type) -> Option<(&str, usize)> {
    let Type::Generic { name, args } = ty else {
        return None;
    };
    if name != "Vec" {
        return None;
    }
    match args.as_slice() {
        [Type::Primitive(element)] if element == "u8" => Some(("u8", 1)),
        [Type::Primitive(element)] if element == "i16" => Some(("i16", 2)),
        [Type::Primitive(element)] if element == "i32" => Some(("i32", 4)),
        _ => None,
    }
}

fn pipeline_input_compatible(shared: &Type, input: &Type) -> bool {
    if shared == input {
        return true;
    }
    let Some((shared_element, _)) = reviewed_owned_element(shared) else {
        return false;
    };
    matches!(
        input,
        Type::Slice {
            mutable: false,
            elem
        } if matches!(elem.as_ref(), Type::Primitive(name) if name == shared_element)
    )
}

fn buffer_receipt(
    ty: &Type,
    value: &Value,
    native: Option<&crate::ffi::NativeReceipt>,
) -> Result<Option<PipelineBufferReceipt>, String> {
    let Some((element_type, element_bytes)) = reviewed_owned_element(ty) else {
        return Ok(None);
    };
    let Value::Array(values) = value else {
        return Err("pipeline owned-vector output is not an array value".to_string());
    };
    let mut canonical = Vec::with_capacity(values.len().saturating_mul(element_bytes));
    for value in values {
        let Value::Int(value) = value else {
            return Err("pipeline owned-vector output contains a non-integer value".to_string());
        };
        match element_type {
            "u8" => canonical.push(u8::try_from(*value).map_err(|_| {
                "pipeline Vec[u8] output contains an out-of-range value".to_string()
            })?),
            "i16" => canonical.extend_from_slice(
                &i16::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[i16] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            "i32" => canonical.extend_from_slice(
                &i32::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[i32] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            _ => unreachable!("reviewed owned element"),
        }
    }
    if let Some(reported) = native.and_then(|receipt| receipt.owned_output_bytes) {
        if reported != canonical.len() {
            return Err(format!(
                "pipeline owned-vector byte accounting mismatch: runtime reported {reported}, canonical handoff has {}",
                canonical.len()
            ));
        }
    }
    Ok(Some(PipelineBufferReceipt {
        schema: "forgentex.pipeline_buffer.v1".to_string(),
        element_type: element_type.to_string(),
        elements: values.len(),
        bytes: canonical.len(),
        encoding: "little_endian_integer_v1".to_string(),
        sha256: crate::crypto::sha256_hex(&canonical),
    }))
}

fn share_type<'a>(program: &'a Program, name: &str) -> Option<&'a Type> {
    program
        .shares()
        .find(|share| share.name == name)
        .map(|share| &share.ty)
}

fn is_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Bool)
        || matches!(ty, Type::Primitive(name) if crate::ast::is_integer_primitive(name) || crate::ast::is_float_primitive(name) || name == "str")
        || matches!(ty, Type::Named(name) if name == "str")
}

fn is_pipeline_value(program: &Program, ty: &Type) -> bool {
    if is_scalar(ty) || reviewed_owned_element(ty).is_some() {
        return true;
    }
    let Type::Named(name) = ty else {
        return false;
    };
    let Some(decl) = program.structs().find(|decl| decl.name == *name) else {
        return false;
    };
    let repr_c = decl
        .attrs
        .iter()
        .any(|attr| attr.name == "repr" && attr.args.iter().any(|arg| arg == "C"));
    repr_c
        && !decl.fields.is_empty()
        && !decl.attrs.iter().any(|attr| {
            attr.name == "packed"
                || attr.name == "aligned"
                || (attr.name == "repr" && attr.args.iter().any(|arg| arg == "packed"))
        })
        && decl.fields.iter().all(|field| {
            field.attrs.is_empty()
                && matches!(&field.ty, Type::Primitive(name) if crate::ast::is_integer_primitive(name) || crate::ast::is_float_primitive(name))
        })
}

fn capsule_value_to_value(value: &CapsuleValue) -> Value {
    match value {
        CapsuleValue::Int(value) => Value::Int(*value),
        CapsuleValue::Float(value) => Value::Float(*value),
        CapsuleValue::Bool(value) => Value::Bool(*value),
        CapsuleValue::Str(value) => Value::String(value.clone()),
    }
}

fn value_matches_type(program: &Program, value: &Value, ty: &Type) -> bool {
    match (value, ty) {
        (Value::Bool(_), Type::Bool) => true,
        (Value::Int(_), Type::Primitive(name)) => crate::ast::is_integer_primitive(name),
        (Value::Float(_), Type::Primitive(name)) => crate::ast::is_float_primitive(name),
        (Value::String(_), Type::Primitive(name) | Type::Named(name)) => name == "str",
        (Value::Array(values), ty) => reviewed_owned_element(ty).is_some_and(|(element, _)| {
            values.iter().all(|value| match (element, value) {
                ("u8", Value::Int(value)) => u8::try_from(*value).is_ok(),
                ("i16", Value::Int(value)) => i16::try_from(*value).is_ok(),
                ("i32", Value::Int(value)) => i32::try_from(*value).is_ok(),
                _ => false,
            })
        }),
        (Value::Struct(name, fields), Type::Named(struct_name)) if name == struct_name => program
            .structs()
            .find(|decl| decl.name == *struct_name)
            .map(|decl| {
                fields.len() == decl.fields.len()
                    && decl.fields.iter().all(|field| {
                        fields
                            .get(&field.name)
                            .map(|field_value| value_matches_type(program, field_value, &field.ty))
                            .unwrap_or(false)
                    })
            })
            .unwrap_or(false),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn stage_event(
    capsule: &Capsule,
    source: String,
    target: String,
    call: Option<&CapsuleCall>,
    input: String,
    output: Option<String>,
    error: Option<String>,
    render_context: Option<&RenderContext>,
    encode_context: Option<&EncodeContext>,
) -> PipelineStage {
    let redacted_inputs = call
        .map(|item| item.redacted_inputs.clone())
        .unwrap_or_default();
    let input = if redacted_inputs.is_empty() {
        input
    } else {
        "{\"redacted\":true}".to_string()
    };
    let error = if redacted_inputs.is_empty() {
        error
    } else {
        error.map(|_| {
            call.and_then(|item| item.native_failure.as_ref())
                .map(|receipt| format!("{}: diagnostic redacted by input policy", receipt.kind))
                .unwrap_or_else(|| "capsule diagnostic redacted by input policy".to_string())
        })
    };
    let render = render_context.and_then(|context| {
        let native = call.and_then(|item| item.native.as_ref())?;
        let artifact = native.artifact.clone()?;
        Some(RenderReceipt {
            schema: "forgentex.render_receipt.v1".to_string(),
            profile: context.profile.clone(),
            renderer: capsule.name.clone(),
            scene_type: context.scene_type.clone(),
            scene_sha256: context.scene_sha256.clone(),
            seed: context.seed,
            width: context.width,
            height: context.height,
            effects: capsule.effects.clone(),
            boundary_hash: native.boundary_hash.clone(),
            toolchain: native.toolchain.clone(),
            wall_limit_ms: native.wall_limit_ms,
            cpu_limit_ms: native.cpu_limit_ms,
            memory_limit_bytes: native.memory_limit_bytes,
            artifact,
        })
    });
    let encode = encode_context.and_then(|context| {
        let native = call.and_then(|item| item.native.as_ref())?;
        let artifact = native.artifact.clone()?;
        Some(EncodeReceipt {
            schema: "forgentex.encode_receipt.v1".to_string(),
            profile: context.profile.clone(),
            encoder: capsule.name.clone(),
            input_sha256: context.input_sha256.clone(),
            input_bytes: context.input_bytes,
            effects: capsule.effects.clone(),
            boundary_hash: native.boundary_hash.clone(),
            toolchain: native.toolchain.clone(),
            wall_limit_ms: native.wall_limit_ms,
            cpu_limit_ms: native.cpu_limit_ms,
            memory_limit_bytes: native.memory_limit_bytes,
            artifact,
        })
    });
    PipelineStage {
        capsule: capsule.name.clone(),
        source,
        target,
        decision: call
            .map(|item| item.decision.clone())
            .unwrap_or_else(|| "error".to_string()),
        input,
        redacted_inputs,
        output,
        error,
        native: call.and_then(|item| item.native.as_ref()).is_some(),
        buffer: None,
        artifact: call
            .and_then(|item| item.native.as_ref())
            .and_then(|receipt| receipt.artifact.clone()),
        render,
        encode,
    }
}

fn report(
    file: &str,
    status: &str,
    diagnostics: Vec<String>,
    stages: Vec<PipelineStage>,
    shared: BTreeMap<String, Value>,
    program: &Program,
) -> PipelineReport {
    let redacted_shares = program
        .capsules()
        .filter(|capsule| {
            !crate::sandbox::analyze_capsule(capsule)
                .redacted_inputs
                .is_empty()
        })
        .flat_map(|capsule| {
            capsule
                .bindings
                .iter()
                .filter(|binding| binding.mode == ShareMode::Read)
                .map(|binding| binding.name.clone())
        })
        .collect::<BTreeSet<_>>();
    let shared = shared
        .into_iter()
        .filter_map(|(name, value)| {
            share_type(program, &name).map(|ty| PipelineValue {
                value: if redacted_shares.contains(&name) {
                    "{\"redacted\":true}".to_string()
                } else {
                    value_to_json(&value)
                },
                name,
                ty: ty.to_agent(),
            })
        })
        .collect();
    PipelineReport {
        schema: "forgentex.pipeline_run.v1".to_string(),
        file: file.to_string(),
        mode: "deterministic_single_threaded".to_string(),
        status: status.to_string(),
        diagnostics,
        stages,
        shared,
    }
}

impl PipelineReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "Forgentex deterministic pipeline\nschema={} file={} mode={} status={}\n",
            self.schema, self.file, self.mode, self.status
        );
        for stage in &self.stages {
            out.push_str(&format!(
                "  {}: {} -> {} decision={} input={} redacted=[{}] output={}{}{}\n",
                stage.capsule,
                stage.source,
                stage.target,
                stage.decision,
                stage.input,
                stage.redacted_inputs.join(","),
                stage.output.as_deref().unwrap_or("null"),
                stage
                    .error
                    .as_ref()
                    .map(|error| format!(" error={error}"))
                    .unwrap_or_default(),
                stage
                    .buffer
                    .as_ref()
                    .map(|buffer| {
                        format!(
                            " buffer={} elements={} bytes={} sha256={}",
                            buffer.element_type, buffer.elements, buffer.bytes, buffer.sha256
                        )
                    })
                    .unwrap_or_default()
                    + &stage
                        .artifact
                        .as_ref()
                        .map(|artifact| format!(
                            " artifact={} bytes={} sha256={}",
                            artifact.media_type, artifact.bytes, artifact.sha256
                        ))
                        .unwrap_or_default()
                    + &stage
                        .render
                        .as_ref()
                        .map(|render| {
                            format!(
                                " render={} {}x{} seed={} scene_sha256={}",
                                render.profile,
                                render.width,
                                render.height,
                                render.seed,
                                render.scene_sha256
                            )
                        })
                        .unwrap_or_default()
                    + &stage
                        .encode
                        .as_ref()
                        .map(|encode| {
                            format!(
                                " encode={} input_bytes={} input_sha256={}",
                                encode.profile, encode.input_bytes, encode.input_sha256
                            )
                        })
                        .unwrap_or_default()
            ));
        }
        for value in &self.shared {
            out.push_str(&format!(
                "  shared {}: {} = {}\n",
                value.name, value.ty, value.value
            ));
        }
        if !self.diagnostics.is_empty() {
            out.push_str(&format!("  diagnostics=[{}]\n", self.diagnostics.join(",")));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let stages = self
            .stages
            .iter()
            .map(|stage| {
                let artifact = stage.artifact.as_ref().map(|artifact| format!(
                    "{{\"schema\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"path\":{}}}",
                    json::string(&artifact.schema), json::string(&artifact.media_type), artifact.bytes,
                    json::string(&artifact.sha256), json::opt_string(artifact.path.as_deref())
                )).unwrap_or_else(|| "null".to_string());
                let buffer = stage.buffer.as_ref().map(|buffer| format!(
                    "{{\"schema\":{},\"element_type\":{},\"elements\":{},\"bytes\":{},\"encoding\":{},\"sha256\":{}}}",
                    json::string(&buffer.schema), json::string(&buffer.element_type), buffer.elements,
                    buffer.bytes, json::string(&buffer.encoding), json::string(&buffer.sha256)
                )).unwrap_or_else(|| "null".to_string());
                let render = stage.render.as_ref().map(|render| format!(
                    "{{\"schema\":{},\"profile\":{},\"renderer\":{},\"scene_type\":{},\"scene_sha256\":{},\"seed\":{},\"width\":{},\"height\":{},\"effects\":{},\"boundary_hash\":{},\"toolchain\":{},\"wall_limit_ms\":{},\"cpu_limit_ms\":{},\"memory_limit_bytes\":{},\"artifact\":{{\"schema\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"path\":{}}}}}",
                    json::string(&render.schema), json::string(&render.profile), json::string(&render.renderer),
                    json::string(&render.scene_type), json::string(&render.scene_sha256), render.seed,
                    render.width, render.height, json::string_array(&render.effects),
                    json::string(&render.boundary_hash), json::string(&render.toolchain),
                    render.wall_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    json::string(&render.artifact.schema), json::string(&render.artifact.media_type),
                    render.artifact.bytes, json::string(&render.artifact.sha256),
                    json::opt_string(render.artifact.path.as_deref())
                )).unwrap_or_else(|| "null".to_string());
                let encode = stage.encode.as_ref().map(|encode| format!(
                    "{{\"schema\":{},\"profile\":{},\"encoder\":{},\"input_sha256\":{},\"input_bytes\":{},\"effects\":{},\"boundary_hash\":{},\"toolchain\":{},\"wall_limit_ms\":{},\"cpu_limit_ms\":{},\"memory_limit_bytes\":{},\"artifact\":{{\"schema\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"path\":{}}}}}",
                    json::string(&encode.schema), json::string(&encode.profile), json::string(&encode.encoder),
                    json::string(&encode.input_sha256), encode.input_bytes, json::string_array(&encode.effects),
                    json::string(&encode.boundary_hash), json::string(&encode.toolchain),
                    encode.wall_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    encode.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    encode.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    json::string(&encode.artifact.schema), json::string(&encode.artifact.media_type),
                    encode.artifact.bytes, json::string(&encode.artifact.sha256),
                    json::opt_string(encode.artifact.path.as_deref())
                )).unwrap_or_else(|| "null".to_string());
                format!(
                "{{\"capsule\":{},\"source\":{},\"target\":{},\"decision\":{},\"input\":{},\"redacted_inputs\":{},\"output\":{},\"error\":{},\"native\":{},\"buffer\":{},\"artifact\":{},\"render\":{},\"encode\":{}}}",
                json::string(&stage.capsule), json::string(&stage.source), json::string(&stage.target),
                json::string(&stage.decision), stage.input, json::string_array(&stage.redacted_inputs), stage.output.as_deref().unwrap_or("null"),
                json::opt_string(stage.error.as_deref()), stage.native, buffer, artifact, render, encode
            )})
            .collect::<Vec<_>>()
            .join(",");
        let shared = self
            .shared
            .iter()
            .map(|value| {
                format!(
                    "{{\"name\":{},\"type\":{},\"value\":{}}}",
                    json::string(&value.name),
                    json::string(&value.ty),
                    value.value
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":{},\"file\":{},\"mode\":{},\"status\":{},\"diagnostics\":{},\"stages\":[{}],\"shared\":[{}]}}",
            json::string(&self.schema), json::string(&self.file), json::string(&self.mode), json::string(&self.status),
            json::string_array(&self.diagnostics), stages, shared
        )
    }
}
