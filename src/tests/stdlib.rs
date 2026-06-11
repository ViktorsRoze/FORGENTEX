use std::path::Path;

use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::project::{check_project, generate_project_map};
use crate::stdlib::{standard_library_json, standard_library_text};
use crate::typecheck::check;

use super::manifest_path;

#[test]
fn standard_library_catalog_lists_core_modules() {
    let text = standard_library_text();
    assert!(text.contains("Forgentex standard library v0"));
    assert!(text.contains("core.mem"));
    assert!(text.contains("core.slice"));
    assert!(text.contains("core.math"));
    assert!(text.contains("core.result"));
    assert!(text.contains("core.option"));
    assert!(text.contains("core.bits"));
    assert!(text.contains("core.simd"));
    assert!(text.contains("core.target"));
    assert!(text.contains("core.fs"));
    assert!(text.contains("core.net"));
    assert!(text.contains("core.serialize"));
    assert!(text.contains("core.stream"));
    assert!(text.contains("core.crypto"));
    assert!(text.contains("core.agent"));

    let json = standard_library_json();
    assert!(json.contains("\"schema\": \"forgentex.stdlib.v1\""));
    assert!(json.contains("\"name\": \"core.target\""));
    assert!(json.contains("\"name\": \"core.fs\""));
    assert!(json.contains("\"name\": \"core.crypto\""));
    assert!(json.contains("\"name\": \"core.agent\""));
    assert!(json.contains("core_math_checked_add_i32"));
    assert!(json.contains("core_net_http_get"));
    assert!(json.contains("core_crypto_sign_ed25519"));
    assert!(json.contains("core_agent_payment_receipt"));
}

#[test]
fn standard_library_source_package_typechecks() {
    let diagnostics = check_project(&manifest_path("stdlib")).unwrap();
    assert_eq!(diagnostics, Vec::new());
}

#[test]
fn media_project_resolves_imported_standard_modules() {
    let diagnostics = check_project(&manifest_path(".")).unwrap();
    assert_eq!(diagnostics, Vec::new());

    let map = generate_project_map(&manifest_path(".")).unwrap();
    assert!(map.contains("M core.math stdlib/src/core/math.fgx"));
    assert!(map.contains("M core.slice stdlib/src/core/slice.fgx"));
    assert!(map.contains("M core.bits stdlib/src/core/bits.fgx"));
    assert!(map.contains("imports=core.math,core.slice,media.types"));
}

#[test]
fn standard_runtime_intrinsics_conform() {
    let source = r#"
fn add_ok() -> i32:
    return core_math_checked_add_i32(20, 22)

fn header_bits(byte: u8) -> u8:
    return core_bits_extract_u8(byte, 5, 3)

fn choose(flag: bool, fallback: u8) -> u8:
    let value: Option[u8] = core_option_some_u8(9)
    if flag:
        return core_option_unwrap_or_u8(value, fallback)
    return core_option_unwrap_or_u8(core_option_none_u8(), fallback)

fn choose_result(flag: bool, fallback: u8) -> u8:
    let value: Result[u8,u8] = core_result_ok_u8(11)
    if flag:
        return core_result_unwrap_or_u8(value, fallback)
    return core_result_unwrap_or_u8(core_result_err_u8(1), fallback)
"#;
    let program = parse_file(Path::new("stdlib_conformance.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let add = run_entry_json(
        &program,
        "add_ok",
        r#"{ "args": {}, "expect": { "return": 42 } }"#,
    )
    .unwrap();
    let bits = run_entry_json(
        &program,
        "header_bits",
        r#"{ "args": { "byte": 97 }, "expect": { "return": 3 } }"#,
    )
    .unwrap();
    let option = run_entry_json(
        &program,
        "choose",
        r#"{ "args": { "flag": false, "fallback": 4 }, "expect": { "return": 4 } }"#,
    )
    .unwrap();
    let result = run_entry_json(
        &program,
        "choose_result",
        r#"{ "args": { "flag": true, "fallback": 4 }, "expect": { "return": 11 } }"#,
    )
    .unwrap();

    assert!(add.contains("\"status\": \"pass\""));
    assert!(bits.contains("\"status\": \"pass\""));
    assert!(option.contains("\"status\": \"pass\""));
    assert!(result.contains("\"status\": \"pass\""));
}

#[test]
fn checked_arithmetic_reports_deterministic_overflow_traps() {
    let source = r#"
fn overflow() -> i32:
    return core_math_checked_add_i32(2147483647, 1)
"#;
    let program = parse_file(Path::new("stdlib_overflow.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let err = run_entry_json(&program, "overflow", r#"{ "args": {} }"#).unwrap_err();
    assert!(err.contains("trap: integer_overflow"));
}
