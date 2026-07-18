// rt.1 — the live capsule runtime. A running Forgentex program calls a capsule
// with real runtime values (not mock fixtures): the interpreter marshals the
// arguments across the typed boundary, dispatches to the host runtime, and
// returns the real result into program flow, recording an audit transcript.

use crate::interp::{
    run_entry_json, run_entry_with_capsule_effects, run_entry_with_capsules,
    run_entry_with_warm_capsules, Value,
};
use crate::parser::parse_human;

use super::parse_fixture;

// A pure-stdlib Python capsule plus a Forgentex function that calls it live.
const CELSIUS: &str = "foreign to_fahrenheit python version \"3.13\" env \"cpython-3.13\" effects [alloc]:\n    input celsius: f64\n    output fahrenheit: f64\n    sandbox \"fs.none\"\n    mock 0.0 -> 32.0\n    code:\n        def to_fahrenheit(celsius):\n            return celsius * 9.0 / 5.0 + 32.0\n\nfn convert(c: f64) -> f64:\n    return to_fahrenheit(c)\n";

// native-call v1: the checked C scalar ABI now participates in a live
// Forgentex run. The C body is compiled and invoked by an isolated child
// process; the result returns through the normal capsule-call path.
const CLAMP_C_LIVE: &str = "foreign clamp_sample c version \"c11\" effects [cpu.avx2]:\n    input x: f32\n    output y: i16\n    sandbox \"fs.none\"\n    mock 40000.0 -> 32767\n    code:\n        short clamp_sample(float x) {\n            if (x > 32767.0f) return 32767;\n            if (x < -32768.0f) return -32768;\n            return (short)x;\n        }\n\nfn clamp_live(x: f32) -> i16 effects [cpu.avx2]:\n    return clamp_sample(x)\n";

// native-call v1.1: a Forgentex-owned read-only slice becomes `const T *` plus
// `size_t len` in the C ABI. The child runner rebuilds a bounded temporary
// buffer; the C capsule cannot retain a pointer into interpreter memory.
const SUM_I16_C_LIVE: &str = "foreign sum_i16 c version \"c11\" effects [cpu.avx2]:\n    input samples: &[i16]\n    output total: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdint.h>\n        #include <stddef.h>\n        int32_t sum_i16(const int16_t *samples, size_t samples_len) {\n            int32_t total = 0;\n            for (size_t i = 0; i < samples_len; ++i) total += samples[i];\n            return total;\n        }\n\nfn sum_live(samples: &[i16]) -> i32 effects [cpu.avx2]:\n    return sum_i16(samples)\n";

// native-call v1.2: a mutable slice is copied into the child runner and copied
// back only after its structured response validates. The function reads the
// changed value after returning from C, proving the caller environment updated.
const GAIN_I16_C_LIVE: &str = "foreign add_gain_i16 c version \"c11\" effects [cpu.avx2]:\n    input samples: &mut [i16]\n    input gain: i16\n    output changed: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdint.h>\n        #include <stddef.h>\n        int32_t add_gain_i16(int16_t *samples, size_t samples_len, int16_t gain) {\n            for (size_t i = 0; i < samples_len; ++i) samples[i] += gain;\n            return (int32_t)samples_len;\n        }\n\nfn boost_then_first(samples: &mut [i16]) -> i16 effects [cpu.avx2]:\n    let changed: i32 = add_gain_i16(samples, 5)\n    return samples[0]\n";

// native-call v1.3: C++ joins the same child-process C ABI only with an
// explicit `extern "C"` export. The sample deliberately uses a mutable slice
// to prove C++ shares v1.2's bounded copy-back rules rather than a separate
// foreign-memory path.
const GAIN_I16_CPP_LIVE: &str = "foreign add_gain_i16_cpp cpp version \"c++17\" effects [cpu.avx2]:\n    input samples: &mut [i16]\n    output changed: i32\n    sandbox \"fs.none\"\n    code:\n        #include <cstddef>\n        #include <cstdint>\n        extern \"C\" std::int32_t add_gain_i16_cpp(std::int16_t *samples, std::size_t samples_len) {\n            for (std::size_t i = 0; i < samples_len; ++i) samples[i] += 7;\n            return static_cast<std::int32_t>(samples_len);\n        }\n\nfn boost_cpp_then_first(samples: &mut [i16]) -> i16 effects [cpu.avx2]:\n    let changed: i32 = add_gain_i16_cpp(samples)\n    return samples[0]\n";

// A direct child that never returns. `limit.wall_ms` must kill it rather than
// allowing native code to hold the Forgentex run indefinitely.
const SPIN_C_LIVE: &str = "foreign spin_forever c version \"c11\":\n    output result: i32\n    sandbox \"limit.wall_ms 50\"\n    code:\n        int spin_forever(void) { for (;;) {} }\n\nfn run_spin() -> i32:\n    return spin_forever()\n";

// A native abort must be contained to the generated child process and become
// a typed capsule error rather than terminating the Forgentex interpreter.
const ABORT_C_LIVE: &str = "foreign abort_now c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdlib.h>\n        int abort_now(void) { abort(); }\n\nfn run_abort() -> i32:\n    return abort_now()\n";

