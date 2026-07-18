use std::path::{Path, PathBuf};

use crate::ast::Type;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StdModule {
    pub name: &'static str,
    pub purpose: &'static str,
    pub functions: &'static [&'static str],
}

const MODULES: &[StdModule] = &[
    StdModule {
        name: "core.mem",
        purpose: "allocation-free memory predicates and raw-memory review boundaries",
        functions: &["core_mem_is_aligned_i16"],
    },
    StdModule {
        name: "core.slice",
        purpose: "slice length, bounds, and precondition helpers",
        functions: &[
            "core_slice_len_i16",
            "core_slice_len_u8",
            "core_slice_len_mut_i16",
            "core_slice_require_same_len_i16",
            "core_slice_require_mut_same_len_i16",
            "core_slice_copy_i16",
        ],
    },
    StdModule {
        name: "core.math",
        purpose: "checked arithmetic and DSP-friendly numeric helpers",
        functions: &[
            "core_math_clamp_i16",
            "core_math_checked_add_i32",
            "core_math_checked_sub_i32",
            "core_math_checked_mul_i32",
            "core_math_saturating_add_i16",
        ],
    },
    StdModule {
        name: "core.music",
        purpose: "deterministic bar/beat/tick conversion, cue quantization, and tempo maps",
        functions: &[
            "core_music_position_valid",
            "core_music_to_tick",
            "core_music_bar_start_tick",
            "core_music_tick_to_millis",
            "core_music_quantize_tick",
            "core_music_make_cue",
            "core_music_make_action",
            "core_music_action_valid",
            "core_music_cue_valid",
            "core_music_cue_action_valid",
            "core_music_cues_overlap",
            "core_music_cues_conflict",
            "core_music_cue_wins",
            "core_music_conflict_winner_id",
            "core_music_tempo_map_valid",
            "core_music_tempo_map_millis",
        ],
    },
    StdModule {
        name: "core.result",
        purpose: "allocation-free Result constructors and predicates",
        functions: &[
            "core_result_ok_u8",
            "core_result_err_u8",
            "core_result_is_ok_u8",
            "core_result_is_err_u8",
            "core_result_unwrap_or_u8",
        ],
    },
    StdModule {
        name: "core.option",
        purpose: "allocation-free Option constructors and predicates",
        functions: &[
            "core_option_some_u8",
            "core_option_none_u8",
            "core_option_is_some_u8",
            "core_option_is_none_u8",
            "core_option_unwrap_or_u8",
        ],
    },
    StdModule {
        name: "core.bits",
        purpose:
            "bit extraction, population count, count-leading/trailing-zero, and rotation helpers",
        functions: &[
            "core_bits_extract_u8",
            "core_bits_popcount_u32",
            "core_bits_clz_u32",
            "core_bits_ctz_u32",
            "core_bits_rotl_u32",
        ],
    },
    StdModule {
        name: "core.simd",
        purpose: "portable SIMD API names layered over Stage 13 intrinsics",
        functions: &[
            "core_simd_load_i16x8",
            "core_simd_store_i16x8",
            "core_simd_vec_i16x8",
            "core_simd_splat_i16x8",
            "core_simd_saturating_add",
            "core_simd_eq",
            "core_simd_select",
            "core_simd_reduce_add",
        ],
    },
    StdModule {
        name: "core.target",
        purpose: "runtime and compile-time target feature queries",
        functions: &[
            "core_target_has_feature_neon",
            "core_target_has_feature_avx2",
            "core_target_has_feature_riscv_v",
            "core_target_feature_enabled_neon",
            "core_target_feature_enabled_avx2",
            "core_target_feature_enabled_riscv_v",
            "core_target_is_x86_64",
            "core_target_is_aarch64",
            "core_target_is_riscv64",
        ],
    },
    StdModule {
        name: "core.fs",
        purpose: "capability-scoped synchronous filesystem APIs backed by the demo runtime",
        functions: &[
            "core_fs_scope_read",
            "core_fs_scope_write",
            "core_fs_read_text",
            "core_fs_write_text",
            "core_fs_copy_text",
        ],
    },
    StdModule {
        name: "core.io",
        purpose: "real, effect-gated filesystem I/O executed and transcribed by `fgx io-run`",
        functions: &[
            "core_io_read_file",
            "core_io_write_file",
            "core_io_copy_file",
        ],
    },
    StdModule {
        name: "core.hash",
        purpose: "real, test-vector-verified SHA-256, HMAC-SHA256, and constant-time compare",
        functions: &[
            "core_hash_sha256",
            "core_hash_hmac_sha256",
            "core_hash_ct_eq",
        ],
    },
    StdModule {
        name: "core.socket",
        purpose: "real, effect-gated TCP sockets executed and transcribed by `fgx net-run`",
        functions: &["core_socket_tcp_request", "core_socket_tcp_send"],
    },
    StdModule {
        name: "core.net",
        purpose: "capability-scoped network API names for HTTP, TCP, UDP, and WebSocket demos",
        functions: &[
            "core_net_http_get",
            "core_net_http_post",
            "core_net_tcp_send",
            "core_net_udp_send",
            "core_net_websocket_send",
        ],
    },
    StdModule {
        name: "core.serialize",
        purpose: "structured serialization helpers for deterministic agent/runtime demos",
        functions: &["core_serialize_text_len"],
    },
    StdModule {
        name: "core.stream",
        purpose: "streaming helper surface for chunk-oriented I/O demos",
        functions: &["core_stream_count_bytes"],
    },
    StdModule {
        name: "core.crypto",
        purpose: "typed mock cryptographic primitives and secret-safe API boundaries",
        functions: &[
            "core_crypto_mock_secret_key",
            "core_crypto_public_key_ed25519",
            "core_crypto_hash_blake3",
            "core_crypto_sign_ed25519",
            "core_crypto_verify_ed25519",
            "core_crypto_nonce",
            "core_crypto_export_secret_key",
            "core_crypto_clone_secret_key",
        ],
    },
    StdModule {
        name: "core.agent",
        purpose: "mock agent-economy token, receipt, and signed-patch helpers",
        functions: &[
            "core_agent_token_amount",
            "core_agent_payment_receipt",
            "core_agent_receipt_ok",
            "core_agent_signed_patch_hash",
            "core_agent_log_secret",
        ],
    },
];

