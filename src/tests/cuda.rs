//! Live CUDA capsules. Host-independent refusals run everywhere; the GPU tests
//! run only where an NVIDIA GPU and a CUDA toolkit able to target it exist, and
//! otherwise return early, like the other toolchain-dependent native tests.

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::cuda::{gpu_devices, GpuTarget};
use crate::ffi::{invoke_live, invoke_live_cached, NativeRunnerCache, NativeValue};
use crate::parser::parse_human;

const SAXPY: &str = include_str!("../../examples/capsule/saxpy_live_cuda.fgx");

/// Whether `capsule` can run here: a GPU its target runs on, and a toolkit
/// inside its declaration.
fn runnable(capsule: &Capsule) -> bool {
    let Some(target) = GpuTarget::of(capsule) else {
        return false;
    };
    let Some(device) = gpu_devices().first() else {
        return false;
    };
    let want = crate::cuda::capsule_toolkit_want(capsule);
    target.runs_on(device.arch)
        && crate::native_build::resolve_cuda_toolchain(&want).is_some_and(|toolchain| {
            crate::toolchain_version::Version::extract(&toolchain.version)
                .is_some_and(|version| want.matches(&version))
        })
}

fn floats(values: &[f64]) -> Vec<CapsuleValue> {
    values.iter().copied().map(CapsuleValue::Float).collect()
}

fn saxpy_args() -> Vec<NativeValue> {
    vec![
        NativeValue::Scalar(CapsuleValue::Float(2.0)),
        NativeValue::ReadSlice(floats(&[1.0, 2.0, 3.0, 4.0])),
        NativeValue::MutSlice(floats(&[10.0, 20.0, 30.0, 40.0])),
    ]
}

fn first_capsule(program: &Program) -> &Capsule {
    program.capsules().next().unwrap()
}

/// A one-capsule CUDA program with the given header pieces and body.
fn cuda_program(version: &str, env: &str, effects: &str, sandbox: &str, body: &str) -> Program {
    parse_human(&format!(
        "foreign probe cuda version \"{version}\" env \"{env}\" effects [{effects}]:\n    input n: i32\n    output y: i32\n{sandbox}    code:\n{body}"
    ))
    .unwrap()
}

const IDENTITY_BODY: &str = "        extern \"C\" int probe(int n) { return n; }\n";

#[test]
fn cuda_declarations_are_refused_before_any_host_probe() {
    let refuse = |program: &Program| {
        invoke_live(
            program,
            first_capsule(program),
            &[NativeValue::Scalar(CapsuleValue::Int(1))],
        )
        .unwrap_err()
    };
    let grant = "    sandbox \"gpu.compute device:0\"\n";

    let not_a_toolkit = cuda_program(
        "c++17",
        "cuda-13-sm_120",
        "gpu.compute",
        grant,
        IDENTITY_BODY,
    );
    assert!(
        refuse(&not_a_toolkit).contains("declares an unsupported toolkit"),
        "{}",
        refuse(&not_a_toolkit)
    );

    let too_old = cuda_program(
        "cuda-10.2",
        "cuda-10-sm_75",
        "gpu.compute",
        grant,
        IDENTITY_BODY,
    );
    assert!(refuse(&too_old).contains("older than the oldest supported toolkit (>=11.0.0)"));

    let no_c_linkage = cuda_program(
        "cuda-13.0",
        "cuda-13-sm_120",
        "gpu.compute",
        grant,
        "        int probe(int n) { return n; }\n",
    );
    assert!(refuse(&no_c_linkage).contains(
        "CUDA capsule 'probe' must define its boundary function with explicit extern \"C\" linkage"
    ));

    if cfg!(target_os = "linux") {
        let no_effect = cuda_program(
            "cuda-13.0",
            "cuda-13-sm_120",
            "alloc",
            "    sandbox \"fs.none\"\n",
            IDENTITY_BODY,
        );
        assert!(
            refuse(&no_effect).contains("must declare the gpu.compute effect"),
            "{}",
            refuse(&no_effect)
        );
        let bad_scope = cuda_program(
            "cuda-13.0",
            "cuda-13-sm_120",
            "gpu.compute",
            "    sandbox \"gpu.compute all\"\n",
            IDENTITY_BODY,
        );
        assert!(
            refuse(&bad_scope).contains("expected device:N"),
            "{}",
            refuse(&bad_scope)
        );
    } else {
        let program = cuda_program(
            "cuda-13.0",
            "cuda-13-sm_120",
            "gpu.compute",
            grant,
            IDENTITY_BODY,
        );
        assert!(refuse(&program).contains("live CUDA capsules run on Linux hosts"));
    }
}

