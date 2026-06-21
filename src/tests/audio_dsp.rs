use crate::interp::run_entry_json;
use crate::typecheck::check;

use super::parse_fixture;

#[test]
fn dsp_core_checks_clean() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let diagnostics = check(&program);
    assert!(
        diagnostics.is_empty(),
        "audio DSP core should check clean: {diagnostics:?}"
    );
}

#[test]
fn float_gain_runs_on_a_real_buffer() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let output = run_entry_json(
        &program,
        "apply_gain",
        r#"{"args": {"dst": [1000, -20000, 16000], "gain": 0.5},
            "expect": {"return": null, "bindings": {"dst": [500, -10000, 8000]}}}"#,
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""));
}

#[test]
fn q15_gain_is_bit_exact() {
    // gain_q15 = 16384 is exactly 0.5; pure integer math, no float rounding.
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let output = run_entry_json(
        &program,
        "apply_gain_q15",
        r#"{"args": {"dst": [20000, -30000, 8000], "gain_q15": 16384},
            "expect": {"return": null, "bindings": {"dst": [10000, -15000, 4000]}}}"#,
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""));
}

#[test]
fn q15_mix_saturates_exactly() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let output = run_entry_json(
        &program,
        "mix_q15",
        r#"{"args": {"dst": [0, 0], "a": [30000, 20000], "b": [10000, 1000], "gain_q15": 32768},
            "expect": {"return": null, "bindings": {"dst": [32767, 21000]}}}"#,
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""));
}

#[test]
fn interleave_and_peak_run() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let interleaved = run_entry_json(
        &program,
        "interleave_stereo",
        r#"{"args": {"dst": [0, 0, 0, 0], "left": [1, 2], "right": [3, 4]},
            "expect": {"return": null, "bindings": {"dst": [1, 3, 2, 4]}}}"#,
    )
    .unwrap();
    assert!(interleaved.contains("\"status\": \"pass\""));

    let peak = run_entry_json(
        &program,
        "peak",
        r#"{"args": {"xs": [1000, -20000, 16000, -30000]},
            "expect": {"return": 30000}}"#,
    )
    .unwrap();
    assert!(peak.contains("\"status\": \"pass\""));
}
