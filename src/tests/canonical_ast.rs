use std::path::Path;

use crate::format::{format_agent, format_human};
use crate::parser::parse_file;

use super::{fixture, parse_fixture};

#[test]
fn canonical_ast_has_schema_and_stable_node_ids() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"schema\": \"forgentex.ast.v1\""));
    assert!(canonical.contains("\"id\": \"@struct.AudioBlock.field.samples\""));
    assert!(canonical.contains("\"id\": \"@function.mix_samples.param.dst\""));
    assert!(canonical.contains("\"loop_id\": \"@function.mix_samples.stmt.2.loop\""));
    assert!(canonical.contains("\"id\": \"@function.mix_samples.stmt.2.body.1.target\""));
}

#[test]
fn human_and_agent_views_have_equal_canonical_ast() {
    let human = parse_fixture("examples/media/audio_mix.fgx");
    let agent = parse_fixture("examples/media/audio_mix.fgai");
    assert!(human.canonical_eq(&agent));
}

#[test]
fn roundtrip_equality_uses_canonical_ast_contract() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let as_agent = format_agent(&program);
    let as_human = format_human(&program);
    let reparsed_agent = parse_file(Path::new("audio_mix.fgai"), &as_agent).unwrap();
    let reparsed_human = parse_file(Path::new("audio_mix.fgx"), &as_human).unwrap();
    assert!(program.canonical_eq(&reparsed_agent));
    assert!(program.canonical_eq(&reparsed_human));
}

#[test]
fn canonical_ast_snapshot_for_hello_is_stable() {
    let human = parse_file(Path::new("hello.fgx"), fixture("examples/hello.fgx")).unwrap();
    let canonical = human.to_canonical_json();
    assert!(canonical.contains("\"schema\": \"forgentex.ast.v1\""));
    assert!(canonical.contains("\"id\": \"@struct.Point.field.x\""));
    assert!(canonical.contains("\"id\": \"@function.area.param.width\""));
    assert!(canonical.contains("\"id\": \"@function.area.stmt.0.expr.left\""));
}

#[test]
fn canonical_ast_exposes_target_metadata() {
    let program = parse_fixture("examples/media/popcount_x86.fgx");
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"target_metadata\": [\"cpu.popcnt\", \"target.x86_64\"]"));
}