const CLAMP_RUST_LIVE: &str = include_str!("../../examples/capsule/clamp_rust.fgx");
const CLAMP_ZIG_LIVE: &str = include_str!("../../examples/capsule/clamp_zig.fgx");
const SUM_I16_ZIG_LIVE: &str = include_str!("../../examples/capsule/sum_i16_zig.fgx");
const GAIN_I16_ZIG_LIVE: &str = include_str!("../../examples/capsule/add_gain_i16_zig.fgx");
const SUM_I16_RUST_LIVE: &str = include_str!("../../examples/capsule/sum_i16_rust.fgx");
const GAIN_I16_RUST_LIVE: &str = include_str!("../../examples/capsule/add_gain_i16_rust.fgx");
const OWNED_C_LIVE: &str = include_str!("../../examples/capsule/make_bytes_c.fgx");
const OWNED_RUST_LIVE: &str = include_str!("../../examples/capsule/make_bytes_rust.fgx");
const OWNED_ZIG_LIVE: &str = include_str!("../../examples/capsule/make_bytes_zig.fgx");
const OWNED_I16_C_LIVE: &str = include_str!("../../examples/capsule/make_samples_c.fgx");
const OWNED_I16_RUST_LIVE: &str = include_str!("../../examples/capsule/make_samples_rust.fgx");
const OWNED_I16_ZIG_LIVE: &str = include_str!("../../examples/capsule/make_samples_zig.fgx");
const OWNED_I32_C_LIVE: &str = include_str!("../../examples/capsule/make_values_c.fgx");
const OWNED_I32_RUST_LIVE: &str = include_str!("../../examples/capsule/make_values_rust.fgx");
const OWNED_I32_ZIG_LIVE: &str = include_str!("../../examples/capsule/make_values_zig.fgx");
const OWNED_STRING_C_LIVE: &str = include_str!("../../examples/capsule/make_greeting_c.fgx");
const OWNED_STRING_RUST_LIVE: &str = include_str!("../../examples/capsule/make_greeting_rust.fgx");
const OWNED_STRING_ZIG_LIVE: &str = include_str!("../../examples/capsule/make_greeting_zig.fgx");
const STRUCT_RUST_LIVE: &str = include_str!("../../examples/capsule/render_params_rust.fgx");
const STRUCT_ZIG_LIVE: &str = include_str!("../../examples/capsule/render_params_zig.fgx");

const SPIN_RUST_LIVE: &str = "foreign spin_rust rust version \"2021\":\n    output result: i32\n    sandbox \"limit.wall_ms 50\"\n    code:\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn spin_rust() -> i32 { loop {} }\n\nfn run_rust_spin() -> i32:\n    return spin_rust()\n";

#[test]
fn capsule_call_is_first_class_under_the_type_checker() {
    // Calling a capsule like a function must not be an `unknown_function`; it
    // types as the capsule's output and its argument is checked against the
    // capsule's typed input.
    let diagnostics = crate::typecheck::check(&parse_human(CELSIUS).unwrap());
    assert!(
        !diagnostics.iter().any(|d| d.code == "unknown_function"),
        "capsule call should be recognized, got: {diagnostics:?}"
    );
}

#[test]
fn live_capsule_calls_require_the_runtime_capability() {
    // The default deterministic interpreter has no live capsule runtime, so a
    // capsule call traps rather than silently spawning a host process.
    let program = parse_human(CELSIUS).unwrap();
    let err = run_entry_json(
        &program,
        "convert",
        "{ \"args\": { \"c\": 100.0 }, \"expect\": { \"return\": 212.0 } }",
    )
    .unwrap_err();
    assert!(
        err.contains("capsule_call_not_permitted"),
        "expected a gated trap, got: {err}"
    );
}

#[test]
fn live_python_capsule_call_returns_a_real_result() {
    // Runtime-gated: skips when no host python is present. `detect_python`
    // honors $PYTHON, so CI can point it at any interpreter (e.g. the 3.13
    // "sound" env). When present, this is a real subprocess round-trip.
    if crate::python_run::detect_python().is_none() {
        return;
    }
    let program = parse_human(CELSIUS).unwrap();
    let (value, calls, err) =
        run_entry_with_capsules(&program, "convert", vec![Value::Float(100.0)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Float(212.0)));

    // The live call is recorded in the audit transcript.
    assert_eq!(calls.len(), 1, "one capsule call expected");
    assert_eq!(calls[0].capsule, "to_fahrenheit");
    assert_eq!(calls[0].language, "python");
    assert_eq!(calls[0].decision, "allow");
    assert_eq!(calls[0].inputs, "[100.0]");
    assert_eq!(calls[0].output, "212.0");
}

#[test]
fn live_c_capsule_call_returns_a_real_result() {
    // Host-gated like the existing C-ABI fixture runner. Without a C compiler,
    // native-call v1 reports a structured runtime-unavailable trap instead of
    // silently using a mock, so the portable suite skips this host proof.
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(CLAMP_C_LIVE).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );

    let (value, calls, err) =
        run_entry_with_capsules(&program, "clamp_live", vec![Value::Float(40000.0)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(32767)));
    assert_eq!(calls.len(), 1, "one native capsule call expected");
    assert_eq!(calls[0].capsule, "clamp_sample");
    assert_eq!(calls[0].language, "c");
    assert_eq!(calls[0].decision, "allow");
    assert_eq!(calls[0].inputs, "[40000.0]");
    assert_eq!(calls[0].output, "32767");
    let receipt = calls[0]
        .native
        .as_ref()
        .expect("native C calls must record a receipt");
    assert_eq!(receipt.schema, "forgentex.native_receipt.v1");
    assert_eq!(receipt.boundary_hash.len(), 16);
    assert!(receipt.toolchain.contains("gcc") || receipt.toolchain.contains("clang"));
    assert!(receipt.wall_limit_ms.is_none());
}

