// lang.3 — growable collections. A `Vec<T>` you can create empty, push onto,
// index, pop from, and ask the length of — the first real dynamic data
// structure in the language (backed by the interpreter's array value).

use crate::interp::run_entry_json;
use crate::parser::parse_human;

const VEC_PROG: &str = "fn build() -> i32 effects [alloc]:\n    let v: Vec[i32] = vec_new()\n    push(v, 10)\n    push(v, 20)\n    push(v, 30)\n    return v[0] + v[2]\n\nfn shrink() -> i32 effects [alloc]:\n    let v: Vec[i32] = vec_new()\n    push(v, 5)\n    push(v, 9)\n    pop(v)\n    push(v, 7)\n    return v[0] + v[1]\n\nfn size_plus() -> usize effects [alloc]:\n    let v: Vec[i32] = vec_new()\n    push(v, 1)\n    push(v, 1)\n    return v.len\n";

fn expect_return(entry: &str, expected: i64) {
    let program = parse_human(VEC_PROG).unwrap();
    let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
    let output = run_entry_json(&program, entry, &input).unwrap();
    assert!(
        output.contains("\"status\": \"pass\""),
        "{entry}:\n{output}"
    );
}

#[test]
fn vec_push_and_index() {
    // Three pushes then read elements: v[0] + v[2] = 10 + 30 = 40.
    expect_return("build", 40);
}

#[test]
fn vec_pop_mutates_in_place() {
    // push 5, 9; pop removes 9; push 7 -> [5, 7]; v[0] + v[1] = 12.
    expect_return("shrink", 12);
}

#[test]
fn vec_len_grows_with_pushes() {
    // Two pushes -> length 2.
    expect_return("size_plus", 2);
}

