use std::path::PathBuf;

use crate::ast::CapsuleValue;
use crate::ffi::{
    invoke_live, invoke_live_audited, invoke_live_cached, parse_native_invocation, run,
    validate_native_toolchain_version, NativeRunnerCache, NativeValue,
};
use crate::native_build::Toolchain;
use crate::parser::parse_human;

use super::parse_fixture;

fn out_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_ffi_{tag}", std::process::id()))
}

#[test]
fn derives_signature_and_marshaling_and_gates_a_c_capsule() {
    let program = parse_fixture("examples/capsule/clamp_c.fgx");
    let plan = run("clamp_c.fgx", &program, None, &out_dir("plan")).unwrap();

    assert_eq!(plan.schema, "forgentex.ffi_plan.v1");
    assert_eq!(plan.language, "c");
    assert_eq!(plan.risk_class, "native");
    assert_eq!(plan.extern_c_signature, "int16_t clamp_sample(float x)");

    // Marshaling: f32 -> float (in), i16 -> int16_t (out).
    let input = plan
        .marshaling
        .iter()
        .find(|m| m.direction == "in")
        .unwrap();
    assert_eq!(input.c_type, "float");
    let output = plan
        .marshaling
        .iter()
        .find(|m| m.direction == "out")
        .unwrap();
    assert_eq!(output.c_type, "int16_t");

    // The contract passes check and pins its deps, so the gate allows it.
    assert_eq!(plan.gate_decision, "allow");
    assert!(plan.deps_pinned);

    // Either it really executed (compiler present) or there was no compiler.
    assert!(
        plan.is_success(),
        "unexpected status {}",
        plan.execution_status
    );
    if plan.execution_status == "executed" {
        assert_eq!(plan.passed, 3);
        assert_eq!(plan.total, 3);
        assert!(plan.transcript.iter().all(|r| r.pass));
        assert!(plan.toolchain.is_some());
        assert_eq!(plan.link_units.len(), 2);
    }
    let _ = std::fs::remove_dir_all(out_dir("plan"));
}

#[test]
fn managed_language_capsule_is_rejected() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let err = run("normalize_python.fgx", &program, None, &out_dir("managed")).unwrap_err();
    assert!(
        err.contains("managed"),
        "expected managed rejection, got: {err}"
    );
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/clamp_c.fgx");
    let json = run("clamp_c.fgx", &program, None, &out_dir("json"))
        .unwrap()
        .to_json();
    assert!(json.contains("\"schema\": \"forgentex.ffi_plan.v1\""));
    assert!(json.contains("\"extern_c_signature\": \"int16_t clamp_sample(float x)\""));
    assert!(json.contains("\"gate_decision\""));
    assert!(json.contains("\"transcript\""));
    let _ = std::fs::remove_dir_all(out_dir("json"));
}

#[test]
fn live_native_arguments_are_range_checked_before_compiler_lookup() {
    // This must be deterministic even on a machine without a C compiler. An
    // out-of-range i8 value is rejected by the live boundary before any host
    // toolchain lookup, source generation, or process spawn.
    let program = parse_human(
        "foreign narrow c version \"c11\" effects [cpu.avx2]:\n    input value: i8\n    output result: i8\n    sandbox \"fs.none\"\n    code:\n        signed char narrow(signed char value) { return value; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(128))],
    )
    .unwrap_err();
    assert!(err.contains("native_capsule_argument_range"), "got: {err}");
    assert!(err.contains("i8"), "got: {err}");
}

#[test]
fn live_native_mutable_slices_require_a_mutable_runtime_value() {
    // The live boundary distinguishes the two slice modes before compiler
    // lookup. A read-only value cannot be passed to a copy-back parameter.
    let program = parse_human(
        "foreign mutate c version \"c11\" effects [cpu.avx2]:\n    input samples: &mut [i16]\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int mutate(void) { return 0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(
        &program,
        capsule,
        &[NativeValue::ReadSlice(vec![CapsuleValue::Int(1)])],
    )
    .unwrap_err();
    assert!(err.contains("native_capsule_argument_type"), "got: {err}");
    assert!(err.contains("expects a mutable slice"), "got: {err}");
}

#[test]
fn read_slice_abi_derives_pointer_and_length() {
    let program = parse_fixture("examples/capsule/sum_i16_c.fgx");
    let plan = run("sum_i16_c.fgx", &program, None, &out_dir("read_slice")).unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "int32_t sum_i16(const int16_t *samples, size_t samples_len)"
    );
    let input = plan
        .marshaling
        .iter()
        .find(|marshal| marshal.direction == "in")
        .unwrap();
    assert_eq!(input.fgx_type, "&[i16]");
    assert_eq!(input.c_type, "const int16_t* + size_t");
    assert!(
        plan.is_success(),
        "unexpected status: {}",
        plan.execution_status
    );
    let _ = std::fs::remove_dir_all(out_dir("read_slice"));
}

#[test]
fn mutable_slice_abi_derives_writable_pointer_and_length() {
    let program = parse_fixture("examples/capsule/add_gain_i16_c.fgx");
    let plan = run("add_gain_i16_c.fgx", &program, None, &out_dir("mut_slice")).unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "int32_t add_gain_i16(int16_t *samples, size_t samples_len, int16_t gain)"
    );
    let input = plan
        .marshaling
        .iter()
        .find(|marshal| marshal.name == "samples")
        .unwrap();
    assert_eq!(input.fgx_type, "&mut[i16]");
    assert_eq!(input.c_type, "int16_t* + size_t (copy-back)");
    assert!(
        plan.is_success(),
        "unexpected status: {}",
        plan.execution_status
    );
    let _ = std::fs::remove_dir_all(out_dir("mut_slice"));
}