pub fn standard_library_modules() -> &'static [StdModule] {
    MODULES
}

pub fn standard_library_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib")
}

pub fn standard_library_source_root() -> PathBuf {
    standard_library_root().join("src")
}

pub fn is_standard_module(name: &str) -> bool {
    MODULES.iter().any(|module| module.name == name)
}

pub fn standard_library_text() -> String {
    let mut out = String::new();
    out.push_str("Forgentex standard library v0\n");
    out.push_str("Schema: forgentex.stdlib.v1\n");
    out.push_str(&format!(
        "Source package: {}\n",
        standard_library_root().display()
    ));
    for module in MODULES {
        out.push_str(&format!(
            "  - {} purpose={} functions=[{}]\n",
            module.name,
            module.purpose,
            module.functions.join(",")
        ));
    }
    out
}

pub fn standard_library_json() -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.stdlib.v1\",\n");
    out.push_str(&format!(
        "  \"source_package\": {},\n",
        quoted(
            &standard_library_root()
                .display()
                .to_string()
                .replace('\\', "/")
        )
    ));
    out.push_str("  \"modules\": [");
    for (idx, module) in MODULES.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"name\": {},\n      \"purpose\": {},",
            quoted(module.name),
            quoted(module.purpose)
        ));
        out.push_str("\n      \"functions\": [");
        for (function_idx, function) in module.functions.iter().enumerate() {
            if function_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&quoted(function));
        }
        out.push_str("]\n    }");
    }
    if !MODULES.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("]\n}");
    out
}