#[test]
fn live_native_call_redacts_selected_audit_inputs_without_changing_dispatch() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = CLAMP_C_LIVE.replace(
        "    sandbox \"fs.none\"",
        "    sandbox \"fs.none\"\n    sandbox \"audit.redact x\"",
    );
    let program = parse_human(&source).unwrap();
    let (value, calls, err) =
        run_entry_with_capsules(&program, "clamp_live", vec![Value::Float(40000.0)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(
        value,
        Some(Value::Int(32767)),
        "wire input must stay intact"
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].inputs, "[{\"redacted\":true}]");
    assert_eq!(calls[0].redacted_inputs, vec!["x".to_string()]);
    assert!(!calls[0].inputs.contains("40000"));
}

#[test]
fn warm_native_calls_reuse_compilation_but_keep_fresh_contained_processes() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = "foreign increment c version \"c11\" effects [cpu.avx2]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int increment(int value) { static int process_calls = 0; process_calls++; return value + process_calls; }\n\nfn twice(value: i32) -> i32 effects [cpu.avx2]:\n    let first: i32 = increment(value)\n    return increment(first)\n";
    let program = parse_human(source).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "diagnostics: {diagnostics:?}");
    let (value, calls, error) =
        run_entry_with_warm_capsules(&program, "twice", vec![Value::Int(10)]);
    assert!(error.is_none(), "unexpected error: {error:?}");
    // A persistent process would return 13 because its static counter reaches
    // two. Fresh runner processes each see one call, preserving per-call
    // containment while the compiled executable is reused.
    assert_eq!(value, Some(Value::Int(12)));
    assert_eq!(calls.len(), 2);
    let first = calls[0].native.as_ref().unwrap();
    let second = calls[1].native.as_ref().unwrap();
    assert!(!first.runner_reused);
    assert!(second.runner_reused);
    assert_eq!(first.containment, "process_tree");
    assert_eq!(second.containment, "process_tree");
}

#[test]
fn live_rust_capsule_returns_a_real_scalar_and_is_audited() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(CLAMP_RUST_LIVE).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    let (value, calls, error) =
        run_entry_with_capsules(&program, "clamp_with_rust", vec![Value::Int(40_000)]);
    assert!(error.is_none(), "unexpected: {error:?}");
    assert_eq!(value, Some(Value::Int(32_767)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "rust");
    assert_eq!(calls[0].decision, "allow");
    assert!(calls[0]
        .native
        .as_ref()
        .is_some_and(|receipt| receipt.toolchain.contains("rustc")));
}

#[test]
fn live_zig_capsule_returns_a_real_scalar_and_is_audited() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program = parse_human(CLAMP_ZIG_LIVE).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    let (value, calls, error) =
        run_entry_with_capsules(&program, "clamp_with_zig", vec![Value::Float(40_000.0)]);
    assert!(error.is_none(), "unexpected: {error:?}");
    assert_eq!(value, Some(Value::Int(32_767)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "zig");
    assert_eq!(calls[0].decision, "allow");
    assert!(calls[0]
        .native
        .as_ref()
        .is_some_and(|receipt| receipt.toolchain.contains("zig")));
}

#[test]
fn live_zig_effect_denial_happens_before_zig_dispatch() {
    let program = parse_human(CLAMP_ZIG_LIVE).unwrap();
    let (value, calls, error) = run_entry_with_capsule_effects(
        &program,
        "clamp_with_zig",
        vec![Value::Float(40_000.0)],
        vec!["alloc".to_string()],
    );
    assert!(value.is_none());
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("capsule_effect_denied")),
        "got: {error:?}"
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "zig");
    assert_eq!(calls[0].decision, "denied");
    assert!(calls[0].native.is_none());
}

#[test]
fn live_zig_capsule_reads_and_copies_back_scalar_slices() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let read_program = parse_human(SUM_I16_ZIG_LIVE).unwrap();
    let samples = Value::Array(vec![Value::Int(1200), Value::Int(-200), Value::Int(3500)]);
    let (value, calls, error) =
        run_entry_with_capsules(&read_program, "sum_with_zig", vec![samples]);
    assert!(error.is_none(), "unexpected: {error:?}");
    assert_eq!(value, Some(Value::Int(4500)));
    assert_eq!(calls[0].language, "zig");

    let mutate_program = parse_human(GAIN_I16_ZIG_LIVE).unwrap();
    let samples = Value::Array(vec![Value::Int(100), Value::Int(-4)]);
    let (value, calls, error) =
        run_entry_with_capsules(&mutate_program, "boost_zig_then_first", vec![samples]);
    assert!(error.is_none(), "unexpected: {error:?}");
    assert_eq!(value, Some(Value::Int(105)));
    assert_eq!(calls[0].language, "zig");
    assert_eq!(calls[0].decision, "allow");
}

#[test]
fn live_rust_effect_denial_happens_before_rustc_dispatch() {
    let program = parse_human(CLAMP_RUST_LIVE).unwrap();
    let (value, calls, error) = run_entry_with_capsule_effects(
        &program,
        "clamp_with_rust",
        vec![Value::Int(40_000)],
        vec!["alloc".to_string()],
    );
    assert!(value.is_none());
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("capsule_effect_denied")),
        "got: {error:?}"
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "rust");
    assert_eq!(calls[0].decision, "denied");
    assert!(calls[0].native.is_none());
}