#[test]
fn gpu_compute_stays_refused_for_non_cuda_native_capsules() {
    let program = parse_human(
        "foreign probe c version \"c11\" effects [gpu.compute]:\n    input n: i32\n    output y: i32\n    sandbox \"gpu.compute device:0\"\n    code:\n        int probe(int n) { return n; }\n",
    )
    .unwrap();
    let err = invoke_live(
        &program,
        first_capsule(&program),
        &[NativeValue::Scalar(CapsuleValue::Int(1))],
    )
    .unwrap_err();
    assert!(
        err.starts_with("native_capsule_capability_unsupported"),
        "{err}"
    );
    assert!(err.contains("gpu.compute"), "{err}");
}

#[test]
fn live_cuda_saxpy_runs_on_the_gpu_and_copies_back() {
    let program = parse_human(SAXPY).unwrap();
    let capsule = first_capsule(&program);
    if !runnable(capsule) {
        return;
    }
    let invocation = invoke_live(&program, capsule, &saxpy_args()).unwrap();
    assert_eq!(invocation.output, "true");
    assert_eq!(invocation.mutations.len(), 1);
    assert_eq!(invocation.mutations[0].input_index, 2);
    assert_eq!(
        invocation.mutations[0].values,
        floats(&[12.0, 24.0, 36.0, 48.0])
    );

    let receipt = invocation.receipt.unwrap();
    assert!(receipt.toolchain.contains("nvcc"), "{}", receipt.toolchain);
    assert!(receipt.toolchain_semver.unwrap().starts_with("13."));
    assert_eq!(receipt.toolchain_requirement.as_deref(), Some("13.x"));
    assert_eq!(receipt.memory_limit_bytes, Some(512 * 1024 * 1024));
    assert_eq!(receipt.memory_limit_source, "declared");
    let gpu = receipt.gpu.unwrap();
    let device = &gpu_devices()[0];
    assert_eq!(gpu.target, "sm_120");
    assert_eq!(gpu.device_index, 0);
    assert_eq!(gpu.device_name, device.name);
    assert_eq!(gpu.compute_capability, device.compute_capability);
    assert_eq!(gpu.visible_devices, "0");
    assert_eq!(gpu.host_memory_limit, "data_segment");
    assert_eq!(gpu.jit_cache, "disabled");
}

#[test]
fn live_cuda_runner_is_reused_from_the_warm_cache() {
    let program = parse_human(SAXPY).unwrap();
    let capsule = first_capsule(&program);
    if !runnable(capsule) {
        return;
    }
    let mut cache = NativeRunnerCache::default();
    let first = invoke_live_cached(&program, capsule, &saxpy_args(), &mut cache).unwrap();
    let second = invoke_live_cached(&program, capsule, &saxpy_args(), &mut cache).unwrap();
    assert_eq!(first.mutations, second.mutations);
    assert!(!first.receipt.unwrap().runner_reused);
    assert!(second.receipt.unwrap().runner_reused);
}

#[test]
fn live_cuda_program_call_updates_the_callers_slice_and_audits_the_gpu() {
    let program = parse_human(SAXPY).unwrap();
    if !runnable(first_capsule(&program)) {
        return;
    }
    assert!(crate::typecheck::check(&program).is_empty());
    let json = crate::interp::run_entry_json_live(
        &program,
        "saxpy_then_first",
        "{ \"args\": { \"a\": 0.5, \"x\": [2.0, 4.0], \"y\": [1.0, 1.0] } }",
        None,
    )
    .unwrap();
    assert!(
        json.contains("\"return\": 2.0") || json.contains("\"return\": 2"),
        "{json}"
    );
    assert!(json.contains("\"gpu\": {\"target\": \"sm_120\""), "{json}");
    assert!(
        json.contains("\"host_memory_limit\": \"data_segment\""),
        "{json}"
    );
}

