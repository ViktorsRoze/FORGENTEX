mod abi;
mod assembly;
mod audio_dsp;
mod backend;
mod borrow_conformance;
mod borrow_report;
mod c_backend;
mod canonical_ast;
mod capsule;
mod capsule_lock;
mod capsule_tokens;
mod capsule_workbench;
mod cfg;
mod checker;
mod compress;
mod contract;
mod crypto;
mod crypto_agent;
mod diagnostic_catalog;
mod dotnet_run;
mod edit_bench;
mod effects;
mod ffi;
mod formatter;
mod fuzzing;
mod go_run;
mod graph;
mod inline_tests;
mod interpreter;
mod io_network;
mod io_run;
mod json;
mod jvm_run;
mod lsp;
mod native_build;
mod native_capsule;
mod net_run;
mod node_run;
mod numeric;
mod ownership;
mod package;
mod parser;
mod parser_infrastructure;
mod patcher;
mod perf;
mod polyglot;
mod prelude;
mod project;
mod project_map;
mod python_run;
mod release;
mod runtime;
mod sandbox_tests;
mod security;
mod share;
mod shell;
mod simd;
mod stdlib;
mod targets;
mod tokens;
mod trust;
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
        "examples/io/real_copy.fgx" => include_str!("../../examples/io/real_copy.fgx"),
        "examples/network/agent_ping.fgx" => include_str!("../../examples/network/agent_ping.fgx"),
        "examples/network/tcp_ping.fgx" => include_str!("../../examples/network/tcp_ping.fgx"),
        "examples/crypto/hashing.fgx" => include_str!("../../examples/crypto/hashing.fgx"),
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
        "examples/capsule/normalize_python.fgx" => {
            include_str!("../../examples/capsule/normalize_python.fgx")
        }
        "examples/capsule/normalize_python.fgai" => {
            include_str!("../../examples/capsule/normalize_python.fgai")
        }
        "examples/capsule/price_oracle_solidity.fgx" => {
            include_str!("../../examples/capsule/price_oracle_solidity.fgx")
        }
        "examples/capsule/web_counter.fgx" => {
            include_str!("../../examples/capsule/web_counter.fgx")
        }
        "examples/capsule/web_counter.fgai" => {
            include_str!("../../examples/capsule/web_counter.fgai")
        }
        "examples/capsule/stats_python.fgx" => {
            include_str!("../../examples/capsule/stats_python.fgx")
        }
        "examples/capsule/celsius_python.fgx" => {
            include_str!("../../examples/capsule/celsius_python.fgx")
        }
        "examples/polyglot/pipeline.fgx" => {
            include_str!("../../examples/polyglot/pipeline.fgx")
        }
        "examples/web/one_roof.fgx" => include_str!("../../examples/web/one_roof.fgx"),
        "examples/ts/classify.fgx" => include_str!("../../examples/ts/classify.fgx"),
        "examples/jvm/tax.fgx" => include_str!("../../examples/jvm/tax.fgx"),
        "examples/dotnet/discount.fgx" => include_str!("../../examples/dotnet/discount.fgx"),
        "examples/go/scale.fgx" => include_str!("../../examples/go/scale.fgx"),
        "examples/capsule/web_session.fgx" => {
            include_str!("../../examples/capsule/web_session.fgx")
        }
        "examples/capsule/order_total_java.fgx" => {
            include_str!("../../examples/capsule/order_total_java.fgx")
        }
        "examples/capsule/clamp_c.fgx" => include_str!("../../examples/capsule/clamp_c.fgx"),
        "examples/capsule/saxpy_cuda.fgx" => {
            include_str!("../../examples/capsule/saxpy_cuda.fgx")
        }
        "examples/audio/src/dsp.fgx" => include_str!("../../examples/audio/src/dsp.fgx"),
        "examples/capsule/backup_bash.fgx" => {
            include_str!("../../examples/capsule/backup_bash.fgx")
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