#[test]
fn live_rust_capsule_honors_declared_wall_time_limit() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(SPIN_RUST_LIVE).unwrap();
    let (value, calls, error) = run_entry_with_capsules(&program, "run_rust_spin", vec![]);
    assert!(value.is_none());
    let error = error.expect("expected Rust timeout");
    assert!(error.contains("native_capsule_timeout"), "got: {error}");
    assert!(error.contains("limit.wall_ms 50"), "got: {error}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "rust");
    assert_eq!(calls[0].decision, "error");
}

#[test]
fn live_rust_capsule_reads_a_scalar_slice() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(SUM_I16_RUST_LIVE).unwrap();
    let samples = Value::Array(vec![Value::Int(1200), Value::Int(-200), Value::Int(3500)]);
    let (value, calls, error) = run_entry_with_capsules(&program, "sum_with_rust", vec![samples]);
    assert!(error.is_none(), "unexpected: {error:?}");
    assert_eq!(value, Some(Value::Int(4500)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "rust");
    assert!(calls[0].native.is_some());
}

#[test]
fn live_rust_capsule_copies_back_a_mutable_scalar_slice() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(GAIN_I16_RUST_LIVE).unwrap();
    let samples = Value::Array(vec![Value::Int(10), Value::Int(20), Value::Int(-30)]);
    let (value, calls, error) =
        run_entry_with_capsules(&program, "boost_rust_then_first", vec![samples]);
    assert!(error.is_none(), "unexpected: {error:?}");
    assert_eq!(value, Some(Value::Int(15)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "rust");
    assert!(calls[0].native.is_some());
}

#[test]
fn live_c_rust_and_zig_owned_outputs_return_arrays_and_receipt_lengths() {
    let cases = [
        (
            OWNED_C_LIVE,
            "bytes_from_c",
            "c",
            crate::native_build::detect_toolchain().is_some(),
        ),
        (
            OWNED_RUST_LIVE,
            "bytes_from_rust",
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
        ),
        (
            OWNED_ZIG_LIVE,
            "bytes_from_zig",
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
        ),
    ];
    for (source, entry, language, available) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let (value, calls, error) = run_entry_with_capsules(&program, entry, vec![Value::Int(4)]);
        assert!(error.is_none(), "unexpected: {error:?}");
        assert_eq!(
            value,
            Some(Value::Array(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]))
        );
        assert_eq!(calls[0].language, language);
        assert_eq!(
            calls[0]
                .native
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(4)
        );
        let artifact = calls[0]
            .native
            .as_ref()
            .and_then(|receipt| receipt.artifact.as_ref())
            .expect("native artifact receipt");
        assert_eq!(artifact.media_type, "application/octet-stream");
        assert_eq!(artifact.bytes, 4);
        assert_eq!(artifact.sha256.len(), 64);
    }
}

#[test]
fn live_c_rust_and_zig_owned_i16_outputs_return_typed_arrays() {
    let cases = [
        (
            OWNED_I16_C_LIVE,
            "samples_from_c",
            crate::native_build::detect_toolchain().is_some(),
        ),
        (
            OWNED_I16_RUST_LIVE,
            "samples_from_rust",
            crate::native_build::detect_rust_toolchain().is_some(),
        ),
        (
            OWNED_I16_ZIG_LIVE,
            "samples_from_zig",
            crate::native_build::detect_zig_toolchain().is_some(),
        ),
    ];
    for (source, entry, available) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let (value, calls, error) = run_entry_with_capsules(&program, entry, vec![Value::Int(4)]);
        assert!(error.is_none(), "unexpected: {error:?}");
        assert_eq!(
            value,
            Some(Value::Array(vec![
                Value::Int(-200),
                Value::Int(-100),
                Value::Int(0),
                Value::Int(100),
            ]))
        );
        assert_eq!(
            calls[0]
                .native
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(8)
        );
        assert!(calls[0]
            .native
            .as_ref()
            .is_some_and(|receipt| receipt.artifact.is_none()));
    }
}

#[test]
fn live_c_rust_and_zig_owned_i32_outputs_return_typed_arrays() {
    let cases = [
        (
            OWNED_I32_C_LIVE,
            "values_from_c",
            crate::native_build::detect_toolchain().is_some(),
        ),
        (
            OWNED_I32_RUST_LIVE,
            "values_from_rust",
            crate::native_build::detect_rust_toolchain().is_some(),
        ),
        (
            OWNED_I32_ZIG_LIVE,
            "values_from_zig",
            crate::native_build::detect_zig_toolchain().is_some(),
        ),
    ];
    for (source, entry, available) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let (value, calls, error) = run_entry_with_capsules(&program, entry, vec![Value::Int(4)]);
        assert!(error.is_none(), "unexpected: {error:?}");
        assert_eq!(
            value,
            Some(Value::Array(vec![
                Value::Int(-200000),
                Value::Int(-100000),
                Value::Int(0),
                Value::Int(100000),
            ]))
        );
        assert_eq!(
            calls[0]
                .native
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(16)
        );
        assert!(calls[0]
            .native
            .as_ref()
            .is_some_and(|receipt| receipt.artifact.is_none()));
    }
}