#[test]
fn live_native_result_is_validated_before_mutable_copy_back() {
    let program = parse_human(
        "foreign mutate c version \"c11\" effects [cpu.avx2]:\n    input samples: &mut [i16]\n    output result: f64\n    sandbox \"fs.none\"\n    code:\n        double mutate(short *samples, size_t samples_len) { return 0.0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let args = [NativeValue::MutSlice(vec![CapsuleValue::Int(1)])];

    // Even though the mutation envelope itself is well-formed, the entire
    // response is rejected because NaN is not a valid finite Forgentex/JSON
    // scalar. No NativeInvocation reaches the interpreter for copy-back.
    let err =
        parse_native_invocation("result=nan\nmut:0=99\n", &program, capsule, &args).unwrap_err();
    assert!(err.contains("native_capsule_bad_output"), "got: {err}");
    assert!(err.contains("non-finite"), "got: {err}");
}

#[test]
fn repr_c_struct_output_derives_a_by_value_return_signature() {
    let program = parse_fixture("examples/capsule/scene_metrics_c.fgx");
    let plan = run(
        "scene_metrics_c.fgx",
        &program,
        None,
        &out_dir("struct_output_signature"),
    )
    .unwrap();
    assert_eq!(plan.extern_c_signature, "SceneMetrics measure_scene(void)");
    assert!(plan.is_success(), "plan: {plan:?}");
    let _ = std::fs::remove_dir_all(out_dir("struct_output_signature"));
}

#[test]
fn live_native_struct_output_requires_repr_c_before_compiler_lookup() {
    let program = parse_human(
        "struct Metrics:\n    frames: i32\n\nforeign metrics c version \"c11\" effects [cpu]:\n    output value: Metrics\n    sandbox \"fs.none\"\n    code:\n        int unused(void) { return 0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(&program, capsule, &[]).unwrap_err();
    assert!(err.contains("@repr(C)"), "got: {err}");
}

#[test]
fn audited_native_preflight_failure_has_a_stable_non_payload_receipt() {
    let program = parse_human(
        "struct Metrics:\n    frames: i32\n\nforeign metrics c version \"c11\":\n    output value: Metrics\n    sandbox \"limit.cpu_ms 250\"\n    code:\n        int unused(void) { return 0; }\n",
    )
    .unwrap();
    let failure = invoke_live_audited(&program, program.capsules().next().unwrap(), &[])
        .expect_err("missing @repr(C) must fail before compiler lookup");
    assert!(failure.message.contains("@repr(C)"));
    assert_eq!(failure.receipt.schema, "forgentex.native_failure.v1");
    assert_eq!(failure.receipt.kind, "native_capsule_boundary_unsupported");
    assert_eq!(failure.receipt.phase, "preflight");
    assert_eq!(failure.receipt.cpu_limit_ms, Some(250));
    assert!(!failure.receipt.limits_applied);
    assert_eq!(failure.receipt.containment, None);
}

#[test]
fn malformed_native_struct_result_is_rejected_before_any_mutation_is_exposed() {
    let program = parse_fixture("examples/capsule/scene_metrics_c.fgx");
    let capsule = program.capsules().next().unwrap();
    let err = parse_native_invocation("result=24,180,nan\n", &program, capsule, &[]).unwrap_err();
    assert!(err.contains("native_capsule_bad_output"), "got: {err}");
    assert!(err.contains("non-finite"), "got: {err}");
}

#[test]
fn live_native_large_slice_uses_request_file_not_process_arguments() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/sum_i16_c.fgx");
    let capsule = program.capsules().next().unwrap();
    // Its wire representation is well over the usual Windows 32 KiB command
    // line limit, proving slice transport is independent of argv capacity.
    let values = vec![CapsuleValue::Int(1); 10_000];
    let invocation = invoke_live(&program, capsule, &[NativeValue::ReadSlice(values)]).unwrap();
    assert_eq!(invocation.output, "10000");
    assert!(invocation.mutations.is_empty());
}

#[test]
fn native_cpu_budget_terminates_busy_work_before_wall_fallback() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign burn_cpu c version \"c11\":\n    output result: i32\n    sandbox \"limit.cpu_ms 50\"\n    sandbox \"limit.wall_ms 3000\"\n    code:\n        int burn_cpu(void) { volatile unsigned long long n = 0; for (;;) { n++; } }\n",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(
        error.contains("native_capsule_failed") || error.contains("native_capsule_timeout"),
        "got: {error}"
    );
    // On a saturated parallel test host the child may receive less than 50 ms
    // of CPU before the wall fallback. Either bound must contain the work.
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
}

#[test]
fn cached_runner_keeps_per_call_timeout_and_survives_timed_out_process() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign maybe_spin c version \"c11\":\n    input spin: i32\n    output result: i32\n    sandbox \"limit.wall_ms 50\"\n    code:\n        int maybe_spin(int spin) { if (!spin) return 7; for (;;) {} }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let mut cache = NativeRunnerCache::default();
    let first = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(0))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(first.output, "7");
    assert!(!first.receipt.unwrap().runner_reused);

    let error = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(1))],
        &mut cache,
    )
    .unwrap_err();
    assert!(error.contains("native_capsule_timeout"), "got: {error}");

    let recovered = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(0))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(recovered.output, "7");
    assert!(recovered.receipt.unwrap().runner_reused);
}

