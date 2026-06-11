mod abi;
mod assembly;
mod backend;
mod borrow_conformance;
mod borrow_report;
mod c_backend;
mod canonical_ast;
mod cfg;
mod checker;
mod crypto_agent;
mod diagnostic_catalog;
mod effects;
mod formatter;
mod fuzzing;
mod graph;
mod inline_tests;
mod interpreter;
mod io_network;
mod json;
mod lsp;
mod ownership;
mod package;
mod parser;
mod parser_infrastructure;
mod patcher;
mod perf;
mod project;
mod project_map;
mod release;
mod security;
mod simd;
mod stdlib;
mod targets;
mod type_system;
mod vscode_extension;
mod workbench;
mod zig_backend;

use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::parser::parse_file;

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn fixture(relative: &str) -> &'static str {
    match relative {
        "examples/hello.fgx" => include_str!("../../examples/hello.fgx"),
        "examples/hello.fgai" => include_str!("../../examples/hello.fgai"),
        "examples/abi/layout_demo.fgx" => include_str!("../../examples/abi/layout_demo.fgx"),
        "examples/media/audio_mix.fgx" => include_str!("../../examples/media/audio_mix.fgx"),
        "examples/media/audio_mix.fgai" => include_str!("../../examples/media/audio_mix.fgai"),
        "examples/media/audio_mix_simd.fgx" => {
            include_str!("../../examples/media/audio_mix_simd.fgx")
        }
        "examples/media/bitreader.fgx" => include_str!("../../examples/media/bitreader.fgx"),
        "examples/media/h264_nal_header.fgx" => {
            include_str!("../../examples/media/h264_nal_header.fgx")
        }
        "examples/media/idct4x4.fgx" => include_str!("../../examples/media/idct4x4.fgx"),
        "examples/media/popcount_x86.fgx" => {
            include_str!("../../examples/media/popcount_x86.fgx")
        }
        "examples/media/popcount_aarch64.fgx" => {
            include_str!("../../examples/media/popcount_aarch64.fgx")
        }
        "examples/media/popcount_riscv64.fgx" => {
            include_str!("../../examples/media/popcount_riscv64.fgx")
        }
        "examples/io/file_copy.fgx" => include_str!("../../examples/io/file_copy.fgx"),
        "examples/network/agent_ping.fgx" => include_str!("../../examples/network/agent_ping.fgx"),
        "examples/agent/signed_patch.fgx" => include_str!("../../examples/agent/signed_patch.fgx"),
        "examples/agent/payment_receipt.fgx" => {
            include_str!("../../examples/agent/payment_receipt.fgx")
        }
        "examples/smoke/inline_tests.fgx" => {
            include_str!("../../examples/smoke/inline_tests.fgx")
        }
        "examples/arch/x86_64_popcount.fgx" => {
            include_str!("../../examples/arch/x86_64_popcount.fgx")
        }
        "examples/arch/aarch64_popcount.fgx" => {
            include_str!("../../examples/arch/aarch64_popcount.fgx")
        }
        "examples/arch/riscv64_add.fgx" => include_str!("../../examples/arch/riscv64_add.fgx"),
        "examples/arch/aarch64_copy_neon.fgx" => {
            include_str!("../../examples/arch/aarch64_copy_neon.fgx")
        }
        "examples/arch/riscv64_vector_add_v.fgx" => {
            include_str!("../../examples/arch/riscv64_vector_add_v.fgx")
        }
        "patches/add_fast_path.fgp" => include_str!("../../patches/add_fast_path.fgp"),
        "patches/signed_add_crypto_effect.fgp" => {
            include_str!("../../patches/signed_add_crypto_effect.fgp")
        }
        _ => panic!("unknown fixture: {relative}"),
    }
}

fn parse_fixture(relative: &str) -> Program {
    parse_file(Path::new(relative), fixture(relative)).expect("fixture should parse")
}