#[test]
fn live_c_rust_and_zig_owned_strings_return_validated_language_values() {
    let cases = [
        (
            OWNED_STRING_C_LIVE,
            "greeting_from_c",
            crate::native_build::detect_toolchain().is_some(),
        ),
        (
            OWNED_STRING_RUST_LIVE,
            "greeting_from_rust",
            crate::native_build::detect_rust_toolchain().is_some(),
        ),
        (
            OWNED_STRING_ZIG_LIVE,
            "greeting_from_zig",
            crate::native_build::detect_zig_toolchain().is_some(),
        ),
    ];
    for (source, entry, available) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let (value, calls, error) = run_entry_with_capsules(&program, entry, Vec::new());
        assert!(error.is_none(), "unexpected: {error:?}");
        assert_eq!(value, Some(Value::String("Forgentex,\n✓".to_string())));
        assert_eq!(
            calls[0]
                .native
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(14)
        );
        assert!(calls[0]
            .native
            .as_ref()
            .is_some_and(|receipt| receipt.artifact.is_none()));
    }
}

#[test]
fn owned_output_effect_denial_happens_before_native_dispatch() {
    let program = parse_human(OWNED_C_LIVE).unwrap();
    let (value, calls, error) = run_entry_with_capsule_effects(
        &program,
        "bytes_from_c",
        vec![Value::Int(4)],
        vec!["alloc".to_string()],
    );
    assert!(value.is_none());
    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.contains("capsule_effect_denied")),
        "got: {error:?}"
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].decision, "denied");
    assert!(calls[0].native.is_none());
}

#[test]
fn live_c_capsule_effect_denial_happens_before_native_dispatch() {
    // No C compiler is needed: the host effect gate denies this C capsule
    // before it reaches the native compiler or child runner.
    let source = CLAMP_C_LIVE.replace(
        "    sandbox \"fs.none\"",
        "    sandbox \"fs.none\"\n    sandbox \"audit.redact x\"",
    );
    let program = parse_human(&source).unwrap();
    let (value, calls, err) = run_entry_with_capsule_effects(
        &program,
        "clamp_live",
        vec![Value::Float(40000.0)],
        vec!["alloc".to_string()],
    );
    assert!(
        value.is_none(),
        "denied native work must not return a value"
    );
    let err = err.expect("expected a native capsule effect denial");
    assert!(err.contains("capsule_effect_denied"), "got: {err}");
    assert!(err.contains("cpu.avx2"), "got: {err}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "c");
    assert_eq!(calls[0].decision, "denied");
    assert_eq!(calls[0].inputs, "[{\"redacted\":true}]");
    assert_eq!(calls[0].redacted_inputs, vec!["x".to_string()]);
}

#[test]
fn live_c_capsule_honors_declared_wall_time_limit() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(SPIN_C_LIVE).unwrap();
    let (value, calls, err) = run_entry_with_capsules(&program, "run_spin", vec![]);
    assert!(value.is_none(), "timed-out native work must not return");
    let err = err.expect("expected a native timeout");
    assert!(err.contains("native_capsule_timeout"), "got: {err}");
    assert!(err.contains("limit.wall_ms 50"), "got: {err}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "spin_forever");
    assert_eq!(calls[0].decision, "error");
    assert!(calls[0].native.is_none());
    let receipt = calls[0]
        .native_failure
        .as_ref()
        .expect("timed-out native calls must record failure evidence");
    assert_eq!(receipt.schema, "forgentex.native_failure.v1");
    assert_eq!(receipt.kind, "native_capsule_timeout");
    assert_eq!(receipt.phase, "execute");
    assert_eq!(receipt.wall_limit_ms, Some(50));
    assert!(receipt.limits_applied);
    assert_eq!(receipt.containment.as_deref(), Some("process_tree"));
}

#[test]
fn live_c_capsule_receipt_is_rendered_in_json_audit() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(CLAMP_C_LIVE).unwrap();
    let json = crate::interp::run_entry_json_live(
        &program,
        "clamp_live",
        "{ \"args\": { \"x\": 40000.0 } }",
        None,
    )
    .unwrap();
    assert!(
        json.contains("\"schema\": \"forgentex.native_receipt.v1\""),
        "{json}"
    );
    assert!(json.contains("\"boundary_hash\""), "{json}");
    assert!(json.contains("\"toolchain\""), "{json}");
    assert!(json.contains("\"duration_ms\""), "{json}");
    assert!(json.contains("\"containment\": \"process_tree\""), "{json}");
    assert!(json.contains("\"cpu_limit_ms\": null"), "{json}");
    assert!(json.contains("\"memory_limit_bytes\": null"), "{json}");
    assert!(json.contains("\"runner_reused\": false"), "{json}");
}

#[test]
fn native_artifact_metadata_is_rendered_in_json_audit() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(OWNED_C_LIVE).unwrap();
    let json = crate::interp::run_entry_json_live(
        &program,
        "bytes_from_c",
        "{ \"args\": { \"count\": 4 } }",
        None,
    )
    .unwrap();
    assert!(
        json.contains("\"schema\": \"forgentex.native_artifact.v1\""),
        "{json}"
    );
    assert!(
        json.contains("\"media_type\": \"application/octet-stream\""),
        "{json}"
    );
    assert!(json.contains("\"bytes\": 4"), "{json}");
    assert!(json.contains("\"sha256\""), "{json}");
}

