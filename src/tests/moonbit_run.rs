//! MoonBit capsules. Refusals that need no toolchain run everywhere; the rest
//! return early without a MoonBit toolchain.

use crate::interp::{run_entry_with_capsules, Value};
use crate::moonbit_run::{discover_moonbit, invoke_live, run};
use crate::parser::parse_human;

use super::parse_fixture;

fn moonbit_available() -> bool {
    !discover_moonbit().is_empty()
}

fn probe_program(source_body: &str, header: &str) -> crate::ast::Program {
    parse_human(&format!(
        "foreign probe moonbit version \"0.10\" effects [{header}]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n{source_body}"
    ))
    .unwrap()
}

#[test]
fn moonbit_fixtures_run_in_the_wasm_sandbox() {
    let program = parse_fixture("examples/moonbit/checksum.fgx");
    let report = run("checksum.fgx", &program, Some("collatz_steps")).unwrap();
    assert_eq!(report.schema, "forgentex.moonbit_run.v1");
    assert_eq!(report.entry.as_deref(), Some("collatz_steps"));
    assert_eq!(report.total, 3);
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 3);
        assert!(report.runtime.as_deref().unwrap().starts_with("moonc 0."));
    } else {
        assert_eq!(report.status, "runtime_unavailable");
    }
}

#[test]
fn moonbit_live_call_takes_a_slice() {
    if !moonbit_available() {
        return;
    }
    let program = parse_fixture("examples/moonbit/checksum.fgx");
    let data = Value::Array((1..=5).map(Value::Int).collect());
    let (value, calls, err) = run_entry_with_capsules(&program, "checksum_of", vec![data]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    // Fletcher-16 of 1..=5: sum1 = 15, sum2 = 35.
    assert_eq!(value, Some(Value::Int((35 << 8) | 15)));
    assert_eq!(calls[0].language, "moonbit");
    assert_eq!(calls[0].decision, "allow");
}

#[test]
fn moonbit_boundary_values_round_trip_exactly() {
    if !moonbit_available() {
        return;
    }
    let program = parse_human(
        "foreign echo_all moonbit version \"0.10\" effects [alloc]:\n    input big: i64\n    input huge: u64\n    input ratio: f64\n    input flag: bool\n    input name: str\n    input rows: Vec[Vec[i32]]\n    output summary: str\n    sandbox \"fs.none\"\n    code:\n        fn echo_all(big : Int64, huge : UInt64, ratio : Double, flag : Bool, name : String, rows : Array[Array[Int]]) -> String {\n          big.to_string() + \"|\" + huge.to_string() + \"|\" + ratio.to_string() + \"|\" + flag.to_string() + \"|\" + name + \"|\" + rows[1][0].to_string()\n        }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let output = invoke_live(
        capsule,
        "[9007199254740993, 18446744073709551615, 0.25, true, \"h\\u00e9 \\\"q\\\"\", [[1], [-7, 8]]]",
    )
    .unwrap();
    assert_eq!(
        output,
        "\"9007199254740993|18446744073709551615|0.25|true|hé \\\"q\\\"|-7\""
    );
}

#[test]
fn moonbit_body_output_is_diagnostics_not_the_result() {
    if !moonbit_available() {
        return;
    }
    let program = probe_program(
        "        fn probe(x : Int64) -> Int64 {\n          println(\"working on \" + x.to_string())\n          x * 2L\n        }\n",
        "alloc",
    );
    let capsule = program.capsules().next().unwrap();
    assert_eq!(invoke_live(capsule, "[21]").unwrap(), "42");
}

#[test]
fn moonbit_runtime_faults_are_reported() {
    if !moonbit_available() {
        return;
    }
    let program = probe_program(
        "        fn probe(x : Int64) -> Int64 {\n          let values = [1L, 2L]\n          values[x.to_int()]\n        }\n",
        "alloc",
    );
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[9]").unwrap_err();
    assert!(err.starts_with("moonbit capsule 'probe' failed"), "{err}");
}

#[test]
fn moonbit_capsules_cannot_import_host_functions() {
    if !moonbit_available() {
        return;
    }
    // Reading the environment needs a host import the policy would mediate,
    // but a capsule must not get that far: its module is refused.
    let program = probe_program(
        "        fn probe(x : Int64) -> Int64 {\n          match @env.get_env_var(\"HOME\") {\n            Some(home) => home.length().to_int64() + x\n            None => x\n          }\n        }\n",
        "alloc",
    );
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[1]").unwrap_err();
    assert!(err.starts_with("moonbit_capsule_import_denied"), "{err}");
    assert!(err.contains("__moonbit_fs_unstable.get_env_var"), "{err}");

    // A hand-declared WASI import, which moonrun's policy does not cover.
    let program = probe_program(
        "        fn wasi_exit(code : Int) = \"wasi_snapshot_preview1\" \"proc_exit\"\n\n        fn probe(x : Int64) -> Int64 {\n          wasi_exit(0)\n          x\n        }\n",
        "alloc",
    );
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[1]").unwrap_err();
    assert!(err.contains("wasi_snapshot_preview1.proc_exit"), "{err}");
}

#[test]
fn moonbit_host_effects_are_refused_before_building() {
    let program = parse_human(
        "foreign reader moonbit version \"0.10\" effects [fs.read]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.read data/\"\n    code:\n        fn reader(x : Int64) -> Int64 {\n          x\n        }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[1]").unwrap_err();
    assert!(
        err.contains("requests host access (effect fs.read, grant fs.read)"),
        "{err}"
    );
}

#[test]
fn moonbit_struct_boundaries_are_refused_with_the_supported_types() {
    let program = parse_human(
        "struct Point:\n    x: f64\n    y: f64\n\nforeign norm moonbit version \"0.10\" effects [alloc]:\n    input p: Point\n    output len: f64\n    sandbox \"fs.none\"\n    code:\n        fn norm(p : Double) -> Double {\n          p\n        }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[{}]").unwrap_err();
    assert!(err.contains("input 'p' has type 'Point'"), "{err}");
}

#[test]
fn moonbit_versions_below_the_supported_floor_are_refused() {
    let program = parse_human(
        "foreign old moonbit version \"0.9\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        fn old(x : Int64) -> Int64 {\n          x\n        }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[1]").unwrap_err();
    assert!(
        err.contains("older than the oldest supported moonc (>=0.10.0)"),
        "{err}"
    );
}
