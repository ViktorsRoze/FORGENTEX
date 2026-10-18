//! Julia capsules. Every test that needs a host Julia returns early without
//! one, like the other managed-runtime tests.

use crate::interp::{run_entry_with_capsules, run_entry_with_warm_capsules, Value};
use crate::julia_run::{discover_julia, invoke_live, run};
use crate::parser::parse_human;

use super::parse_fixture;

fn julia_available() -> bool {
    !discover_julia().is_empty()
}

fn capsule_program(source: &str) -> crate::ast::Program {
    parse_human(source).unwrap()
}

#[test]
fn julia_fixtures_run_on_the_host_julia() {
    let program = parse_fixture("examples/julia/norm.fgx");
    let report = run("norm.fgx", &program, Some("fahrenheit")).unwrap();
    assert_eq!(report.schema, "forgentex.julia_run.v1");
    assert_eq!(report.entry.as_deref(), Some("fahrenheit"));
    assert_eq!(report.total, 3);
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 3);
        assert!(report.runtime.as_deref().unwrap().starts_with("julia 1."));
    } else {
        assert_eq!(report.status, "runtime_unavailable");
    }
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.julia_run.v1\""));
}

#[test]
fn julia_live_call_converts_a_slice_and_returns_a_float() {
    if !julia_available() {
        return;
    }
    let program = parse_fixture("examples/julia/norm.fgx");
    let samples = Value::Array(vec![Value::Float(3.0), Value::Float(4.0)]);
    let (value, calls, err) = run_entry_with_capsules(&program, "signal_level", vec![samples]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    let Some(Value::Float(level)) = value else {
        panic!("expected a float, got {value:?}");
    };
    assert!((level - 12.5f64.sqrt()).abs() < 1e-12, "{level}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "julia");
    assert_eq!(calls[0].decision, "allow");
}

#[test]
fn julia_boundary_types_dispatch_and_round_trip_text() {
    if !julia_available() {
        return;
    }
    // Typed methods only: a JSON integer must arrive as an Int32, and the
    // string must survive escapes and non-ASCII text both ways.
    let program = capsule_program(
        "foreign shout julia version \"1.10\" effects [alloc]:\n    input text: str\n    input times: i32\n    output loud: str\n    sandbox \"fs.none\"\n    code:\n        shout(text::String, times::Int32)::String = repeat(uppercase(text) * \"!\", times)\n",
    );
    let capsule = program.capsules().next().unwrap();
    let output = invoke_live(capsule, "[\"h\\u00e9 \\\"q\\\"\\n\", 2]").unwrap();
    assert_eq!(output, "\"HÉ \\\"Q\\\"\\n!HÉ \\\"Q\\\"\\n!\"");
}

#[test]
fn julia_struct_inputs_arrive_as_named_tuples() {
    if !julia_available() {
        return;
    }
    let program = capsule_program(
        "struct Point:\n    x: f64\n    y: f64\n\nforeign length_of julia version \"1.10\" effects [alloc]:\n    input p: Point\n    output len: f64\n    sandbox \"fs.none\"\n    code:\n        length_of(p) = hypot(p.x, p.y)\n",
    );
    let capsule = program.capsules().next().unwrap();
    let output = invoke_live(
        capsule,
        "[{\"$struct\": \"Point\", \"x\": 3.0, \"y\": 4.0}]",
    )
    .unwrap();
    assert_eq!(output, "5.0");
}

#[test]
fn julia_body_output_is_diagnostics_not_the_result() {
    if !julia_available() {
        return;
    }
    let program = capsule_program(
        "foreign noisy julia version \"1.10\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        println(\"loading noisy\")\n        function noisy(x::Int64)\n            println(\"called with \", x)\n            print(\"partial \")\n            x + 1\n        end\n",
    );
    let capsule = program.capsules().next().unwrap();
    assert_eq!(invoke_live(capsule, "[41]").unwrap(), "42");
}

#[test]
fn julia_exceptions_are_reported_with_their_message() {
    if !julia_available() {
        return;
    }
    let program = capsule_program(
        "foreign strict julia version \"1.10\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        strict(x::Int64) = x > 0 ? x : throw(ArgumentError(\"x must be positive\"))\n",
    );
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[-1]").unwrap_err();
    assert!(err.starts_with("julia capsule 'strict' raised"), "{err}");
    assert!(err.contains("x must be positive"), "{err}");
}

#[test]
fn julia_versions_below_the_supported_floor_are_refused() {
    let program = capsule_program(
        "foreign old julia version \"0.7\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        old(x) = x\n",
    );
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(capsule, "[1]").unwrap_err();
    assert!(
        err.contains("older than the oldest supported Julia (>=1.10.0)"),
        "{err}"
    );
}

#[test]
fn julia_capsules_run_on_every_installed_series_they_pin() {
    // Several Julias may be installed side by side; `~X.Y` must run on X.Y.
    let mut series: Vec<(u64, u64)> = discover_julia()
        .iter()
        .filter_map(|julia| crate::toolchain_version::Version::extract(&julia.version))
        .filter(|version| crate::julia_run::julia_min_supported().matches(version))
        .map(|version| (version.major, version.minor))
        .collect();
    series.sort();
    series.dedup();
    for (major, minor) in series {
        let program = capsule_program(&format!(
            "foreign which julia version \"~{major}.{minor}\" effects [alloc]:\n    input x: i64\n    output v: str\n    sandbox \"fs.none\"\n    code:\n        which(x) = string(VERSION)\n"
        ));
        let capsule = program.capsules().next().unwrap();
        let output = invoke_live(capsule, "[0]").unwrap();
        assert!(
            output.starts_with(&format!("\"{major}.{minor}.")),
            "a ~{major}.{minor} capsule ran on {output}"
        );
    }
}

#[test]
fn julia_warm_worker_serves_calls_and_survives_an_exception() {
    if !julia_available() {
        return;
    }
    let program = capsule_program(
        "foreign safe_div julia version \"1.10\" effects [alloc]:\n    input a: i64\n    input b: i64\n    output q: i64\n    sandbox \"fs.none\"\n    code:\n        safe_div(a::Int64, b::Int64) = div(a, b)\n\nfn both(a: i64) -> i64:\n    let first: i64 = safe_div(a, 2)\n    let second: i64 = safe_div(a, 3)\n    return first + second\n\nfn boom(a: i64) -> i64:\n    return safe_div(a, 0)\n",
    );
    let (value, calls, err) = run_entry_with_warm_capsules(&program, "both", vec![Value::Int(12)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(10)));
    assert_eq!(calls.len(), 2);
    let (_value, _calls, err) = run_entry_with_warm_capsules(&program, "boom", vec![Value::Int(1)]);
    assert!(err.unwrap().contains("DivideError"));
}