#[test]
fn native_memory_budget_prevents_unbounded_committed_allocation() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign consume_memory c version \"c11\" effects [alloc]:\n    output result: i32\n    sandbox \"fs.none\"\n    sandbox \"limit.mem_mb 32\"\n    sandbox \"limit.wall_ms 3000\"\n    code:\n        #include <stdlib.h>\n        #include <string.h>\n        int consume_memory(void) { int count = 0; while (count < 256) { void *p = malloc(1024 * 1024); if (p == NULL) break; memset(p, 1, 1024 * 1024); count++; } return count; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    match invoke_live(&program, capsule, &[]) {
        Ok(invocation) => {
            let allocated = invocation.output.parse::<i32>().unwrap();
            assert!(allocated < 32, "memory budget allowed {allocated} MiB");
        }
        Err(error) => assert!(error.contains("native_capsule_failed"), "got: {error}"),
    };
}

#[test]
fn successful_native_receipt_records_applied_resource_budgets() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign budgeted c version \"c11\":\n    output result: i32\n    sandbox \"limit.cpu_ms 1000\"\n    sandbox \"limit.mem_mb 64\"\n    code:\n        int budgeted(void) { return 7; }\n",
    )
    .unwrap();
    let receipt = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .unwrap()
        .receipt
        .unwrap();
    assert_eq!(receipt.cpu_limit_ms, Some(1000));
    assert_eq!(receipt.memory_limit_bytes, Some(64 * 1024 * 1024));
}

#[test]
fn live_cpp_requires_explicit_c_abi_export_before_compiler_lookup() {
    let program = parse_human(
        "foreign unmangled cpp version \"c++17\" effects [cpu.avx2]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int unmangled(int value) { return value; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(7))],
    )
    .unwrap_err();
    assert!(
        err.contains("native_capsule_boundary_unsupported"),
        "got: {err}"
    );
    assert!(err.contains("extern \"C\""), "got: {err}");
}

#[test]
fn live_rust_scalar_boundary_compiles_runs_and_records_rustc() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/clamp_rust.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    assert_eq!(capsule.risk_class(), "native");
    let mut cache = NativeRunnerCache::default();
    let invocation = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(40_000))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(invocation.output, "32767");
    assert!(invocation.mutations.is_empty());
    let receipt = invocation.receipt.expect("Rust call needs native receipt");
    assert!(receipt.toolchain.contains("rustc"), "{receipt:?}");
    assert_eq!(receipt.boundary_hash.len(), 16);
    assert_eq!(receipt.containment, "process_tree");
    assert!(!receipt.runner_reused);
    let reused = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(-40_000))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(reused.output, "-32768");
    assert!(reused.receipt.unwrap().runner_reused);
}

#[test]
fn live_zig_scalar_boundary_compiles_runs_and_records_zig() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/clamp_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let mut cache = NativeRunnerCache::default();
    let invocation = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Float(40_000.0))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(invocation.output, "32767");
    assert!(invocation.mutations.is_empty());
    assert!(!invocation.receipt.as_ref().unwrap().runner_reused);
    let reused = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Float(-40_000.0))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(reused.output, "-32768");
    assert!(reused.receipt.unwrap().runner_reused);
    let receipt = invocation.receipt.expect("Zig call needs native receipt");
    assert!(receipt.toolchain.contains("zig"), "{receipt:?}");
    assert_eq!(receipt.boundary_hash.len(), 16);
}

#[test]
fn live_zig_requires_explicit_export_before_toolchain_lookup() {
    let program = parse_human(
        "foreign hidden zig version \"0.14\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub fn hidden(value: i32) i32 { return value; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(7))],
    )
    .unwrap_err();
    assert!(error.contains("pub export fn hidden"), "got: {error}");
}

#[test]
fn live_zig_rejects_non_numeric_slice_elements_before_toolchain_lookup() {
    let program = parse_human(
        "foreign any_zig zig version \"0.14\":\n    input values: &[bool]\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub export fn any_zig(values: [*]const bool, len: usize) i32 { _ = values; return @intCast(len); }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::ReadSlice(vec![CapsuleValue::Bool(true)])],
    )
    .unwrap_err();
    assert!(error.contains("numeric scalar slice"), "got: {error}");
}

#[test]
fn live_zig_reads_and_mutates_scalar_slices() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let read_program = parse_fixture("examples/capsule/sum_i16_zig.fgx");
    let read = invoke_live(
        &read_program,
        read_program.capsules().next().unwrap(),
        &[NativeValue::ReadSlice(vec![
            CapsuleValue::Int(1200),
            CapsuleValue::Int(-200),
            CapsuleValue::Int(3500),
        ])],
    )
    .unwrap();
    assert_eq!(read.output, "4500");
    assert!(read.mutations.is_empty());

    let mutate_program = parse_fixture("examples/capsule/add_gain_i16_zig.fgx");
    let mutate = invoke_live(
        &mutate_program,
        mutate_program.capsules().next().unwrap(),
        &[
            NativeValue::MutSlice(vec![CapsuleValue::Int(100), CapsuleValue::Int(-4)]),
            NativeValue::Scalar(CapsuleValue::Int(5)),
        ],
    )
    .unwrap();
    assert_eq!(mutate.output, "2");
    assert_eq!(mutate.mutations.len(), 1);
    assert_eq!(mutate.mutations[0].input_index, 0);
    assert_eq!(
        mutate.mutations[0].values,
        vec![CapsuleValue::Int(105), CapsuleValue::Int(1)]
    );
}