#[test]
fn cuda_ptx_target_runs_through_the_driver_jit() {
    // compute_75 is PTX only: the driver compiles it for the installed GPU at
    // load time, with the JIT cache disabled.
    let source = SAXPY.replace("cuda-13-sm_120", "cuda-13-compute_75");
    let program = parse_human(&source).unwrap();
    let capsule = first_capsule(&program);
    if !runnable(capsule) {
        return;
    }
    let invocation = invoke_live(&program, capsule, &saxpy_args()).unwrap();
    assert_eq!(invocation.output, "true");
    assert_eq!(
        invocation.mutations[0].values,
        floats(&[12.0, 24.0, 36.0, 48.0])
    );
    assert_eq!(
        invocation.receipt.unwrap().gpu.unwrap().target,
        "compute_75"
    );
}

#[test]
fn cuda_target_newer_than_the_gpu_is_refused_before_compiling() {
    let Some(device) = gpu_devices().first() else {
        return;
    };
    let newer = format!("sm_{}", device.arch + 10);
    let program = cuda_program(
        "cuda-13.0",
        &format!("cuda-13-{newer}"),
        "gpu.compute",
        "    sandbox \"gpu.compute device:0\"\n",
        IDENTITY_BODY,
    );
    let capsule = first_capsule(&program);
    if crate::native_build::resolve_cuda_toolchain(&crate::cuda::capsule_toolkit_want(capsule))
        .is_none()
    {
        return;
    }
    let err = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(1))],
    )
    .unwrap_err();
    assert!(
        err.starts_with("native_capsule_gpu_target_unsupported"),
        "{err}"
    );
    assert!(err.contains(&newer) && err.contains(&device.name), "{err}");
}

#[test]
fn cuda_grant_for_a_missing_gpu_is_refused() {
    if gpu_devices().is_empty() {
        return;
    }
    let missing = gpu_devices()
        .iter()
        .map(|device| device.index)
        .max()
        .unwrap()
        + 1;
    let program = cuda_program(
        "cuda-13.0",
        "cuda-13-compute_75",
        "gpu.compute",
        &format!("    sandbox \"gpu.compute device:{missing}\"\n"),
        IDENTITY_BODY,
    );
    let capsule = first_capsule(&program);
    if crate::native_build::resolve_cuda_toolchain(&crate::cuda::capsule_toolkit_want(capsule))
        .is_none()
    {
        return;
    }
    let err = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(1))],
    )
    .unwrap_err();
    assert!(
        err.contains(&format!(
            "is granted GPU {missing}, which this host does not have"
        )),
        "{err}"
    );
}

/// A CUDA capsule that initializes the CUDA context, then tries to allocate
/// and touch `n` MiB of host memory: 1 when it could, 0 when the allocation was
/// refused, and -1 when the context could not be created at all. The touches
/// are volatile so the compiler cannot elide the allocation.
const HOST_MEMORY_BODY: &str = "        #include <cuda_runtime.h>\n        #include <cstddef>\n        #include <cstdlib>\n        extern \"C\" int probe(int n) {\n            if (cudaFree(0) != cudaSuccess) return -1;\n            const std::size_t bytes = static_cast<std::size_t>(n) << 20;\n            char *block = static_cast<char *>(std::malloc(bytes));\n            if (block == nullptr) return 0;\n            volatile char *touch = block;\n            for (std::size_t i = 0; i < bytes; i += 4096) touch[i] = 1;\n            std::free(block);\n            return 1;\n        }\n";

