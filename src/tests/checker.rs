use std::path::Path;

use crate::diag::diagnostics_to_json;
use crate::parser::parse_file;
use crate::typecheck::check;

use super::{fixture, parse_fixture};

#[test]
fn all_v0_examples_typecheck() {
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
        "examples/media/popcount_aarch64.fgx",
        "examples/media/popcount_riscv64.fgx",
        "examples/arch/x86_64_popcount.fgx",
        "examples/arch/aarch64_popcount.fgx",
        "examples/arch/riscv64_add.fgx",
        "examples/arch/aarch64_copy_neon.fgx",
        "examples/arch/riscv64_vector_add_v.fgx",
        "examples/io/file_copy.fgx",
        "examples/network/agent_ping.fgx",
        "examples/agent/signed_patch.fgx",
        "examples/agent/payment_receipt.fgx",
    ] {
        let program = parse_file(Path::new(path), fixture(path)).unwrap();
        let diagnostics = check(&program);
        assert_eq!(diagnostics, Vec::new(), "{path} should typecheck");
    }
}

#[test]
fn diagnostics_json_snapshot_is_stable() {
    let source = "fn bad() -> i32:\n    return true\n";
    let program = parse_file(Path::new("bad.fgx"), source).unwrap();
    let diagnostics = check(&program);
    let json = diagnostics_to_json(&diagnostics);
    assert!(json.contains("\"error\": \"return_type_mismatch\""));
    assert!(json.contains("\"node\": \"@function.bad\""));
    assert!(json.contains("\"expected\": \"i32\""));
    assert!(json.contains("\"found\": \"bool\""));
    assert!(json.contains("change return type or insert cast"));
}

#[test]
fn unsafe_assembly_is_required() {
    let source = "fn bad(x: u64) -> u32:\n    asm x86_64:\n        in rax: x\n        op popcnt rax rax\n    return u32(x)\n";
    let program = parse_file(Path::new("bad.fgx"), source).unwrap();
    let diagnostics = check(&program);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "unsafe_required"));
}

#[test]
fn audio_typechecks() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    assert_eq!(check(&program), Vec::new());
}