#[test]
fn live_zig_struct_boundary_compiles_and_validates_both_directions() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/render_params_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1280),
            CapsuleValue::Int(720),
            CapsuleValue::Float(1.25),
        ])],
    )
    .unwrap();
    assert_eq!(
        invocation.output,
        "{\"width\":1296,\"height\":729,\"exposure\":2.5}"
    );
    assert!(invocation.mutations.is_empty());
    assert!(invocation.receipt.unwrap().toolchain.contains("zig"));
}

#[test]
fn zig_struct_requires_matching_extern_source_before_toolchain_lookup() {
    let program = parse_human(
        "@repr(C)\nstruct Pair:\n    left: i32\n    right: i32\n\nforeign score_zig zig version \"0.14\":\n    input value: Pair\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub const Pair = struct { left: i32, right: i32, };\n        pub export fn score_zig(value: Pair) i32 { return value.left + value.right; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
        ])],
    )
    .unwrap_err();
    assert!(error.contains("extern struct"), "got: {error}");
}

#[test]
fn zig_struct_rejects_a_mismatched_field_layout_before_toolchain_lookup() {
    let program = parse_human(
        "@repr(C)\nstruct Pair:\n    left: i32\n    right: i64\n\nforeign score_zig zig version \"0.14\":\n    input value: Pair\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub const Pair = extern struct { right: i64, left: i32, };\n        pub export fn score_zig(value: Pair) i32 { return value.left; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
        ])],
    )
    .unwrap_err();
    assert!(error.contains("fields must exactly match"), "got: {error}");
}

#[test]
fn malformed_zig_struct_result_is_rejected() {
    let program = parse_fixture("examples/capsule/render_params_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let args = [NativeValue::Struct(vec![
        CapsuleValue::Int(1280),
        CapsuleValue::Int(720),
        CapsuleValue::Float(1.25),
    ])];
    let error =
        parse_native_invocation("result=1296,729,nan\n", &program, capsule, &args).unwrap_err();
    assert!(error.contains("non-finite"), "got: {error}");
}

#[test]
fn zig_struct_runner_constructs_and_serializes_exact_fields() {
    let program = parse_fixture("examples/capsule/render_params_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("const params: capsule.RenderParams"));
    assert!(runner.contains(".width = try parseArg(i32"));
    assert!(runner.contains("result.exposure"));
}

#[test]
fn zig_slice_runner_emits_pointer_length_and_mutation_envelope() {
    let program = parse_fixture("examples/capsule/add_gain_i16_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("parseSlice(i16"));
    assert!(runner.contains("samples.ptr, samples.len, gain"));
    assert!(runner.contains("mut:0="));
    assert!(runner.contains("SliceTooLarge"));
}

#[test]
fn live_zig_pins_the_reviewed_language_version() {
    let program = parse_human(
        "foreign identity_zig zig version \"0.15\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub export fn identity_zig(value: i32) i32 { return value; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(7))],
    )
    .unwrap_err();
    assert!(error.contains("pins Zig 0.14"), "got: {error}");
}

#[test]
fn live_zig_rejects_an_unpinned_host_toolchain_version() {
    let program = parse_fixture("examples/capsule/clamp_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let error = validate_native_toolchain_version(
        capsule,
        &Toolchain {
            name: "zig".to_string(),
            version: "0.15.2".to_string(),
        },
    )
    .unwrap_err();
    assert!(error.contains("version_mismatch"), "got: {error}");
}

#[test]
fn zig_runner_uses_private_request_transport_and_typed_parse() {
    let program = parse_fixture("examples/capsule/clamp_zig.fgx");
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("readFileAlloc"));
    assert!(runner.contains("parseArg(f32"));
    assert!(runner.contains("capsule.clamp_zig(x)"));
    assert!(runner.contains("result={}"));
}

#[test]
fn live_rust_requires_explicit_c_abi_before_toolchain_lookup() {
    let program = parse_human(
        "foreign hidden rust version \"2021\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub fn hidden(value: i32) -> i32 { value }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(7))],
    )
    .unwrap_err();
    assert!(error.contains("extern \"C\""), "got: {error}");
}

#[test]
fn live_rust_struct_boundary_compiles_and_validates_both_directions() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/render_params_rust.fgx");
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1280),
            CapsuleValue::Int(720),
            CapsuleValue::Float(1.25),
        ])],
    )
    .unwrap();
    assert_eq!(
        invocation.output,
        "{\"width\":1296,\"height\":729,\"exposure\":2.5}"
    );
    assert!(invocation.mutations.is_empty());
    assert!(invocation.receipt.unwrap().toolchain.contains("rustc"));
}

#[test]
fn rust_struct_requires_matching_repr_c_source_before_toolchain_lookup() {
    let program = parse_human(
        "@repr(C)\nstruct Pair:\n    left: i32\n    right: i32\n\nforeign score_rust rust version \"2021\":\n    input value: Pair\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub struct Pair { pub left: i32, pub right: i32 }\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn score_rust(value: Pair) -> i32 { value.left + value.right }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
        ])],
    )
    .unwrap_err();
    assert!(error.contains("matching #[repr(C)]"), "got: {error}");
}

