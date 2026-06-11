use std::path::Path;

use crate::format::{format_agent, format_human};
use crate::parser::parse_file;

use super::{fixture, parse_fixture};

#[test]
fn audio_roundtrips_through_agent_syntax() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let agent = format_agent(&program);
    let reparsed = parse_file(Path::new("audio_mix.fgai"), &agent).unwrap();
    assert_eq!(program, reparsed);
}

#[test]
fn formatting_is_idempotent_for_human_and_agent_modes() {
    let program = parse_fixture("examples/media/audio_mix.fgx");

    let human_once = format_human(&program);
    let human_twice = format_human(&parse_file(Path::new("audio_mix.fgx"), &human_once).unwrap());
    assert_eq!(human_once, human_twice);

    let agent_once = format_agent(&program);
    let agent_twice = format_agent(&parse_file(Path::new("audio_mix.fgai"), &agent_once).unwrap());
    assert_eq!(agent_once, agent_twice);
}

#[test]
fn formatted_hello_agent_snapshot_is_stable() {
    let program = parse_fixture("examples/hello.fgx");
    let expected = "S Point { x i32; y i32 }\n\n\nF area(width i32, height i32) -> i32 {\n  R width * height\n}\n";
    assert_eq!(format_agent(&program), expected);
}

#[test]
fn all_v0_examples_roundtrip_to_their_own_mode() {
    for path in [
        "examples/hello.fgx",
        "examples/hello.fgai",
        "examples/abi/layout_demo.fgx",
        "examples/media/audio_mix.fgx",
        "examples/media/audio_mix.fgai",
        "examples/media/audio_mix_simd.fgx",
        "examples/media/bitreader.fgx",
        "examples/media/h264_nal_header.fgx",
        "examples/media/idct4x4.fgx",
        "examples/media/popcount_x86.fgx",
        "examples/io/file_copy.fgx",
        "examples/network/agent_ping.fgx",
        "examples/agent/signed_patch.fgx",
        "examples/agent/payment_receipt.fgx",
    ] {
        let program = parse_file(Path::new(path), fixture(path)).unwrap();
        let rendered = if path.ends_with(".fgai") {
            format_agent(&program)
        } else {
            format_human(&program)
        };
        let reparsed = parse_file(Path::new(path), &rendered).unwrap();
        assert_eq!(program, reparsed, "{path} should roundtrip");
    }
}
