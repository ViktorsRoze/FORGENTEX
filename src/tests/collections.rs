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
fn c_and_zig_lower_bounded_owned_local_vec_i32() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_native_vec_i32", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"nativevec\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/vectors.fgx"),
        "export fn vec_score() -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 10)\n    push(values, 20)\n    push(values, 30)\n    pop(values)\n    values[0] = 7\n    return values[0] + values[1] + i32(values.len)\n\nexport fn early(flag: bool) -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 41)\n    if flag:\n        return values[0]\n    return 0\n",
    )
    .unwrap();

    let (_, raw_c_source) = crate::c_backend::generate_c_for_project(&dir).unwrap();
    let raw_zig_source = crate::zig_backend::generate_zig_for_project(&dir).unwrap();
    assert!(raw_c_source.contains("fgx_vec_i32_push(&values, 30);"));
    assert!(raw_zig_source.contains("defer fgx_vec_i32_free(&values);"));

    let c = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap();
    let header = std::fs::read_to_string(&c.header).unwrap();
    let c_source = std::fs::read_to_string(&c.source).unwrap();
    assert!(
        header.contains("FGX_VEC_I32_MAX_ELEMENTS ((size_t)1048576)"),
        "{header}"
    );
    assert!(header.contains("fgx_vec_i32_reserve_for_push"), "{header}");
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
            "#include \"nativevec.h\"\nint main(int argc, char **argv) { (void)argv; if (argc > 1) { fgx_vec_i32 values = fgx_vec_i32_new(); values.len = FGX_VEC_I32_MAX_ELEMENTS; fgx_vec_i32_push(&values, 1); return 2; } return vec_score() == 29 && early(true) == 41 && early(false) == 0 ? 0 : 1; }\n",
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
            "const lib = @import(\"out-zig/nativevec.zig\");\nconst std = @import(\"std\");\ntest \"owned local vec\" {\n    try std.testing.expectEqual(@as(i32, 29), lib.vec_score());\n    try std.testing.expectEqual(@as(i32, 41), lib.early(true));\n    try std.testing.expectEqual(@as(i32, 0), lib.early(false));\n}\n",
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
        "fn other_element() -> usize effects [alloc]:\n    let values: Vec[str] = vec_new()\n    push(values, \"x\")\n    return values.len\n\nfn alias() -> usize effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let moved: Vec[i32] = values\n    return moved.len\n\nfn pop_value() -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let value: i32 = pop(values)\n    return value\n\nfn smuggle_allocator() -> usize effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    alloc()\n    return values.len\n\nfn signature(values: Vec[i32]) -> usize:\n    return values.len\n",
    )
    .unwrap();

    let c_err = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    let zig_err = crate::zig_backend::build_zig_project(&dir, &dir.join("out-zig")).unwrap_err();
    for error in [&c_err, &zig_err] {
        assert!(error.contains("generic_type:Vec"), "{error}");
        assert!(error.contains("native_vec:ownership_escape"), "{error}");
        assert!(
            error.contains("native_vec:pop_requires_discarded_local_vec_i32"),
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
