use std::fs;

use crate::c_backend::{build_c_project, generate_c_for_project};

use super::manifest_path;

#[test]
fn c_backend_generates_header_and_source_for_media_project() {
    let (header, source) = generate_c_for_project(&manifest_path(".")).unwrap();

    assert!(header.contains("#include <stdint.h>"));
    assert!(header.contains("FIR signatures: forgentex.backend_ir.v1"));
    assert!(header.contains("FIR struct @struct.AudioBlock"));
    assert!(header.contains("typedef struct fgx_slice_i16"));
    assert!(header.contains("typedef struct AudioBlock"));
    assert!(header.contains("void mix_samples(fgx_slice_i16 dst"));
    assert!(header.contains("H264NalHeader parse_h264_nal_header(uint8_t byte);"));

    assert!(source.contains("#include \"forgentex_demo_media.h\""));
    assert!(source.contains("FIR signatures: forgentex.backend_ir.v1"));
    assert!(source.contains("FIR body entry"));
    assert!(source.contains("fgx_trap(\"require_failed\")"));
    assert!(source.contains("fgx_bounds_index((size_t)(i), dst.len)"));
    assert!(source.contains("dst.data[fgx_bounds_index"));
    assert!(source.contains("return ((H264NalHeader){ .forbidden_zero = forbidden_zero"));
}

#[test]
fn c_backend_writes_artifacts() {
    let out_dir = std::env::temp_dir().join(format!("forgentex_{}_c_backend", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let artifacts = build_c_project(&manifest_path("."), &out_dir).unwrap();

    let header = fs::read_to_string(&artifacts.header).unwrap();
    let source = fs::read_to_string(&artifacts.source).unwrap();
    assert!(header.contains("FORGENTEX_DEMO_MEDIA_H_INCLUDED"));
    assert!(header.contains("FIR signatures: forgentex.backend_ir.v1"));
    assert!(header.contains("FIR struct @struct.AudioBlock"));
    assert!(source.contains("FIR body entry"));
    assert!(source.contains("void mix_samples"));

    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn c_backend_lowers_control_flow_from_fir() {
    let root =
        std::env::temp_dir().join(format!("forgentex_{}_c_backend_fir_cf", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"fir-cf-c\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/control.fgx"),
        "module fir.cf.c\nexport fn branch_loop(n: usize) -> usize:\n    let total: usize = 0\n    for i in 0 .. n:\n        if i > 2:\n            continue\n        total = total + i\n    return total\n",
    )
    .unwrap();

    let (_header, source) = generate_c_for_project(&root).unwrap();
    assert!(source.contains("/* FIR body entry */"), "{source}");
    assert!(
        source.contains("for (size_t i = (size_t)(0); i < (size_t)(n); ++i)"),
        "{source}"
    );
    assert!(source.contains("if ((i > 2))"), "{source}");
    assert!(source.contains("continue;"), "{source}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn c_backend_lowers_match_from_fir() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_c_backend_fir_match",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"fir-match-c\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/match.fgx"),
        "module fir.match.c\n\nenum Shape:\n    Circle(i32)\n    Empty\n\nexport fn area(shape: Shape) -> i32:\n    match shape:\n        case Shape::Circle(r):\n            return r * r\n        case Shape::Empty:\n            return 0\n",
    )
    .unwrap();

    let (_header, source) = generate_c_for_project(&root).unwrap();
    assert!(source.contains("/* FIR body entry */"), "{source}");
    assert!(source.contains("switch (_fgx_scrut.tag)"), "{source}");
    assert!(
        source.contains("int32_t r = _fgx_scrut.data.Circle;"),
        "{source}"
    );
    assert!(source.contains("case Shape_Empty:"), "{source}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn c_backend_rejects_review_only_assembly() {
    let root =
        std::env::temp_dir().join(format!("forgentex_{}_c_backend_unsafe", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"unsafe-demo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/pop.fgx"),
        "module unsafe.demo\nexport unsafe fn popcount_x86(x: u32) -> u32 effects [cpu.asm]:\n    asm x86_64:\n        in eax: x\n        out eax: x\n        clobber flags\n        op popcnt eax eax\n    return x\n",
    )
    .unwrap();

    let err = generate_c_for_project(&root).unwrap_err();
    // The FIR contract preflight now rejects before source emission — same
    // honesty, one uniform gate for every build entry point.
    assert!(
        err.contains("FIR contract blocks backend") && err.contains("popcount_x86"),
        "expected a contract blocker naming the function, got: {err}"
    );

    let _ = fs::remove_dir_all(root);
}
