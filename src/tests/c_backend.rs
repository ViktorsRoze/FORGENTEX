use std::fs;

use crate::c_backend::{build_c_project, generate_c_for_project};

use super::manifest_path;

#[test]
fn c_backend_generates_header_and_source_for_media_project() {
    let (header, source) = generate_c_for_project(&manifest_path(".")).unwrap();

    assert!(header.contains("#include <stdint.h>"));
    assert!(header.contains("typedef struct fgx_slice_i16"));
    assert!(header.contains("typedef struct AudioBlock"));
    assert!(header.contains("void mix_samples(fgx_slice_i16 dst"));
    assert!(header.contains("H264NalHeader parse_h264_nal_header(uint8_t byte);"));

    assert!(source.contains("#include \"forgentex_demo_media.h\""));
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
    assert!(source.contains("void mix_samples"));

    let _ = fs::remove_dir_all(out_dir);
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
    assert!(err.contains("C backend cannot lower unsafe or assembly function 'popcount_x86'"));

    let _ = fs::remove_dir_all(root);
}