#[test]
fn rust_struct_rejects_a_mismatched_field_layout_before_toolchain_lookup() {
    let program = parse_human(
        "@repr(C)\nstruct Pair:\n    left: i32\n    right: i64\n\nforeign score_rust rust version \"2021\":\n    input value: Pair\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #[repr(C)]\n        pub struct Pair { pub right: i64, pub left: i32 }\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn score_rust(value: Pair) -> i32 { value.left }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
        ])],
    )
    .unwrap_err();
    assert!(error.contains("fields must exactly match"), "got: {error}");
}

#[test]
fn malformed_rust_struct_result_is_rejected() {
    let program = parse_fixture("examples/capsule/render_params_rust.fgx");
    let capsule = program.capsules().next().unwrap();
    let args = [NativeValue::Struct(vec![
        CapsuleValue::Int(1280),
        CapsuleValue::Int(720),
        CapsuleValue::Float(1.25),
    ])];
    let error =
        parse_native_invocation("result=1296,729,nan\n", &program, capsule, &args).unwrap_err();
    assert!(error.contains("non-finite"), "got: {error}");
}

#[test]
fn live_rust_reads_and_mutates_scalar_slices() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let read_program =
        parse_human(include_str!("../../examples/capsule/sum_i16_rust.fgx")).unwrap();
    let read_capsule = read_program.capsules().next().unwrap();
    let read = invoke_live(
        &read_program,
        read_capsule,
        &[NativeValue::ReadSlice(vec![
            CapsuleValue::Int(1200),
            CapsuleValue::Int(-200),
            CapsuleValue::Int(3500),
        ])],
    )
    .unwrap();
    assert_eq!(read.output, "4500");
    assert!(read.mutations.is_empty());

    let mut_program =
        parse_human(include_str!("../../examples/capsule/add_gain_i16_rust.fgx")).unwrap();
    let mut_capsule = mut_program.capsules().next().unwrap();
    let changed = invoke_live(
        &mut_program,
        mut_capsule,
        &[
            NativeValue::MutSlice(vec![CapsuleValue::Int(10), CapsuleValue::Int(-20)]),
            NativeValue::Scalar(CapsuleValue::Int(5)),
        ],
    )
    .unwrap();
    assert_eq!(changed.output, "2");
    assert_eq!(changed.mutations.len(), 1);
    assert_eq!(
        changed.mutations[0].values,
        vec![CapsuleValue::Int(15), CapsuleValue::Int(-15)]
    );
}

#[test]
fn rust_slice_boundary_derives_the_same_pointer_length_c_abi() {
    let program = parse_human(include_str!("../../examples/capsule/sum_i16_rust.fgx")).unwrap();
    let plan = run(
        "sum_i16_rust.fgx",
        &program,
        None,
        &out_dir("rust_slice_signature"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "int32_t sum_i16_rust(const int16_t *samples, size_t samples_len)"
    );
    assert_eq!(plan.execution_status, "execution_unsupported_in_slice");
}

#[test]
fn c_rust_and_zig_owned_u8_outputs_cross_as_forgentex_owned_vecs() {
    let cases = [
        (
            "../../examples/capsule/make_bytes_c.fgx",
            include_str!("../../examples/capsule/make_bytes_c.fgx"),
            crate::native_build::detect_toolchain().is_some(),
        ),
        (
            "../../examples/capsule/make_bytes_rust.fgx",
            include_str!("../../examples/capsule/make_bytes_rust.fgx"),
            crate::native_build::detect_rust_toolchain().is_some(),
        ),
        (
            "../../examples/capsule/make_bytes_zig.fgx",
            include_str!("../../examples/capsule/make_bytes_zig.fgx"),
            crate::native_build::detect_zig_toolchain().is_some(),
        ),
    ];
    for (name, source, available) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let invocation = invoke_live(
            &program,
            capsule,
            &[NativeValue::Scalar(CapsuleValue::Int(4))],
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(invocation.output, "[1,2,3,4]");
        assert_eq!(invocation.owned_output_bytes, Some(4));
        let artifact = invocation.artifact.as_ref().expect("artifact receipt");
        assert_eq!(artifact.schema, "forgentex.native_artifact.v1");
        assert_eq!(artifact.media_type, "application/octet-stream");
        assert_eq!(artifact.bytes, 4);
        assert_eq!(artifact.sha256, crate::crypto::sha256_hex(&[1, 2, 3, 4]));
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(4)
        );
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.artifact.as_ref())
                .map(|artifact| artifact.sha256.as_str()),
            Some(crate::crypto::sha256_hex(&[1, 2, 3, 4]).as_str())
        );
    }
}

#[test]
fn c_rust_and_zig_owned_i16_outputs_are_typed_bounded_and_byte_accounted() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_samples_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_samples_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_samples_zig.fgx"),
        ),
    ];
    for (language, available, source) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let invocation = invoke_live(
            &program,
            capsule,
            &[NativeValue::Scalar(CapsuleValue::Int(4))],
        )
        .unwrap_or_else(|error| panic!("{language}: {error}"));
        assert_eq!(invocation.output, "[-200,-100,0,100]");
        assert_eq!(invocation.owned_output_bytes, Some(8));
        assert!(invocation.artifact.is_none());
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(8)
        );
    }
}

