use std::fs;
use std::path::Path;

use crate::backend::generate_backend_ir;
use crate::interp::{run_demo, run_demo_with_ir, run_entry_json};
use crate::parser::parse_file;
use crate::typecheck::check;

use super::parse_fixture;

#[test]
fn audio_mix_interpreter_demo_passes() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let output = run_demo(&program).unwrap();
    assert!(output.contains("run mix_samples: pass"));
    assert!(output.contains("dst: [5500, 0, -12000, 17000]"));
}

#[test]
fn demo_run_with_ir_executes_fir_body_instead_of_ast_body() {
    let root = std::env::temp_dir().join(format!("forgentex_{}_demo_fir_body", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let ir_path = root.join("clamp_demo.fgx");
    fs::write(
        &ir_path,
        "fn clamp_i16(x: f32) -> i16:\n    return i16(7)\n",
    )
    .unwrap();
    let ir = generate_backend_ir(&ir_path).unwrap();
    let ast_program = parse_file(
        Path::new("clamp_demo_ast.fgx"),
        "fn clamp_i16(x: f32) -> i16:\n    return i16(0)\n",
    )
    .unwrap();

    let output = run_demo_with_ir(&ast_program, &ir).unwrap();
    assert!(output.contains("run clamp_i16(40000.0): 7"), "{output}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn assembly_only_example_is_not_runnable() {
    let program = parse_fixture("examples/media/popcount_x86.fgx");
    let err = run_demo(&program).unwrap_err();
    assert!(err.contains("no runnable demo entrypoint"));
}

#[test]
fn json_entrypoint_runs_audio_mix_with_golden_expectations() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let output = run_entry_json(
        &program,
        "mix_samples",
        r#"{
          "args": {
            "dst": [1000, -20000, 16000, 32000],
            "a": [1000, -20000, 16000, 32000],
            "b": [5000, 10000, -20000, 1000],
            "gain": 0.5
          },
          "expect": {
            "return": null,
            "bindings": {
              "dst": [5500, 0, -12000, 17000]
            }
          }
        }"#,
    )
    .unwrap();

    assert!(output.contains("\"entry\": \"mix_samples\""));
    assert!(output.contains("\"status\": \"pass\""));
    assert!(output.contains("\"dst\": [5500, 0, -12000, 17000]"));
}

#[test]
fn mutable_slice_changes_propagate_through_nested_calls() {
    let source = r#"
fn fill_first(dst: &mut [i16]) -> void:
    dst[0] = 7

fn wrapper(dst: &mut [i16]) -> void:
    fill_first(dst)
"#;
    let program = parse_file(Path::new("refs.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let output = run_entry_json(
        &program,
        "wrapper",
        r#"{
          "args": { "dst": [1, 2, 3] },
          "expect": { "bindings": { "dst": [7, 2, 3] } }
        }"#,
    )
    .unwrap();

    assert!(output.contains("\"status\": \"pass\""));
    assert!(output.contains("\"dst\": [7, 2, 3]"));
}

#[test]
fn structs_option_and_result_values_execute_in_safe_code() {
    let source = r#"
struct Header:
    kind: u8
    len: u8

fn make_header(kind: u8, len: u8) -> Header:
    return Header(kind, len)

fn choose(flag: bool, fallback: u8) -> u8:
    let ok: Option[u8] = Some(9)
    if flag:
        return unwrap_or(ok, fallback)
    return unwrap_or(None(), fallback)

fn choose_result(flag: bool, fallback: u8) -> u8:
    let ok: Result[u8, u8] = Ok(11)
    if flag:
        return unwrap_or(ok, fallback)
    return unwrap_or(Err(1), fallback)
"#;
    let program = parse_file(Path::new("runtime_values.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let header = run_entry_json(
        &program,
        "make_header",
        r#"{
          "args": { "kind": 5, "len": 12 },
          "expect": { "return": { "$struct": "Header", "kind": 5, "len": 12 } }
        }"#,
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

    assert!(header.contains("\"$struct\": \"Header\""));
    assert!(option.contains("\"status\": \"pass\""));
    assert!(result.contains("\"status\": \"pass\""));
}

#[test]
fn runtime_traps_are_deterministic() {
    let source = r#"
fn bad_index(values: &[u8]) -> u8:
    return values[4]

fn bad_div(a: i32, b: i32) -> i32:
    return a / b
"#;
    let program = parse_file(Path::new("traps.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let index_err =
        run_entry_json(&program, "bad_index", r#"{ "args": { "values": [1] } }"#).unwrap_err();
    let div_err =
        run_entry_json(&program, "bad_div", r#"{ "args": { "a": 5, "b": 0 } }"#).unwrap_err();

    assert!(index_err.contains("trap: index_out_of_bounds"));
    assert!(div_err.contains("trap: division_by_zero"));
}

#[test]
fn an_owned_vector_lends_immutable_slice_views_without_moving() {
    // An owned `Vec[T]` reads as `&[T]`, so a buffer can be lent to helpers
    // repeatedly and still be used by its owner afterwards. Windows of it
    // project to the same immutable view.
    let source = "fn total(values: &[i32]) -> i32:\n    let sum: i32 = 0\n    let at: usize = 0\n    let stop: usize = values.len\n    while at < stop:\n        sum = sum + values[at]\n        at = at + 1\n    return sum\n\nfn driver() -> i32 effects [alloc]:\n    let owned: Vec[i32] = vec_new()\n    push(owned, 10)\n    push(owned, 20)\n    push(owned, 30)\n    push(owned, 40)\n    let whole: i32 = total(owned)\n    let window: i32 = total(owned[1 .. 3])\n    return whole + window + owned[0]\n";
    let program = parse_file(Path::new("lend.fgx"), source).expect("source should parse");
    assert!(check(&program).is_empty(), "{:?}", check(&program));

    // 100 (whole) + 50 (window) + 10 (still owned) == 160
    let output = run_entry_json(&program, "driver", r#"{ "args": {} }"#).unwrap();
    assert!(output.contains("160"), "unexpected result: {output}");
}

#[test]
fn lending_a_vector_does_not_weaken_move_checking() {
    // The shared borrow must not turn into a licence to use a moved value:
    // passing the buffer by value still moves it.
    let source = "fn consume(values: Vec[i32]) -> i32:\n    return values[0]\n\nfn driver() -> i32 effects [alloc]:\n    let owned: Vec[i32] = vec_new()\n    push(owned, 3)\n    let taken: i32 = consume(owned)\n    return taken + owned[0]\n";
    let program = parse_file(Path::new("moved.fgx"), source).expect("source should parse");
    let diagnostics = check(&program);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "expected use_after_move, got {diagnostics:?}"
    );
}

#[test]
fn an_owned_vector_crosses_the_native_boundary_whole_and_windowed() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    // The reviewed native boundary takes `&[T]`, so lending an owned buffer
    // (and a bounded window of it) is what lets real payloads reach native
    // code without a copy through an intermediate binding.
    let source = "foreign sum_c c version \"c11\" effects [alloc]:\n    input values: &[i32]\n    output total: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stddef.h>\n        #include <stdint.h>\n\n        int32_t sum_c(const int32_t *values, size_t values_len) {\n            int32_t total = 0;\n            for (size_t i = 0; i < values_len; ++i) total += values[i];\n            return total;\n        }\n\nfn driver() -> i32 effects [alloc]:\n    let owned: Vec[i32] = vec_new()\n    push(owned, 10)\n    push(owned, 20)\n    push(owned, 30)\n    push(owned, 40)\n    let whole: i32 = sum_c(owned)\n    let window: i32 = sum_c(owned[1 .. 3])\n    return whole + window\n";
    let program = parse_file(Path::new("native_lend.fgx"), source).expect("source should parse");
    assert!(check(&program).is_empty(), "{:?}", check(&program));

    // 100 (whole) + 50 (window)
    let output = crate::interp::run_entry_json_live(&program, "driver", r#"{ "args": {} }"#, None)
        .expect("live native call");
    assert!(output.contains("150"), "unexpected native result: {output}");
}

#[test]
fn windowing_a_large_owned_vector_stays_linear() {
    // `Expr::SliceRange` used to clone the whole base out of the environment
    // before projecting, so taking N windows of one buffer cost O(N * len)
    // even when every window was small. This takes 512 windows of a 32k
    // buffer; before the fix that copied ~16.8M elements.
    let source = "fn width(window: &[i32]) -> i32:\n    return i32(window.len)\n\nfn driver(limit: i32) -> i32 effects [alloc]:\n    let owned: Vec[i32] = vec_new()\n    let i: i32 = 0\n    while i < limit:\n        push(owned, i)\n        i = i + 1\n    let total: i32 = 0\n    let at: usize = 0\n    while at < 512:\n        total = total + width(owned[at .. at + 8])\n        at = at + 1\n    return total\n";
    let program = parse_file(Path::new("window.fgx"), source).expect("source should parse");
    assert!(check(&program).is_empty());

    let started = std::time::Instant::now();
    let output = run_entry_json(&program, "driver", r#"{ "args": { "limit": 32768 } }"#).unwrap();
    let elapsed = started.elapsed();

    assert!(output.contains("4096"), "unexpected width total: {output}");
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "windowing took {elapsed:?}, which suggests slice ranges regressed to cloning the base"
    );
}

#[test]
fn scanning_a_large_owned_vector_stays_linear() {
    // Reading `values[at]` used to evaluate the base expression first, which
    // cloned the whole vector for every element and made a linear scan
    // O(n^2): 2.6 MB of bytes took over ten minutes. The scan below is ~65k
    // index reads; before the fix it copied ~2.1e9 elements. The generous
    // bound still separates linear behaviour from quadratic by orders of
    // magnitude, so it will not flake on a loaded host.
    let source = "fn scan(limit: i32) -> i32 effects [alloc]:\n    let values: Vec[i32] = vec_new()\n    let i: i32 = 0\n    while i < limit:\n        push(values, i)\n        i = i + 1\n    let total: i32 = 0\n    let at: usize = 0\n    let stop: usize = values.len\n    while at < stop:\n        total = total + values[at]\n        at = at + 1\n    return total\n";
    let program = parse_file(Path::new("scan.fgx"), source).expect("source should parse");
    assert!(check(&program).is_empty());

    let started = std::time::Instant::now();
    let output = run_entry_json(&program, "scan", r#"{ "args": { "limit": 32768 } }"#).unwrap();
    let elapsed = started.elapsed();

    // 0 + 1 + ... + 32767
    assert!(
        output.contains("536854528"),
        "unexpected scan result: {output}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "large-vector scan took {elapsed:?}, which suggests indexing regressed to O(n) per read"
    );
}