pub fn standard_builtin_call_type(name: &str) -> Option<Type> {
    let i16_ty = primitive("i16");
    let i32_ty = primitive("i32");
    let u8_ty = primitive("u8");
    let u32_ty = primitive("u32");
    let usize_ty = primitive("usize");
    let str_ty = primitive("str");
    let hash_blake3_ty = generic("Hash", vec![named("Blake3")]);
    let secret_ed25519_ty = generic("SecretKey", vec![named("Ed25519")]);
    let public_ed25519_ty = generic("PublicKey", vec![named("Ed25519")]);
    let signature_ed25519_ty = generic("Signature", vec![named("Ed25519")]);
    let token_amount_ty = generic("TokenAmount", vec![named("MockLedger")]);
    match name {
        "core_mem_is_aligned_i16" => Some(Type::Bool),
        "core_slice_len_i16" | "core_slice_len_u8" | "core_slice_len_mut_i16" => Some(usize_ty),
        "core_slice_require_same_len_i16"
        | "core_slice_require_mut_same_len_i16"
        | "core_slice_copy_i16" => Some(Type::Void),
        "core_math_clamp_i16" | "core_math_saturating_add_i16" => Some(i16_ty),
        "core_math_checked_add_i32" | "core_math_checked_sub_i32" | "core_math_checked_mul_i32" => {
            Some(i32_ty)
        }
        "core_option_some_u8" | "core_option_none_u8" => Some(Type::Generic {
            name: "Option".to_string(),
            args: vec![u8_ty],
        }),
        "core_option_is_some_u8" | "core_option_is_none_u8" => Some(Type::Bool),
        "core_option_unwrap_or_u8" => Some(primitive("u8")),
        "core_result_ok_u8" | "core_result_err_u8" => Some(Type::Generic {
            name: "Result".to_string(),
            args: vec![primitive("u8"), primitive("u8")],
        }),
        "core_result_is_ok_u8" | "core_result_is_err_u8" => Some(Type::Bool),
        "core_result_unwrap_or_u8" => Some(primitive("u8")),
        "core_bits_extract_u8" => Some(primitive("u8")),
        "core_bits_popcount_u32"
        | "core_bits_clz_u32"
        | "core_bits_ctz_u32"
        | "core_bits_rotl_u32" => Some(u32_ty),
        "core_fs_scope_read"
        | "core_runtime_fs_scope_read"
        | "core_fs_scope_write"
        | "core_runtime_fs_scope_write" => Some(Type::Bool),
        "core_io_read_file" => Some(str_ty.clone()),
        "core_io_write_file" | "core_io_copy_file" => Some(usize_ty.clone()),
        "core_socket_tcp_request" => Some(str_ty.clone()),
        "core_socket_tcp_send" => Some(usize_ty.clone()),
        "core_hash_sha256" | "core_hash_hmac_sha256" => Some(str_ty.clone()),
        "core_hash_ct_eq" => Some(Type::Bool),
        "core_fs_read_text"
        | "core_runtime_fs_read_text"
        | "core_net_http_get"
        | "core_runtime_net_http_get"
        | "core_net_http_post"
        | "core_runtime_net_http_post" => Some(Type::Generic {
            name: "Result".to_string(),
            args: vec![str_ty.clone(), str_ty.clone()],
        }),
        "core_fs_write_text"
        | "core_runtime_fs_write_text"
        | "core_fs_copy_text"
        | "core_runtime_fs_copy_text"
        | "core_net_tcp_send"
        | "core_runtime_net_tcp_send"
        | "core_net_udp_send"
        | "core_runtime_net_udp_send"
        | "core_net_websocket_send"
        | "core_runtime_net_websocket_send" => Some(Type::Generic {
            name: "Result".to_string(),
            args: vec![usize_ty.clone(), str_ty],
        }),
        "core_serialize_text_len"
        | "core_runtime_serialize_text_len"
        | "core_stream_count_bytes"
        | "core_runtime_stream_count_bytes" => Some(usize_ty),
        "core_crypto_mock_secret_key" | "core_runtime_crypto_mock_secret_key" => {
            Some(secret_ed25519_ty.clone())
        }
        "core_crypto_public_key_ed25519" | "core_runtime_crypto_public_key_ed25519" => {
            Some(public_ed25519_ty.clone())
        }
        "core_crypto_hash_blake3" | "core_runtime_crypto_hash_blake3" => {
            Some(hash_blake3_ty.clone())
        }
        "core_crypto_sign_ed25519" | "core_runtime_crypto_sign_ed25519" => {
            Some(signature_ed25519_ty.clone())
        }
        "core_crypto_verify_ed25519" | "core_runtime_crypto_verify_ed25519" => Some(Type::Bool),
        "core_crypto_nonce" | "core_runtime_crypto_nonce" => Some(named("Nonce")),
        "core_crypto_export_secret_key" | "core_runtime_crypto_export_secret_key" => {
            Some(primitive("str"))
        }
        "core_crypto_clone_secret_key" | "core_runtime_crypto_clone_secret_key" => {
            Some(secret_ed25519_ty)
        }
        "core_agent_log_secret" | "core_runtime_agent_log_secret" => Some(Type::Void),
        "core_agent_token_amount" | "core_runtime_agent_token_amount" => Some(token_amount_ty),
        "core_agent_payment_receipt" | "core_runtime_agent_payment_receipt" => {
            Some(named("Receipt"))
        }
        "core_agent_receipt_ok" | "core_runtime_agent_receipt_ok" => Some(Type::Bool),
        "core_agent_signed_patch_hash" | "core_runtime_agent_signed_patch_hash" => {
            Some(hash_blake3_ty)
        }
        _ if is_standard_target_probe(name) => Some(Type::Bool),
        _ if canonical_simd_intrinsic(name).is_some() => None,
        _ => None,
    }
}

pub fn canonical_simd_intrinsic(name: &str) -> Option<String> {
    if name.starts_with("simd_") {
        return Some(name.to_string());
    }
    name.strip_prefix("core_simd_")
        .map(|rest| format!("simd_{rest}"))
}

pub fn standard_target_feature_probe(name: &str) -> Option<String> {
    name.strip_prefix("cpu_has_")
        .or_else(|| name.strip_prefix("core_target_has_feature_"))
        .or_else(|| name.strip_prefix("core_target_feature_enabled_"))
        .and_then(normalize_feature_name)
}

pub fn standard_target_family_probe(name: &str) -> Option<String> {
    name.strip_prefix("core_target_is_").and_then(|family| {
        let family = normalize_feature_alias(family);
        if family.is_empty() {
            None
        } else {
            Some(family)
        }
    })
}

pub fn is_standard_target_probe(name: &str) -> bool {
    standard_target_feature_probe(name).is_some() || standard_target_family_probe(name).is_some()
}

fn normalize_feature_name(name: &str) -> Option<String> {
    let normalized = normalize_feature_alias(name);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_feature_alias(name: &str) -> String {
    match name {
        "riscv_v" | "riscv_vector" => "v".to_string(),
        "x86_64" => "x86_64".to_string(),
        other => other.to_string(),
    }
}

fn primitive(name: &str) -> Type {
    Type::Primitive(name.to_string())
}

fn named(name: &str) -> Type {
    Type::Named(name.to_string())
}

fn generic(name: &str, args: Vec<Type>) -> Type {
    Type::Generic {
        name: name.to_string(),
        args,
    }
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
