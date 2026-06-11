use std::path::Path;

use crate::asmcheck::{analyze_assembly, check_assembly, review_text};
use crate::parser::parse_file;

use super::{fixture, parse_fixture};

#[test]
fn arch_examples_pass_assembly_check_for_matching_targets() {
    let cases = [
        (
            "examples/arch/x86_64_popcount.fgx",
            "x86_64-pc-windows-msvc",
        ),
        (
            "examples/arch/aarch64_popcount.fgx",
            "aarch64-unknown-linux-gnu",
        ),
        (
            "examples/arch/riscv64_add.fgx",
            "riscv64gc-unknown-linux-gnu",
        ),
        (
            "examples/arch/aarch64_copy_neon.fgx",
            "aarch64-unknown-linux-gnu",
        ),
        (
            "examples/arch/riscv64_vector_add_v.fgx",
            "riscv64gc-unknown-linux-gnu",
        ),
    ];
    for (path, target) in cases {
        let program = parse_file(Path::new(path), fixture(path)).unwrap();
        let diagnostics = check_assembly(&program, target);
        assert_eq!(diagnostics, Vec::new(), "{path} should asm-check");
    }
}

#[test]
fn assembly_outside_unsafe_fails_assembly_check() {
    let source = "fn bad(x: u64) -> u32:\n    asm x86_64:\n        in rax: x\n        out rax: y\n        op popcnt rax rax\n    return u32(y)\n";
    let program = parse_file(Path::new("bad.fgx"), source).unwrap();
    let diagnostics = check_assembly(&program, "x86_64-pc-windows-msvc");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "unsafe_required"));
}

#[test]
fn invalid_register_for_target_fails_assembly_check() {
    let source = "unsafe fn bad(x: u64) -> u64:\n    asm aarch64:\n        in rax: x\n        out x0: y\n        op add x0 x0 x0\n    return y\n";
    let program = parse_file(Path::new("bad_register.fgx"), source).unwrap();
    let diagnostics = check_assembly(&program, "aarch64-unknown-linux-gnu");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "asm_invalid_register"));
}

#[test]
fn feature_specific_instruction_requires_cpu_effect() {
    let source = "unsafe fn bad(x: u64) -> u32:\n    asm x86_64:\n        in rax: x\n        out rax: y\n        op popcnt rax rax\n    return u32(y)\n";
    let program = parse_file(Path::new("missing_feature.fgx"), source).unwrap();
    let diagnostics = check_assembly(&program, "x86_64-pc-windows-msvc");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "asm_feature_required"));
}

#[test]
fn declared_cpu_feature_allows_feature_specific_instruction() {
    let program = parse_fixture("examples/arch/x86_64_popcount.fgx");
    let diagnostics = check_assembly(&program, "x86_64-pc-windows-msvc");
    assert_eq!(diagnostics, Vec::new());
}

#[test]
fn riscv_vector_register_requires_vector_feature() {
    let source = "unsafe fn bad(a: u64, b: u64) -> u64:\n    asm riscv64:\n        in a0: a\n        in a1: b\n        out a0: y\n        op vadd.vv v0 v0 v1\n    return y\n";
    let program = parse_file(Path::new("missing_rvv.fgx"), source).unwrap();
    let diagnostics = check_assembly(&program, "riscv64gc-unknown-linux-gnu");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "asm_feature_required"));
}

#[test]
fn assembly_review_reports_registers_clobbers_memory_and_abi_risk() {
    let program = parse_fixture("examples/arch/x86_64_popcount.fgx");
    let result = analyze_assembly(&program, "x86_64-pc-windows-msvc");
    assert_eq!(result.diagnostics, Vec::new());
    let review = review_text(&result);
    assert!(review.contains("changed=[rax]"));
    assert!(review.contains("clobbers=[flags]"));
    assert!(review.contains("features=[popcnt]"));
    assert!(review.contains("memory=yes"));
    assert!(review.contains("abi_risk=medium"));
}
