use std::path::Path;

use crate::abi::{abi_json, abi_text, analyze_abi, generate_header, generate_struct_header};
use crate::format::{format_agent, format_human};
use crate::parser::parse_file;
use crate::typecheck::check;

use super::{fixture, parse_fixture};

#[test]
fn layout_attributes_parse_format_roundtrip_and_typecheck() {
    let program = parse_fixture("examples/abi/layout_demo.fgx");
    assert_eq!(check(&program), Vec::new());

    let human = format_human(&program);
    assert!(human.contains("@repr(C)"));
    assert!(human.contains("@aligned(16)"));
    assert!(human.contains("    @endian(little)"));

    let agent = format_agent(&program);
    assert!(agent.contains("A repr(C)"));
    assert!(agent.contains("@endian(little)"));

    let reparsed_human = parse_file(Path::new("layout_demo.fgx"), &human).unwrap();
    let reparsed_agent = parse_file(Path::new("layout_demo.fgai"), &agent).unwrap();
    assert_eq!(program, reparsed_human);
    assert_eq!(program, reparsed_agent);
}

#[test]
fn abi_report_is_target_aware_for_pointer_sized_fields() {
    let program = parse_fixture("examples/abi/layout_demo.fgx");
    let x86 = analyze_abi(&program, "x86_64-unknown-linux-gnu");
    let arm = analyze_abi(&program, "armv7-unknown-linux-gnueabihf");

    let x86_packet = x86
        .structs
        .iter()
        .find(|layout| layout.name == "AbiPacket")
        .unwrap();
    let arm_packet = arm
        .structs
        .iter()
        .find(|layout| layout.name == "AbiPacket")
        .unwrap();

    assert_eq!(x86.model.pointer_size, 8);
    assert_eq!(arm.model.pointer_size, 4);
    assert_eq!(x86_packet.size, 32);
    assert_eq!(arm_packet.size, 16);
    assert_eq!(x86_packet.fields[3].name, "data");
    assert_eq!(x86_packet.fields[3].size, 8);
    assert_eq!(arm_packet.fields[3].size, 4);
    assert_eq!(x86.diagnostics, Vec::new());
    assert_eq!(arm.diagnostics, Vec::new());

    let text = abi_text(&x86);
    assert!(text.contains("cc=sysv64"));
    assert!(text.contains("explicit_padding=yes"));
    let json = abi_json(&arm);
    assert!(json.contains("\"schema\": \"forgentex.abi.v1\""));
    assert!(json.contains("\"calling_convention\": \"aapcs32\""));
}

#[test]
fn packed_and_endian_layout_are_reported() {
    let program = parse_fixture("examples/abi/layout_demo.fgx");
    let report = analyze_abi(&program, "riscv64gc-unknown-linux-gnu");
    let packed = report
        .structs
        .iter()
        .find(|layout| layout.name == "PackedHeader")
        .unwrap();

    assert!(packed.packed);
    assert_eq!(packed.size, 5);
    assert_eq!(packed.align, 1);
    assert_eq!(packed.fields[1].offset, 1);
    assert_eq!(packed.fields[1].endian.as_deref(), Some("big"));
}

#[test]
fn header_generation_includes_layout_and_c_abi_declarations() {
    let program = parse_fixture("examples/abi/layout_demo.fgx");
    let header = generate_header(&program, "aarch64-unknown-linux-gnu").unwrap();

    assert!(header.contains("#define FGX_ABI_TARGET \"aarch64-unknown-linux-gnu\""));
    assert!(header.contains("typedef struct FGX_ALIGNAS(16) AbiPacket"));
    assert!(header.contains("uint8_t _pad0[3];"));
    assert!(header.contains("#pragma pack(push, 1)"));
    assert!(header.contains("FGX_EXTERN size_t inspect_packet(const AbiPacket * packet);"));
}

#[test]
fn selected_struct_header_orders_embedded_dependencies_before_users() {
    let source = r#"
@repr(C)
struct AOuter:
    nested: ZNested

@repr(C)
struct ZNested:
    value: i32
"#;
    let program = parse_file(Path::new("nested_header.fgx"), source).unwrap();
    let header = generate_struct_header(
        &program,
        "x86_64-unknown-linux-gnu",
        &["AOuter".to_string(), "ZNested".to_string()],
    )
    .unwrap();
    let nested = header.find("typedef struct ZNested").unwrap();
    let outer = header.find("typedef struct AOuter").unwrap();
    assert!(nested < outer, "embedded type must be declared first");
}

#[test]
fn selected_struct_header_emits_native_multidimensional_array_declarators() {
    let source = r#"
@repr(C)
struct Point:
    x: i16

@repr(C)
struct Grid:
    points: [[Point; 2]; 3]
"#;
    let program = parse_file(Path::new("nested_array_header.fgx"), source).unwrap();
    let header = generate_struct_header(
        &program,
        "x86_64-unknown-linux-gnu",
        &["Grid".to_string(), "Point".to_string()],
    )
    .unwrap();
    assert!(header.contains("Point points[3][2];"), "{header}");
    assert!(
        header.find("typedef struct Point").unwrap() < header.find("typedef struct Grid").unwrap()
    );
}

#[test]
fn abi_diagnostics_cover_invalid_layout_annotations() {
    let source = r#"
@aligned(3)
struct Bad:
    @endian(middle)
    ptr: *const u8
    value: Option[u8]
"#;
    let program = parse_file(Path::new("bad_abi.fgx"), source).unwrap();
    let diagnostics = analyze_abi(&program, "x86_64-pc-windows-msvc").diagnostics;

    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "abi_repr_required"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "abi_invalid_alignment"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "abi_invalid_endian"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "abi_endian_non_integer"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "abi_unsupported_type"));
}

#[test]
fn fixture_is_available_for_parser_lists() {
    let source = fixture("examples/abi/layout_demo.fgx");
    let program = parse_file(Path::new("layout_demo.fgx"), source).unwrap();
    assert_eq!(program.structs().count(), 2);
}