#[test]
fn cuda_host_memory_limit_bounds_allocations_without_breaking_the_driver() {
    let program = cuda_program(
        "cuda-13.0",
        "cuda-13-compute_75",
        "gpu.compute, alloc",
        "    sandbox \"gpu.compute device:0\"\n    sandbox \"limit.mem_mb 256\"\n",
        HOST_MEMORY_BODY,
    );
    let capsule = first_capsule(&program);
    if !runnable(capsule) {
        return;
    }
    let call = |megabytes: i64| {
        invoke_live(
            &program,
            capsule,
            &[NativeValue::Scalar(CapsuleValue::Int(megabytes))],
        )
        .unwrap()
        .output
    };
    // The driver comes up under a 256 MiB ceiling that an address-space limit
    // could never give it, a small host allocation succeeds, and one past the
    // ceiling is refused.
    assert_eq!(call(32), "1");
    assert_eq!(call(1024), "0");
}

/// A CUDA capsule reporting how many devices the CUDA runtime can see.
const DEVICE_COUNT_BODY: &str = "        #include <cuda_runtime.h>\n        extern \"C\" int probe(int n) {\n            int count = -1;\n            if (cudaGetDeviceCount(&count) != cudaSuccess) return -1;\n            return count + n;\n        }\n";

#[test]
fn cuda_device_grant_scopes_what_the_runner_sees() {
    let program = cuda_program(
        "cuda-13.0",
        "cuda-13-compute_75",
        "gpu.compute",
        "    sandbox \"gpu.compute device:0\"\n",
        DEVICE_COUNT_BODY,
    );
    let capsule = first_capsule(&program);
    if !runnable(capsule) {
        return;
    }
    let output = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(0))],
    )
    .unwrap()
    .output;
    assert_eq!(output, "1");
}

#[test]
fn live_cuda_runs_on_every_installed_toolkit_that_supports_this_host() {
    // Several toolkits may be installed side by side; a capsule declaring one
    // must compile with a toolkit of that release line. A toolkit that cannot
    // build on this host (its supported host compilers or C library are older
    // than the system's) must fail with an actionable hint.
    let Some(device) = gpu_devices().first() else {
        return;
    };
    let target = GpuTarget::parse("compute_75").unwrap();
    if !target.runs_on(device.arch) {
        return;
    }
    let mut releases: Vec<(u64, u64)> = crate::native_build::discover_cuda_toolchains()
        .iter()
        .filter_map(|toolchain| crate::toolchain_version::Version::extract(&toolchain.version))
        .filter(|version| version.major >= 12)
        .map(|version| (version.major, version.minor))
        .collect();
    releases.sort();
    releases.dedup();
    let mut ran = 0;
    for (major, minor) in releases {
        let source = SAXPY
            .replace("cuda-13.0", &format!("cuda-{major}.{minor}"))
            .replace("cuda-13-sm_120", &format!("cuda-{major}-compute_75"));
        let program = parse_human(&source).unwrap();
        let capsule = first_capsule(&program);
        match invoke_live(&program, capsule, &saxpy_args()) {
            Ok(invocation) => {
                assert_eq!(invocation.output, "true", "CUDA {major}.{minor}");
                let semver = invocation.receipt.unwrap().toolchain_semver.unwrap();
                assert!(
                    semver.starts_with(&format!("{major}.")),
                    "a CUDA {major}.{minor} capsule compiled with {semver}"
                );
                ran += 1;
            }
            Err(err) => {
                // Only acceptable when no toolkit the declaration admits can
                // build on this host, and then the error must say what to do.
                let want = crate::cuda::capsule_toolkit_want(capsule);
                let buildable = crate::native_build::discover_cuda_toolchains()
                    .into_iter()
                    .filter(|toolchain| {
                        crate::toolchain_version::Version::extract(&toolchain.version)
                            .is_some_and(|version| want.matches(&version))
                    })
                    .any(|toolchain| {
                        crate::native_build::cuda_toolkit_builds_here(&toolchain.name)
                    });
                assert!(
                    !buildable,
                    "CUDA {major}.{minor} had a usable toolkit: {err}"
                );
                assert!(
                    err.starts_with("native_capsule_compile_failed") && err.contains("(hint: "),
                    "CUDA {major}.{minor}: {err}"
                );
            }
        }
    }
    assert!(ran > 0 || crate::native_build::discover_cuda_toolchains().is_empty());
}