#[test]
fn c_rust_and_zig_owned_i32_outputs_are_typed_bounded_and_byte_accounted() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_values_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_values_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_values_zig.fgx"),
        ),
    ];
    for (language, available, source) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let invocation = invoke_live(
            &program,
            capsule,
            &[NativeValue::Scalar(CapsuleValue::Int(4))],
        )
        .unwrap_or_else(|error| panic!("{language}: {error}"));
        assert_eq!(invocation.output, "[-200000,-100000,0,100000]");
        assert_eq!(invocation.owned_output_bytes, Some(16));
        assert!(invocation.artifact.is_none());
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(16)
        );
    }
}

#[test]
fn c_rust_and_zig_owned_strings_are_utf8_validated_and_byte_accounted() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_greeting_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_greeting_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_greeting_zig.fgx"),
        ),
    ];
    for (language, available, source) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let invocation = invoke_live(&program, capsule, &[])
            .unwrap_or_else(|error| panic!("{language}: {error}"));
        assert_eq!(invocation.output, "\"Forgentex,\\n✓\"");
        assert_eq!(invocation.owned_output_bytes, Some(14));
        assert!(invocation.artifact.is_none());
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(14)
        );
    }
}

#[test]
fn owned_string_response_rejects_invalid_utf8() {
    let program = parse_human(
        "foreign bad_text c version \"c11\" effects [alloc]:\n    output text: str\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedStr bad_text(void) { FgxOwnedStr out = { NULL, 0, 0 }; return out; }\n        void bad_text_free(uint8_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let error = parse_native_invocation(
        "result=255\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap_err();
    assert!(error.contains("not valid UTF-8"), "got: {error}");
}

#[test]
fn empty_owned_string_returns_an_empty_language_value() {
    let program = parse_human(include_str!("../../examples/capsule/make_greeting_c.fgx")).unwrap();
    let invocation = parse_native_invocation(
        "result=\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap();
    assert_eq!(invocation.output, "\"\"");
    assert_eq!(invocation.owned_output_bytes, Some(0));
    assert!(invocation.artifact.is_none());
}

#[test]
fn invalid_utf8_is_copied_and_freed_before_parent_rejection() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign invalid_text c version \"c11\" effects [alloc]:\n    output text: str\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdint.h>\n        #include <stdlib.h>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) abort(); }\n        FgxOwnedStr invalid_text(void) { uint8_t *ptr = (uint8_t *)malloc(1); if (ptr == NULL) { FgxOwnedStr empty = { NULL, 0, 0 }; return empty; } ptr[0] = 255; atexit(verify_free_count); FgxOwnedStr out = { ptr, 1, 1 }; return out; }\n        void invalid_text_free(uint8_t *ptr, size_t len, size_t capacity) { (void)len; (void)capacity; ++free_count; free(ptr); }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("not valid UTF-8"), "got: {error}");
}

#[test]
fn owned_i16_response_rejects_out_of_range_elements() {
    let program = parse_human(
        "foreign bad_samples c version \"c11\" effects [alloc]:\n    output samples: Vec[i16]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedI16 bad_samples(void) { FgxOwnedI16 out = { NULL, 0, 0 }; return out; }\n        void bad_samples_free(int16_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let error = parse_native_invocation(
        "result=32768\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap_err();
    assert!(
        error.contains("outside -32768..32767 for i16"),
        "got: {error}"
    );
}

#[test]
fn owned_i32_response_rejects_out_of_range_elements() {
    let program = parse_human(
        "foreign bad_values c version \"c11\" effects [alloc]:\n    output values: Vec[i32]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedI32 bad_values(void) { FgxOwnedI32 out = { NULL, 0, 0 }; return out; }\n        void bad_values_free(int32_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let error = parse_native_invocation(
        "result=2147483648\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap_err();
    assert!(
        error.contains("outside -2147483648..2147483647 for i32"),
        "got: {error}"
    );
}

#[test]
fn artifact_policy_rejects_an_oversized_valid_owned_result() {
    let program = parse_human(
        "foreign tiny_artifact c version \"c11\":\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 2\"\n    code:\n        int tiny_artifact(void) { return 0; }\n",
    )
    .unwrap();
    let error = parse_native_invocation(
        "result=1,2,3\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap_err();
    assert!(error.contains("artifact_too_large"), "got: {error}");
    assert!(error.contains("declared limit is 2"), "got: {error}");
}

#[test]
fn governed_artifact_persistence_is_scoped_immutable_and_receipted() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let relative = format!(
        "target/test-artifacts/native-persist-{}.bin",
        std::process::id()
    );
    let source = format!(
        "foreign persist_test c version \"c11\" effects [alloc, fs.write]:\n    output bytes: Vec[u8]\n    sandbox \"fs.write target/test-artifacts\"\n    sandbox \"artifact.media_type application/octet-stream\"\n    sandbox \"artifact.max_bytes 4\"\n    sandbox \"artifact.path {relative}\"\n    code:\n        #include \"types.h\"\n        #include <stdlib.h>\n        FgxOwnedU8 persist_test(void) {{ uint8_t *ptr = (uint8_t *)malloc(3); ptr[0] = 7; ptr[1] = 8; ptr[2] = 9; FgxOwnedU8 out = {{ ptr, 3, 3 }}; return out; }}\n        void persist_test_free(uint8_t *ptr, size_t len, size_t capacity) {{ (void)len; (void)capacity; free(ptr); }}\n"
    );
    let target = std::path::Path::new(&relative);
    let _ = std::fs::remove_file(target);
    let program = parse_human(&source).unwrap();
    let capsule = program.capsules().next().unwrap();
    let first = invoke_live(&program, capsule, &[]).unwrap();
    assert_eq!(std::fs::read(target).unwrap(), vec![7, 8, 9]);
    assert_eq!(
        first
            .receipt
            .as_ref()
            .and_then(|receipt| receipt.artifact.as_ref())
            .and_then(|artifact| artifact.path.as_deref()),
        Some(relative.as_str())
    );

    // Re-publishing identical content is idempotent; different bytes fail
    // closed instead of overwriting an existing artifact.
    invoke_live(&program, capsule, &[]).unwrap();
    std::fs::write(target, [1_u8, 2, 3]).unwrap();
    let error = invoke_live(&program, capsule, &[]).unwrap_err();
    assert!(error.contains("artifact_exists"), "got: {error}");
    assert_eq!(std::fs::read(target).unwrap(), vec![1, 2, 3]);
    let _ = std::fs::remove_file(target);
}

#[test]
fn zig_owned_output_runner_copies_before_exactly_once_free() {
    let program = parse_human(include_str!("../../examples/capsule/make_bytes_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("result.ptr.?[0..result.len]"));
    assert!(runner.contains("defer capsule.make_bytes_zig_free"));
    assert_eq!(runner.matches("capsule.make_bytes_zig_free").count(), 2);
    assert!(runner.contains("InvalidOwnedBufferDescriptor"));
    assert!(runner.contains("result.len > 1048576"));
}

#[test]
fn zig_typed_owned_output_uses_element_count_limit_and_typed_copy() {
    let program = parse_human(include_str!("../../examples/capsule/make_values_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("const result: capsule.FgxOwnedI32"));
    assert!(runner.contains("allocator.alloc(i32, result.len)"));
    assert!(runner.contains("result.len > 16777216"));
    assert!(runner.contains("result.ptr.?[0..result.len]"));
    assert_eq!(runner.matches("capsule.make_values_zig_free").count(), 2);
}

#[test]
fn zig_owned_string_runner_uses_byte_limit_and_copy_before_free() {
    let program =
        parse_human(include_str!("../../examples/capsule/make_greeting_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("const result: capsule.FgxOwnedStr"));
    assert!(runner.contains("allocator.alloc(u8, result.len)"));
    assert!(runner.contains("result.len > 67108864"));
    assert!(runner.contains("result.ptr.?[0..result.len]"));
    assert_eq!(runner.matches("capsule.make_greeting_zig_free").count(), 2);
}

#[test]
fn zig_owned_output_requires_exact_descriptor_before_toolchain_lookup() {
    let program = parse_human(
        "foreign bad_zig_owned zig version \"0.14\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        pub export fn bad_zig_owned() i32 { return 0; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedU8"), "got: {error}");
}

#[test]
fn zig_typed_owned_output_rejects_a_mismatched_descriptor() {
    let program = parse_human(
        "foreign bad_zig_samples zig version \"0.14\" effects [alloc]:\n    output samples: Vec[i16]\n    sandbox \"fs.none\"\n    code:\n        pub const FgxOwnedI16 = extern struct { ptr: ?[*]u8, len: usize, capacity: usize, };\n        pub export fn bad_zig_samples() FgxOwnedI16 { return .{ .ptr = null, .len = 0, .capacity = 0 }; }\n        pub export fn bad_zig_samples_free(ptr: ?[*]u8, len: usize, capacity: usize) void { _ = ptr; _ = len; _ = capacity; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedI16"), "got: {error}");
    assert!(error.contains("Vec[i16]"), "got: {error}");
}

#[test]
fn zig_owned_string_rejects_a_mismatched_descriptor() {
    let program = parse_human(
        "foreign bad_zig_text zig version \"0.14\" effects [alloc]:\n    output text: str\n    sandbox \"fs.none\"\n    code:\n        pub const FgxOwnedStr = extern struct { ptr: ?[*]i16, len: usize, capacity: usize, };\n        pub export fn bad_zig_text() FgxOwnedStr { return .{ .ptr = null, .len = 0, .capacity = 0 }; }\n        pub export fn bad_zig_text_free(ptr: ?[*]i16, len: usize, capacity: usize) void { _ = ptr; _ = len; _ = capacity; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedStr"), "got: {error}");
    assert!(error.contains("Zig str output"), "got: {error}");
}

#[test]
fn owned_output_plan_derives_descriptor_and_deallocator_contract() {
    let program = parse_human(include_str!("../../examples/capsule/make_bytes_c.fgx")).unwrap();
    let plan = run(
        "make_bytes_c.fgx",
        &program,
        None,
        &out_dir("owned_output_plan"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "FgxOwnedU8 make_bytes_c(int32_t count)"
    );
    assert_eq!(plan.execution_status, "execution_unsupported_in_slice");
    assert!(plan
        .marshaling
        .iter()
        .any(|item| item.c_type.contains("required deallocator")));
    let _ = std::fs::remove_dir_all(out_dir("owned_output_plan"));
}

#[test]
fn owned_string_plan_derives_descriptor_deallocator_and_validation_contract() {
    let program = parse_human(include_str!("../../examples/capsule/make_greeting_c.fgx")).unwrap();
    let plan = run(
        "make_greeting_c.fgx",
        &program,
        None,
        &out_dir("owned_string_plan"),
    )
    .unwrap();
    assert_eq!(plan.extern_c_signature, "FgxOwnedStr make_greeting_c(void)");
    assert_eq!(plan.execution_status, "execution_unsupported_in_slice");
    assert!(plan.marshaling.iter().any(|item| {
        item.c_type
            .contains("FgxOwnedStr + required deallocator + UTF-8 validation")
    }));
    let _ = std::fs::remove_dir_all(out_dir("owned_string_plan"));
}

#[test]
fn empty_owned_output_accepts_only_the_zero_length_descriptor() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/make_bytes_c.fgx")).unwrap();
    let invocation = invoke_live(
        &program,
        program.capsules().next().unwrap(),
        &[NativeValue::Scalar(CapsuleValue::Int(0))],
    )
    .unwrap();
    assert_eq!(invocation.output, "[]");
    assert_eq!(invocation.owned_output_bytes, Some(0));
}

#[test]
fn malformed_owned_descriptor_is_freed_and_rejected_in_the_child() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign bad_owned c version \"c11\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedU8 bad_owned(void) { FgxOwnedU8 out = { NULL, 1, 1 }; return out; }\n        void bad_owned_free(uint8_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("native_capsule_failed"), "got: {error}");
    assert!(
        error.contains("invalid owned buffer descriptor"),
        "got: {error}"
    );
}

#[test]
fn oversized_owned_descriptor_is_rejected_before_copying() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign huge_owned c version \"c11\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdlib.h>\n        FgxOwnedU8 huge_owned(void) { FgxOwnedU8 out = { (uint8_t *)malloc(1), 67108865, 67108865 }; return out; }\n        void huge_owned_free(uint8_t *ptr, size_t len, size_t capacity) { (void)len; (void)capacity; free(ptr); }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(
        error.contains("invalid owned buffer descriptor"),
        "got: {error}"
    );
}

#[test]
fn owned_output_requires_the_derived_deallocator_export() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign missing_free c version \"c11\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedU8 missing_free(void) { FgxOwnedU8 out = { NULL, 0, 0 }; return out; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(
        error.contains("native_capsule_compile_failed"),
        "got: {error}"
    );
    assert!(error.contains("missing_free_free"), "got: {error}");
}

#[test]
fn owned_output_deallocator_runs_exactly_once() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign free_once c version \"c11\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdlib.h>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) abort(); }\n        FgxOwnedU8 free_once(void) { atexit(verify_free_count); uint8_t *ptr = (uint8_t *)malloc(1); ptr[0] = 9; FgxOwnedU8 out = { ptr, 1, 1 }; return out; }\n        void free_once_free(uint8_t *ptr, size_t len, size_t capacity) { (void)len; (void)capacity; ++free_count; free(ptr); }\n",
    )
    .unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap();
    assert_eq!(invocation.output, "[9]");
}

