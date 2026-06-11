use std::path::Path;

use crate::parser::{parse_agent, parse_file, parse_human};

use super::{fixture, parse_fixture};

#[test]
fn hello_human_and_agent_share_ast() {
    let human_ast = parse_fixture("examples/hello.fgx");
    let agent_ast = parse_fixture("examples/hello.fgai");
    assert_eq!(human_ast, agent_ast);
}

#[test]
fn all_v0_examples_parse() {
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
        parse_file(Path::new(path), fixture(path)).unwrap_or_else(|err| {
            panic!("{path} should parse, got: {err}");
        });
    }
}

#[test]
fn common_human_syntax_mistakes_have_clear_errors() {
    let struct_err = parse_human("struct Bad\n    x: i32\n").unwrap_err();
    assert!(struct_err.contains("struct declaration must end with ':'"));

    let if_err =
        parse_human("fn bad(x: i32) -> i32:\n    if x > 0\n        return x\n").unwrap_err();
    assert!(if_err.contains("if statement must end with ':'"));

    let for_err =
        parse_human("fn bad(xs: &[i16]) -> void:\n    for i in 0 .. xs.len\n").unwrap_err();
    assert!(for_err.contains("for loop must end with ':'"));

    let asm_err =
        parse_human("unsafe fn bad(x: u64) -> void:\n    asm x86_64\n        in rax: x\n")
            .unwrap_err();
    assert!(asm_err.contains("asm block must end with ':'"));
}

#[test]
fn common_agent_syntax_mistakes_have_clear_errors() {
    let missing_brace = parse_agent("F bad(x i32) -> i32 {\n  R x\n").unwrap_err();
    assert!(missing_brace.contains("missing closing '}'"));

    let missing_nested_brace =
        parse_agent("F bad(x i32) -> i32 {\n  IF x > 0 {\n    R x\n}\n").unwrap_err();
    assert!(missing_nested_brace.contains("missing closing '}'"));
}
