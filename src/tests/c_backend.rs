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

#[test]
fn checked_multiplication_traps_exactly_when_the_product_overflows() {
    // The emitted guard previously compared `a` against `INT64_MAX / b`
    // without accounting for the sign of `b`. That trapped on ordinary
    // products — `2 * -3`, `-2 * -3`, `1000 * -1000` — and could divide by -1,
    // which is undefined for INT64_MIN. The helper is currently emitted but
    // unreferenced, so this is a latent defect; the test exists so it stays
    // correct before anything wires it up.
    //
    // Every sign pairing and integer boundary is compared against a 128-bit
    // reference product, so the guard is checked against the real answer
    // rather than against a restatement of its own arithmetic.
    let Some(toolchain) = crate::native_build::detect_toolchain() else {
        eprintln!("skipping: no host C toolchain");
        return;
    };

    let (header, _source) = generate_c_for_project(&manifest_path("examples/packages/codec_app"))
        .expect("C generation should succeed");
    assert!(
        header.contains("fgx_checked_i64_mul"),
        "the guard should be emitted into the header"
    );

    let root = std::env::temp_dir().join(format!("forgentex_mulguard_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    // Extract just the guard. The generated header also defines `fgx_trap`
    // itself, so the whole header cannot be included alongside a stub that
    // records whether a trap fired.
    let start = header
        .find("static inline int64_t fgx_checked_i64_mul")
        .expect("guard present");
    let end = header[start..]
        .find(
            "
}
",
        )
        .map(|offset| start + offset + 3)
        .expect("guard body terminates");
    fs::write(root.join("guard.h"), &header[start..end]).unwrap();

    // `__int128` is available on the gcc/clang toolchains this suite uses; a
    // compiler without it simply fails to build and the test reports that.
    let harness = root.join("mulguard.c");
    fs::write(
        &harness,
        r#"#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
static int trapped;
static void fgx_trap(const char *w) { (void)w; trapped = 1; }
#include "guard.h"
static void check(int64_t a, int64_t b) {
    trapped = 0;
    __int128 exact = (__int128)a * (__int128)b;
    int overflows = exact > (__int128)INT64_MAX || exact < (__int128)INT64_MIN;
    int64_t got = fgx_checked_i64_mul(a, b);
    if (trapped != overflows) {
        printf("MISMATCH a=%lld b=%lld trapped=%d expected=%d\n",
               (long long)a, (long long)b, trapped, overflows);
        exit(1);
    }
    if (!overflows && got != (int64_t)exact) {
        printf("WRONG a=%lld b=%lld got=%lld\n",
               (long long)a, (long long)b, (long long)got);
        exit(1);
    }
}
int main(void) {
    int64_t vals[] = {0,1,-1,2,-2,3,-3,1000,-1000,
                      INT64_MAX, INT64_MIN, INT64_MAX/2, INT64_MIN/2,
                      3037000499LL,-3037000499LL,3037000500LL,-3037000500LL};
    unsigned n = sizeof(vals)/sizeof(vals[0]);
    for (unsigned i=0;i<n;i++) for (unsigned j=0;j<n;j++) check(vals[i], vals[j]);
    return 0;
}
"#,
    )
    .unwrap();

    let executable = root.join(if cfg!(windows) {
        "mulguard.exe"
    } else {
        "mulguard"
    });
    let argv = vec![
        toolchain.name.clone(),
        "-std=c11".to_string(),
        "-O2".to_string(),
        harness.display().to_string(),
        "-I".to_string(),
        root.display().to_string(),
        "-o".to_string(),
        executable.display().to_string(),
    ];
    let compile =
        crate::native_build::run_with_path(&argv, std::path::Path::new(&toolchain.name).parent())
            .expect("failed to invoke the host C compiler");
    assert!(
        compile.status.success(),
        "guard harness failed to compile: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = std::process::Command::new(&executable)
        .output()
        .expect("failed to run the guard harness");
    assert!(
        run.status.success(),
        "checked multiplication disagrees with the 128-bit reference:\n{}",
        String::from_utf8_lossy(&run.stdout)
    );
    let _ = fs::remove_dir_all(root);
}