#[test]
fn live_c_capsule_crash_is_contained_and_audited() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(ABORT_C_LIVE).unwrap();
    let (value, calls, err) = run_entry_with_capsules(&program, "run_abort", vec![]);
    assert!(value.is_none(), "crashed native work must not return");
    let err = err.expect("expected a native crash error");
    assert!(err.contains("native_capsule_failed"), "got: {err}");
    assert!(
        err.contains("exit code") || err.contains("terminated by signal"),
        "crash status should be stable: {err}"
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "abort_now");
    assert_eq!(calls[0].decision, "error");
    let receipt = calls[0]
        .native_failure
        .as_ref()
        .expect("crashed native calls must record failure evidence");
    assert_eq!(receipt.kind, "native_capsule_failed");
    assert_eq!(receipt.phase, "execute");
    assert!(receipt.limits_applied);
    assert_eq!(receipt.containment.as_deref(), Some("process_tree"));
}

#[test]
fn redacted_native_failure_does_not_repeat_foreign_stderr() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = "foreign fail_secret c version \"c11\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    sandbox \"audit.redact value\"\n    code:\n        #include <stdio.h>\n        #include <stdlib.h>\n        int fail_secret(int value) { fprintf(stderr, \"%d\", value); abort(); }\n\nfn run_secret(value: i32) -> i32:\n    return fail_secret(value)\n";
    let program = parse_human(source).unwrap();
    let (value, calls, error) =
        run_entry_with_capsules(&program, "run_secret", vec![Value::Int(424242)]);
    assert!(value.is_none());
    let error = error.expect("crash must fail");
    assert!(error.contains("native_capsule_failed"), "got: {error}");
    assert!(
        !error.contains("424242"),
        "caller diagnostic leaked: {error}"
    );
    assert_eq!(calls[0].inputs, "[{\"redacted\":true}]");
    assert!(!calls[0].output.contains("424242"));
    assert_eq!(
        calls[0].output,
        "capsule diagnostic redacted by input policy"
    );
    assert_eq!(
        calls[0].native_failure.as_ref().unwrap().kind,
        "native_capsule_failed"
    );
}

#[test]
fn live_c_capsule_reads_a_scalar_slice() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(SUM_I16_C_LIVE).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );

    let samples = Value::Array(vec![Value::Int(1200), Value::Int(-200), Value::Int(3500)]);
    let (value, calls, err) = run_entry_with_capsules(&program, "sum_live", vec![samples]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(4500)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "sum_i16");
    assert_eq!(calls[0].language, "c");
    assert_eq!(calls[0].decision, "allow");
    assert_eq!(calls[0].inputs, "[[1200, -200, 3500]]");
    assert_eq!(calls[0].output, "4500");
}

#[test]
fn live_c_capsule_copies_back_a_mutable_scalar_slice() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(GAIN_I16_C_LIVE).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );

    let samples = Value::Array(vec![Value::Int(100), Value::Int(-4)]);
    let (value, calls, err) = run_entry_with_capsules(&program, "boost_then_first", vec![samples]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(105)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "add_gain_i16");
    assert_eq!(calls[0].language, "c");
    assert_eq!(calls[0].decision, "allow");
    assert_eq!(calls[0].inputs, "[[100, -4],5]");
    assert_eq!(calls[0].output, "2");
}

#[test]
fn live_cpp_capsule_copies_back_a_mutable_scalar_slice() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program = parse_human(GAIN_I16_CPP_LIVE).unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );

    let samples = Value::Array(vec![Value::Int(100), Value::Int(-4)]);
    let (value, calls, err) =
        run_entry_with_capsules(&program, "boost_cpp_then_first", vec![samples]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(107)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "add_gain_i16_cpp");
    assert_eq!(calls[0].language, "cpp");
    assert_eq!(calls[0].decision, "allow");
    assert_eq!(calls[0].inputs, "[[100, -4]]");
    assert_eq!(calls[0].output, "2");
}

#[test]
fn live_c_capsule_accepts_a_reviewed_repr_c_struct_input() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/render_params_c.fgx");
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
    let mut fields = std::collections::HashMap::new();
    fields.insert("width".to_string(), Value::Int(1280));
    fields.insert("height".to_string(), Value::Int(720));
    fields.insert("exposure".to_string(), Value::Float(3.5));
    let params = Value::Struct("RenderParams".to_string(), fields);
    let (value, calls, err) = run_entry_with_capsules(&program, "render_param_score", vec![params]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(2003)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "score_render_params");
    assert_eq!(calls[0].decision, "allow");
}

#[test]
fn live_c_capsule_returns_a_reviewed_repr_c_struct_by_value() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/scene_metrics_c.fgx");
    let (value, calls, err) = run_entry_with_capsules(&program, "make_scene_metrics", vec![]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    let Value::Struct(name, fields) = value.expect("struct result") else {
        panic!("native result was not a struct");
    };
    assert_eq!(name, "SceneMetrics");
    assert_eq!(fields.get("glyph_count"), Some(&Value::Int(24)));
    assert_eq!(fields.get("frame_count"), Some(&Value::Int(180)));
    assert_eq!(fields.get("exposure"), Some(&Value::Float(2.5)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].decision, "allow");
    assert!(calls[0].output.contains("\"glyph_count\":24"));
}

