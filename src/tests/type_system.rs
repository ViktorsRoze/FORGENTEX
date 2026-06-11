use std::path::Path;

use crate::diag::{diagnostics_to_json, Diagnostic};
use crate::format::{format_agent, format_human};
use crate::parser::parse_file;
use crate::typecheck::check;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let program = parse_file(Path::new("type_system.fgx"), source).unwrap();
    check(&program)
}

fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|diagnostic| diagnostic.code == code)
}

#[test]
fn generic_and_array_types_parse_format_and_check() {
    let source = "\
struct Slot:
    value: Option[u8]
    bytes: [u8; 4]

fn keep(opt: Option[u8], res: Result[u8, ParseError], bytes: [u8; 4]) -> Option[u8]:
    let first: u8 = bytes[0]
    return opt
";
    let program = parse_file(Path::new("generic_arrays.fgx"), source).unwrap();

    assert_eq!(check(&program), Vec::new());
    assert!(format_human(&program).contains("value: Option[u8]"));
    assert!(format_human(&program).contains("bytes: [u8; 4]"));
    assert!(format_agent(&program).contains("Result[u8,ParseError]"));
    assert!(format_agent(&program).contains("[u8;4]"));
}

#[test]
fn agent_struct_fields_split_array_types_at_top_level_only() {
    let source = "\
S Packet { bytes [u8;4]; maybe Option[u8] }

F first(packet Packet, xs [u8;4]) -> u8 {
  R xs[0]
}
";
    let program = parse_file(Path::new("packet.fgai"), source).unwrap();

    assert_eq!(check(&program), Vec::new());
    assert!(format_agent(&program).contains("S Packet { bytes [u8;4]; maybe Option[u8] }"));
}

#[test]
fn integer_literals_are_checked_against_destination_width() {
    let too_large = diagnostics("fn bad() -> u8:\n    return 300\n");
    assert!(has_code(&too_large, "integer_literal_out_of_range"));

    let negative_unsigned = diagnostics("fn bad() -> u8:\n    return -1\n");
    assert!(has_code(&negative_unsigned, "integer_literal_out_of_range"));

    let annotated_local = diagnostics("fn bad() -> void:\n    let byte: u8 = 999\n    return\n");
    assert!(has_code(&annotated_local, "integer_literal_out_of_range"));
}

#[test]
fn constructor_fields_are_type_and_range_checked() {
    let source = "\
struct Byte:
    value: u8

fn bad() -> Byte:
    return Byte(300)
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"integer_literal_out_of_range\""));
    assert!(json.contains("\"expected\": \"u8\""));
    assert!(json.contains("\"found\": \"$int\""));
    assert!(json.contains("replace_literal"));
}

#[test]
fn arithmetic_between_distinct_concrete_numeric_types_requires_a_cast() {
    let source = "\
fn bad(a: i16, b: i32) -> i32:
    return a + b
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"arithmetic_type_mismatch\""));
    assert!(json.contains("\"expected\": \"i16\""));
    assert!(json.contains("\"found\": \"i32\""));
    assert!(json.contains("insert_explicit_cast"));
}

#[test]
fn float_literals_do_not_flow_into_integer_api_types_implicitly() {
    let diagnostics = diagnostics("fn bad() -> i32:\n    return 1.0\n");
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"return_type_mismatch\""));
    assert!(json.contains("\"expected\": \"i32\""));
    assert!(json.contains("\"found\": \"$float\""));
    assert!(json.contains("insert_explicit_cast"));
}

#[test]
fn local_bindings_require_explicit_types() {
    let diagnostics = diagnostics("fn bad() -> i32:\n    let x = 1\n    return x\n");
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"local_type_required\""));
    assert!(json.contains("add_local_type_annotation"));
}

#[test]
fn call_arguments_and_indices_have_stable_machine_diagnostics() {
    let call_source = "\
fn takes(x: u8) -> u8:
    return x

fn bad() -> u8:
    return takes(300)
";
    let call_diagnostics = diagnostics(call_source);
    assert!(has_code(&call_diagnostics, "integer_literal_out_of_range"));

    let index_diagnostics = diagnostics("fn bad(xs: &[u8]) -> u8:\n    return xs[1.5]\n");
    let json = diagnostics_to_json(&index_diagnostics);
    assert!(json.contains("\"error\": \"index_not_integer\""));
    assert!(json.contains("\"expected\": \"usize\""));
    assert!(json.contains("\"found\": \"$float\""));
    assert!(json.contains("replace_index_expression"));
}
