use std::path::Path;

use crate::diag::{diagnostics_to_json, Diagnostic};
use crate::effects::{analyze, effects_to_json};
use crate::format::{format_agent, format_human};
use crate::parser::parse_file;
use crate::patch::apply_patch;
use crate::typecheck::check;

use super::{fixture, parse_fixture};

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let program = parse_file(Path::new("effects.fgx"), source).unwrap();
    check(&program)
}

#[test]
fn effect_declarations_parse_format_and_roundtrip() {
    let source = "\
fn load() -> u8 effects [io.read, crypto.hash]:
    return 0
";
    let program = parse_file(Path::new("effects.fgx"), source).unwrap();

    assert_eq!(check(&program), Vec::new());
    assert!(
        format_human(&program).contains("effects [crypto.hash, io.read]")
            || format_human(&program).contains("effects [io.read, crypto.hash]")
    );
    assert!(format_agent(&program).contains("effects["));
    let agent = format_agent(&program);
    let reparsed = parse_file(Path::new("effects.fgai"), &agent).unwrap();
    assert_eq!(program, reparsed);
}

#[test]
fn caller_must_declare_callee_effects() {
    let source = "\
fn read_byte() -> u8 effects [io.read]:
    return 1

fn bad() -> u8:
    return read_byte()
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"undeclared_effect\""));
    assert!(json.contains("\"expected\": \"io.read\""));
    assert!(json.contains("add_effect_declaration"));
}

#[test]
fn builtin_effects_require_explicit_capabilities() {
    let missing = diagnostics("fn bad() -> u8:\n    return io_read()\n");
    let json = diagnostics_to_json(&missing);
    assert!(json.contains("\"error\": \"undeclared_effect\""));
    assert!(json.contains("\"expected\": \"io.read\""));

    let declared = diagnostics("fn ok() -> u8 effects [io.read]:\n    return io_read()\n");
    assert_eq!(declared, Vec::new());
}

#[test]
fn safe_functions_cannot_claim_raw_memory_or_assembly_effects() {
    let diagnostics = diagnostics("fn bad() -> void effects [memory.raw]:\n    return\n");
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"unsafe_effect_requires_unsafe\""));
    assert!(json.contains("mark_function_unsafe"));
}

#[test]
fn effects_command_json_shape_is_stable() {
    let source = "\
fn read_byte() -> u8 effects [io.read]:
    return io_read()
";
    let program = parse_file(Path::new("effects.fgx"), source).unwrap();
    let json = effects_to_json(&analyze(&program));

    assert!(json.contains("\"schema\": \"forgentex.effects.v1\""));
    assert!(json.contains("\"declared\": [\"io.read\"]"));
    assert!(json.contains("\"inferred\": [\"io.read\"]"));
    assert!(json.contains("\"capabilities\": [\"io.read\"]"));
}

#[test]
fn semantic_patch_review_reports_effect_changes() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let result = apply_patch(program, fixture("patches/add_fast_path.fgp")).unwrap();
    let review = result.review.render();

    assert!(result.review.effect_changed);
    assert!(review.contains("Effect or capability changed: yes"));
    assert!(review.contains("Added effect/capability 'cpu.avx2' to mix_samples."));
    assert!(format_human(&result.program).contains("effects [cpu.avx2]"));
    assert_eq!(check(&result.program), Vec::new());
}

#[test]
fn semantic_patch_can_add_effect_declaration_directly() {
    let program = parse_file(
        Path::new("effect_patch.fgx"),
        "fn read() -> u8:\n    return io_read()\n",
    )
    .unwrap();
    let result = apply_patch(program, "PATCH @function.read\nADD_EFFECT io.read\n").unwrap();

    assert!(result.review.effect_changed);
    assert_eq!(check(&result.program), Vec::new());
    assert!(format_agent(&result.program).contains("effects[io.read]"));
}