#[test]
fn live_rust_capsule_accepts_and_returns_a_reviewed_repr_c_struct_by_value() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(STRUCT_RUST_LIVE).unwrap();
    let mut fields = std::collections::HashMap::new();
    fields.insert("width".to_string(), Value::Int(1280));
    fields.insert("height".to_string(), Value::Int(720));
    fields.insert("exposure".to_string(), Value::Float(1.25));
    let params = Value::Struct("RenderParams".to_string(), fields);
    let (value, calls, err) =
        run_entry_with_capsules(&program, "adjust_render_params", vec![params]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    let Value::Struct(name, fields) = value.expect("struct result") else {
        panic!("native result was not a struct");
    };
    assert_eq!(name, "RenderParams");
    assert_eq!(fields.get("width"), Some(&Value::Int(1296)));
    assert_eq!(fields.get("height"), Some(&Value::Int(729)));
    assert_eq!(fields.get("exposure"), Some(&Value::Float(2.5)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "rust");
    assert_eq!(calls[0].decision, "allow");
}

#[test]
fn live_zig_capsule_accepts_and_returns_a_reviewed_repr_c_struct_by_value() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program = parse_human(STRUCT_ZIG_LIVE).unwrap();
    let mut fields = std::collections::HashMap::new();
    fields.insert("width".to_string(), Value::Int(1280));
    fields.insert("height".to_string(), Value::Int(720));
    fields.insert("exposure".to_string(), Value::Float(1.25));
    let params = Value::Struct("RenderParams".to_string(), fields);
    let (value, calls, err) =
        run_entry_with_capsules(&program, "adjust_render_params_with_zig", vec![params]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    let Value::Struct(name, fields) = value.expect("struct result") else {
        panic!("native result was not a struct");
    };
    assert_eq!(name, "RenderParams");
    assert_eq!(fields.get("width"), Some(&Value::Int(1296)));
    assert_eq!(fields.get("height"), Some(&Value::Int(729)));
    assert_eq!(fields.get("exposure"), Some(&Value::Float(2.5)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "zig");
    assert_eq!(calls[0].decision, "allow");
}

// A node capsule, proving the live dispatch generalizes beyond one language.
const TAX_NODE: &str = "foreign add_tax node version \"node-24\" effects [alloc]:\n    input amount: i32\n    output total: i32\n    sandbox \"fs.none\"\n    mock 100 -> 120\n    code:\n        export function add_tax(amount) {\n            return Math.round(amount * 1.2);\n        }\n\nfn checkout(price: i32) -> i32:\n    return add_tax(price)\n";

#[test]
fn live_node_capsule_call_returns_a_real_result() {
    // Runtime-gated: skips when no host node is present (honors $NODE).
    if crate::node_run::detect_node().is_none() {
        return;
    }
    let program = parse_human(TAX_NODE).unwrap();
    let (value, calls, err) = run_entry_with_capsules(&program, "checkout", vec![Value::Int(100)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(120)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "add_tax");
    assert_eq!(calls[0].language, "node");
    assert_eq!(calls[0].decision, "allow");
}

// A go capsule, proving the uniform JSON boundary reaches a compiled,
// statically-typed runtime (the driver unmarshals each typed argument).
const SCALE_GO: &str = "foreign scale go version \"1.26\" effects [alloc]:\n    input value: i64\n    output doubled: i64\n    sandbox \"fs.none\"\n    mock 21 -> 42\n    code:\n        func scale(value int64) int64 {\n            return value * 2\n        }\n\nfn double(n: i64) -> i64:\n    return scale(n)\n";

#[test]
fn live_go_capsule_call_returns_a_real_result() {
    // Runtime-gated: skips when no host go toolchain is present (honors $GO).
    if crate::go_run::detect_go().is_none() {
        return;
    }
    let program = parse_human(SCALE_GO).unwrap();
    let (value, calls, err) = run_entry_with_capsules(&program, "double", vec![Value::Int(21)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(42)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "scale");
    assert_eq!(calls[0].language, "go");
    assert_eq!(calls[0].decision, "allow");
}

// A JVM capsule (compiled, no stdlib JSON): the type-directed path. Numeric
// output prints as valid JSON directly.
const TAX_JAVA: &str = "foreign cents_with_tax java version \"21\" env \"temurin-21\" effects [alloc]:\n    input subtotal_cents: i64\n    output total_cents: i64\n    sandbox \"fs.none\"\n    mock 10000 -> 10825\n    code:\n        public final class OrderTotal {\n            public static long centsWithTax(long subtotalCents) {\n                long tax = subtotalCents * 825L / 10000L;\n                return subtotalCents + tax;\n            }\n        }\n\nfn total(cents: i64) -> i64:\n    return cents_with_tax(cents)\n";

// A JVM capsule with a String output, exercising the driver's JSON quoting.
const BAND_JAVA: &str = "foreign band java version \"21\" effects [alloc]:\n    input score: i32\n    output label: str\n    sandbox \"fs.none\"\n    mock 95 -> \"high\"\n    code:\n        public final class Band {\n            public static String band(int score) {\n                return score >= 80 ? \"high\" : score >= 30 ? \"mid\" : \"low\";\n            }\n        }\n\nfn classify(s: i32) -> str:\n    return band(s)\n";

#[test]
fn live_jvm_capsule_call_returns_a_numeric_result() {
    // Runtime-gated: skips when no host JDK is present.
    if crate::jvm_run::detect_jdk().is_none() {
        return;
    }
    let program = parse_human(TAX_JAVA).unwrap();
    let (value, calls, err) = run_entry_with_capsules(&program, "total", vec![Value::Int(10000)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(10825)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].capsule, "cents_with_tax");
    assert_eq!(calls[0].language, "java");
}

#[test]
fn live_jvm_capsule_call_returns_a_string_result() {
    // The String output must come back through the JSON boundary, so the
    // generated Java driver quotes it.
    if crate::jvm_run::detect_jdk().is_none() {
        return;
    }
    let program = parse_human(BAND_JAVA).unwrap();
    let (value, _calls, err) = run_entry_with_capsules(&program, "classify", vec![Value::Int(95)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::String("high".to_string())));
}

// A capsule whose output bundles several values into a struct — the practical
// "multi-output" boundary. The host returns a JSON object; the runtime tags it
// with the declared struct type so it is used by field access in typed code.
const STATS_PY: &str = "struct Stats:\n    total: i32\n    count: i32\n\nforeign summarize python version \"3.13\" effects [alloc]:\n    input n: i32\n    output stats: Stats\n    sandbox \"fs.none\"\n    code:\n        def summarize(n):\n            return {\"total\": n * 10, \"count\": n}\n\nfn analyze(n: i32) -> i32:\n    let s: Stats = summarize(n)\n    return s.total + s.count\n\nfn get_stats(n: i32) -> Stats:\n    return summarize(n)\n";

#[test]
fn struct_output_capsule_type_checks() {
    // The struct-returning capsule call types as its declared struct, and field
    // access on the result is well-typed — no diagnostics.
    let diagnostics = crate::typecheck::check(&parse_human(STATS_PY).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn live_capsule_returns_a_struct_output() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    let program = parse_human(STATS_PY).unwrap();
    // analyze bundles the struct's fields back into one number: 3*10 + 3 = 33.
    let (value, _calls, err) = run_entry_with_capsules(&program, "analyze", vec![Value::Int(3)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(33)));

    // Returned directly, the value is the declared struct, retagged from the
    // generic JSON object, with its fields populated from the host.
    let (stats, _calls, err) = run_entry_with_capsules(&program, "get_stats", vec![Value::Int(4)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    match stats {
        Some(Value::Struct(name, fields)) => {
            assert_eq!(name, "Stats");
            assert_eq!(fields.get("total"), Some(&Value::Int(40)));
            assert_eq!(fields.get("count"), Some(&Value::Int(4)));
        }
        other => panic!("expected a Stats struct, got: {other:?}"),
    }
}

// A C# capsule: the same type-directed boundary as the JVM. Wired as a
// best-effort placeholder — this test is gated on a host .NET SDK and skips
// where none is installed (so it does not falsely pass as verified).
const DISCOUNT_CSHARP: &str = "foreign apply_discount csharp version \"netcoreapp\" effects [alloc]:\n    input price_cents: i64\n    output discounted_cents: i64\n    sandbox \"fs.none\"\n    mock 10000 -> 9000\n    code:\n        public static class Pricing {\n            public static long ApplyDiscount(long priceCents) {\n                return priceCents - priceCents / 10;\n            }\n        }\n\nfn checkout(cents: i64) -> i64:\n    return apply_discount(cents)\n";

#[test]
fn live_csharp_capsule_call_returns_a_real_result() {
    // Gated on a host .NET SDK; skips when absent (e.g. this dev box).
    if crate::dotnet_run::detect_dotnet().is_none() {
        return;
    }
    let program = parse_human(DISCOUNT_CSHARP).unwrap();
    let (value, calls, err) =
        run_entry_with_capsules(&program, "checkout", vec![Value::Int(10000)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Int(9000)));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].language, "csharp");
}

// A capsule declaring an effect (net.write) the caller may withhold.
const NET_PY: &str = "foreign fetch python version \"3.13\" effects [alloc, net.write]:\n    input url: str\n    output body: str\n    sandbox \"fs.none\"\n    code:\n        def fetch(url):\n            return \"ok\"\n\nfn get(u: str) -> str:\n    return fetch(u)\n";

#[test]
fn capsule_effect_gate_denies_ungranted_effects() {
    // No host runtime needed: the effect gate trips before any dispatch, so a
    // capsule needing net.write is denied when the caller grants only alloc.
    let program = parse_human(NET_PY).unwrap();
    let (value, calls, err) = run_entry_with_capsule_effects(
        &program,
        "get",
        vec![Value::String("http://x".to_string())],
        vec!["alloc".to_string()],
    );
    assert!(value.is_none(), "the call must not run");
    let err = err.expect("expected a denial");
    assert!(err.contains("capsule_effect_denied"), "got: {err}");
    assert!(
        err.contains("net.write"),
        "should name the denied effect: {err}"
    );
    // The denial is still audited.
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].decision, "denied");
}

#[test]
fn capsule_effect_gate_allows_granted_effects() {
    // The Celsius capsule declares only `alloc`; granting it lets the call run.
    if crate::python_run::detect_python().is_none() {
        return;
    }
    let program = parse_human(CELSIUS).unwrap();
    let (value, calls, err) = run_entry_with_capsule_effects(
        &program,
        "convert",
        vec![Value::Float(100.0)],
        vec!["alloc".to_string()],
    );
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Float(212.0)));
    assert_eq!(calls[0].decision, "allow");
}

#[test]
fn run_entry_json_live_emits_result_and_audit_transcript() {
    // The `fgx run --live` path: a result JSON with the capsule_calls transcript.
    if crate::python_run::detect_python().is_none() {
        return;
    }
    let program = parse_human(CELSIUS).unwrap();
    let json = crate::interp::run_entry_json_live(
        &program,
        "convert",
        "{ \"args\": { \"c\": 100.0 } }",
        None,
    )
    .unwrap();
    assert!(json.contains("\"return\": 212.0"), "{json}");
    assert!(json.contains("\"capsule_calls\""), "{json}");
    assert!(json.contains("to_fahrenheit"), "{json}");
}