#[test]
fn crashing_owned_output_deallocator_is_contained() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign crash_free c version \"c11\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdlib.h>\n        FgxOwnedU8 crash_free(void) { uint8_t *ptr = (uint8_t *)malloc(1); ptr[0] = 1; FgxOwnedU8 out = { ptr, 1, 1 }; return out; }\n        void crash_free_free(uint8_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; abort(); }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("native_capsule_failed"), "got: {error}");
}

#[test]
fn rust_malformed_owned_descriptor_is_rejected_before_pointer_read() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign bad_rust_owned rust version \"2021\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #[repr(C)]\n        pub struct FgxOwnedU8 { pub ptr: *mut u8, pub len: usize, pub capacity: usize }\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn bad_rust_owned() -> FgxOwnedU8 { FgxOwnedU8 { ptr: std::ptr::null_mut(), len: 1, capacity: 1 } }\n        #[unsafe(no_mangle)]\n        pub unsafe extern \"C\" fn bad_rust_owned_free(ptr: *mut u8, len: usize, capacity: usize) { let _ = (ptr, len, capacity); }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(
        error.contains("invalid owned buffer descriptor"),
        "got: {error}"
    );
}

#[test]
fn cpp_slice_boundary_compiles_with_the_cpp_driver() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/add_gain_i16_cpp.fgx");
    let plan = run(
        "add_gain_i16_cpp.fgx",
        &program,
        None,
        &out_dir("cpp_slice"),
    )
    .unwrap();
    assert_eq!(plan.language, "cpp");
    assert_eq!(
        plan.extern_c_signature,
        "int32_t add_gain_i16_cpp(int16_t *samples, size_t samples_len)"
    );
    assert_eq!(plan.execution_status, "compiled");
    assert!(plan.toolchain.is_some());
    let _ = std::fs::remove_dir_all(out_dir("cpp_slice"));
}

#[test]
fn live_native_struct_requires_repr_c_before_compiler_lookup() {
    let program = parse_human(
        "struct Pixel:\n    x: i32\n\nforeign score c version \"c11\" effects [cpu.avx2]:\n    input pixel: Pixel\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int score(Pixel pixel) { return 0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let err = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![CapsuleValue::Int(1)])],
    )
    .unwrap_err();
    assert!(
        err.contains("native struct input 'Pixel' requires @repr(C)"),
        "got: {err}"
    );
}

#[test]
fn repr_c_struct_boundary_derives_a_value_parameter() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_fixture("examples/capsule/render_params_c.fgx");
    let plan = run(
        "render_params_c.fgx",
        &program,
        None,
        &out_dir("struct_value"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "int32_t score_render_params(RenderParams params)"
    );
    let input = plan
        .marshaling
        .iter()
        .find(|marshal| marshal.name == "params")
        .unwrap();
    assert_eq!(input.c_type, "RenderParams");
    assert_eq!(plan.execution_status, "compiled");
    let _ = std::fs::remove_dir_all(out_dir("struct_value"));
}