#[test]
fn vec_program_type_checks_clean() {
    // The whole program is well-typed: vec_new binds to Vec[i32], push is void,
    // indexing yields i32, and .len is usize.
    let diagnostics = crate::typecheck::check(&parse_human(VEC_PROG).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn push_to_a_non_vec_is_a_runtime_trap() {
    // push requires an array value; pushing onto a scalar traps.
    let src = "fn f() -> i32:\n    let x: i32 = 3\n    push(x, 1)\n    return x\n";
    let program = parse_human(src).unwrap();
    let err = run_entry_json(&program, "f", "{ \"args\": {} }").unwrap_err();
    assert!(
        err.contains("not a vec"),
        "expected a type trap, got: {err}"
    );
}

#[test]
fn c_and_zig_lower_bounded_owned_local_native_vec_layouts() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_native_vecs", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"nativevec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();

    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn vec_score() -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 10)\n    push(values, 20)\n    push(values, 30)\n    pop(values)\n    values[0] = 7\n    return values[0] + values[1] + i32(values.len)\n\nexport fn byte_score() -> i32 effects [alloc]:\n    let bytes: Vec[u8] = vec_new()\n    push(bytes, 250)\n    push(bytes, 5)\n    bytes[1] = 7\n    return i32(bytes[0]) + i32(bytes[1]) + i32(bytes.len)\n\nexport fn sample_score() -> i32 effects [alloc]:\n    let samples: Vec[i16] = vec_new()\n    push(samples, 1000)\n    push(samples, 24)\n    push(samples, 99)\n    pop(samples)\n    return i32(samples[0]) + i32(samples[1]) + i32(samples.len)\n\nexport fn pixel_score() -> u32 effects [alloc]:\n    let pixels: Vec[u32] = vec_new()\n    push(pixels, 1000)\n    push(pixels, 24)\n    pixels[1] = 25\n    return pixels[0] + pixels[1] + u32(pixels.len)\n\nexport fn gain_score() -> f32 effects [alloc]:\n    let gains: Vec[f32] = vec_new()\n    push(gains, f32(1.5))\n    push(gains, f32(2.25))\n    return gains[0] + gains[1] + f32(gains.len)\n\nexport fn early(flag: bool) -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 41)\n    if flag:\n        return values[0]\n    return 0\n",
    )
    .unwrap();

    let (_, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_source.contains("fgx_vec_i32_push(&values, 30);"));
    assert!(raw_c_source.contains("fgx_vec_u8_push(&bytes, 250);"));
    assert!(raw_c_source.contains("fgx_vec_i16_push(&samples, 1000);"));
    assert!(raw_c_source.contains("fgx_vec_u32_push(&pixels, 1000);"));
    assert!(raw_c_source.contains("fgx_vec_f32_push(&gains, ((float)(1.5)));"));
    assert!(raw_zig_source.contains("defer fgx_vec_i32_free(&values);"));
    assert!(raw_zig_source.contains("defer fgx_vec_u8_free(&bytes);"));
    assert!(raw_zig_source.contains("defer fgx_vec_i16_free(&samples);"));
    assert!(raw_zig_source.contains("defer fgx_vec_u32_free(&pixels);"));
    assert!(raw_zig_source.contains("defer fgx_vec_f32_free(&gains);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let header = std::fs::read_to_string(&c.header).unwrap();
    let c_source = std::fs::read_to_string(&c.source).unwrap();
    assert!(
        header.contains("FGX_VEC_I32_MAX_ELEMENTS ((size_t)1048576)"),
        "{header}"
    );
    assert!(header.contains("fgx_vec_i32_reserve_for_push"), "{header}");
    assert!(header.contains("fgx_vec_u8_reserve_for_push"), "{header}");
    assert!(header.contains("fgx_vec_i16_reserve_for_push"), "{header}");
    assert!(header.contains("fgx_vec_u32_reserve_for_push"), "{header}");
    assert!(header.contains("fgx_vec_f32_reserve_for_push"), "{header}");
    assert!(
        c_source.contains("fgx_vec_i32 values = fgx_vec_i32_new();"),
        "{c_source}"
    );
    assert!(
        c_source.contains("fgx_vec_i32_push(&values, 30);"),
        "{c_source}"
    );
    assert!(c_source.contains("fgx_vec_i32_pop(&values);"), "{c_source}");
    assert!(
        c_source.contains("fgx_vec_i32_free(&values);"),
        "{c_source}"
    );
    assert!(
        c_source.contains("fgx_vec_u8 bytes = fgx_vec_u8_new();"),
        "{c_source}"
    );
    assert!(
        c_source.contains("fgx_vec_i16 samples = fgx_vec_i16_new();"),
        "{c_source}"
    );
    assert!(
        c_source.contains("fgx_vec_u32 pixels = fgx_vec_u32_new();"),
        "{c_source}"
    );
    assert!(
        c_source.contains("fgx_vec_f32 gains = fgx_vec_f32_new();"),
        "{c_source}"
    );

    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let zig_source = std::fs::read_to_string(&zig.source).unwrap();
    assert!(
        zig_source.contains("FGX_VEC_I32_MAX_ELEMENTS: usize = 1024 * 1024"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("var values: FgxVecI32 = fgx_vec_i32_new();"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("values.data.?[fgx_bounds_index"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("defer fgx_vec_i32_free(&values);"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("var bytes: FgxVecU8 = fgx_vec_u8_new();"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("var samples: FgxVecI16 = fgx_vec_i16_new();"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("var pixels: FgxVecU32 = fgx_vec_u32_new();"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("var gains: FgxVecF32 = fgx_vec_f32_new();"),
        "{zig_source}"
    );

    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(opt_c.source)
        .unwrap()
        .contains("fgx_vec_i32_push(&values, 30);"));
    assert!(std::fs::read_to_string(opt_zig.source)
        .unwrap()
        .contains("defer fgx_vec_i32_free(&values);"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"nativevec.h\"\nint main(int argc, char **argv) { (void)argv; if (argc > 1) { fgx_vec_i32 values = fgx_vec_i32_new(); values.len = FGX_VEC_I32_MAX_ELEMENTS; fgx_vec_i32_push(&values, 1); return 2; } float gain = gain_score(); return vec_score() == 29 && byte_score() == 259 && sample_score() == 1026 && pixel_score() == 1027 && gain > 5.749f && gain < 5.751f && early(true) == 41 && early(false) == 0 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "vec_harness.exe"
        } else {
            "vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated Vec C harness");
        assert!(run.status.success(), "native Vec behavior diverged");
        let overflow = std::process::Command::new(&executable)
            .arg("overflow")
            .output()
            .expect("failed to execute generated Vec capacity harness");
        assert!(
            !overflow.status.success(),
            "Vec growth exceeded the generated allocation bound"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/nativevec.zig\");\nconst std = @import(\"std\");\ntest \"owned local vec layouts\" {\n    try std.testing.expectEqual(@as(i32, 29), lib.vec_score());\n    try std.testing.expectEqual(@as(i32, 259), lib.byte_score());\n    try std.testing.expectEqual(@as(i32, 1026), lib.sample_score());\n    try std.testing.expectEqual(@as(u32, 1027), lib.pixel_score());\n    try std.testing.expectApproxEqAbs(@as(f32, 5.75), lib.gain_score(), @as(f32, 0.001));\n    try std.testing.expectEqual(@as(i32, 41), lib.early(true));\n    try std.testing.expectEqual(@as(i32, 0), lib.early(false));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig Vec harness");
        assert!(
            run.status.success(),
            "generated Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_every_scalar_local_vec_layout() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_scalar_vecs",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"scalarvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn scalar_score() -> i64 effects [alloc]:\n    let tiny: Vec[i8] = vec_new()\n    push(tiny, 8)\n    let words: Vec[u16] = vec_new()\n    push(words, 16)\n    let wide: Vec[i64] = vec_new()\n    push(wide, 64)\n    let large: Vec[u64] = vec_new()\n    push(large, 128)\n    let offsets: Vec[isize] = vec_new()\n    push(offsets, 4)\n    let sizes: Vec[usize] = vec_new()\n    push(sizes, 5)\n    return i64(tiny[0]) + i64(words[0]) + wide[0] + i64(large[0]) + i64(offsets[0]) + i64(sizes[0])\n\nexport fn precise_score() -> f64 effects [alloc]:\n    let values: Vec[f64] = vec_new()\n    push(values, 2.5)\n    return values[0] + f64(values.len)\n\nexport fn flag_score() -> bool effects [alloc]:\n    let flags: Vec[bool] = vec_new()\n    push(flags, true)\n    push(flags, false)\n    flags[1] = true\n    return flags[0] == flags[1]\n",
    )
    .unwrap();

    let (_, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for suffix in ["bool", "i8", "i64", "isize", "u16", "u64", "usize", "f64"] {
        assert!(
            raw_c_source.contains(&format!("fgx_vec_{suffix}")),
            "missing C Vec use {suffix}"
        );
        assert!(
            raw_zig_source.contains(&format!("fgx_vec_{suffix}_reserve_for_push")),
            "missing Zig Vec layout {suffix}"
        );
    }
    assert!(raw_c_source.contains("fgx_vec_bool_push(&flags, true);"));
    assert!(raw_c_source.contains("fgx_vec_f64_free(&values);"));
    assert!(raw_zig_source.contains("defer fgx_vec_bool_free(&flags);"));
    assert!(raw_zig_source.contains("defer fgx_vec_f64_free(&values);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let c_header = std::fs::read_to_string(&c.header).unwrap();
    for suffix in ["bool", "i8", "i64", "isize", "u16", "u64", "usize", "f64"] {
        assert!(
            c_header.contains(&format!("fgx_vec_{suffix}_reserve_for_push")),
            "missing C Vec layout {suffix}"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("scalar_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"scalarvec.h\"\nint main(void) { double precise = precise_score(); return scalar_score() == 225 && precise > 3.499 && precise < 3.501 && flag_score() ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "scalar_vec_harness.exe"
        } else {
            "scalar_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated scalar Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated scalar Vec C harness");
        assert!(run.status.success(), "native scalar Vec behavior diverged");
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("scalar_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/scalarvec.zig\");\nconst std = @import(\"std\");\ntest \"all scalar local vec layouts\" {\n    try std.testing.expectEqual(@as(i64, 225), lib.scalar_score());\n    try std.testing.expectApproxEqAbs(@as(f64, 3.5), lib.precise_score(), @as(f64, 0.001));\n    try std.testing.expect(lib.flag_score());\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig scalar Vec harness");
        assert!(
            run.status.success(),
            "generated scalar Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_owned_local_enum_vec_layouts() {
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_native_enum_vecs", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"enumvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "enum Mode:\n    Idle\n    Active\n\nenum Signal:\n    Empty\n    Code(i32)\n\nexport fn mode_score() -> i32 effects [alloc]:\n    let modes: Vec[Mode] = vec_new()\n    push(modes, Mode::Idle)\n    push(modes, Mode::Active)\n    pop(modes)\n    match modes[0]:\n        case Mode::Idle:\n            return 7 + i32(modes.len)\n        case Mode::Active:\n            return 0\n\nexport fn signal_score(drop_last: bool) -> i32 effects [alloc]:\n    let signals: Vec[Signal] = vec_new()\n    push(signals, Signal::Empty)\n    push(signals, Signal::Code(40))\n    signals[0] = Signal::Code(1)\n    if drop_last:\n        pop(signals)\n    match signals[0]:\n        case Signal::Empty:\n            return 0\n        case Signal::Code(value):\n            return value + i32(signals.len)\n",
    )
    .unwrap();

    let (raw_c_header, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(
        raw_c_header.contains("typedef struct { Mode *data;"),
        "{raw_c_header}"
    );
    assert!(
        raw_c_header.contains("typedef struct { Signal *data;"),
        "{raw_c_header}"
    );
    assert!(raw_c_source.contains("fgx_vec_Mode_push(&modes, Mode_Idle);"));
    assert!(raw_c_source.contains("fgx_vec_Signal_push(&signals"));
    assert!(raw_c_source.contains("fgx_vec_Mode_free(&modes);"));
    assert!(raw_c_source.contains("fgx_vec_Signal_free(&signals);"));
    assert!(raw_zig_source.contains("const FgxVecMode = struct { data: ?[]Mode"));
    assert!(raw_zig_source.contains("const FgxVecSignal = struct { data: ?[]Signal"));
    assert!(raw_zig_source.contains("defer fgx_vec_Mode_free(&modes);"));
    assert!(raw_zig_source.contains("defer fgx_vec_Signal_free(&signals);"));

    let signal_start = raw_c_source.find("int32_t signal_score").unwrap();
    let signal_source = &raw_c_source[signal_start..];
    let saved_result = signal_source.find("int32_t _fgx_return_").unwrap();
    let cleanup = signal_source
        .find("fgx_vec_Signal_free(&signals);")
        .unwrap();
    let returned = signal_source.find("return _fgx_return_").unwrap();
    assert!(saved_result < cleanup && cleanup < returned);

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("enum_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"enumvec.h\"\nint main(void) { return mode_score() == 8 && signal_score(false) == 3 && signal_score(true) == 2 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "enum_vec_harness.exe"
        } else {
            "enum_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated enum Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated enum Vec C harness");
        assert!(run.status.success(), "native enum Vec behavior diverged");
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("enum_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/enumvec.zig\");\nconst std = @import(\"std\");\ntest \"owned local enum vec layouts\" {\n    try std.testing.expectEqual(@as(i32, 8), lib.mode_score());\n    try std.testing.expectEqual(@as(i32, 3), lib.signal_score(false));\n    try std.testing.expectEqual(@as(i32, 2), lib.signal_score(true));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig enum Vec harness");
        assert!(
            run.status.success(),
            "generated enum Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_owned_local_struct_vec_layouts() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_struct_vecs",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"structvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "struct Point:\n    x: i32\n    y: i32\n\nstruct Sample:\n    origin: Point\n    weight: i32\n\nexport struct Marker:\n    id: u32\n    enabled: bool\n\nexport fn point_score(drop_last: bool) -> i32 effects [alloc]:\n    let points: Vec[Point] = vec_new()\n    push(points, Point(2, 3))\n    push(points, Point(5, 7))\n    points[0] = Point(10, 20)\n    if drop_last:\n        pop(points)\n    return points[0].x + points[0].y + i32(points.len)\n\nexport fn sample_score() -> i32 effects [alloc]:\n    let samples: Vec[Sample] = vec_new()\n    push(samples, Sample(Point(1, 2), 4))\n    return samples[0].origin.x + samples[0].origin.y + samples[0].weight + i32(samples.len)\n\nexport fn marker_score() -> i32 effects [alloc]:\n    let markers: Vec[Marker] = vec_new()\n    push(markers, Marker(9, true))\n    if markers[0].enabled:\n        return i32(markers[0].id) + i32(markers.len)\n    return 0\n",
    )
    .unwrap();

    let (raw_c_header, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_header.contains("typedef struct Marker"));
    assert!(raw_c_header.contains("typedef struct { Marker *data;"));
    assert!(!raw_c_header.contains("typedef struct { Point *data;"));
    assert!(raw_c_source.contains("typedef struct Point"));
    assert!(raw_c_source.contains("typedef struct Sample"));
    assert!(raw_c_source.contains("typedef struct { Point *data;"));
    assert!(raw_c_source.contains("typedef struct { Sample *data;"));
    assert!(raw_c_source.contains("fgx_vec_Point_push(&points"));
    assert!(raw_c_source.contains("fgx_vec_Sample_push(&samples"));
    assert!(raw_c_source.contains("fgx_vec_Marker_push(&markers"));
    assert!(raw_zig_source.contains("const FgxVecPoint = struct { data: ?[]Point"));
    assert!(raw_zig_source.contains("const FgxVecSample = struct { data: ?[]Sample"));
    assert!(raw_zig_source.contains("const FgxVecMarker = struct { data: ?[]Marker"));

    let point_start = raw_c_source.find("int32_t point_score").unwrap();
    let point_source = &raw_c_source[point_start..];
    let saved_result = point_source.find("int32_t _fgx_return_").unwrap();
    let cleanup = point_source.find("fgx_vec_Point_free(&points);").unwrap();
    let returned = point_source.find("return _fgx_return_").unwrap();
    assert!(saved_result < cleanup && cleanup < returned);
    assert!(raw_zig_source.contains("defer fgx_vec_Point_free(&points);"));
    assert!(raw_zig_source.contains("defer fgx_vec_Sample_free(&samples);"));
    assert!(raw_zig_source.contains("defer fgx_vec_Marker_free(&markers);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("struct_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"structvec.h\"\nint main(void) { return point_score(false) == 32 && point_score(true) == 31 && sample_score() == 8 && marker_score() == 10 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "struct_vec_harness.exe"
        } else {
            "struct_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated struct Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated struct Vec C harness");
        assert!(run.status.success(), "native struct Vec behavior diverged");
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("struct_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/structvec.zig\");\nconst std = @import(\"std\");\ntest \"owned local struct vec layouts\" {\n    try std.testing.expectEqual(@as(i32, 32), lib.point_score(false));\n    try std.testing.expectEqual(@as(i32, 31), lib.point_score(true));\n    try std.testing.expectEqual(@as(i32, 8), lib.sample_score());\n    try std.testing.expectEqual(@as(i32, 10), lib.marker_score());\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig struct Vec harness");
        assert!(
            run.status.success(),
            "generated struct Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_owned_local_borrowed_string_vec_layout() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_borrowed_string_vec",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"stringvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn borrowed_string_score(input: str, drop_last: bool) -> i32 effects [alloc]:\n    let words: Vec[str] = vec_new()\n    push(words, \"lambda\")\n    push(words, input)\n    words[0] = \"beta\"\n    if drop_last:\n        pop(words)\n    if words[0] == \"beta\":\n        if drop_last:\n            return i32(words.len) + i32(str_len(words[0]))\n        if str_contains(words[1], \"é\"):\n            return i32(words.len) + i32(str_len(words[1]))\n    return 0\n",
    )
    .unwrap();

    let (raw_c_header, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_header.contains("typedef struct { const char * *data;"));
    assert!(raw_c_header.contains("fgx_vec_str_push"));
    assert!(raw_c_source.contains("fgx_vec_str words = fgx_vec_str_new();"));
    assert!(raw_c_source.contains("fgx_vec_str_push(&words, input);"));
    assert!(raw_c_source.contains("fgx_str_eq(words.data[fgx_bounds_index"));
    assert!(raw_c_source.contains("fgx_vec_str_free(&words);"));
    assert!(raw_zig_source.contains("const FgxVecStr = struct { data: ?[][]const u8"));
    assert!(raw_zig_source.contains("fgx_vec_str_push(&words, input);"));
    assert!(raw_zig_source.contains("defer fgx_vec_str_free(&words);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("string_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"stringvec.h\"\nint main(void) { return borrowed_string_score(\"café\", false) == 7 && borrowed_string_score(\"café\", true) == 5 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "string_vec_harness.exe"
        } else {
            "string_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated borrowed-string Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated borrowed-string Vec C harness");
        assert!(
            run.status.success(),
            "native borrowed-string Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("string_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/stringvec.zig\");\nconst std = @import(\"std\");\ntest \"owned local borrowed-string vec layout\" {\n    try std.testing.expectEqual(@as(i32, 7), lib.borrowed_string_score(\"café\", false));\n    try std.testing.expectEqual(@as(i32, 5), lib.borrowed_string_score(\"café\", true));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig borrowed-string Vec harness");
        assert!(
            run.status.success(),
            "generated borrowed-string Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_owned_local_immutable_slice_vec_layout() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_immutable_slice_vec",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"slicevec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn slice_vec_score(first: &[i32], second: &[i32], drop_last: bool) -> i32 effects [alloc]:\n    let views: Vec[&[i32]] = vec_new()\n    push(views, first)\n    push(views, second)\n    views[0] = second\n    if drop_last:\n        pop(views)\n    return views[0][1] + i32(views[0].len) + i32(views.len)\n",
    )
    .unwrap();

    let (raw_c_header, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_header.contains("typedef struct fgx_const_slice_i32"));
    assert!(raw_c_header.contains("typedef struct { fgx_const_slice_i32 *data;"));
    assert!(raw_c_header.contains("fgx_vec_slice_i32_push"));
    assert!(raw_c_source.contains("fgx_vec_slice_i32 views = fgx_vec_slice_i32_new();"));
    assert!(raw_c_source.contains("fgx_vec_slice_i32_push(&views, first);"));
    assert!(raw_c_source.contains("views.data[fgx_bounds_index"));
    assert!(raw_c_source.contains("fgx_vec_slice_i32_free(&views);"));
    assert!(raw_zig_source.contains("const FgxVecSliceI32 = struct { data: ?[][]const i32"));
    assert!(raw_zig_source.contains("fgx_vec_slice_i32_push(&views, first);"));
    assert!(raw_zig_source.contains("defer fgx_vec_slice_i32_free(&views);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("slice_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"slicevec.h\"\nint main(void) { int32_t first_data[] = {1, 2}; int32_t second_data[] = {5, 7, 9}; fgx_const_slice_i32 first = { first_data, 2 }; fgx_const_slice_i32 second = { second_data, 3 }; return slice_vec_score(first, second, false) == 12 && slice_vec_score(first, second, true) == 11 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "slice_vec_harness.exe"
        } else {
            "slice_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated immutable-slice Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated immutable-slice Vec C harness");
        assert!(
            run.status.success(),
            "native immutable-slice Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("slice_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/slicevec.zig\");\nconst std = @import(\"std\");\ntest \"owned local immutable-slice vec layout\" {\n    const first = [_]i32{ 1, 2 };\n    const second = [_]i32{ 5, 7, 9 };\n    try std.testing.expectEqual(@as(i32, 12), lib.slice_vec_score(&first, &second, false));\n    try std.testing.expectEqual(@as(i32, 11), lib.slice_vec_score(&first, &second, true));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig immutable-slice Vec harness");
        assert!(
            run.status.success(),
            "generated immutable-slice Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_owned_local_fixed_scalar_array_vec_layouts() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_fixed_array_vec",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"arrayvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn array_vec_score(first: [i32; 2], second: [i32; 2], drop_last: bool) -> i32 effects [alloc]:\n    let rows: Vec[[i32; 2]] = vec_new()\n    push(rows, first)\n    push(rows, second)\n    rows[0] = second\n    if drop_last:\n        pop(rows)\n        return rows[0][0] + rows[0][1] + i32(rows.len)\n    return rows[0][1] + rows[1][0] + i32(rows.len)\n\nexport fn nested_array_score(first: [[i32; 2]; 2], second: [[i32; 2]; 2], drop_last: bool) -> i32 effects [alloc]:\n    let grids: Vec[[[i32; 2]; 2]] = vec_new()\n    push(grids, first)\n    push(grids, second)\n    grids[0] = second\n    if drop_last:\n        pop(grids)\n        return grids[0][1][0] + i32(grids.len)\n    return grids[0][0][1] + grids[1][1][0] + i32(grids.len)\n\nexport fn bool_array_score(value: [bool; 1]) -> bool effects [alloc]:\n    let flags: Vec[[bool; 1]] = vec_new()\n    push(flags, value)\n    return flags[0][0]\n\nexport fn float_array_score(value: [f64; 2]) -> f64 effects [alloc]:\n    let samples: Vec[[f64; 2]] = vec_new()\n    push(samples, value)\n    return samples[0][0] + samples[0][1] + f64(samples.len)\n",
    )
    .unwrap();

    let (raw_c_header, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_header
        .contains("typedef struct fgx_array_i32_2 { int32_t data[2]; } fgx_array_i32_2;"));
    assert!(raw_c_header.contains("typedef struct { fgx_array_i32_2 *data;"));
    assert!(raw_c_header.contains("fgx_vec_array_i32_2_push"));
    let inner_array = raw_c_header.find("typedef struct fgx_array_i32_2").unwrap();
    let outer_array = raw_c_header
        .find("typedef struct fgx_array_array_i32_2_2")
        .unwrap();
    let outer_vec = raw_c_header
        .find("fgx_vec_array_array_i32_2_2_push")
        .unwrap();
    assert!(inner_array < outer_array && outer_array < outer_vec);
    assert!(raw_c_source.contains("fgx_vec_array_i32_2 rows = fgx_vec_array_i32_2_new();"));
    assert!(raw_c_source.contains("fgx_vec_array_i32_2_push(&rows, first);"));
    assert!(raw_c_source.contains("fgx_vec_array_i32_2_free(&rows);"));
    assert!(raw_zig_source.contains("const FgxVecArrayI32_2 = struct { data: ?[][2]i32"));
    assert!(raw_zig_source.contains("fgx_vec_array_i32_2_push(&rows, first);"));
    assert!(raw_zig_source.contains("defer fgx_vec_array_i32_2_free(&rows);"));
    assert!(raw_zig_source.contains("const FgxVecArrayBool_1"));
    assert!(raw_zig_source.contains("const FgxVecArrayF64_2"));
    assert!(raw_zig_source.contains("const FgxVecArrayArrayI32_2_2 = struct { data: ?[][2][2]i32"));
    assert!(raw_zig_source.contains("fgx_vec_array_array_i32_2_2_push(&grids, first);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(&opt_c.header)
        .unwrap()
        .contains("fgx_vec_array_i32_2_push"));
    assert!(std::fs::read_to_string(&opt_zig.source)
        .unwrap()
        .contains("fgx_vec_array_i32_2_push"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("array_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"arrayvec.h\"\nint main(void) { fgx_array_i32_2 first = { .data = {1, 2} }; fgx_array_i32_2 second = { .data = {5, 7} }; fgx_array_array_i32_2_2 first_grid = { .data = { { .data = {1, 2} }, { .data = {3, 4} } } }; fgx_array_array_i32_2_2 second_grid = { .data = { { .data = {5, 7} }, { .data = {9, 11} } } }; fgx_array_bool_1 flag = { .data = {true} }; fgx_array_f64_2 samples = { .data = {1.5, 2.25} }; return array_vec_score(first, second, false) == 14 && array_vec_score(first, second, true) == 13 && nested_array_score(first_grid, second_grid, false) == 18 && nested_array_score(first_grid, second_grid, true) == 10 && bool_array_score(flag) && float_array_score(samples) == 4.75 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "array_vec_harness.exe"
        } else {
            "array_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated fixed-array Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated fixed-array Vec C harness");
        assert!(
            run.status.success(),
            "native fixed-array Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("array_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/arrayvec.zig\");\nconst std = @import(\"std\");\ntest \"owned local fixed-array vec layouts\" {\n    try std.testing.expectEqual(@as(i32, 14), lib.array_vec_score(.{ 1, 2 }, .{ 5, 7 }, false));\n    try std.testing.expectEqual(@as(i32, 13), lib.array_vec_score(.{ 1, 2 }, .{ 5, 7 }, true));\n    try std.testing.expectEqual(@as(i32, 18), lib.nested_array_score(.{ .{ 1, 2 }, .{ 3, 4 } }, .{ .{ 5, 7 }, .{ 9, 11 } }, false));\n    try std.testing.expectEqual(@as(i32, 10), lib.nested_array_score(.{ .{ 1, 2 }, .{ 3, 4 } }, .{ .{ 5, 7 }, .{ 9, 11 } }, true));\n    try std.testing.expect(lib.bool_array_score(.{true}));\n    try std.testing.expectEqual(@as(f64, 4.75), lib.float_array_score(.{ 1.5, 2.25 }));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig fixed-array Vec harness");
        assert!(
            run.status.success(),
            "generated fixed-array Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_fixed_array_vecs_with_immutable_borrowed_leaves() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_borrowed_array_vec",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"borrowedarrayvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn string_group_score(first: [str; 2], second: [str; 2], drop_last: bool) -> i32 effects [alloc]:\n    let groups: Vec[[str; 2]] = vec_new()\n    push(groups, first)\n    push(groups, second)\n    groups[0] = second\n    if drop_last:\n        pop(groups)\n    if str_contains(groups[0][1], \"é\"):\n        return i32(str_len(groups[0][1])) + i32(groups.len)\n    return 0\n\nexport fn slice_group_score(first: [&[i32]; 2], second: [&[i32]; 2], drop_last: bool) -> i32 effects [alloc]:\n    let groups: Vec[[&[i32]; 2]] = vec_new()\n    push(groups, first)\n    push(groups, second)\n    groups[0] = second\n    if drop_last:\n        pop(groups)\n    return groups[0][1][0] + i32(groups[0][1].len) + i32(groups.len)\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_header.contains("typedef struct fgx_array_str_2"));
    let slice = c_header.find("typedef struct fgx_const_slice_i32").unwrap();
    let array = c_header
        .find("typedef struct fgx_array_slice_const_i32_2")
        .unwrap();
    let vector = c_header
        .find("fgx_vec_array_slice_const_i32_2_push")
        .unwrap();
    assert!(slice < array && array < vector);
    assert!(c_source.contains("fgx_vec_array_str_2 groups = fgx_vec_array_str_2_new();"));
    assert!(c_source.contains("fgx_vec_array_slice_const_i32_2_push(&groups, first);"));
    assert!(c_source.contains("fgx_vec_array_slice_const_i32_2_free(&groups);"));
    assert!(zig_source.contains("const FgxVecArrayStr_2 = struct { data: ?[][2][]const u8"));
    assert!(zig_source.contains("const FgxVecArraySliceI32_2 = struct { data: ?[][2][]const i32"));
    assert!(zig_source.contains("fgx_vec_array_slice_i32_2_push(&groups, first);"));
    assert!(zig_source.contains("defer fgx_vec_array_slice_i32_2_free(&groups);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(&opt_c.header)
        .unwrap()
        .contains("fgx_vec_array_slice_const_i32_2_push"));
    assert!(std::fs::read_to_string(&opt_zig.source)
        .unwrap()
        .contains("fgx_vec_array_slice_i32_2_push"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("borrowed_array_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"borrowedarrayvec.h\"\nint main(void) { fgx_array_str_2 first_words = { .data = {\"a\", \"b\"} }; fgx_array_str_2 second_words = { .data = {\"beta\", \"café\"} }; int32_t a_data[] = {1, 2}; int32_t b_data[] = {3, 4}; int32_t c_data[] = {8}; int32_t d_data[] = {5, 7, 9}; fgx_const_slice_i32 a = { a_data, 2 }; fgx_const_slice_i32 b = { b_data, 2 }; fgx_const_slice_i32 c = { c_data, 1 }; fgx_const_slice_i32 d = { d_data, 3 }; fgx_array_slice_const_i32_2 first_slices = { .data = {a, b} }; fgx_array_slice_const_i32_2 second_slices = { .data = {c, d} }; return string_group_score(first_words, second_words, false) == 7 && string_group_score(first_words, second_words, true) == 6 && slice_group_score(first_slices, second_slices, false) == 10 && slice_group_score(first_slices, second_slices, true) == 9 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "borrowed_array_vec_harness.exe"
        } else {
            "borrowed_array_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated borrowed-array Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated borrowed-array Vec C harness");
        assert!(
            run.status.success(),
            "native borrowed-array Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("borrowed_array_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/borrowedarrayvec.zig\");\nconst std = @import(\"std\");\ntest \"fixed array vec borrowed leaves\" {\n    try std.testing.expectEqual(@as(i32, 7), lib.string_group_score(.{ \"a\", \"b\" }, .{ \"beta\", \"café\" }, false));\n    try std.testing.expectEqual(@as(i32, 6), lib.string_group_score(.{ \"a\", \"b\" }, .{ \"beta\", \"café\" }, true));\n    try std.testing.expectEqual(@as(i32, 10), lib.slice_group_score(.{ &[_]i32{ 1, 2 }, &[_]i32{ 3, 4 } }, .{ &[_]i32{8}, &[_]i32{ 5, 7, 9 } }, false));\n    try std.testing.expectEqual(@as(i32, 9), lib.slice_group_score(.{ &[_]i32{ 1, 2 }, &[_]i32{ 3, 4 } }, .{ &[_]i32{8}, &[_]i32{ 5, 7, 9 } }, true));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig borrowed-array Vec harness");
        assert!(
            run.status.success(),
            "generated borrowed-array Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_fixed_array_vecs_with_named_value_leaves() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_named_array_vec",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"namedarrayvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export enum Mode:\n    Idle\n    Active\n\nstruct Point:\n    x: i32\n    y: i32\n\nstruct PointBatch:\n    points: [Point; 2]\n\nexport struct Marker:\n    id: u32\n    enabled: bool\n\nexport struct MarkerBatch:\n    markers: [Marker; 2]\n\nexport fn mode_array_score(first: [Mode; 2], second: [Mode; 2], drop_last: bool) -> i32 effects [alloc]:\n    let groups: Vec[[Mode; 2]] = vec_new()\n    push(groups, first)\n    push(groups, second)\n    groups[0] = second\n    if drop_last:\n        pop(groups)\n    match groups[0][1]:\n        case Mode::Idle:\n            return i32(groups.len)\n        case Mode::Active:\n            return 10 + i32(groups.len)\n\nexport fn marker_array_score(first: [Marker; 2], second: [Marker; 2], drop_last: bool) -> i32 effects [alloc]:\n    let groups: Vec[[Marker; 2]] = vec_new()\n    push(groups, first)\n    push(groups, second)\n    groups[0] = second\n    if drop_last:\n        pop(groups)\n    if groups[0][1].enabled:\n        return i32(groups[0][1].id) + i32(groups.len)\n    return 0\n\nfn point_array_score(first: [[Point; 2]; 2], second: [[Point; 2]; 2]) -> i32 effects [alloc]:\n    let groups: Vec[[[Point; 2]; 2]] = vec_new()\n    push(groups, first)\n    push(groups, second)\n    return groups[0][0][0].x + groups[1][1][1].y + i32(groups.len)\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    let mode_decl = c_header.find("typedef enum").unwrap();
    let mode_array = c_header.find("typedef struct fgx_array_Mode_2").unwrap();
    let mode_vec = c_header.find("fgx_vec_array_Mode_2_push").unwrap();
    assert!(mode_decl < mode_array && mode_array < mode_vec);
    let marker_decl = c_header.find("typedef struct Marker").unwrap();
    let marker_array = c_header.find("typedef struct fgx_array_Marker_2").unwrap();
    let marker_vec = c_header.find("fgx_vec_array_Marker_2_push").unwrap();
    assert!(marker_decl < marker_array && marker_array < marker_vec);
    let marker_batch = c_header.find("typedef struct MarkerBatch").unwrap();
    assert!(marker_array < marker_batch);
    assert!(!c_header.contains("fgx_array_Point_2"));
    let point_decl = c_source.find("typedef struct Point").unwrap();
    let point_array = c_source.find("typedef struct fgx_array_Point_2").unwrap();
    let point_batch = c_source.find("typedef struct PointBatch").unwrap();
    let nested_point_array = c_source
        .find("typedef struct fgx_array_array_Point_2_2")
        .unwrap();
    let point_vec = c_source.find("fgx_vec_array_array_Point_2_2_push").unwrap();
    assert!(point_decl < point_array && point_array < point_batch);
    assert!(point_array < nested_point_array && nested_point_array < point_vec);
    assert!(c_source.contains("fgx_vec_array_Mode_2_push(&groups, first);"));
    assert!(c_source.contains("fgx_vec_array_Marker_2_free(&groups);"));
    assert!(zig_source.contains("const FgxVecArrayMode_2 = struct { data: ?[][2]Mode"));
    assert!(zig_source.contains("const FgxVecArrayMarker_2 = struct { data: ?[][2]Marker"));
    assert!(zig_source.contains("const FgxVecArrayArrayPoint_2_2 = struct { data: ?[][2][2]Point"));
    assert!(zig_source.contains("defer fgx_vec_array_array_Point_2_2_free(&groups);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(&opt_c.header)
        .unwrap()
        .contains("fgx_vec_array_Marker_2_push"));
    assert!(std::fs::read_to_string(&opt_zig.source)
        .unwrap()
        .contains("fgx_vec_array_Marker_2_push"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("named_array_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"namedarrayvec.h\"\nint main(void) { fgx_array_Mode_2 first_modes = { .data = {Mode_Idle, Mode_Idle} }; fgx_array_Mode_2 second_modes = { .data = {Mode_Idle, Mode_Active} }; fgx_array_Marker_2 first_markers = { .data = {{1, false}, {2, false}} }; fgx_array_Marker_2 second_markers = { .data = {{5, false}, {9, true}} }; return mode_array_score(first_modes, second_modes, false) == 12 && mode_array_score(first_modes, second_modes, true) == 11 && marker_array_score(first_markers, second_markers, false) == 11 && marker_array_score(first_markers, second_markers, true) == 10 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "named_array_vec_harness.exe"
        } else {
            "named_array_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated named-array Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated named-array Vec C harness");
        assert!(
            run.status.success(),
            "native named-array Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("named_array_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/namedarrayvec.zig\");\nconst std = @import(\"std\");\ntest \"fixed array vec named value leaves\" {\n    try std.testing.expectEqual(@as(i32, 12), lib.mode_array_score(.{ .Idle, .Idle }, .{ .Idle, .Active }, false));\n    try std.testing.expectEqual(@as(i32, 11), lib.mode_array_score(.{ .Idle, .Idle }, .{ .Idle, .Active }, true));\n    try std.testing.expectEqual(@as(i32, 11), lib.marker_array_score(.{ .{ .id = 1, .enabled = false }, .{ .id = 2, .enabled = false } }, .{ .{ .id = 5, .enabled = false }, .{ .id = 9, .enabled = true } }, false));\n    try std.testing.expectEqual(@as(i32, 10), lib.marker_array_score(.{ .{ .id = 1, .enabled = false }, .{ .id = 2, .enabled = false } }, .{ .{ .id = 5, .enabled = false }, .{ .id = 9, .enabled = true } }, true));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig named-array Vec harness");
        assert!(
            run.status.success(),
            "generated named-array Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fixed_scalar_array_native_vec_rejects_invalid_recursive_shapes() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_fixed_array_vec_limits",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"arrayveclimits\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();

    let mut too_deep = "i32".to_string();
    for _ in 0..17 {
        too_deep = format!("[{too_deep}; 1]");
    }
    for (case, ty) in [
        ("zero", "[i32; 0]".to_string()),
        ("wide", "[i32; 1025]".to_string()),
        ("leaves", "[[i32; 33]; 33]".to_string()),
        ("depth", too_deep),
        ("mutable", "[&mut [i32]; 2]".to_string()),
        ("owned", "[Option[i32]; 2]".to_string()),
    ] {
        std::fs::write(
            dir.join("src/vectors.fgx"),
            format!("fn blocked() -> usize effects [alloc]:\n    let values: Vec[{ty}] = vec_new()\n    return values.len\n"),
        )
        .unwrap();
        let c_out = dir.join(format!("out-c-{case}"));
        let zig_out = dir.join(format!("out-zig-{case}"));
        let c_err = crate::c_backend::build_c_project(&dir, &c_out).unwrap_err();
        let zig_err = crate::zig_backend::build_zig_project(&dir, &zig_out).unwrap_err();
        for error in [&c_err, &zig_err] {
            assert!(error.contains("generic_type:Vec"), "{error}");
            assert!(error.contains("effect:alloc"), "{error}");
        }
        assert!(!c_out.exists());
        assert!(!zig_out.exists());
    }

    let mut max_depth = "i32".to_string();
    for _ in 0..16 {
        max_depth = format!("[{max_depth}; 1]");
    }
    std::fs::write(
        dir.join("src/vectors.fgx"),
        format!("fn max_depth() -> usize effects [alloc]:\n    let values: Vec[{max_depth}] = vec_new()\n    return values.len\n\nfn max_leaves() -> usize effects [alloc]:\n    let values: Vec[[[i32; 32]; 32]] = vec_new()\n    return values.len\n"),
    )
    .unwrap();
    let (c_header, _) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_header.contains("fgx_vec_array_array_i32_32_32_new"));
    assert!(zig_source.contains("fgx_vec_array_array_i32_32_32_new"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_branch_and_match_arm_native_vecs_with_scope_cleanup() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_branch_vecs",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"branchvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn branch_score(flag: bool) -> i32 effects [alloc]:\n    let result: i32 = 0\n    if flag:\n        let values: Vec[i32] = vec_new()\n        push(values, 41)\n        result = values[0] + i32(values.len)\n    else:\n        let samples: Vec[u16] = vec_new()\n        push(samples, 9)\n        result = i32(samples[0])\n    return result\n\nexport fn nested_score(outer: bool, inner: bool) -> i32 effects [alloc]:\n    let result: i32 = 0\n    if outer:\n        let values: Vec[i32] = vec_new()\n        push(values, 5)\n        if inner:\n            let bytes: Vec[u8] = vec_new()\n            push(bytes, 7)\n            result = values[0] + i32(bytes[0])\n        else:\n            result = values[0]\n    return result\n\nexport fn branch_early(flag: bool) -> i32 effects [alloc]:\n    if flag:\n        let values: Vec[i32] = vec_new()\n        push(values, 77)\n        return values[0]\n    return 0\n",
    )
    .unwrap();

    let (_, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_source.contains("fgx_vec_i32_free(&values);"));
    assert!(raw_c_source.contains("fgx_vec_u16_free(&samples);"));
    assert!(raw_c_source.contains("fgx_vec_u8_free(&bytes);"));
    assert!(raw_zig_source.contains("defer fgx_vec_i32_free(&values);"));
    assert!(raw_zig_source.contains("defer fgx_vec_u16_free(&samples);"));
    assert!(raw_zig_source.contains("defer fgx_vec_u8_free(&bytes);"));

    let early_start = raw_c_source.find("int32_t branch_early").unwrap();
    let early_source = &raw_c_source[early_start..];
    let saved_result = early_source.find("int32_t _fgx_return_0").unwrap();
    let early_free = early_source.find("fgx_vec_i32_free(&values);").unwrap();
    let early_return = early_source.find("return _fgx_return_0;").unwrap();
    assert!(saved_result < early_free && early_free < early_return);

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("branch_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"branchvec.h\"\nint main(void) { return branch_score(true) == 42 && branch_score(false) == 9 && nested_score(true, true) == 12 && nested_score(true, false) == 5 && nested_score(false, true) == 0 && branch_early(true) == 77 && branch_early(false) == 0 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "branch_vec_harness.exe"
        } else {
            "branch_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated branch-local Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated branch-local Vec C harness");
        assert!(
            run.status.success(),
            "native branch-local Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("branch_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/branchvec.zig\");\nconst std = @import(\"std\");\ntest \"branch-local vec cleanup\" {\n    try std.testing.expectEqual(@as(i32, 42), lib.branch_score(true));\n    try std.testing.expectEqual(@as(i32, 9), lib.branch_score(false));\n    try std.testing.expectEqual(@as(i32, 12), lib.nested_score(true, true));\n    try std.testing.expectEqual(@as(i32, 5), lib.nested_score(true, false));\n    try std.testing.expectEqual(@as(i32, 0), lib.nested_score(false, true));\n    try std.testing.expectEqual(@as(i32, 77), lib.branch_early(true));\n    try std.testing.expectEqual(@as(i32, 0), lib.branch_early(false));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig branch-local Vec harness");
        assert!(
            run.status.success(),
            "generated branch-local Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    std::fs::write(
        dir.join("src/vectors.fgx"),
        "enum Choice:\n    One\n    Two\n\nenum PayloadChoice:\n    Value(i32)\n    Empty\n\nexport fn match_score(first: bool) -> i32 effects [alloc]:\n    let choice: Choice = Choice::Two\n    if first:\n        choice = Choice::One\n    let result: i32 = 0\n    match choice:\n        case Choice::One:\n            let values: Vec[i32] = vec_new()\n            push(values, 40)\n            result = values[0] + i32(values.len)\n        case Choice::Two:\n            let samples: Vec[u16] = vec_new()\n            push(samples, 9)\n            result = i32(samples[0])\n    return result\n\nexport fn match_early(first: bool) -> i32 effects [alloc]:\n    let choice: Choice = Choice::Two\n    if first:\n        choice = Choice::One\n    match choice:\n        case Choice::One:\n            let values: Vec[i32] = vec_new()\n            push(values, 77)\n            return values[0]\n        case Choice::Two:\n            return 0\n\nexport fn tagged_match_score(has_value: bool) -> i32 effects [alloc]:\n    let choice: PayloadChoice = PayloadChoice::Empty\n    if has_value:\n        choice = PayloadChoice::Value(5)\n    let result: i32 = -1\n    match choice:\n        case PayloadChoice::Value(value):\n            let tagged_values: Vec[i32] = vec_new()\n            push(tagged_values, value)\n            result = tagged_values[0] + i32(tagged_values.len)\n        case PayloadChoice::Empty:\n            result = -1\n    return result\n",
    )
    .unwrap();

    let (_, match_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let match_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(match_c_source.contains("fgx_vec_i32_free(&values);"));
    assert!(match_c_source.contains("fgx_vec_u16_free(&samples);"));
    assert!(match_c_source.contains("fgx_vec_i32_free(&tagged_values);"));
    assert!(match_zig_source.contains("defer fgx_vec_i32_free(&values);"));
    assert!(match_zig_source.contains("defer fgx_vec_u16_free(&samples);"));
    assert!(match_zig_source.contains("defer fgx_vec_i32_free(&tagged_values);"));

    let score_start = match_c_source.find("int32_t match_score").unwrap();
    let score_end = match_c_source.find("int32_t match_early").unwrap();
    let score_source = &match_c_source[score_start..score_end];
    let score_return = score_source.find("return result;").unwrap();
    assert!(score_source.find("fgx_vec_i32_free(&values);").unwrap() < score_return);
    assert!(score_source.find("fgx_vec_u16_free(&samples);").unwrap() < score_return);

    let tagged_start = match_c_source.find("int32_t tagged_match_score").unwrap();
    let tagged_source = &match_c_source[tagged_start..];
    assert!(
        tagged_source
            .find("fgx_vec_i32_free(&tagged_values);")
            .unwrap()
            < tagged_source.find("return result;").unwrap()
    );

    let early_start = match_c_source.find("int32_t match_early").unwrap();
    let early_source = &match_c_source[early_start..];
    let saved_result = early_source.find("int32_t _fgx_return_0").unwrap();
    let early_free = early_source.find("fgx_vec_i32_free(&values);").unwrap();
    let early_return = early_source.find("return _fgx_return_0;").unwrap();
    assert!(saved_result < early_free && early_free < early_return);

    let match_c = crate::c_backend::build_c_project(&dir, &dir.join("out-c-match")).unwrap();
    let match_zig =
        crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig-match")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("match_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"branchvec.h\"\nint main(void) { return match_score(true) == 41 && match_score(false) == 9 && match_early(true) == 77 && match_early(false) == 0 && tagged_match_score(true) == 6 && tagged_match_score(false) == -1 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "match_vec_harness.exe"
        } else {
            "match_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            match_c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c-match").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated match-arm-local Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated match-arm-local Vec C harness");
        assert!(
            run.status.success(),
            "native match-arm-local Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("match_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig-match/branchvec.zig\");\nconst std = @import(\"std\");\ntest \"match-arm-local vec cleanup\" {\n    try std.testing.expectEqual(@as(i32, 41), lib.match_score(true));\n    try std.testing.expectEqual(@as(i32, 9), lib.match_score(false));\n    try std.testing.expectEqual(@as(i32, 77), lib.match_early(true));\n    try std.testing.expectEqual(@as(i32, 0), lib.match_early(false));\n    try std.testing.expectEqual(@as(i32, 6), lib.tagged_match_score(true));\n    try std.testing.expectEqual(@as(i32, -1), lib.tagged_match_score(false));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig match-arm Vec harness");
        assert!(
            run.status.success(),
            "generated match-arm-local Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    assert!(match_c.header.exists());
    assert!(match_zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_loop_local_native_vecs_with_control_cleanup() {
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_native_loop_vecs", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"loopvec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn continue_score() -> i32 effects [alloc]:\n    let total: i32 = 0\n    for i in 0 .. 4:\n        let values: Vec[i32] = vec_new()\n        push(values, i32(i))\n        if i == 1:\n            continue\n        total = total + values[0]\n    return total\n\nexport fn break_score() -> i32 effects [alloc]:\n    let total: i32 = 0\n    for i in 0 .. 5:\n        let values: Vec[i32] = vec_new()\n        push(values, 10 + i32(i))\n        total = total + values[0]\n        if i == 2:\n            break\n    return total\n\nexport fn while_score() -> i32 effects [alloc]:\n    let i: usize = 0\n    let total: i32 = 0\n    while i < 3:\n        let values: Vec[i32] = vec_new()\n        push(values, i32(i))\n        total = total + values[0]\n        i = i + 1\n    return total\n\nexport fn loop_early() -> i32 effects [alloc]:\n    for i in 0 .. 3:\n        let values: Vec[i32] = vec_new()\n        push(values, 99)\n        if i == 1:\n            return values[0]\n    return 0\n\nexport fn nested_loop_score() -> i32 effects [alloc]:\n    let total: i32 = 0\n    for i in 0 .. 2:\n        let outer_values: Vec[i32] = vec_new()\n        push(outer_values, 10 + i32(i))\n        for j in 0 .. 3:\n            let inner_values: Vec[i32] = vec_new()\n            push(inner_values, i32(j))\n            if j == 1:\n                break\n            total = total + outer_values[0] + inner_values[0]\n        total = total + outer_values[0]\n    return total\n",
    )
    .unwrap();

    let (_, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    for function in [
        "continue_score",
        "break_score",
        "while_score",
        "loop_early",
        "nested_loop_score",
    ] {
        assert!(raw_c_source.contains(&format!("int32_t {function}")));
        assert!(raw_zig_source.contains(&format!("pub fn {function}")));
    }

    let continue_start = raw_c_source.find("int32_t continue_score").unwrap();
    let continue_source = &raw_c_source[continue_start..];
    let continue_pos = continue_source.find("continue;").unwrap();
    let continue_free = continue_source[..continue_pos]
        .rfind("fgx_vec_i32_free(&values);")
        .unwrap();
    assert!(continue_free < continue_pos);

    let break_start = raw_c_source.find("int32_t break_score").unwrap();
    let break_source = &raw_c_source[break_start..];
    let break_pos = break_source.find("break;").unwrap();
    let break_free = break_source[..break_pos]
        .rfind("fgx_vec_i32_free(&values);")
        .unwrap();
    assert!(break_free < break_pos);

    let early_start = raw_c_source.find("int32_t loop_early").unwrap();
    let early_source = &raw_c_source[early_start..];
    let return_pos = early_source.find("return _fgx_return_0;").unwrap();
    let return_free = early_source[..return_pos]
        .rfind("fgx_vec_i32_free(&values);")
        .unwrap();
    assert!(return_free < return_pos);
    assert!(raw_zig_source.contains("defer fgx_vec_i32_free(&values);"));
    assert!(raw_zig_source.contains("defer fgx_vec_i32_free(&inner_values);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("loop_vec_harness.c");
        std::fs::write(
            &harness,
            "#include \"loopvec.h\"\nint main(void) { return continue_score() == 5 && break_score() == 33 && while_score() == 3 && loop_early() == 99 && nested_loop_score() == 42 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "loop_vec_harness.exe"
        } else {
            "loop_vec_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated loop-local Vec C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated loop-local Vec C harness");
        assert!(
            run.status.success(),
            "native loop-local Vec behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("loop_vec_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/loopvec.zig\");\nconst std = @import(\"std\");\ntest \"loop-local vec cleanup\" {\n    try std.testing.expectEqual(@as(i32, 5), lib.continue_score());\n    try std.testing.expectEqual(@as(i32, 33), lib.break_score());\n    try std.testing.expectEqual(@as(i32, 3), lib.while_score());\n    try std.testing.expectEqual(@as(i32, 99), lib.loop_early());\n    try std.testing.expectEqual(@as(i32, 42), lib.nested_loop_score());\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig loop-local Vec harness");
        assert!(
            run.status.success(),
            "generated loop-local Vec Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_immediately_consumed_native_vec_pop_values() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_vec_pop_values",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"vecpopvalue\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export enum Mode:\n    Idle\n    Active\n\nexport struct Marker:\n    id: u32\n    enabled: bool\n\nexport fn scalar_pop_score(has_value: bool) -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    if has_value:\n        push(values, 41)\n    let value: i32 = unwrap_or(pop(values), 7)\n    return value + i32(values.len)\n\nexport fn pop_has_value(has_value: bool) -> bool effects [alloc]:\n    let values: Vec[u8] = vec_new()\n    if has_value:\n        push(values, 9)\n    return is_some(pop(values))\n\nexport fn pop_is_empty(has_value: bool) -> bool effects [alloc]:\n    let values: Vec[u8] = vec_new()\n    if has_value:\n        push(values, 9)\n    return is_none(pop(values))\n\nexport fn array_pop_score(value: [i32; 2], fallback: [i32; 2], has_value: bool) -> i32 effects [alloc]:\n    let rows: Vec[[i32; 2]] = vec_new()\n    if has_value:\n        push(rows, value)\n    let row: [i32; 2] = unwrap_or(pop(rows), fallback)\n    return row[0] + row[1] + i32(rows.len)\n\nexport fn enum_pop_score(has_value: bool) -> i32 effects [alloc]:\n    let modes: Vec[Mode] = vec_new()\n    if has_value:\n        push(modes, Mode::Active)\n    let mode: Mode = unwrap_or(pop(modes), Mode::Idle)\n    match mode:\n        case Mode::Idle:\n            return 1\n        case Mode::Active:\n            return 2\n\nexport fn marker_pop_score(value: Marker, fallback: Marker, has_value: bool) -> i32 effects [alloc]:\n    let markers: Vec[Marker] = vec_new()\n    if has_value:\n        push(markers, value)\n    let marker: Marker = unwrap_or(pop(markers), fallback)\n    if marker.enabled:\n        return i32(marker.id)\n    return -i32(marker.id)\n\nexport fn string_pop_score(value: str, fallback: str, has_value: bool) -> i32 effects [alloc]:\n    let words: Vec[str] = vec_new()\n    if has_value:\n        push(words, value)\n    let word: str = unwrap_or(pop(words), fallback)\n    return i32(str_len(word)) + i32(words.len)\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_header.contains("fgx_vec_i32_pop_or"));
    assert!(c_header.contains("fgx_vec_u8_pop_has_value"));
    assert!(c_header.contains("fgx_vec_array_i32_2_pop_or"));
    assert!(c_header.contains("fgx_vec_Mode_pop_or"));
    assert!(c_header.contains("fgx_vec_Marker_pop_or"));
    assert!(c_source.contains("fgx_vec_i32_pop_or(&values, 7)"));
    assert!(c_source.contains("fgx_vec_u8_pop_has_value(&values)"));
    assert!(zig_source.contains("fgx_vec_i32_pop_value(&values) orelse 7"));
    assert!(zig_source.contains("fgx_vec_array_i32_2_pop_value(&rows) orelse fallback"));
    assert!(zig_source.contains("fgx_vec_Marker_pop_value(&markers) orelse fallback"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(&opt_c.source)
        .unwrap()
        .contains("fgx_vec_Marker_pop_or"));
    assert!(std::fs::read_to_string(&opt_zig.source)
        .unwrap()
        .contains("fgx_vec_Marker_pop_value"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("vec_pop_value_harness.c");
        std::fs::write(
            &harness,
            "#include \"vecpopvalue.h\"\nint main(void) { fgx_array_i32_2 value = { .data = {5, 7} }; fgx_array_i32_2 fallback = { .data = {2, 3} }; Marker active = { .id = 9, .enabled = true }; Marker idle = { .id = 4, .enabled = false }; return scalar_pop_score(true) == 41 && scalar_pop_score(false) == 7 && pop_has_value(true) && !pop_has_value(false) && !pop_is_empty(true) && pop_is_empty(false) && array_pop_score(value, fallback, true) == 12 && array_pop_score(value, fallback, false) == 5 && enum_pop_score(true) == 2 && enum_pop_score(false) == 1 && marker_pop_score(active, idle, true) == 9 && marker_pop_score(active, idle, false) == -4 && string_pop_score(\"lambda\", \"beta\", true) == 6 && string_pop_score(\"lambda\", \"beta\", false) == 4 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "vec_pop_value_harness.exe"
        } else {
            "vec_pop_value_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated value-pop C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated value-pop C harness");
        assert!(run.status.success(), "native C value-pop behavior diverged");
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("vec_pop_value_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/vecpopvalue.zig\");\nconst std = @import(\"std\");\ntest \"immediately consumed native vec pop values\" {\n    try std.testing.expectEqual(@as(i32, 41), lib.scalar_pop_score(true));\n    try std.testing.expectEqual(@as(i32, 7), lib.scalar_pop_score(false));\n    try std.testing.expect(lib.pop_has_value(true));\n    try std.testing.expect(!lib.pop_has_value(false));\n    try std.testing.expect(!lib.pop_is_empty(true));\n    try std.testing.expect(lib.pop_is_empty(false));\n    try std.testing.expectEqual(@as(i32, 12), lib.array_pop_score(.{ 5, 7 }, .{ 2, 3 }, true));\n    try std.testing.expectEqual(@as(i32, 5), lib.array_pop_score(.{ 5, 7 }, .{ 2, 3 }, false));\n    try std.testing.expectEqual(@as(i32, 2), lib.enum_pop_score(true));\n    try std.testing.expectEqual(@as(i32, 1), lib.enum_pop_score(false));\n    try std.testing.expectEqual(@as(i32, 9), lib.marker_pop_score(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }, true));\n    try std.testing.expectEqual(@as(i32, -4), lib.marker_pop_score(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }, false));\n    try std.testing.expectEqual(@as(i32, 6), lib.string_pop_score(\"lambda\", \"beta\", true));\n    try std.testing.expectEqual(@as(i32, 4), lib.string_pop_score(\"lambda\", \"beta\", false));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig value-pop harness");
        assert!(
            run.status.success(),
            "generated value-pop Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_stored_local_native_vec_pop_options() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_stored_native_vec_pop_options",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"storedvecpop\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export enum Mode:\n    Idle\n    Active\n\nexport struct Marker:\n    id: i32\n    enabled: bool\n\nexport fn scalar_score(has_value: bool) -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    if has_value:\n        push(values, 40)\n    let popped: Option[i32] = pop(values)\n    let remaining: usize = values.len\n    return unwrap_or(popped, 7) + i32(remaining)\n\nexport fn stored_is_some(has_value: bool) -> bool effects [alloc]:\n    let values: Vec[u8] = vec_new()\n    if has_value:\n        push(values, 1)\n    let popped: Option[u8] = pop(values)\n    return is_some(popped)\n\nexport fn stored_is_none(has_value: bool) -> bool effects [alloc]:\n    let values: Vec[u8] = vec_new()\n    if has_value:\n        push(values, 1)\n    let popped: Option[u8] = pop(values)\n    return is_none(popped)\n\nexport fn array_score(value: [i32; 2], fallback: [i32; 2], has_value: bool) -> i32 effects [alloc]:\n    let rows: Vec[[i32; 2]] = vec_new()\n    if has_value:\n        push(rows, value)\n    let popped: Option[[i32; 2]] = pop(rows)\n    let row: [i32; 2] = unwrap_or(popped, fallback)\n    return row[0] + row[1]\n\nexport fn enum_score(has_value: bool) -> i32 effects [alloc]:\n    let modes: Vec[Mode] = vec_new()\n    if has_value:\n        push(modes, Mode::Active)\n    let popped: Option[Mode] = pop(modes)\n    let mode: Mode = unwrap_or(popped, Mode::Idle)\n    match mode:\n        case Mode::Idle:\n            return 1\n        case Mode::Active:\n            return 2\n\nexport fn marker_score(value: Marker, fallback: Marker, has_value: bool) -> i32 effects [alloc]:\n    let markers: Vec[Marker] = vec_new()\n    if has_value:\n        push(markers, value)\n    let popped: Option[Marker] = pop(markers)\n    let marker: Marker = unwrap_or(popped, fallback)\n    if marker.enabled:\n        return marker.id\n    return -marker.id\n\nexport fn string_score(value: str, fallback: str, has_value: bool) -> i32 effects [alloc]:\n    let words: Vec[str] = vec_new()\n    if has_value:\n        push(words, value)\n    let popped: Option[str] = pop(words)\n    let word: str = unwrap_or(popped, fallback)\n    return i32(str_len(word))\n",
    )
    .unwrap();

    std::fs::write(
        dir.join("src/more_vectors.fgx"),
        "export fn slice_score(value: &[i32], fallback: &[i32], has_value: bool) -> i32 effects [alloc]:\n    let rows: Vec[&[i32]] = vec_new()\n    if has_value:\n        push(rows, value)\n    let popped: Option[&[i32]] = pop(rows)\n    let row: &[i32] = unwrap_or(popped, fallback)\n    return row[0] + i32(row.len)\n\nexport fn marker_array_score(value: [Marker; 2], fallback: [Marker; 2], has_value: bool) -> i32 effects [alloc]:\n    let rows: Vec[[Marker; 2]] = vec_new()\n    if has_value:\n        push(rows, value)\n    let popped: Option[[Marker; 2]] = pop(rows)\n    let row: [Marker; 2] = unwrap_or(popped, fallback)\n    return row[0].id + row[1].id\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_header.contains("typedef struct { bool has_value; int32_t value; } fgx_option_i32;"));
    assert!(c_source.contains("fgx_option_i32 popped = fgx_vec_i32_pop_value(&values);"));
    assert!(c_source.contains("(popped).has_value ? (popped).value : 7"));
    assert!(c_source.contains("fgx_option_array_i32_2 popped"));
    assert!(c_source.contains("fgx_option_Mode popped"));
    assert!(c_source.contains("fgx_option_Marker popped"));
    assert!(c_source.contains("fgx_option_str popped"));
    assert!(c_source.contains("fgx_option_slice_i32 popped"));
    assert!(c_source.contains("fgx_option_array_Marker_2 popped"));
    assert!(zig_source.contains("var popped: ?i32 = fgx_vec_i32_pop_value(&values);"));
    assert!(zig_source.contains("popped orelse 7"));
    assert!(zig_source.contains("var popped: ?Marker = fgx_vec_Marker_pop_value(&markers);"));
    assert!(zig_source.contains("var popped: ?[]const i32"));
    assert!(zig_source.contains("var popped: ?[2]Marker"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(opt_c.source)
        .unwrap()
        .contains("fgx_option_Marker popped"));
    assert!(std::fs::read_to_string(opt_zig.source)
        .unwrap()
        .contains("var popped: ?Marker"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("stored_vec_pop_harness.c");
        std::fs::write(
            &harness,
            "#include \"storedvecpop.h\"\nint main(void) { fgx_array_i32_2 value = { .data = {5, 7} }; fgx_array_i32_2 fallback = { .data = {2, 3} }; Marker active = { .id = 9, .enabled = true }; Marker idle = { .id = 4, .enabled = false }; return scalar_score(true) == 40 && scalar_score(false) == 7 && stored_is_some(true) && !stored_is_some(false) && !stored_is_none(true) && stored_is_none(false) && array_score(value, fallback, true) == 12 && array_score(value, fallback, false) == 5 && enum_score(true) == 2 && enum_score(false) == 1 && marker_score(active, idle, true) == 9 && marker_score(active, idle, false) == -4 && string_score(\"lambda\", \"beta\", true) == 6 && string_score(\"lambda\", \"beta\", false) == 4 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "stored_vec_pop_harness.exe"
        } else {
            "stored_vec_pop_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated stored-pop C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated stored-pop C harness");
        assert!(
            run.status.success(),
            "native C stored-pop behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("stored_vec_pop_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/storedvecpop.zig\");\nconst std = @import(\"std\");\ntest \"stored native vec pop options\" {\n    try std.testing.expectEqual(@as(i32, 40), lib.scalar_score(true));\n    try std.testing.expectEqual(@as(i32, 7), lib.scalar_score(false));\n    try std.testing.expect(lib.stored_is_some(true));\n    try std.testing.expect(!lib.stored_is_some(false));\n    try std.testing.expect(!lib.stored_is_none(true));\n    try std.testing.expect(lib.stored_is_none(false));\n    try std.testing.expectEqual(@as(i32, 12), lib.array_score(.{ 5, 7 }, .{ 2, 3 }, true));\n    try std.testing.expectEqual(@as(i32, 5), lib.array_score(.{ 5, 7 }, .{ 2, 3 }, false));\n    try std.testing.expectEqual(@as(i32, 2), lib.enum_score(true));\n    try std.testing.expectEqual(@as(i32, 1), lib.enum_score(false));\n    try std.testing.expectEqual(@as(i32, 9), lib.marker_score(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }, true));\n    try std.testing.expectEqual(@as(i32, -4), lib.marker_score(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }, false));\n    try std.testing.expectEqual(@as(i32, 6), lib.string_score(\"lambda\", \"beta\", true));\n    try std.testing.expectEqual(@as(i32, 4), lib.string_score(\"lambda\", \"beta\", false));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig stored-pop harness");
        assert!(
            run.status.success(),
            "generated stored-pop Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_return_native_vec_pop_options_after_cleanup() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_returned_native_vec_pop_options",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"returnedvecpop\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export struct Marker:\n    id: i32\n    enabled: bool\n\nexport fn direct_scalar(has_value: bool) -> Option[i32] effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    if has_value:\n        push(values, 41)\n    return pop(values)\n\nexport fn stored_marker(value: Marker, has_value: bool) -> Option[Marker] effects [alloc]:\n    let markers: Vec[Marker] = vec_new()\n    if has_value:\n        push(markers, value)\n    let popped: Option[Marker] = pop(markers)\n    let remaining: usize = markers.len\n    return popped\n\nexport fn direct_marker_array(value: [Marker; 2], has_value: bool) -> Option[[Marker; 2]] effects [alloc]:\n    let rows: Vec[[Marker; 2]] = vec_new()\n    if has_value:\n        push(rows, value)\n    return pop(rows)\n\nexport fn stored_string(value: str, has_value: bool) -> Option[str] effects [alloc]:\n    let words: Vec[str] = vec_new()\n    if has_value:\n        push(words, value)\n    let popped: Option[str] = pop(words)\n    return popped\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_header.contains("fgx_option_i32 direct_scalar(bool has_value);"));
    assert!(c_header.contains("fgx_option_Marker stored_marker"));
    assert!(c_header.contains("fgx_option_array_Marker_2 direct_marker_array"));
    assert!(c_header.contains("fgx_option_str stored_string"));

    let direct_start = c_source.find("fgx_option_i32 direct_scalar").unwrap();
    let direct_source = &c_source[direct_start..];
    let save = direct_source
        .find("fgx_option_i32 _fgx_return_0 = fgx_vec_i32_pop_value(&values);")
        .unwrap();
    let free = direct_source.find("fgx_vec_i32_free(&values);").unwrap();
    let returned = direct_source.find("return _fgx_return_0;").unwrap();
    assert!(save < free && free < returned);

    let marker_start = c_source.find("fgx_option_Marker stored_marker").unwrap();
    let marker_source = &c_source[marker_start..];
    let save = marker_source
        .find("fgx_option_Marker _fgx_return_0 = popped;")
        .unwrap();
    let free = marker_source
        .find("fgx_vec_Marker_free(&markers);")
        .unwrap();
    let returned = marker_source.find("return _fgx_return_0;").unwrap();
    assert!(save < free && free < returned);
    assert!(zig_source.contains("pub fn direct_scalar(has_value: bool) ?i32"));
    assert!(zig_source.contains("return fgx_vec_i32_pop_value(&values);"));
    assert!(zig_source.contains("pub fn stored_marker(value: Marker, has_value: bool) ?Marker"));
    assert!(zig_source.contains("return popped;"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(opt_c.source)
        .unwrap()
        .contains("fgx_option_Marker _fgx_return_0 = popped;"));
    assert!(std::fs::read_to_string(opt_zig.source)
        .unwrap()
        .contains("return fgx_vec_i32_pop_value(&values);"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("returned_vec_pop_harness.c");
        std::fs::write(
            &harness,
            "#include \"returnedvecpop.h\"\n#include <string.h>\nint main(void) { Marker marker = { .id = 9, .enabled = true }; fgx_array_Marker_2 row = { .data = {{ .id = 3, .enabled = false }, { .id = 4, .enabled = true }} }; fgx_option_i32 full = direct_scalar(true); fgx_option_i32 empty = direct_scalar(false); fgx_option_Marker marked = stored_marker(marker, true); fgx_option_Marker unmarked = stored_marker(marker, false); fgx_option_array_Marker_2 array = direct_marker_array(row, true); fgx_option_str word = stored_string(\"lambda\", true); return full.has_value && full.value == 41 && !empty.has_value && marked.has_value && marked.value.id == 9 && marked.value.enabled && !unmarked.has_value && array.has_value && array.value.data[0].id == 3 && array.value.data[1].id == 4 && word.has_value && strcmp(word.value, \"lambda\") == 0 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "returned_vec_pop_harness.exe"
        } else {
            "returned_vec_pop_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated returned-pop C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated returned-pop C harness");
        assert!(
            run.status.success(),
            "native C returned-pop behavior diverged"
        );
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("returned_vec_pop_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/returnedvecpop.zig\");\nconst std = @import(\"std\");\ntest \"returned native vec pop options\" {\n    try std.testing.expectEqual(@as(?i32, 41), lib.direct_scalar(true));\n    try std.testing.expectEqual(@as(?i32, null), lib.direct_scalar(false));\n    try std.testing.expectEqual(@as(i32, 9), lib.stored_marker(.{ .id = 9, .enabled = true }, true).?.id);\n    try std.testing.expect(lib.stored_marker(.{ .id = 9, .enabled = true }, false) == null);\n    const row = lib.direct_marker_array(.{ .{ .id = 3, .enabled = false }, .{ .id = 4, .enabled = true } }, true).?;\n    try std.testing.expectEqual(@as(i32, 7), row[0].id + row[1].id);\n    try std.testing.expectEqualStrings(\"lambda\", lib.stored_string(\"lambda\", true).?);\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig returned-pop harness");
        assert!(
            run.status.success(),
            "generated returned-pop Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_constructed_native_options() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_constructed_native_options",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"constructedoption\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/options.fgx"),
        "export enum Mode:\n    Idle\n    Active\n\nexport struct Marker:\n    id: i32\n    enabled: bool\n\nexport fn scalar_score(value: i32, present: bool) -> i32:\n    if present:\n        let item: Option[i32] = Some(value)\n        return unwrap_or(item, 7)\n    let item: Option[i32] = None()\n    return unwrap_or(item, 7)\n\nexport fn string_score(value: str, fallback: str, present: bool) -> usize:\n    if present:\n        let item: Option[str] = Some(value)\n        return str_len(unwrap_or(item, fallback))\n    let item: Option[str] = None()\n    return str_len(unwrap_or(item, fallback))\n\nexport fn slice_score(value: &[i32], fallback: &[i32], present: bool) -> i32:\n    if present:\n        let item: Option[&[i32]] = Some(value)\n        let selected: &[i32] = unwrap_or(item, fallback)\n        return selected[0] + i32(selected.len)\n    let item: Option[&[i32]] = None()\n    let selected: &[i32] = unwrap_or(item, fallback)\n    return selected[0] + i32(selected.len)\n\nexport fn array_score(value: [i32; 2], fallback: [i32; 2], present: bool) -> i32:\n    if present:\n        let item: Option[[i32; 2]] = Some(value)\n        let selected: [i32; 2] = unwrap_or(item, fallback)\n        return selected[0] + selected[1]\n    let item: Option[[i32; 2]] = None()\n    let selected: [i32; 2] = unwrap_or(item, fallback)\n    return selected[0] + selected[1]\n\nexport fn enum_some() -> i32:\n    let item: Option[Mode] = Some(Mode::Active)\n    let selected: Mode = unwrap_or(item, Mode::Idle)\n    match selected:\n        case Mode::Idle:\n            return 1\n        case Mode::Active:\n            return 2\n\nexport fn enum_none() -> i32:\n    let item: Option[Mode] = None()\n    let selected: Mode = unwrap_or(item, Mode::Idle)\n    match selected:\n        case Mode::Idle:\n            return 1\n        case Mode::Active:\n            return 2\n\nexport fn marker_score(value: Marker, fallback: Marker, present: bool) -> i32:\n    if present:\n        let item: Option[Marker] = Some(value)\n        let selected: Marker = unwrap_or(item, fallback)\n        return selected.id\n    let item: Option[Marker] = None()\n    let selected: Marker = unwrap_or(item, fallback)\n    return selected.id\n\nexport fn scalar_param(value: Option[i32], fallback: i32) -> i32:\n    return unwrap_or(value, fallback)\n\nexport fn string_param(value: Option[str], fallback: str) -> usize:\n    return str_len(unwrap_or(value, fallback))\n\nexport fn slice_param(value: Option[&[i32]], fallback: &[i32]) -> i32:\n    let selected: &[i32] = unwrap_or(value, fallback)\n    return selected[0] + i32(selected.len)\n\nexport fn array_param(value: Option[[i32; 2]], fallback: [i32; 2]) -> i32:\n    let selected: [i32; 2] = unwrap_or(value, fallback)\n    return selected[0] + selected[1]\n\nexport fn enum_param(value: Option[Mode], fallback: Mode) -> i32:\n    let selected: Mode = unwrap_or(value, fallback)\n    match selected:\n        case Mode::Idle:\n            return 1\n        case Mode::Active:\n            return 2\n\nexport fn marker_param(value: Option[Marker], fallback: Marker) -> i32:\n    return unwrap_or(value, fallback).id\n\nexport fn relay_param(value: Option[i32]) -> Option[i32]:\n    return value\n\nexport fn return_some(value: i32) -> Option[i32]:\n    return Some(value)\n\nexport fn return_none() -> Option[i32]:\n    return None()\n\nexport fn return_stored(value: Marker) -> Option[Marker]:\n    let item: Option[Marker] = Some(value)\n    return item\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_source
        .contains("fgx_option_i32 item = (fgx_option_i32){ .has_value = true, .value = value };"));
    assert!(c_source.contains("fgx_option_i32 item = (fgx_option_i32){ .has_value = false };"));
    assert!(c_header.contains("fgx_option_i32 return_some(int32_t value);"));
    assert!(c_header.contains("int32_t scalar_param(fgx_option_i32 value, int32_t fallback);"));
    assert!(c_header.contains("fgx_option_i32 relay_param(fgx_option_i32 value);"));
    assert!(c_source.contains("return (fgx_option_i32){ .has_value = true, .value = value };"));
    assert!(c_source.contains("fgx_option_Marker item = (fgx_option_Marker)"));
    assert!(zig_source.contains("var item: ?i32 = value;"));
    assert!(zig_source.contains("var item: ?i32 = null;"));
    assert!(zig_source.contains("pub fn return_some(value: i32) ?i32"));
    assert!(zig_source.contains("return value;"));
    assert!(zig_source.contains("pub fn return_none() ?i32"));
    assert!(zig_source.contains("return null;"));
    assert!(zig_source.contains("pub fn scalar_param(value: ?i32, fallback: i32) i32"));
    assert!(zig_source.contains("pub fn relay_param(value: ?i32) ?i32"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(opt_c.source)
        .unwrap()
        .contains(".has_value = true"));
    assert!(std::fs::read_to_string(opt_zig.source)
        .unwrap()
        .contains("pub fn return_none() ?i32"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("constructed_option_harness.c");
        std::fs::write(
            &harness,
            "#include \"constructedoption.h\"\nint main(void) { int32_t values[] = { 8, 9 }; int32_t fallback[] = { 3, 4 }; fgx_const_slice_i32 value_slice = { .data = values, .len = 2 }; fgx_const_slice_i32 fallback_slice = { .data = fallback, .len = 2 }; fgx_array_i32_2 value_array = { .data = { 5, 7 } }; fgx_array_i32_2 fallback_array = { .data = { 2, 3 } }; Marker marker = { .id = 9, .enabled = true }; Marker other = { .id = 4, .enabled = false }; fgx_option_i32 some = return_some(11); fgx_option_i32 none = return_none(); fgx_option_Marker stored = return_stored(marker); fgx_option_str some_string = { .has_value = true, .value = \"lambda\" }; fgx_option_slice_i32 some_slice = { .has_value = true, .value = value_slice }; fgx_option_array_i32_2 some_array = { .has_value = true, .value = value_array }; fgx_option_Mode some_mode = { .has_value = true, .value = Mode_Active }; fgx_option_Marker some_marker = { .has_value = true, .value = marker }; fgx_option_i32 relayed = relay_param(some); return scalar_score(41, true) == 41 && scalar_score(41, false) == 7 && string_score(\"lambda\", \"beta\", true) == 6 && string_score(\"lambda\", \"beta\", false) == 4 && slice_score(value_slice, fallback_slice, true) == 10 && slice_score(value_slice, fallback_slice, false) == 5 && array_score(value_array, fallback_array, true) == 12 && array_score(value_array, fallback_array, false) == 5 && enum_some() == 2 && enum_none() == 1 && marker_score(marker, other, true) == 9 && marker_score(marker, other, false) == 4 && scalar_param(some, 7) == 11 && scalar_param(none, 7) == 7 && string_param(some_string, \"beta\") == 6 && slice_param(some_slice, fallback_slice) == 10 && array_param(some_array, fallback_array) == 12 && enum_param(some_mode, Mode_Idle) == 2 && marker_param(some_marker, other) == 9 && relayed.has_value && relayed.value == 11 && some.has_value && some.value == 11 && !none.has_value && stored.has_value && stored.value.id == 9 ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "constructed_option_harness.exe"
        } else {
            "constructed_option_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated constructed-Option C failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        assert!(std::process::Command::new(&executable)
            .status()
            .expect("failed to execute constructed-Option C harness")
            .success());
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("constructed_option_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/constructedoption.zig\");\nconst std = @import(\"std\");\ntest \"constructed native options\" {\n    try std.testing.expectEqual(@as(i32, 41), lib.scalar_score(41, true));\n    try std.testing.expectEqual(@as(i32, 7), lib.scalar_score(41, false));\n    try std.testing.expectEqual(@as(usize, 6), lib.string_score(\"lambda\", \"beta\", true));\n    try std.testing.expectEqual(@as(usize, 4), lib.string_score(\"lambda\", \"beta\", false));\n    try std.testing.expectEqual(@as(i32, 10), lib.slice_score(&.{ 8, 9 }, &.{ 3, 4 }, true));\n    try std.testing.expectEqual(@as(i32, 5), lib.slice_score(&.{ 8, 9 }, &.{ 3, 4 }, false));\n    try std.testing.expectEqual(@as(i32, 12), lib.array_score(.{ 5, 7 }, .{ 2, 3 }, true));\n    try std.testing.expectEqual(@as(i32, 5), lib.array_score(.{ 2, 3 }, .{ 2, 3 }, false));\n    try std.testing.expectEqual(@as(i32, 2), lib.enum_some());\n    try std.testing.expectEqual(@as(i32, 1), lib.enum_none());\n    try std.testing.expectEqual(@as(i32, 9), lib.marker_score(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }, true));\n    try std.testing.expectEqual(@as(i32, 4), lib.marker_score(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }, false));\n    try std.testing.expectEqual(@as(i32, 11), lib.scalar_param(11, 7));\n    try std.testing.expectEqual(@as(i32, 7), lib.scalar_param(null, 7));\n    try std.testing.expectEqual(@as(usize, 6), lib.string_param(\"lambda\", \"beta\"));\n    try std.testing.expectEqual(@as(i32, 10), lib.slice_param(&.{ 8, 9 }, &.{ 3, 4 }));\n    try std.testing.expectEqual(@as(i32, 12), lib.array_param(.{ 5, 7 }, .{ 2, 3 }));\n    try std.testing.expectEqual(@as(i32, 2), lib.enum_param(.Active, .Idle));\n    try std.testing.expectEqual(@as(i32, 9), lib.marker_param(.{ .id = 9, .enabled = true }, .{ .id = 4, .enabled = false }));\n    try std.testing.expectEqual(@as(?i32, 11), lib.relay_param(11));\n    try std.testing.expectEqual(@as(?i32, 11), lib.return_some(11));\n    try std.testing.expectEqual(@as(?i32, null), lib.return_none());\n    try std.testing.expectEqual(@as(i32, 9), lib.return_stored(.{ .id = 9, .enabled = true }).?.id);\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig constructed-Option harness");
        assert!(
            run.status.success(),
            "generated constructed-Option Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn c_and_zig_lower_native_option_constructor_reassignment() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_option_reassignment",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"reassignedoption\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/options.fgx"),
        "export fn replace(value: Option[i32], replacement: i32, present: bool) -> Option[i32]:\n    if present:\n        value = Some(replacement)\n    else:\n        value = None()\n    return value\n\nexport fn reset_local(value: i32, present: bool) -> Option[i32]:\n    let item: Option[i32] = Some(value)\n    if !present:\n        item = None()\n    return item\n",
    )
    .unwrap();

    let (c_header, c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(c_header.contains(
        "fgx_option_i32 replace(fgx_option_i32 value, int32_t replacement, bool present);"
    ));
    assert!(
        c_source.contains("value = (fgx_option_i32){ .has_value = true, .value = replacement };")
    );
    assert!(c_source.contains("value = (fgx_option_i32){ .has_value = false };"));
    assert!(zig_source.contains("value = replacement;"));
    assert!(zig_source.contains("value = null;"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let (opt_c, _, _) =
        crate::c_backend::build_c_project_opt(&dir, &dir.join("out-c-opt")).unwrap();
    let (opt_zig, _, _) =
        crate::zig_backend::build_zig_project_opt(&dir, &dir.join("out-zig-opt")).unwrap();
    assert!(std::fs::read_to_string(opt_c.source)
        .unwrap()
        .contains(".has_value = false"));
    assert!(std::fs::read_to_string(opt_zig.source)
        .unwrap()
        .contains("value = null;"));

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("reassigned_option_harness.c");
        std::fs::write(
            &harness,
            "#include \"reassignedoption.h\"\nint main(void) { fgx_option_i32 some = { .has_value = true, .value = 4 }; fgx_option_i32 full = replace(some, 9, true); fgx_option_i32 empty = replace(some, 9, false); fgx_option_i32 kept = reset_local(6, true); fgx_option_i32 reset = reset_local(6, false); return full.has_value && full.value == 9 && !empty.has_value && kept.has_value && kept.value == 6 && !reset.has_value ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "reassigned_option_harness.exe"
        } else {
            "reassigned_option_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke C compiler for option reassignment");
        assert!(
            compile.status.success(),
            "generated option-reassignment C failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        assert!(std::process::Command::new(executable)
            .status()
            .expect("failed to run option-reassignment C harness")
            .success());
    }

    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("reassigned_option_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/reassignedoption.zig\");\nconst std = @import(\"std\");\ntest \"native option reassignment\" {\n    try std.testing.expectEqual(@as(?i32, 9), lib.replace(4, 9, true));\n    try std.testing.expectEqual(@as(?i32, null), lib.replace(4, 9, false));\n    try std.testing.expectEqual(@as(?i32, 6), lib.reset_local(6, true));\n    try std.testing.expectEqual(@as(?i32, null), lib.reset_local(6, false));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig option-reassignment harness");
        assert!(
            run.status.success(),
            "generated option-reassignment Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    assert!(c.header.exists());
    assert!(zig.source.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_option_callee_reassignment_remains_blocked() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_callee_option_reassignment",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"calleeoption\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/options.fgx"),
        "fn make(value: i32) -> Option[i32]:\n    return Some(value)\n\nfn replace(value: i32) -> Option[i32]:\n    let item: Option[i32] = None()\n    item = make(value)\n    return item\n",
    )
    .unwrap();
    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    for error in [&c_err, &zig_err] {
        assert!(error.contains("generic_type:Option"), "{error}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unsupported_native_vec_shapes_remain_explicit_fir_blockers() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_native_vec_blockers",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"vecblock\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "enum Choice:\n    One\n    Two\n\nfn nested_element() -> usize effects [alloc]:\n    let values: Vec[Option[i32]] = vec_new()\n    return values.len\n\nfn alias() -> usize effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let moved: Vec[i32] = values\n    return moved.len\n\nfn match_alias(choice: Choice) -> usize effects [alloc]:\n    let result: usize = 0\n    match choice:\n        case Choice::One:\n            let values: Vec[i32] = vec_new()\n            let moved: Vec[i32] = values\n            result = moved.len\n        case Choice::Two:\n            result = 0\n    return result\n\nfn pop_value() -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let value: Option[i32] = pop(values)\n    return 0\n\nfn observing_fallback() -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    push(values, 2)\n    return unwrap_or(pop(values), values[0])\n\nfn smuggle_allocator() -> usize effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    alloc()\n    return values.len\n\nfn signature(values: Vec[i32]) -> usize:\n    return values.len\n",
    )
    .unwrap();

    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    for error in [&c_err, &zig_err] {
        assert!(error.contains("generic_type:Vec"), "{error}");
        assert!(error.contains("generic_type:Option"), "{error}");
        assert!(error.contains("intrinsic:unwrap_or"), "{error}");
        assert!(error.contains("native_vec:ownership_escape"), "{error}");
        assert!(
            error.contains("native_vec:pop_requires_discarded_local_native_vec"),
            "{error}"
        );
        assert!(error.contains("intrinsic:alloc"), "{error}");
    }
    assert!(!dir.join("out-c").exists());
    assert!(!dir.join("out-zig").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- string library (lang.3) ---

const STR_PROG: &str = "fn greet(name: str) -> str:\n    return str_concat(\"Hi \", name)\n\nfn label(n: i32) -> str:\n    return str_concat(\"count: \", int_to_str(n))\n\nfn name_len(name: str) -> usize:\n    return str_len(name)\n\nfn dot_len(name: str) -> usize:\n    return name.len\n";

fn expect_str(entry: &str, args: &str, expected: &str) {
    let program = parse_human(STR_PROG).unwrap();
    let input = format!("{{ \"args\": {args}, \"expect\": {{ \"return\": {expected} }} }}");
    let output = run_entry_json(&program, entry, &input).unwrap();
    assert!(
        output.contains("\"status\": \"pass\""),
        "{entry}:\n{output}"
    );
}

#[test]
fn str_concat_builds_strings() {
    expect_str("greet", "{ \"name\": \"Ada\" }", "\"Hi Ada\"");
}

#[test]
fn int_to_str_in_a_message() {
    expect_str("label", "{ \"n\": 5 }", "\"count: 5\"");
}

#[test]
fn str_len_and_dot_len_agree() {
    // Both the `str_len` builtin and the `.len` field give the byte length.
    expect_str("name_len", "{ \"name\": \"hello\" }", "5");
    expect_str("dot_len", "{ \"name\": \"hello\" }", "5");
}

const STR_OPS: &str = "fn head(s: str) -> str:\n    return str_slice(s, 0, 3)\n\nfn locate(s: str) -> i32:\n    return unwrap_or(str_find(s, \"lo\"), -1)\n\nfn has(s: str) -> bool:\n    return str_contains(s, \"ell\")\n\nfn parts(s: str) -> usize:\n    let pieces: Vec[str] = str_split(s, \",\")\n    return pieces.len\n\nfn second(s: str) -> str:\n    let pieces: Vec[str] = str_split(s, \",\")\n    return pieces[1]\n";

#[test]
fn str_slice_find_contains_split() {
    let program = parse_human(STR_OPS).unwrap();
    for (entry, args, expected) in [
        ("head", "{ \"s\": \"hello\" }", "\"hel\""),
        ("locate", "{ \"s\": \"hello\" }", "3"),
        ("has", "{ \"s\": \"hello\" }", "true"),
        ("parts", "{ \"s\": \"a,b,c\" }", "3"),
        ("second", "{ \"s\": \"a,b,c\" }", "\"b\""),
    ] {
        let input = format!("{{ \"args\": {args}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(
            output.contains("\"status\": \"pass\""),
            "{entry}:\n{output}"
        );
    }
    // The whole string-ops program is well-typed (split yields Vec[str],
    // indexing it yields str, .len is usize).
    let diagnostics = crate::typecheck::check(&parse_human(STR_OPS).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn str_slice_out_of_bounds_traps() {
    let src = "fn f(s: str) -> str:\n    return str_slice(s, 0, 99)\n";
    let program = parse_human(src).unwrap();
    let err = run_entry_json(&program, "f", "{ \"args\": { \"s\": \"hi\" } }").unwrap_err();
    assert!(err.contains("slice_out_of_bounds"), "got: {err}");
}

// --- growable Map (lang.3) ---

const MAP_PROG: &str = "fn tally() -> i32:\n    let m: Map[str, i32] = map_new()\n    map_set(m, \"a\", 1)\n    map_set(m, \"b\", 2)\n    map_set(m, \"a\", 10)\n    return unwrap_or(map_get(m, \"a\"), 0) + unwrap_or(map_get(m, \"b\"), 0)\n\nfn missing() -> i32:\n    let m: Map[str, i32] = map_new()\n    map_set(m, \"x\", 7)\n    return unwrap_or(map_get(m, \"y\"), -1)\n\nfn evict() -> i32:\n    let m: Map[str, i32] = map_new()\n    map_set(m, \"a\", 1)\n    map_set(m, \"b\", 2)\n    map_remove(m, \"a\")\n    let size: usize = m.len\n    if map_has(m, \"a\"):\n        return -1\n    return i32(size)\n";

fn expect_map(entry: &str, expected: i64) {
    let program = parse_human(MAP_PROG).unwrap();
    let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
    let output = run_entry_json(&program, entry, &input).unwrap();
    assert!(
        output.contains("\"status\": \"pass\""),
        "{entry}:\n{output}"
    );
}

#[test]
fn map_set_get_and_overwrite() {
    // "a" set then overwritten to 10; "b" is 2 -> 12.
    expect_map("tally", 12);
}

#[test]
fn map_get_missing_key_is_none() {
    // Missing key falls back through unwrap_or -> -1.
    expect_map("missing", -1);
}

#[test]
fn map_remove_and_has_and_len() {
    // Remove "a": has(m, "a") is false, len is 1.
    expect_map("evict", 1);
}

#[test]
fn map_program_type_checks_clean() {
    let diagnostics = crate::typecheck::check(&parse_human(MAP_PROG).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn string_program_type_checks_clean() {
    // str_concat/int_to_str type as str, str_len and `.len` as usize.
    let diagnostics = crate::typecheck::check(&parse_human(STR_PROG).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn c_and_zig_lower_bounded_borrowed_string_operations() {
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_borrowed_strings", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"borrowedstr\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/strings.fgx"),
        "fn echo(s: str) -> str:\n    return s\n\nexport fn byte_len(s: str) -> usize:\n    return str_len(s)\n\nexport fn dot_len(s: str) -> usize:\n    return echo(s).len\n\nexport fn has(s: str, needle: str) -> bool:\n    return str_contains(s, needle)\n\nexport fn same(a: str, b: str) -> bool:\n    return a == b\n\nexport fn different(a: str, b: str) -> bool:\n    return a != b\n",
    )
    .unwrap();

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let header = std::fs::read_to_string(&c.header).unwrap();
    let c_source = std::fs::read_to_string(&c.source).unwrap();
    assert!(header.contains("FGX_STR_MAX_BYTES"), "{header}");
    assert!(header.contains("fgx_str_contains"), "{header}");
    assert!(c_source.contains("return fgx_str_len(s);"), "{c_source}");
    assert!(
        c_source.contains("return fgx_str_len(echo(s));"),
        "{c_source}"
    );
    assert!(
        c_source.contains("return fgx_str_contains(s, needle);"),
        "{c_source}"
    );
    assert!(c_source.contains("return fgx_str_eq(a, b);"), "{c_source}");
    assert!(
        c_source.contains("return (!fgx_str_eq(a, b));"),
        "{c_source}"
    );

    let zig = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap();
    let zig_source = std::fs::read_to_string(&zig.source).unwrap();
    assert!(zig_source.contains("FGX_STR_MAX_BYTES"), "{zig_source}");
    assert!(
        zig_source.contains("return std.mem.indexOf(u8, text, needle) != null;"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("return fgx_str_len(echo(s));"),
        "{zig_source}"
    );
    assert!(
        zig_source.contains("return fgx_str_eq(a, b);"),
        "{zig_source}"
    );

    // Compile and execute the C artifact when a host compiler is available.
    // This proves byte-length (including UTF-8), empty-needle, and content
    // equality behavior rather than checking source text alone.
    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let harness = dir.join("string_harness.c");
        std::fs::write(
            &harness,
            "#include \"borrowedstr.h\"\nint main(void) { return byte_len(\"h\\xC3\\xA9\") == 3 && dot_len(\"hello\") == 5 && has(\"hello\", \"ell\") && has(\"hello\", \"\") && same(\"abc\", \"abc\") && different(\"abc\", \"abd\") ? 0 : 1; }\n",
        )
        .unwrap();
        let executable = dir.join(if cfg!(windows) {
            "string_harness.exe"
        } else {
            "string_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            c.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            dir.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke host C compiler");
        assert!(
            compile.status.success(),
            "generated String C failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute generated String C harness");
        assert!(run.status.success(), "native String behavior diverged");
    }

    // Zig receives the same behavioral proof when the pinned host toolchain is
    // present; otherwise the deterministic source assertions above still run.
    if let Some(toolchain) = crate::native_build::detect_zig_toolchain() {
        let harness = dir.join("string_harness.zig");
        std::fs::write(
            &harness,
            "const lib = @import(\"out-zig/borrowedstr.zig\");\nconst std = @import(\"std\");\ntest \"borrowed strings\" {\n    try std.testing.expectEqual(@as(usize, 3), lib.byte_len(\"hé\"));\n    try std.testing.expectEqual(@as(usize, 5), lib.dot_len(\"hello\"));\n    try std.testing.expect(lib.has(\"hello\", \"ell\"));\n    try std.testing.expect(lib.has(\"hello\", \"\"));\n    try std.testing.expect(lib.same(\"abc\", \"abc\"));\n    try std.testing.expect(lib.different(\"abc\", \"abd\"));\n}\n",
        )
        .unwrap();
        let argv = vec![
            toolchain.name.clone(),
            "test".to_string(),
            harness.display().to_string(),
        ];
        let run = crate::native_build::run_with_path(
            &argv,
            std::path::Path::new(&toolchain.name).parent(),
        )
        .expect("failed to invoke Zig String harness");
        assert!(
            run.status.success(),
            "generated String Zig failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn allocating_string_operations_remain_explicit_source_backend_blockers() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_{}_owned_string_blocker",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"ownedstr\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/strings.fgx"),
        "export fn greet(name: str) -> str:\n    return str_concat(\"Hi \", name)\n",
    )
    .unwrap();

    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    assert!(c_err.contains("intrinsic:str_concat"), "{c_err}");
    assert!(zig_err.contains("intrinsic:str_concat"), "{zig_err}");
    assert!(!dir.join("out-c").exists());
    assert!(!dir.join("out-zig").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn borrowed_string_builtins_check_arguments_and_ordering_statically() {
    let bad_arg = parse_human("fn bad() -> usize:\n    return str_len(1)\n").unwrap();
    let diagnostics = crate::typecheck::check(&bad_arg);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "argument_type_mismatch"),
        "{diagnostics:?}"
    );

    let ordered = parse_human("fn bad(a: str, b: str) -> bool:\n    return a < b\n").unwrap();
    let diagnostics = crate::typecheck::check(&ordered);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "comparison_type_mismatch"),
        "{diagnostics:?}"
    );
}
