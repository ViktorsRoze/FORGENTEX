use std::path::Path;

use crate::interp::{run_demo, run_entry_json};
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
