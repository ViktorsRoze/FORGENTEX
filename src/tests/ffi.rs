use std::path::PathBuf;

use crate::ast::{CapsuleValue, Type};
use crate::ffi::{
    invoke_live, invoke_live_audited, invoke_live_cached, invoke_live_cached_audited,
    parse_native_invocation, run, validate_native_toolchain_version, NativeRunnerCache,
    NativeValue,
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
    assert_eq!(failure.receipt.schema, crate::ffi::NATIVE_FAILURE_SCHEMA);
    assert_eq!(failure.receipt.kind, "native_capsule_boundary_unsupported");
    assert_eq!(failure.receipt.phase, "preflight");
    assert_eq!(failure.receipt.cpu_limit_ms, Some(250));
    assert_eq!(failure.receipt.cpu_limit_source, "declared");
    assert_eq!(
        failure.receipt.memory_limit_bytes,
        Some(crate::ffi::DEFAULT_NATIVE_MEMORY_BYTES)
    );
    assert_eq!(failure.receipt.memory_limit_source, "default");
    assert_eq!(failure.receipt.output_limit_source, "default");
    assert!(!failure.receipt.limits_applied);
    assert_eq!(failure.receipt.containment, None);
    assert_eq!(failure.receipt.runner_environment_policy, None);
    assert_eq!(failure.receipt.runner_working_directory_policy, None);
}

#[test]
fn live_native_preflight_rejects_unisolated_os_capabilities() {
    let cases = [
        ("network.connect", "net.connect 127.0.0.1:9"),
        ("fs.read", "fs.read fixtures"),
        ("fs.write", "fs.write target"),
        ("proc.spawn", "proc.spawn"),
        ("env.read", "env.read HOME"),
        ("chain.read", "chain.read local"),
        ("gpu.compute", "gpu.compute device:0"),
    ];
    for (effect, grant) in cases {
        let source = format!(
            "foreign ambient c version \"c11\" effects [{effect}]:\n    output result: i32\n    sandbox \"{grant}\"\n    code:\n        int ambient(void) {{ return 1; }}\n"
        );
        let program = parse_human(&source).unwrap();
        let failure = invoke_live_audited(&program, program.capsules().next().unwrap(), &[])
            .expect_err("unsupported live-native OS authority must fail before compiler lookup");
        assert_eq!(
            failure.receipt.kind, "native_capsule_capability_unsupported",
            "effect={effect}: {}",
            failure.message
        );
        assert_eq!(failure.receipt.phase, "preflight");
        assert!(!failure.receipt.limits_applied);
        assert_eq!(failure.receipt.containment, None);
        assert!(
            failure.message.contains(effect),
            "effect={effect}: {}",
            failure.message
        );
    }

    let unused_grant = parse_human(
        "foreign ambient c version \"c11\":\n    output result: i32\n    sandbox \"net.connect 127.0.0.1:9\"\n    code:\n        int ambient(void) { return 1; }\n",
    )
    .unwrap();
    let failure = invoke_live_audited(&unused_grant, unused_grant.capsules().next().unwrap(), &[])
        .expect_err("an unused native OS grant must not become ambient authority");
    assert_eq!(
        failure.receipt.kind,
        "native_capsule_capability_unsupported"
    );
    assert!(failure.message.contains("grant net.connect"));
}

#[test]
fn live_native_preflight_rejects_unenforced_byte_limits() {
    let cases = if cfg!(unix) {
        vec!["limit.net_bytes 1024", "limit.fs_bytes 1024"]
    } else {
        vec!["limit.net_bytes 1024"]
    };
    for limit in cases {
        let source = format!(
            "foreign bounded c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    sandbox \"{limit}\"\n    code:\n        int bounded(void) {{ return 1; }}\n"
        );
        let program = parse_human(&source).unwrap();
        let failure = invoke_live_audited(&program, program.capsules().next().unwrap(), &[])
            .expect_err("unenforced native byte limits must fail before compiler lookup");
        assert_eq!(failure.receipt.kind, "native_capsule_limit_unsupported");
        assert_eq!(failure.receipt.phase, "preflight");
        assert!(!failure.receipt.limits_applied);
        assert!(
            failure
                .message
                .contains(limit.split_whitespace().next().unwrap()),
            "{}",
            failure.message
        );
    }
}

#[test]
#[cfg(windows)]
fn windows_native_fs_byte_limit_bounds_aggregate_job_io() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign flood_io c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    sandbox \"limit.fs_bytes 1048576\"\n    sandbox \"limit.wall_ms 5000\"\n    code:\n        #include <stdio.h>\n        #include <string.h>\n        int flood_io(void) { char block[65536]; memset(block, 1, sizeof(block)); for (int i = 0; i < 256; ++i) fwrite(block, 1, sizeof(block), stdout); fflush(stdout); return 1; }\n",
    )
    .unwrap();
    let failure = invoke_live_audited(&program, program.capsules().next().unwrap(), &[])
        .expect_err("aggregate Windows Job I/O must be bounded");
    assert_eq!(failure.receipt.kind, "native_capsule_io_limit");
    assert_eq!(failure.receipt.phase, "execute");
    assert_eq!(failure.receipt.io_limit_bytes, Some(1_048_576));
    assert_eq!(failure.receipt.io_limit_source, "declared_fs_bytes");
    assert!(failure.receipt.limits_applied);
}

#[test]
#[cfg(windows)]
fn windows_live_native_runner_uses_a_kernel_verified_restricted_token() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign restricted c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int restricted(void) { return 17; }\n",
    )
    .unwrap();
    let receipt = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .unwrap()
        .receipt
        .unwrap();
    assert_eq!(receipt.windows_restricted_token, Some(true));
    assert_eq!(receipt.windows_restricting_sid_count, Some(3));
    assert_eq!(receipt.windows_integrity_level.as_deref(), Some("low"));
    assert_eq!(receipt.windows_appcontainer, Some(true));
    assert_eq!(receipt.windows_appcontainer_capability_count, Some(0));
    assert_eq!(
        receipt.windows_appcontainer_profile.as_deref(),
        Some(crate::native_build::NATIVE_APPCONTAINER_NAME)
    );
    assert_eq!(
        receipt.runner_process_identity_policy,
        "zero_capability_appcontainer_over_restricted_low_integrity_token"
    );
}

#[test]
#[cfg(windows)]
fn windows_low_integrity_runner_cannot_modify_a_medium_integrity_workspace_file() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let target = std::env::current_dir().unwrap().join(format!(
        ".forgentex-medium-write-probe-{}",
        std::process::id()
    ));
    std::fs::write(&target, b"host-owned").unwrap();
    let c_path = target.to_string_lossy().replace('\\', "/");
    let source = format!(
        "foreign deny_write c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdio.h>\n        int deny_write(void) {{ FILE *file = fopen(\"{c_path}\", \"wb\"); if (file == NULL) return 1; fputs(\"escaped\", file); fclose(file); return 0; }}\n"
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]);
    let contents = std::fs::read(&target);
    let _ = std::fs::remove_file(&target);

    let invocation = invocation.expect("low-integrity runner should complete after write denial");
    assert_eq!(invocation.output, "1");
    assert_eq!(contents.unwrap(), b"host-owned");
    let receipt = invocation.receipt.unwrap();
    assert_eq!(receipt.windows_integrity_level.as_deref(), Some("low"));
}

#[test]
#[cfg(windows)]
fn windows_appcontainer_runner_cannot_read_a_workspace_file() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let target = std::env::current_dir().unwrap().join(format!(
        ".forgentex-medium-read-probe-{}",
        std::process::id()
    ));
    std::fs::write(&target, b"host-secret").unwrap();
    let c_path = target.to_string_lossy().replace('\\', "/");
    let source = format!(
        "foreign deny_read c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdio.h>\n        int deny_read(void) {{ FILE *file = fopen(\"{c_path}\", \"rb\"); if (file == NULL) return 1; fclose(file); return 0; }}\n"
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]);
    let _ = std::fs::remove_file(&target);

    let invocation = invocation.expect("AppContainer runner should complete after read denial");
    assert_eq!(invocation.output, "1");
}

#[test]
#[cfg(windows)]
fn windows_appcontainer_cannot_write_a_user_owned_low_integrity_directory() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "forgentex-low-integrity-probe-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir(&directory).unwrap();
    let labeled = std::process::Command::new("icacls.exe")
        .arg(&directory)
        .arg("/setintegritylevel")
        .arg("(OI)(CI)L")
        .output()
        .unwrap();
    assert!(
        labeled.status.success(),
        "failed to label test directory low integrity: {}",
        String::from_utf8_lossy(&labeled.stderr)
    );

    let target = directory.join("escaped.txt");
    let c_path = target.to_string_lossy().replace('\\', "/");
    let source = format!(
        "foreign deny_low_write c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdio.h>\n        int deny_low_write(void) {{ FILE *file = fopen(\"{c_path}\", \"wb\"); if (file == NULL) return 1; fputs(\"escaped\", file); fclose(file); return 0; }}\n"
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]);
    let escaped = target.exists();
    let _ = std::fs::remove_dir_all(&directory);

    let invocation =
        invocation.expect("AppContainer runner should complete after low-integrity write denial");
    assert_eq!(invocation.output, "1");
    assert!(!escaped);
}

#[test]
#[cfg(windows)]
fn windows_zero_capability_appcontainer_cannot_connect_to_loopback() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let source = format!(
        r#"foreign deny_loopback c version "c11":
    output result: i32
    sandbox "fs.none"
    code:
        #include <windows.h>
        #include <stdint.h>
        typedef uintptr_t socket_handle;
        typedef int (WINAPI *wsa_startup_fn)(WORD, void *);
        typedef socket_handle (WINAPI *socket_fn)(int, int, int);
        typedef int (WINAPI *connect_fn)(socket_handle, const void *, int);
        typedef int (WINAPI *close_socket_fn)(socket_handle);
        typedef int (WINAPI *wsa_cleanup_fn)(void);
        struct local_sockaddr_in {{ short family; unsigned short port; unsigned long address; char zero[8]; }};
        int deny_loopback(void) {{
            HMODULE library = LoadLibraryA("ws2_32.dll");
            if (library == NULL) return 1;
            wsa_startup_fn startup = (wsa_startup_fn)GetProcAddress(library, "WSAStartup");
            socket_fn make_socket = (socket_fn)GetProcAddress(library, "socket");
            connect_fn connect_socket = (connect_fn)GetProcAddress(library, "connect");
            close_socket_fn close_socket = (close_socket_fn)GetProcAddress(library, "closesocket");
            wsa_cleanup_fn cleanup = (wsa_cleanup_fn)GetProcAddress(library, "WSACleanup");
            unsigned char data[512] = {{0}};
            if (startup == NULL || make_socket == NULL || connect_socket == NULL || close_socket == NULL || cleanup == NULL) return 1;
            if (startup(0x0202, data) != 0) return 1;
            socket_handle socket = make_socket(2, 1, 6);
            if (socket == (socket_handle)(~(uintptr_t)0)) {{ cleanup(); return 1; }}
            struct local_sockaddr_in address = {{0}};
            address.family = 2;
            address.port = (unsigned short)(({port}u << 8) | ({port}u >> 8));
            address.address = 0x0100007fu;
            int connected = connect_socket(socket, &address, sizeof(address));
            close_socket(socket);
            cleanup();
            FreeLibrary(library);
            return connected == 0 ? 0 : 1;
        }}
"#
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .expect("zero-capability AppContainer runner should report loopback denial");
    assert_eq!(invocation.output, "1");
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[test]
#[cfg(windows)]
fn windows_appcontainer_cannot_open_an_unrelated_named_pipe() {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let pipe_name = format!(
        r"\\.\pipe\forgentex-appcontainer-probe-{}",
        std::process::id()
    );
    let wide_name = std::ffi::OsStr::new(&pipe_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const PIPE_TYPE_BYTE: u32 = 0;
    // SAFETY: the pipe name is null-terminated and the returned handle is
    // checked before being placed in OwnedHandle.
    let server = unsafe {
        CreateNamedPipeW(
            wide_name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE,
            1,
            4096,
            4096,
            0,
            std::ptr::null(),
        )
    };
    const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;
    assert_ne!(server, INVALID_HANDLE_VALUE);
    // SAFETY: CreateNamedPipeW returned a newly owned valid handle.
    let _server = unsafe { OwnedHandle::from_raw_handle(server.cast()) };

    let c_pipe_name = pipe_name.replace('\\', "\\\\");
    let source = format!(
        "foreign deny_pipe c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <windows.h>\n        int deny_pipe(void) {{ HANDLE pipe = CreateFileA(\"{c_pipe_name}\", GENERIC_READ | GENERIC_WRITE, 0, NULL, OPEN_EXISTING, 0, NULL); if (pipe == INVALID_HANDLE_VALUE) return 1; CloseHandle(pipe); return 0; }}\n"
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .expect("AppContainer runner should complete after named-pipe denial");
    assert_eq!(invocation.output, "1");
}

#[test]
#[cfg(windows)]
fn windows_appcontainer_cannot_modify_the_host_user_registry() {
    let key = format!(
        r"Software\Forgentex\AppContainerProbe-{}",
        std::process::id()
    );
    assert!(!host_registry_key_exists(&key));
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let c_key = key.replace('\\', "\\\\");
    let source = format!(
        r#"foreign isolate_registry c version "c11":
    output result: i32
    sandbox "fs.none"
    code:
        #include <windows.h>
        typedef LSTATUS (WINAPI *create_key_fn)(HKEY, LPCSTR, DWORD, LPSTR, DWORD, REGSAM, const SECURITY_ATTRIBUTES *, PHKEY, LPDWORD);
        typedef LSTATUS (WINAPI *close_key_fn)(HKEY);
        typedef LSTATUS (WINAPI *delete_key_fn)(HKEY, LPCSTR);
        int isolate_registry(void) {{
            HMODULE library = LoadLibraryA("advapi32.dll");
            if (library == NULL) return 1;
            create_key_fn create_key = (create_key_fn)GetProcAddress(library, "RegCreateKeyExA");
            close_key_fn close_key = (close_key_fn)GetProcAddress(library, "RegCloseKey");
            delete_key_fn delete_key = (delete_key_fn)GetProcAddress(library, "RegDeleteKeyA");
            if (create_key == NULL || close_key == NULL || delete_key == NULL) return 1;
            HKEY key = NULL;
            DWORD disposition = 0;
            LSTATUS status = create_key(HKEY_CURRENT_USER, "{c_key}", 0, NULL, 0, KEY_WRITE, NULL, &key, &disposition);
            if (status != ERROR_SUCCESS) return 1;
            close_key(key);
            delete_key(HKEY_CURRENT_USER, "{c_key}");
            FreeLibrary(library);
            return 0;
        }}
"#
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .expect("AppContainer registry probe should complete");
    assert_eq!(invocation.output, "1");
    assert!(
        !host_registry_key_exists(&key),
        "AppContainer write escaped into the host HKCU hive"
    );
}

#[cfg(windows)]
fn host_registry_key_exists(key: &str) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let key = std::ffi::OsStr::new(key)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut handle = std::ptr::null_mut();
    const HKEY_CURRENT_USER: *mut std::ffi::c_void = 0x8000_0001usize as *mut std::ffi::c_void;
    const KEY_READ: u32 = 0x0002_0019;
    // SAFETY: the key is null-terminated and `handle` is writable.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            KEY_READ,
            std::ptr::addr_of_mut!(handle),
        )
    };
    if status == 0 {
        // SAFETY: a successful query returned one owned registry-key handle.
        unsafe {
            let _ = RegCloseKey(handle);
        }
        true
    } else {
        false
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        max_instances: u32,
        output_buffer_size: u32,
        input_buffer_size: u32,
        default_timeout_ms: u32,
        security_attributes: *const std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: *mut std::ffi::c_void,
        sub_key: *const u16,
        options: u32,
        desired_access: u32,
        result: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn RegCloseKey(key: *mut std::ffi::c_void) -> i32;
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
fn live_native_large_slice_uses_anonymous_stdin_not_process_arguments() {
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
fn live_native_request_uses_anonymous_stdin_without_a_scratch_file() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign inspect_transport c version \"c11\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stdio.h>\n        int inspect_transport(int value) { FILE *request = fopen(\"request.txt\", \"rb\"); if (request != NULL) { fclose(request); return value + 1000; } return value; }\n",
    )
    .unwrap();
    let invocation = invoke_live(
        &program,
        program.capsules().next().unwrap(),
        &[NativeValue::Scalar(CapsuleValue::Int(73))],
    )
    .unwrap();
    assert_eq!(invocation.output, "73");
    let receipt = invocation.receipt.unwrap();
    assert_eq!(receipt.runner_input_transport, "anonymous_stdin");
}

#[test]
fn native_cpu_budget_terminates_busy_work_before_wall_fallback() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign burn_cpu c version \"c11\":\n    input spin: i32\n    output result: i32\n    sandbox \"limit.cpu_ms 50\"\n    sandbox \"limit.wall_ms 3000\"\n    code:\n        int burn_cpu(int spin) { if (!spin) return 7; volatile unsigned long long n = 0; for (;;) { n++; } }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let mut cache = NativeRunnerCache::default();
    let warmup = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(0))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(warmup.output, "7");
    let started = std::time::Instant::now();
    let error = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(1))],
        &mut cache,
    )
    .unwrap_err();
    assert!(
        error.contains("native_capsule_failed") || error.contains("native_capsule_timeout"),
        "got: {error}"
    );
    // Compile/cache setup is deliberately outside the timed interval. On a
    // saturated host the child may receive less than 50 ms of CPU before the
    // wall fallback, but execution itself must remain bounded.
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
}

#[test]
fn cached_runner_keeps_per_call_timeout_and_survives_timed_out_process() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign maybe_spin c version \"c11\":\n    input spin: i32\n    output result: i32\n    sandbox \"limit.wall_ms 500\"\n    code:\n        int maybe_spin(int spin) { if (!spin) return 7; for (;;) {} }\n",
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
fn tampered_cached_native_runner_is_rebuilt_before_execution() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign increment c version \"c11\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int increment(int value) { return value + 1; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let mut cache = NativeRunnerCache::default();

    let first = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(6))],
        &mut cache,
    )
    .unwrap();
    let first_receipt = first.receipt.unwrap();
    assert_eq!(first.output, "7");
    assert!(!first_receipt.runner_reused);
    assert_eq!(first_receipt.runner_cache_status, "miss");
    assert_eq!(first_receipt.runner_artifact_sha256.len(), 64);
    assert!(first_receipt
        .runner_artifact_sha256
        .chars()
        .all(|ch| ch.is_ascii_hexdigit()));

    let cached_runner = cache
        .cached_runner_path_for_test()
        .expect("successful cached call must retain its runner");
    let mut replaced = false;
    let mut last_error = None;
    for _ in 0..20 {
        match std::fs::write(&cached_runner, b"forgentex deliberate cache tamper") {
            Ok(()) => {
                replaced = true;
                break;
            }
            Err(error) if cfg!(windows) && error.raw_os_error() == Some(32) => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => panic!("test must be able to replace cached runner bytes: {error}"),
        }
    }
    assert!(
        replaced,
        "test must be able to replace cached runner bytes after bounded sharing-violation retries: {last_error:?}"
    );

    let rebuilt = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(9))],
        &mut cache,
    )
    .unwrap();
    let rebuilt_receipt = rebuilt.receipt.unwrap();
    assert_eq!(rebuilt.output, "10");
    assert!(!rebuilt_receipt.runner_reused);
    assert_eq!(
        rebuilt_receipt.runner_cache_status,
        "rebuilt_after_integrity_mismatch"
    );
    assert_eq!(rebuilt_receipt.runner_artifact_sha256.len(), 64);

    let verified_hit = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(10))],
        &mut cache,
    )
    .unwrap();
    let verified_receipt = verified_hit.receipt.unwrap();
    assert_eq!(verified_hit.output, "11");
    assert!(verified_receipt.runner_reused);
    assert_eq!(verified_receipt.runner_cache_status, "verified_hit");
    assert_eq!(
        verified_receipt.runner_artifact_sha256,
        rebuilt_receipt.runner_artifact_sha256
    );
}

#[test]
fn live_native_scratch_has_protected_access_before_runner_creation() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign identity c version \"c11\":\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int identity(int value) { return value; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let mut cache = NativeRunnerCache::default();
    let invocation = invoke_live_cached(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(31))],
        &mut cache,
    )
    .unwrap();
    assert_eq!(invocation.output, "31");
    let receipt = invocation.receipt.unwrap();
    let runner = cache
        .cached_runner_path_for_test()
        .expect("successful cached call must retain its runner");
    let scratch = runner.parent().expect("runner must have a scratch parent");

    #[cfg(windows)]
    {
        assert_eq!(
            receipt.runner_scratch_access_policy,
            "owner_system_restricted_code_appcontainer_rx_protected_dacl"
        );
        assert!(crate::ffi::private_native_scratch_is_verified_for_test(
            scratch
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(receipt.runner_scratch_access_policy, "owner_mode_0700");
        let mode = std::fs::metadata(scratch).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
    #[cfg(not(any(unix, windows)))]
    assert_eq!(receipt.runner_scratch_access_policy, "private_name_only");
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
    assert_eq!(receipt.cpu_limit_source, "declared");
    assert_eq!(receipt.memory_limit_bytes, Some(64 * 1024 * 1024));
    assert_eq!(receipt.memory_limit_source, "declared");
    if cfg!(windows) {
        assert_eq!(
            receipt.process_limit,
            Some(crate::native_build::DEFAULT_NATIVE_RUNNER_PROCESS_LIMIT)
        );
        assert_eq!(receipt.process_limit_source, "default");
        assert!(receipt.peak_tree_processes.is_some_and(|peak| peak >= 1));
        assert_eq!(
            receipt.windows_ui_restrictions,
            Some(crate::native_build::WINDOWS_JOB_UI_RESTRICTIONS)
        );
        assert_eq!(receipt.ui_isolation_source, "job_object");
        assert_eq!(receipt.windows_restricted_token, Some(true));
        assert_eq!(receipt.windows_restricting_sid_count, Some(3));
        assert_eq!(receipt.windows_integrity_level.as_deref(), Some("low"));
        assert_eq!(receipt.windows_appcontainer, Some(true));
        assert_eq!(receipt.windows_appcontainer_capability_count, Some(0));
        assert_eq!(
            receipt.windows_appcontainer_profile.as_deref(),
            Some(crate::native_build::NATIVE_APPCONTAINER_NAME)
        );
        assert_eq!(
            receipt.runner_process_identity_policy,
            "zero_capability_appcontainer_over_restricted_low_integrity_token"
        );
        assert_eq!(receipt.io_limit_bytes, None);
        assert_eq!(receipt.io_limit_source, "undeclared");
        assert!(receipt.job_read_transfer_bytes.is_some());
        assert!(receipt.job_write_transfer_bytes.is_some());
        assert!(receipt.job_other_transfer_bytes.is_some());
    } else {
        assert_eq!(receipt.process_limit, None);
        assert_eq!(receipt.process_limit_source, "unavailable");
        assert_eq!(receipt.peak_tree_processes, None);
        assert_eq!(receipt.windows_ui_restrictions, None);
        assert_eq!(receipt.ui_isolation_source, "unavailable");
        assert_eq!(receipt.windows_restricted_token, None);
        assert_eq!(receipt.windows_restricting_sid_count, None);
        assert_eq!(receipt.windows_integrity_level, None);
        assert_eq!(receipt.windows_appcontainer, None);
        assert_eq!(receipt.windows_appcontainer_capability_count, None);
        assert_eq!(receipt.windows_appcontainer_profile, None);
        assert_eq!(
            receipt.runner_process_identity_policy,
            "inherited_unix_identity"
        );
        assert_eq!(receipt.io_limit_bytes, None);
        assert_eq!(receipt.io_limit_source, "unavailable");
        assert_eq!(receipt.job_read_transfer_bytes, None);
        assert_eq!(receipt.job_write_transfer_bytes, None);
        assert_eq!(receipt.job_other_transfer_bytes, None);
    }
    assert_eq!(receipt.runner_artifact_sha256.len(), 64);
    assert_eq!(receipt.runner_cache_status, "disabled");
    assert_eq!(receipt.runner_input_transport, "anonymous_stdin");
    assert_eq!(receipt.runner_execution_model, "fresh_process_per_call");
    assert_eq!(
        receipt.runner_environment_policy,
        if cfg!(windows) {
            "controlled_appcontainer_system_and_toolchain"
        } else {
            "clear_except_toolchain_path"
        }
    );
    assert_eq!(receipt.runner_working_directory_policy, "private_scratch");
    if cfg!(windows) {
        assert_eq!(
            receipt.runner_scratch_access_policy,
            "owner_system_restricted_code_appcontainer_rx_protected_dacl"
        );
    } else if cfg!(unix) {
        assert_eq!(receipt.runner_scratch_access_policy, "owner_mode_0700");
    }
    assert_eq!(
        receipt.runner_capability_profile,
        if cfg!(windows) {
            "appcontainer_compute_or_host_artifact_only"
        } else {
            "compute_or_host_artifact_only"
        }
    );
    assert_eq!(
        receipt.filesystem_isolation,
        if cfg!(windows) {
            "appcontainer_default_deny_host_files_explicit_runner_rx"
        } else {
            "private_working_directory_only"
        }
    );
    assert_eq!(
        receipt.network_isolation,
        if cfg!(windows) {
            "appcontainer_zero_network_capabilities"
        } else {
            "unavailable"
        }
    );
    assert_eq!(receipt.output_limit_source, "default");
    assert_eq!(
        receipt.wall_limit_ms,
        Some(u128::from(crate::ffi::DEFAULT_NATIVE_WALL_MS))
    );
    assert_eq!(receipt.wall_limit_source, "default");
    assert_eq!(
        receipt.compile_wall_limit_ms,
        crate::ffi::NATIVE_COMPILE_WALL_MS
    );
    assert_eq!(receipt.compile_containment, "process_tree");
}

#[test]
fn native_runner_applies_default_cpu_memory_and_output_budgets() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign default_budgeted c version \"c11\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        int default_budgeted(void) { return 7; }\n",
    )
    .unwrap();
    let receipt = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .unwrap()
        .receipt
        .unwrap();

    assert_eq!(receipt.schema, crate::ffi::NATIVE_RECEIPT_SCHEMA);
    assert_eq!(
        receipt.cpu_limit_ms,
        Some(crate::ffi::DEFAULT_NATIVE_CPU_MS)
    );
    assert_eq!(receipt.cpu_limit_source, "default");
    assert_eq!(
        receipt.memory_limit_bytes,
        Some(crate::ffi::DEFAULT_NATIVE_MEMORY_BYTES)
    );
    assert_eq!(receipt.memory_limit_source, "default");
    if cfg!(windows) {
        assert_eq!(
            receipt.process_limit,
            Some(crate::native_build::DEFAULT_NATIVE_RUNNER_PROCESS_LIMIT)
        );
        assert_eq!(receipt.process_limit_source, "default");
        assert!(receipt.peak_tree_processes.is_some_and(|peak| peak >= 1));
    } else {
        assert_eq!(receipt.process_limit, None);
        assert_eq!(receipt.process_limit_source, "unavailable");
        assert_eq!(receipt.peak_tree_processes, None);
    }
    assert_eq!(
        receipt.output_limit_bytes,
        crate::native_build::MAX_CONTAINED_OUTPUT_BYTES
    );
    assert_eq!(receipt.output_limit_source, "default");
    assert_eq!(receipt.runner_execution_model, "fresh_process_per_call");
    assert_eq!(
        receipt.runner_environment_policy,
        if cfg!(windows) {
            "controlled_appcontainer_system_and_toolchain"
        } else {
            "clear_except_toolchain_path"
        }
    );
    assert_eq!(receipt.runner_working_directory_policy, "private_scratch");
}

#[test]
fn successful_native_receipt_records_aggregate_tree_peak_memory() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    // The capsule commits and touches a known ~16 MiB block so the aggregate
    // Job Object high-water mark is meaningfully above the process baseline and
    // well under the declared 128 MiB budget.
    let program = parse_human(
        "foreign touch_memory c version \"c11\" effects [alloc]:\n    output result: i32\n    sandbox \"limit.mem_mb 128\"\n    sandbox \"limit.wall_ms 5000\"\n    code:\n        #include <stdlib.h>\n        #include <string.h>\n        int touch_memory(void) { size_t n = 24u * 1024u * 1024u; unsigned char *p = (unsigned char *)malloc(n); if (!p) return 0; volatile unsigned long acc = 0; for (int pass = 0; pass < 96; pass++) { memset(p, (int)(pass & 0xff), n); for (size_t i = 0; i < n; i += 4096) { acc = acc + p[i]; } } free(p); return (int)(acc & 1); }\n",
    )
    .unwrap();
    let receipt = invoke_live(&program, program.capsules().next().unwrap(), &[])
        .unwrap()
        .receipt
        .unwrap();
    let budget = 128 * 1024 * 1024;
    assert_eq!(receipt.memory_limit_bytes, Some(budget));
    if cfg!(windows) {
        // Windows Job Objects account aggregate memory across the tree, sampled
        // live while the child runs. The capsule holds ~24 MiB for ~100 ms, so
        // the sampled peak lands well above a trivial baseline and never above
        // the enforced budget.
        let peak = receipt
            .peak_tree_memory_bytes
            .expect("windows job object reports a sampled aggregate peak");
        assert!(
            peak >= 2 * 1024 * 1024,
            "sampled aggregate peak {peak} is implausibly small for a sustained 24 MiB workload"
        );
        assert!(
            peak <= budget,
            "sampled aggregate peak {peak} exceeds the enforced {budget}-byte budget"
        );
    } else {
        // Platforms without aggregate whole-tree accounting report no peak
        // rather than a per-process approximation.
        assert_eq!(receipt.peak_tree_memory_bytes, None);
    }
}

#[test]
fn native_output_budget_is_audited_and_cached_runner_recovers_after_overflow() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign noisy c version \"c11\":\n    input count: i32\n    output result: i32\n    sandbox \"limit.output_bytes 1024\"\n    code:\n        #include <stdio.h>\n        int noisy(int count) { for (int i = 0; i < count; i++) fputc('x', stderr); return 7; }\n",
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
    let receipt = first.receipt.unwrap();
    assert_eq!(receipt.output_limit_bytes, 1024);
    assert_eq!(receipt.stdout_bytes, if cfg!(windows) { 10 } else { 9 });
    assert_eq!(receipt.stderr_bytes, 0);
    assert!(!receipt.runner_reused);

    let failure = invoke_live_cached_audited(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Int(4096))],
        &mut cache,
    )
    .expect_err("combined child output must be capped");
    assert!(failure.message.contains("native_capsule_output_limit"));
    assert_eq!(failure.receipt.kind, "native_capsule_output_limit");
    assert_eq!(failure.receipt.phase, "response");
    assert_eq!(failure.receipt.output_limit_bytes, 1024);
    assert_eq!(failure.receipt.output_limit_source, "declared");
    assert_eq!(
        failure.receipt.cpu_limit_ms,
        Some(crate::ffi::DEFAULT_NATIVE_CPU_MS)
    );
    assert_eq!(failure.receipt.cpu_limit_source, "default");
    assert_eq!(
        failure.receipt.memory_limit_bytes,
        Some(crate::ffi::DEFAULT_NATIVE_MEMORY_BYTES)
    );
    assert_eq!(failure.receipt.memory_limit_source, "default");
    if cfg!(windows) {
        assert_eq!(
            failure.receipt.process_limit,
            Some(crate::native_build::DEFAULT_NATIVE_RUNNER_PROCESS_LIMIT)
        );
        assert_eq!(failure.receipt.process_limit_source, "default");
    } else {
        assert_eq!(failure.receipt.process_limit, None);
        assert_eq!(failure.receipt.process_limit_source, "unavailable");
    }
    assert_eq!(failure.receipt.wall_limit_source, "default");
    assert_eq!(
        failure.receipt.compile_wall_limit_ms,
        crate::ffi::NATIVE_COMPILE_WALL_MS
    );
    assert!(failure.receipt.limits_applied);
    assert_eq!(
        failure.receipt.runner_environment_policy.as_deref(),
        Some(if cfg!(windows) {
            "controlled_appcontainer_system_and_toolchain"
        } else {
            "clear_except_toolchain_path"
        })
    );
    assert_eq!(
        failure.receipt.runner_working_directory_policy.as_deref(),
        Some("private_scratch")
    );
    assert_eq!(failure.receipt.containment.as_deref(), Some("process_tree"));

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
fn live_cpp_owned_output_requires_explicit_c_abi_free_before_compiler_lookup() {
    let program = parse_human(
        "foreign partly_mangled cpp version \"c++17\" effects [alloc]:\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        extern \"C\" FgxOwnedU8 partly_mangled(void) { FgxOwnedU8 out = { nullptr, 0, 0 }; return out; }\n        void partly_mangled_free(uint8_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let err = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(
        err.contains("native_capsule_boundary_unsupported"),
        "got: {err}"
    );
    assert!(err.contains("partly_mangled_free"), "got: {err}");
    assert!(err.contains("extern \"C\""), "got: {err}");
}

#[test]
fn cpp_linkage_preflight_ignores_comments_and_string_literals() {
    let program = parse_human(
        "foreign fake_linkage cpp version \"c++17\":\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        // extern \"C\" int fake_linkage(void);\n        const char *claim = \"extern \\\"C\\\" fake_linkage\";\n        int fake_linkage(void) { return claim[0]; }\n",
    )
    .unwrap();
    let err = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(err.contains("boundary function"), "got: {err}");
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
    assert!(runner.contains("getStdIn().reader().readAllAlloc"));
    assert!(!runner.contains("readFileAlloc"));
    assert!(!runner.contains("argsAlloc"));
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
fn borrowed_string_boundary_derives_pointer_length_and_utf8_contract() {
    let program = parse_human(include_str!("../../examples/capsule/checksum_utf8_c.fgx")).unwrap();
    let plan = run(
        "checksum_utf8_c.fgx",
        &program,
        None,
        &out_dir("borrowed_string_signature"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "int32_t checksum_utf8_c(const uint8_t *text, size_t text_len)"
    );
    let input = plan
        .marshaling
        .iter()
        .find(|marshal| marshal.direction == "in")
        .unwrap();
    assert_eq!(input.fgx_type, "str");
    assert_eq!(input.c_type, "const uint8_t* + size_t (borrowed UTF-8)");
    assert!(plan.is_success(), "plan: {plan:?}");
    let _ = std::fs::remove_dir_all(out_dir("borrowed_string_signature"));
}

#[test]
fn c_cpp_rust_and_zig_borrow_utf8_strings_with_nul_and_newline_safely() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/checksum_utf8_c.fgx"),
        ),
        (
            "cpp",
            crate::native_build::detect_cpp_toolchain().is_some(),
            include_str!("../../examples/capsule/checksum_utf8_cpp.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/checksum_utf8_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/checksum_utf8_zig.fgx"),
        ),
    ];
    let value = "A\0\né";
    for (language, available, source) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let invocation = invoke_live(
            &program,
            capsule,
            &[NativeValue::Scalar(CapsuleValue::Str(value.to_string()))],
        )
        .unwrap_or_else(|error| panic!("{language}: {error}"));
        assert_eq!(invocation.output, "439", "{language}");
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.borrowed_input_bytes),
            Some(value.len()),
            "{language}"
        );
        assert_eq!(invocation.owned_output_bytes, None);
    }
}

#[test]
fn c_cpp_rust_and_zig_mutable_utf8_strings_are_fixed_length_and_byte_accounted() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/title_utf8_c.fgx"),
        ),
        (
            "cpp",
            crate::native_build::detect_cpp_toolchain().is_some(),
            include_str!("../../examples/capsule/title_utf8_cpp.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/title_utf8_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/title_utf8_zig.fgx"),
        ),
    ];
    for (language, available, source) in cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        assert_eq!(capsule.inputs[0].ty.to_human(), "&mut str");
        assert_eq!(capsule.inputs[0].ty.to_agent(), "&mutstr");
        let invocation = invoke_live(
            &program,
            capsule,
            &[NativeValue::MutString("forgentex,✓".to_string())],
        )
        .unwrap_or_else(|error| panic!("{language}: {error}"));
        assert_eq!(invocation.output, "1");
        assert_eq!(invocation.mutations.len(), 1);
        assert_eq!(invocation.mutations[0].values, Vec::new());
        assert_eq!(
            invocation.mutations[0].string.as_deref(),
            Some("Forgentex,✓")
        );
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.borrowed_input_bytes),
            Some(13)
        );
    }
}

#[test]
fn c_cpp_rust_and_zig_resizable_utf8_strings_use_bounded_capacity_and_copy_back() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/grow_utf8_c.fgx"),
        ),
        (
            "cpp",
            crate::native_build::detect_cpp_toolchain().is_some(),
            include_str!("../../examples/capsule/grow_utf8_cpp.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/grow_utf8_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/grow_utf8_zig.fgx"),
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
            &[NativeValue::MutString("hi".to_string())],
        )
        .unwrap_or_else(|error| panic!("{language}: {error}"));
        assert_eq!(invocation.output, "3", "{language}");
        assert_eq!(
            invocation.mutations[0].string.as_deref(),
            Some("hi!"),
            "{language}"
        );
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.borrowed_input_bytes),
            Some(2),
            "{language}"
        );
    }
}

#[test]
fn resizable_string_boundary_type_is_distinct_from_a_string_element_slice() {
    let resizable = Type::parse("&mut String");
    let string_slice = Type::parse("&mut [String]");
    assert_ne!(resizable, string_slice);
    assert_eq!(resizable.to_human(), "&mut String");
    assert_eq!(resizable.to_agent(), "&mutString");
    assert_eq!(string_slice.to_human(), "&mut [String]");
    assert_eq!(string_slice.to_agent(), "&mut[String]");
}

#[test]
fn resizable_utf8_copy_back_accepts_length_changes_but_rejects_invalid_utf8() {
    let program = parse_human(include_str!("../../examples/capsule/grow_utf8_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let args = [NativeValue::MutString("ab".to_string())];

    let grown =
        parse_native_invocation("result=3\nmut:0=97,98,33\n", &program, capsule, &args).unwrap();
    assert_eq!(grown.mutations[0].string.as_deref(), Some("ab!"));

    let shrunk = parse_native_invocation("result=1\nmut:0=97\n", &program, capsule, &args).unwrap();
    assert_eq!(shrunk.mutations[0].string.as_deref(), Some("a"));

    let invalid =
        parse_native_invocation("result=2\nmut:0=255,98\n", &program, capsule, &args).unwrap_err();
    assert!(invalid.contains("not valid UTF-8"), "got: {invalid}");
}

#[test]
fn live_resizable_string_length_cannot_exceed_runner_capacity() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign exceed_string_capacity c version \"c11\":\n    input text: &mut String\n    output changed: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stddef.h>\n        #include <stdint.h>\n        int32_t exceed_string_capacity(uint8_t *text, size_t *text_len, size_t text_capacity) { (void)text; *text_len = text_capacity + 1; return 1; }\n",
    )
    .unwrap();
    let error = invoke_live(
        &program,
        program.capsules().next().unwrap(),
        &[NativeValue::MutString("safe".to_string())],
    )
    .unwrap_err();
    assert!(
        error.contains("resizable string length exceeds capacity"),
        "got: {error}"
    );
}

#[test]
fn aggregate_resizable_string_capacity_rejects_before_toolchain_lookup() {
    let program = parse_human(
        "foreign too_many_strings c version \"c11\":\n    input a: &mut String\n    input b: &mut String\n    input c: &mut String\n    input d: &mut String\n    input e: &mut String\n    output changed: i32\n    sandbox \"fs.none\"\n    code:\n        int too_many_strings(void) { return 0; }\n",
    )
    .unwrap();
    let args = (0..5)
        .map(|_| NativeValue::MutString(String::new()))
        .collect::<Vec<_>>();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &args).unwrap_err();
    assert!(error.contains("5 resizable string inputs"), "got: {error}");
    assert!(error.contains("aggregate capacity limit"), "got: {error}");
}

#[test]
fn mutable_utf8_copy_back_rejects_invalid_utf8_and_length_changes() {
    let program = parse_human(include_str!("../../examples/capsule/title_utf8_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let args = [NativeValue::MutString("ab".to_string())];

    let invalid =
        parse_native_invocation("result=1\nmut:0=255,98\n", &program, capsule, &args).unwrap_err();
    assert!(invalid.contains("not valid UTF-8"), "got: {invalid}");

    let resized =
        parse_native_invocation("result=1\nmut:0=65\n", &program, capsule, &args).unwrap_err();
    assert!(resized.contains("changed byte length"), "got: {resized}");
}

#[test]
fn live_invalid_mutable_utf8_never_reaches_interpreter_state() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign corrupt_utf8 c version \"c11\":\n    input text: &mut str\n    output changed: i32\n    sandbox \"fs.none\"\n    code:\n        #include <stddef.h>\n        #include <stdint.h>\n        int32_t corrupt_utf8(uint8_t *text, size_t text_len) { if (text_len > 0) text[0] = 255; return 1; }\n",
    )
    .unwrap();
    let error = invoke_live(
        &program,
        program.capsules().next().unwrap(),
        &[NativeValue::MutString("safe".to_string())],
    )
    .unwrap_err();
    assert!(error.contains("not valid UTF-8"), "got: {error}");
}

#[test]
fn generated_rust_and_zig_runners_decode_and_validate_string_envelopes() {
    let rust_program = parse_human(include_str!(
        "../../examples/capsule/checksum_utf8_rust.fgx"
    ))
    .unwrap();
    let rust_capsule = rust_program.capsules().next().unwrap();
    let rust_runner =
        super::super::ffi::generate_rust_live_runner(&rust_program, rust_capsule).unwrap();
    assert!(rust_runner.contains("strip_prefix(\"str:\")"));
    assert!(rust_runner.contains("std::str::from_utf8(&bytes)"));
    assert!(rust_runner.contains("text.as_ptr(), text.len()"));

    let zig_program =
        parse_human(include_str!("../../examples/capsule/checksum_utf8_zig.fgx")).unwrap();
    let zig_capsule = zig_program.capsules().next().unwrap();
    let zig_runner =
        super::super::ffi::generate_zig_live_runner(&zig_program, zig_capsule).unwrap();
    assert!(zig_runner.contains("std.mem.startsWith(u8, text, \"str:\")"));
    assert!(zig_runner.contains("std.unicode.utf8ValidateSlice(result)"));
    assert!(zig_runner.contains("text.ptr, text.len"));
}

#[test]
fn borrowed_string_byte_limit_is_rejected_before_compiler_lookup() {
    let program = parse_human(include_str!("../../examples/capsule/checksum_utf8_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let too_large = "x".repeat(16 * 1024 * 1024 + 1);
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Scalar(CapsuleValue::Str(too_large))],
    )
    .unwrap_err();
    assert!(error.contains("native_capsule_string_too_large"), "{error}");
    assert!(error.contains("16777216"), "{error}");
}

#[test]
fn mutable_string_byte_limit_is_rejected_before_compiler_lookup() {
    let program = parse_human(include_str!("../../examples/capsule/title_utf8_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let too_large = "x".repeat(16 * 1024 * 1024 + 1);
    let error = invoke_live(&program, capsule, &[NativeValue::MutString(too_large)]).unwrap_err();
    assert!(error.contains("native_capsule_string_too_large"), "{error}");
    assert!(error.contains("16777216"), "{error}");
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
fn c_and_rust_owned_u32_outputs_carry_values_above_i32_range() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_pixels_u32_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_pixels_u32_rust.fgx"),
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
        // Every element exceeds i32::MAX (2147483647) yet round-trips exactly.
        assert_eq!(
            invocation.output,
            "[3000000000,3000000001,3000000002,3000000003]"
        );
        assert_eq!(invocation.owned_output_bytes, Some(16));
        assert!(invocation.artifact.is_none());
    }
}

#[test]
fn c_rust_and_zig_owned_u64_outputs_are_full_range_and_byte_accounted() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_ids_u64_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_ids_u64_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_ids_u64_zig.fgx"),
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
        assert_eq!(
            invocation.output,
            "[3000000000,5000000000,7000000000,18446744073709551615]"
        );
        assert_eq!(invocation.owned_output_bytes, Some(32));
        assert!(invocation.artifact.is_none());
    }
}

#[test]
fn c_and_rust_owned_i64_outputs_span_64_bit_values() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_marks_i64_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_marks_i64_rust.fgx"),
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
        // Values well outside 32-bit range survive the reviewed copy-back.
        assert_eq!(invocation.output, "[-5000000000,0,5000000000,10000000000]");
        assert_eq!(invocation.owned_output_bytes, Some(32));
        assert!(invocation.artifact.is_none());
    }
}

#[test]
fn c_rust_and_zig_owned_isize_outputs_use_host_pointer_width() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_offsets_isize_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_offsets_isize_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_offsets_isize_zig.fgx"),
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
        assert_eq!(invocation.output, "[-5000000000,0,5000000000,10000000000]");
        let expected_bytes = 4 * std::mem::size_of::<isize>();
        assert_eq!(invocation.owned_output_bytes, Some(expected_bytes));
        assert!(invocation.artifact.is_none());
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(expected_bytes)
        );
    }
}

#[test]
fn c_cpp_rust_and_zig_owned_usize_outputs_use_host_pointer_width() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_sizes_usize_c.fgx"),
        ),
        (
            "cpp",
            crate::native_build::detect_cpp_toolchain().is_some(),
            include_str!("../../examples/capsule/make_sizes_usize_cpp.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_sizes_usize_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_sizes_usize_zig.fgx"),
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
        assert_eq!(
            invocation.output,
            format!("[0,3000000000,6000000000,{}]", usize::MAX)
        );
        let expected_bytes = 4 * std::mem::size_of::<usize>();
        assert_eq!(invocation.owned_output_bytes, Some(expected_bytes));
        assert!(invocation.artifact.is_none());
        assert_eq!(
            invocation
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.owned_output_bytes),
            Some(expected_bytes)
        );
    }
}

#[test]
fn owned_u16_response_rejects_out_of_range_elements() {
    let program = parse_human(
        "foreign bad_u16 c version \"c11\" effects [alloc]:\n    output values: Vec[u16]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedU16 bad_u16(void) { FgxOwnedU16 out = { NULL, 0, 0 }; return out; }\n        void bad_u16_free(uint16_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    // A foreign runner that claims a value outside u16 range is rejected before
    // the vector is exposed, using the same scalar range gate as every element.
    let error = parse_native_invocation(
        "result=70000\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap_err();
    assert!(error.contains("0..65535 for u16"), "got: {error}");
}

#[test]
fn owned_i8_response_rejects_out_of_range_elements() {
    let program = parse_human(
        "foreign bad_i8 c version \"c11\" effects [alloc]:\n    output values: Vec[i8]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedI8 bad_i8(void) { FgxOwnedI8 out = { NULL, 0, 0 }; return out; }\n        void bad_i8_free(int8_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let error = parse_native_invocation(
        "result=200\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap_err();
    assert!(error.contains("-128..127 for i8"), "got: {error}");
}

#[test]
fn owned_u64_response_preserves_values_above_the_signed_range() {
    let program = parse_human(
        "foreign bounded_u64 c version \"c11\" effects [alloc]:\n    output values: Vec[u64]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedU64 bounded_u64(void) { FgxOwnedU64 out = { NULL, 0, 0 }; return out; }\n        void bounded_u64_free(uint64_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let invocation = parse_native_invocation(
        "result=9223372036854775808\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    )
    .unwrap();
    assert_eq!(invocation.output, "[9223372036854775808]");
    assert_eq!(invocation.owned_output_bytes, Some(8));
}

#[test]
fn owned_u64_max_is_copied_before_exactly_once_free() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign upper_u64 c version \"c11\" effects [alloc]:\n    output values: Vec[u64]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdint.h>\n        #include <stdlib.h>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) abort(); }\n        FgxOwnedU64 upper_u64(void) { uint64_t *ptr = (uint64_t *)malloc(sizeof(uint64_t)); if (ptr == NULL) { FgxOwnedU64 empty = { NULL, 0, 0 }; return empty; } ptr[0] = UINT64_MAX; atexit(verify_free_count); FgxOwnedU64 out = { ptr, 1, 1 }; return out; }\n        void upper_u64_free(uint64_t *ptr, size_t len, size_t capacity) { (void)len; (void)capacity; ++free_count; free(ptr); }\n",
    )
    .unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap();
    assert_eq!(invocation.output, "[18446744073709551615]");
    assert_eq!(invocation.owned_output_bytes, Some(8));
}

#[test]
fn owned_usize_response_preserves_the_64_bit_host_range() {
    let program = parse_human(
        "foreign bounded_usize c version \"c11\" effects [alloc]:\n    output values: Vec[usize]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedUsize bounded_usize(void) { FgxOwnedUsize out = { NULL, 0, 0 }; return out; }\n        void bounded_usize_free(size_t *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let result = parse_native_invocation(
        "result=9223372036854775808\n",
        &program,
        program.capsules().next().unwrap(),
        &[],
    );
    if usize::BITS == 64 {
        let invocation = result.unwrap();
        assert_eq!(invocation.output, "[9223372036854775808]");
        assert_eq!(invocation.owned_output_bytes, Some(8));
    } else {
        let error = result.unwrap_err();
        assert!(error.contains("32-bit host range"), "{error}");
    }
}

#[test]
fn owned_usize_max_is_copied_before_exactly_once_free() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign upper_usize c version \"c11\" effects [alloc]:\n    output values: Vec[usize]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdint.h>\n        #include <stdlib.h>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) abort(); }\n        FgxOwnedUsize upper_usize(void) { size_t *ptr = (size_t *)malloc(sizeof(size_t)); if (ptr == NULL) { FgxOwnedUsize empty = { NULL, 0, 0 }; return empty; } ptr[0] = SIZE_MAX; atexit(verify_free_count); FgxOwnedUsize out = { ptr, 1, 1 }; return out; }\n        void upper_usize_free(size_t *ptr, size_t len, size_t capacity) { (void)len; (void)capacity; ++free_count; free(ptr); }\n",
    )
    .unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap();
    assert_eq!(invocation.output, format!("[{}]", usize::MAX));
    assert_eq!(
        invocation.owned_output_bytes,
        Some(std::mem::size_of::<usize>())
    );
}

#[test]
fn cpp_owned_usize_max_is_copied_before_exactly_once_free() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign upper_usize_cpp cpp version \"c++17\" effects [alloc]:\n    output values: Vec[usize]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <cstdint>\n        #include <cstdlib>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) std::abort(); }\n        extern \"C\" FgxOwnedUsize upper_usize_cpp(void) { auto *ptr = static_cast<std::size_t *>(std::malloc(sizeof(std::size_t))); if (ptr == nullptr) { FgxOwnedUsize empty = { nullptr, 0, 0 }; return empty; } ptr[0] = SIZE_MAX; std::atexit(verify_free_count); FgxOwnedUsize out = { ptr, 1, 1 }; return out; }\n        extern \"C\" void upper_usize_cpp_free(std::size_t *ptr, std::size_t len, std::size_t capacity) { (void)len; (void)capacity; ++free_count; std::free(ptr); }\n",
    )
    .unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap();
    assert_eq!(invocation.output, format!("[{}]", usize::MAX));
    assert_eq!(
        invocation.owned_output_bytes,
        Some(std::mem::size_of::<usize>())
    );
}

#[test]
fn c_rust_and_zig_owned_float_outputs_are_finite_typed_and_byte_accounted() {
    let f32_cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_gains_f32_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_gains_f32_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_gains_f32_zig.fgx"),
        ),
    ];
    for (language, available, source) in f32_cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[])
            .unwrap_or_else(|error| panic!("f32 {language}: {error}"));
        assert_eq!(invocation.output, "[0.10000000149011612,-1.5,1.25,3.5]");
        assert_eq!(invocation.owned_output_bytes, Some(16));
        assert!(invocation.artifact.is_none());
    }

    let f64_cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_precise_f64_c.fgx"),
        ),
        (
            "rust",
            crate::native_build::detect_rust_toolchain().is_some(),
            include_str!("../../examples/capsule/make_precise_f64_rust.fgx"),
        ),
        (
            "zig",
            crate::native_build::detect_zig_toolchain().is_some(),
            include_str!("../../examples/capsule/make_precise_f64_zig.fgx"),
        ),
    ];
    for (language, available, source) in f64_cases {
        if !available {
            continue;
        }
        let program = parse_human(source).unwrap();
        let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[])
            .unwrap_or_else(|error| panic!("f64 {language}: {error}"));
        assert_eq!(
            invocation.output,
            "[1.0000000000000002,-2.5,0.0,3.141592653589793]"
        );
        assert_eq!(invocation.owned_output_bytes, Some(32));
        assert!(invocation.artifact.is_none());
    }
}

#[test]
fn owned_float_response_narrows_f32_and_rejects_non_finite_elements() {
    let f32_program = parse_human(
        "foreign float_wire c version \"c11\" effects [alloc]:\n    output values: Vec[f32]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        FgxOwnedF32 float_wire(void) { FgxOwnedF32 out = { NULL, 0, 0 }; return out; }\n        void float_wire_free(float *ptr, size_t len, size_t capacity) { (void)ptr; (void)len; (void)capacity; }\n",
    )
    .unwrap();
    let invocation = parse_native_invocation(
        "result=0.1\n",
        &f32_program,
        f32_program.capsules().next().unwrap(),
        &[],
    )
    .unwrap();
    assert_eq!(invocation.output, "[0.10000000149011612]");
    assert_eq!(invocation.owned_output_bytes, Some(4));

    for (element, descriptor, c_type, token) in [
        ("f32", "FgxOwnedF32", "float", "NaN"),
        ("f64", "FgxOwnedF64", "double", "inf"),
    ] {
        let source = format!(
            "foreign invalid_float c version \"c11\" effects [alloc]:\n    output values: Vec[{element}]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        {descriptor} invalid_float(void) {{ {descriptor} out = {{ NULL, 0, 0 }}; return out; }}\n        void invalid_float_free({c_type} *ptr, size_t len, size_t capacity) {{ (void)ptr; (void)len; (void)capacity; }}\n",
        );
        let program = parse_human(&source).unwrap();
        let error = parse_native_invocation(
            &format!("result={token}\n"),
            &program,
            program.capsules().next().unwrap(),
            &[],
        )
        .unwrap_err();
        assert!(error.contains("non-finite"), "{element}: {error}");
    }
}

#[test]
fn c_cpp_rust_and_zig_owned_strings_are_utf8_validated_and_byte_accounted() {
    let cases = [
        (
            "c",
            crate::native_build::detect_toolchain().is_some(),
            include_str!("../../examples/capsule/make_greeting_c.fgx"),
        ),
        (
            "cpp",
            crate::native_build::detect_cpp_toolchain().is_some(),
            include_str!("../../examples/capsule/make_greeting_cpp.fgx"),
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
fn cpp_invalid_utf8_is_copied_and_freed_before_parent_rejection() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "foreign invalid_text_cpp cpp version \"c++17\" effects [alloc]:\n    output text: str\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <cstddef>\n        #include <cstdint>\n        #include <cstdlib>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) std::abort(); }\n        extern \"C\" FgxOwnedStr invalid_text_cpp(void) { auto *ptr = static_cast<std::uint8_t *>(std::malloc(1)); if (ptr == nullptr) { FgxOwnedStr empty = { nullptr, 0, 0 }; return empty; } ptr[0] = 255; std::atexit(verify_free_count); FgxOwnedStr out = { ptr, 1, 1 }; return out; }\n        extern \"C\" void invalid_text_cpp_free(std::uint8_t *ptr, std::size_t len, std::size_t capacity) { (void)len; (void)capacity; ++free_count; std::free(ptr); }\n",
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
fn zig_owned_float_runners_use_exact_descriptors_and_typed_copies() {
    for (source, descriptor, element) in [
        (
            include_str!("../../examples/capsule/make_gains_f32_zig.fgx"),
            "capsule.FgxOwnedF32",
            "f32",
        ),
        (
            include_str!("../../examples/capsule/make_precise_f64_zig.fgx"),
            "capsule.FgxOwnedF64",
            "f64",
        ),
    ] {
        let program = parse_human(source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
        assert!(
            runner.contains(&format!("const result: {descriptor}")),
            "{runner}"
        );
        assert!(
            runner.contains(&format!("allocator.alloc({element}, result.len)")),
            "{runner}"
        );
        assert!(runner.contains("result.ptr.?[0..result.len]"), "{runner}");
        assert_eq!(runner.matches("_free(result.ptr").count(), 2, "{runner}");
    }
}

#[test]
fn zig_owned_isize_runner_uses_exact_descriptor_and_typed_copy() {
    let program = parse_human(include_str!(
        "../../examples/capsule/make_offsets_isize_zig.fgx"
    ))
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(
        runner.contains("const result: capsule.FgxOwnedIsize"),
        "{runner}"
    );
    assert!(
        runner.contains("allocator.alloc(isize, result.len)"),
        "{runner}"
    );
    assert!(runner.contains("result.ptr.?[0..result.len]"), "{runner}");
    assert_eq!(
        runner
            .matches("capsule.make_offsets_isize_zig_free")
            .count(),
        2,
        "{runner}"
    );
}

#[test]
fn zig_owned_usize_runner_uses_exact_descriptor_and_typed_copy() {
    let program = parse_human(include_str!(
        "../../examples/capsule/make_sizes_usize_zig.fgx"
    ))
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(
        runner.contains("const result: capsule.FgxOwnedUsize"),
        "{runner}"
    );
    assert!(
        runner.contains("allocator.alloc(usize, result.len)"),
        "{runner}"
    );
    assert!(runner.contains("result.ptr.?[0..result.len]"), "{runner}");
    assert_eq!(
        runner.matches("capsule.make_sizes_usize_zig_free").count(),
        2,
        "{runner}"
    );
}

#[test]
fn zig_owned_u64_runner_uses_exact_descriptor_and_typed_copy() {
    let program = parse_human(include_str!("../../examples/capsule/make_ids_u64_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(
        runner.contains("const result: capsule.FgxOwnedU64"),
        "{runner}"
    );
    assert!(
        runner.contains("allocator.alloc(u64, result.len)"),
        "{runner}"
    );
    assert!(runner.contains("result.ptr.?[0..result.len]"), "{runner}");
    assert_eq!(
        runner.matches("capsule.make_ids_u64_zig_free").count(),
        2,
        "{runner}"
    );
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
fn zig_owned_isize_output_rejects_a_mismatched_descriptor() {
    let program = parse_human(
        "foreign bad_zig_offsets zig version \"0.14\" effects [alloc]:\n    output offsets: Vec[isize]\n    sandbox \"fs.none\"\n    code:\n        pub const FgxOwnedIsize = extern struct { ptr: ?[*]i64, len: usize, capacity: usize, };\n        pub export fn bad_zig_offsets() FgxOwnedIsize { return .{ .ptr = null, .len = 0, .capacity = 0 }; }\n        pub export fn bad_zig_offsets_free(ptr: ?[*]i64, len: usize, capacity: usize) void { _ = ptr; _ = len; _ = capacity; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedIsize"), "got: {error}");
    assert!(error.contains("Vec[isize]"), "got: {error}");
}

#[test]
fn zig_owned_usize_output_rejects_a_mismatched_descriptor() {
    let program = parse_human(
        "foreign bad_zig_sizes zig version \"0.14\" effects [alloc]:\n    output sizes: Vec[usize]\n    sandbox \"fs.none\"\n    code:\n        pub const FgxOwnedUsize = extern struct { ptr: ?[*]isize, len: usize, capacity: usize, };\n        pub export fn bad_zig_sizes() FgxOwnedUsize { return .{ .ptr = null, .len = 0, .capacity = 0 }; }\n        pub export fn bad_zig_sizes_free(ptr: ?[*]isize, len: usize, capacity: usize) void { _ = ptr; _ = len; _ = capacity; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedUsize"), "got: {error}");
    assert!(error.contains("Vec[usize]"), "got: {error}");
}

#[test]
fn zig_owned_u64_output_rejects_a_mismatched_descriptor() {
    let program = parse_human(
        "foreign bad_zig_ids zig version \"0.14\" effects [alloc]:\n    output ids: Vec[u64]\n    sandbox \"fs.none\"\n    code:\n        pub const FgxOwnedU64 = extern struct { ptr: ?[*]i64, len: usize, capacity: usize, };\n        pub export fn bad_zig_ids() FgxOwnedU64 { return .{ .ptr = null, .len = 0, .capacity = 0 }; }\n        pub export fn bad_zig_ids_free(ptr: ?[*]i64, len: usize, capacity: usize) void { _ = ptr; _ = len; _ = capacity; }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedU64"), "got: {error}");
    assert!(error.contains("Vec[u64]"), "got: {error}");
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
    let failure = invoke_live_audited(&program, program.capsules().next().unwrap(), &[])
        .expect_err("missing owned-output deallocator must fail compilation");
    let error = &failure.message;
    assert!(
        error.contains("native_capsule_compile_failed"),
        "got: {error}"
    );
    assert!(error.contains("missing_free_free"), "got: {error}");
    assert_eq!(failure.receipt.phase, "compile");
    assert_eq!(failure.receipt.containment.as_deref(), Some("process_tree"));
    assert_eq!(
        failure.receipt.compile_wall_limit_ms,
        crate::ffi::NATIVE_COMPILE_WALL_MS
    );
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
fn owned_record_wire_requires_complete_records_and_accounts_abi_bytes() {
    let program = parse_human(include_str!("../../examples/capsule/make_records_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = parse_native_invocation(
        "result=7,-3,1.25,18446744073709551615,9,-2.5\n",
        &program,
        capsule,
        &[],
    )
    .unwrap();
    assert_eq!(
        invocation.output,
        "[{\"id\":7,\"delta\":-3,\"score\":1.25},{\"id\":18446744073709551615,\"delta\":9,\"score\":-2.5}]"
    );
    assert_eq!(invocation.owned_output_bytes, Some(48));
    assert!(invocation.owned_output.is_none());
    assert!(invocation.artifact.is_none());

    let error = parse_native_invocation("result=7,-3\n", &program, capsule, &[]).unwrap_err();
    assert!(error.contains("expected a multiple of 3"), "got: {error}");
}

#[test]
fn mutable_record_wire_requires_complete_fixed_count_records() {
    let program = parse_human(include_str!("../../examples/capsule/mutate_records_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let args = [NativeValue::MutStructSlice(vec![
        vec![
            CapsuleValue::UInt(7),
            CapsuleValue::Int(3),
            CapsuleValue::Float(1.25),
        ],
        vec![
            CapsuleValue::UInt(9),
            CapsuleValue::Int(-4),
            CapsuleValue::Float(-2.5),
        ],
    ])];
    let invocation = parse_native_invocation(
        "result=2\nmut:0=7,-17,1.25,18446744073709551615,-4,42.5\n",
        &program,
        capsule,
        &args,
    )
    .unwrap();
    assert_eq!(invocation.mutations.len(), 1);
    assert_eq!(
        invocation.mutations[0].records.as_ref().unwrap()[1][0],
        CapsuleValue::UInt(u64::MAX)
    );

    let incomplete = parse_native_invocation(
        "result=2\nmut:0=7,-17,1.25,9,-4\n",
        &program,
        capsule,
        &args,
    )
    .unwrap_err();
    assert!(
        incomplete.contains("incomplete record"),
        "got: {incomplete}"
    );

    let resized = parse_native_invocation("result=2\nmut:0=7,-17,1.25\n", &program, capsule, &args)
        .unwrap_err();
    assert!(resized.contains("changed record count"), "got: {resized}");

    let invalid_leaf = parse_native_invocation(
        "result=2\nmut:0=7,-17,nan,9,-4,42.5\n",
        &program,
        capsule,
        &args,
    )
    .unwrap_err();
    assert!(invalid_leaf.contains("non-finite"), "got: {invalid_leaf}");
}

#[test]
fn owned_record_wire_rebuilds_nested_records_and_fixed_arrays_atomically() {
    let program = parse_human(
        "@repr(C)\nstruct Pair:\n    values: [i16; 2]\n\n@repr(C)\nstruct Batch:\n    id: u32\n    pair: Pair\n    totals: [i32; 2]\n\nforeign batches c version \"c11\" effects [alloc]:\n    output values: Vec[Batch]\n    sandbox \"fs.none\"\n    code:\n        int unused(void) { return 0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation =
        parse_native_invocation("result=9,-2,3,100,-200\n", &program, capsule, &[]).unwrap();
    assert_eq!(
        invocation.output,
        "[{\"id\":9,\"pair\":{\"values\":[-2,3]},\"totals\":[100,-200]}]"
    );
    assert_eq!(invocation.owned_output_bytes, Some(16));

    let error = parse_native_invocation("result=9,-2,3,100\n", &program, capsule, &[]).unwrap_err();
    assert!(error.contains("expected a multiple of 5"), "got: {error}");
}

#[test]
fn c_owned_record_output_copies_before_exactly_once_free() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let source = include_str!("../../examples/capsule/make_records_c.fgx").replace(
        "#include <stdlib.h>",
        "#include <stdlib.h>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) abort(); }",
    ).replace(
        "FgxOwnedNativeRecord result = { ptr, 2, 2 };",
        "atexit(verify_free_count);\n            FgxOwnedNativeRecord result = { ptr, 2, 2 };",
    ).replace(
        "(void)len; (void)capacity; free(ptr);",
        "(void)len; (void)capacity; ++free_count; free(ptr);",
    );
    let program = parse_human(&source).unwrap();
    let invocation = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap();
    assert_eq!(invocation.owned_output_bytes, Some(48));
    assert!(invocation.output.contains("18446744073709551615"));
}

#[test]
fn oversized_owned_record_descriptor_is_freed_once_before_rejection() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(
        "@repr(C)\nstruct Item:\n    value: u64\n\nforeign huge_records c version \"c11\" effects [alloc]:\n    output items: Vec[Item]\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        #include <stdlib.h>\n        static int free_count = 0;\n        static void verify_free_count(void) { if (free_count != 1) abort(); }\n        FgxOwnedItem huge_records(void) { Item *ptr = (Item *)malloc(sizeof(Item)); atexit(verify_free_count); FgxOwnedItem out = { ptr, 8388609, 8388609 }; return out; }\n        void huge_records_free(Item *ptr, size_t len, size_t capacity) { (void)len; (void)capacity; ++free_count; free(ptr); }\n",
    )
    .unwrap();
    let error = invoke_live(&program, program.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(
        error.contains("invalid owned aggregate descriptor"),
        "got: {error}"
    );
}

#[test]
fn cpp_owned_record_plan_requires_typed_descriptor_and_c_linkage() {
    let program = parse_human(include_str!("../../examples/capsule/make_records_cpp.fgx")).unwrap();
    let plan = run(
        "make_records_cpp.fgx",
        &program,
        None,
        &out_dir("owned_record_cpp_plan"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "FgxOwnedNativeRecord make_records_cpp(void)"
    );
    assert!(plan.marshaling.iter().any(|item| {
        item.c_type.contains("FgxOwnedNativeRecord")
            && item.c_type.contains("recursive layout validation")
    }));
    let _ = std::fs::remove_dir_all(out_dir("owned_record_cpp_plan"));
}

#[test]
fn rust_owned_record_runner_uses_uninitialized_byte_copy_before_free() {
    let program =
        parse_human(include_str!("../../examples/capsule/make_records_rust.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_rust_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("capsule::FgxOwnedNativeRecord"));
    assert!(runner.contains("Vec<std::mem::MaybeUninit<capsule::NativeRecord>>"));
    assert!(runner.contains("std::ptr::copy_nonoverlapping"));
    assert_eq!(runner.matches("capsule::make_records_rust_free").count(), 2);
}

#[test]
fn zig_owned_record_runner_uses_typed_copy_before_free() {
    let program = parse_human(include_str!("../../examples/capsule/make_records_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains("capsule.FgxOwnedNativeRecord"));
    assert!(runner.contains("allocator.alloc(capsule.NativeRecord, result.len)"));
    assert!(runner.contains("result.ptr.?[0..result.len]"));
    assert_eq!(runner.matches("capsule.make_records_zig_free").count(), 2);
}

#[test]
fn aggregate_descriptor_source_is_validated_before_toolchain_lookup() {
    let rust = parse_human(
        "@repr(C)\nstruct Item:\n    value: i32\n\nforeign bad rust version \"2021\" effects [alloc]:\n    output items: Vec[Item]\n    sandbox \"fs.none\"\n    code:\n        #[repr(C)]\n        pub struct Item { pub value: i32, }\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn bad() -> i32 { 0 }\n",
    )
    .unwrap();
    let error = invoke_live(&rust, rust.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedItem"), "got: {error}");

    let zig = parse_human(
        "@repr(C)\nstruct Item:\n    value: i32\n\nforeign bad_zig zig version \"0.14\" effects [alloc]:\n    output items: Vec[Item]\n    sandbox \"fs.none\"\n    code:\n        pub const Item = extern struct { value: i32, };\n        pub export fn bad_zig() i32 { return 0; }\n",
    )
    .unwrap();
    let error = invoke_live(&zig, zig.capsules().next().unwrap(), &[]).unwrap_err();
    assert!(error.contains("FgxOwnedItem"), "got: {error}");
}

#[test]
fn aggregate_slice_signature_and_marshaling_use_a_const_typed_pointer() {
    let program = parse_human(include_str!("../../examples/capsule/record_slice_c.fgx")).unwrap();
    let plan = run(
        "record_slice_c.fgx",
        &program,
        None,
        &out_dir("record_slice_plan"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "uint64_t last_record_id_c(const InputRecord *records, size_t records_len)"
    );
    assert!(plan
        .marshaling
        .iter()
        .any(|item| { item.name == "records" && item.c_type == "const InputRecord* + size_t" }));
    let _ = std::fs::remove_dir_all(out_dir("record_slice_plan"));
}

#[test]
fn aggregate_slice_leaf_shape_and_abi_size_fail_before_compiler_lookup() {
    let program = parse_human(include_str!("../../examples/capsule/record_slice_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::ReadStructSlice(vec![vec![
            CapsuleValue::UInt(7),
        ]])],
    )
    .unwrap_err();
    assert!(error.contains("expects 3 scalar leaf"), "got: {error}");

    let oversized = parse_human(
        "@repr(C)\nstruct Huge:\n    values: [u64; 1024]\n\nforeign inspect_huge c version \"c11\":\n    input values: &[Huge]\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        int32_t inspect_huge(const Huge *values, size_t values_len) { (void)values; return (int32_t)values_len; }\n",
    )
    .unwrap();
    let error = invoke_live(
        &oversized,
        oversized.capsules().next().unwrap(),
        &[NativeValue::ReadStructSlice(vec![Vec::new(); 8193])],
    )
    .unwrap_err();
    assert!(error.contains("limit is 8192"), "got: {error}");
}

#[test]
fn mutable_aggregate_slice_uses_a_typed_pointer_and_mutable_carrier() {
    let program = parse_human(include_str!("../../examples/capsule/mutate_records_c.fgx")).unwrap();
    let plan = run(
        "mutate_records_c.fgx",
        &program,
        None,
        &out_dir("mutate_records_plan"),
    )
    .unwrap();
    assert_eq!(
        plan.extern_c_signature,
        "uint64_t mutate_records_c(MutableRecord *records, size_t records_len)"
    );
    assert!(
        plan.marshaling
            .iter()
            .any(|item| item.name == "records"
                && item.c_type == "MutableRecord* + size_t (copy-back)")
    );

    let error = invoke_live(
        &program,
        program.capsules().next().unwrap(),
        &[NativeValue::ReadStructSlice(vec![vec![CapsuleValue::Int(
            1,
        )]])],
    )
    .unwrap_err();
    assert!(
        error.contains("expects a mutable aggregate slice"),
        "got: {error}"
    );
    let _ = std::fs::remove_dir_all(out_dir("mutate_records_plan"));
}

#[test]
fn rust_and_zig_aggregate_slice_runners_allocate_reviewed_records() {
    let rust = parse_human(include_str!("../../examples/capsule/record_slice_rust.fgx")).unwrap();
    let runner =
        super::super::ffi::generate_rust_live_runner(&rust, rust.capsules().next().unwrap())
            .unwrap();
    assert!(runner.contains("Vec<capsule::InputRecord>"), "{runner}");
    assert!(runner.contains("leaf_values % 3"), "{runner}");
    assert!(runner.contains("records.as_ptr()"), "{runner}");

    let zig = parse_human(include_str!("../../examples/capsule/record_slice_zig.fgx")).unwrap();
    let runner =
        super::super::ffi::generate_zig_live_runner(&zig, zig.capsules().next().unwrap()).unwrap();
    assert!(runner.contains("[]capsule.InputRecord"), "{runner}");
    assert!(runner.contains("leaf_values % 3"), "{runner}");
    assert!(runner.contains("records.ptr"), "{runner}");

    let rust = parse_human(include_str!(
        "../../examples/capsule/mutate_records_rust.fgx"
    ))
    .unwrap();
    let runner =
        super::super::ffi::generate_rust_live_runner(&rust, rust.capsules().next().unwrap())
            .unwrap();
    assert!(runner.contains("records.as_mut_ptr()"), "{runner}");
    assert!(runner.contains("print!(\"mut:0=\")"), "{runner}");

    let zig = parse_human(include_str!(
        "../../examples/capsule/mutate_records_zig.fgx"
    ))
    .unwrap();
    let runner =
        super::super::ffi::generate_zig_live_runner(&zig, zig.capsules().next().unwrap()).unwrap();
    assert!(runner.contains("try stdout.print(\"mut:0=\""), "{runner}");
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

#[test]
fn live_c_nested_struct_boundary_flattens_and_rebuilds_scalar_leaves() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/nested_bounds_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(
        invocation.output,
        "{\"id\":7,\"bounds\":{\"min_x\":0,\"max_x\":30,\"min_y\":-10,\"max_y\":60}}"
    );
}

#[test]
fn live_rust_nested_struct_boundary_flattens_and_rebuilds_scalar_leaves() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!(
        "../../examples/capsule/nested_bounds_rust.fgx"
    ))
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(
        invocation.output,
        "{\"id\":7,\"bounds\":{\"min_x\":0,\"max_x\":30,\"min_y\":-10,\"max_y\":60}}"
    );
}

#[test]
fn rust_nested_struct_requires_every_reachable_declaration_to_match() {
    let program = parse_human(
        "@repr(C)\nstruct Inner:\n    left: i16\n    right: i32\n\n@repr(C)\nstruct Outer:\n    tag: i32\n    inner: Inner\n\nforeign bad rust version \"2021\":\n    input value: Outer\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #[repr(C)]\n        pub struct Inner { pub right: i32, pub left: i16, }\n        #[repr(C)]\n        pub struct Outer { pub tag: i32, pub inner: Inner, }\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn bad(value: Outer) -> i32 { value.tag }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
        ])],
    )
    .unwrap_err();
    assert!(
        error.contains("Rust struct 'Inner' fields must exactly match"),
        "got: {error}"
    );
}

#[test]
fn zig_nested_struct_runner_constructs_and_serializes_scalar_leaves() {
    let program =
        parse_human(include_str!("../../examples/capsule/nested_bounds_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains(".bounds = capsule.Bounds"));
    assert!(runner.contains(".min_x = try parseArg(i16"));
    assert!(runner.contains("result.bounds.max_y"));
}

#[test]
fn live_zig_nested_struct_boundary_flattens_and_rebuilds_scalar_leaves() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program =
        parse_human(include_str!("../../examples/capsule/nested_bounds_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(
        invocation.output,
        "{\"id\":7,\"bounds\":{\"min_x\":0,\"max_x\":30,\"min_y\":-10,\"max_y\":60}}"
    );
}

#[test]
fn live_c_fixed_array_struct_field_round_trips_by_value() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/fixed_samples_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(invocation.output, "{\"id\":7,\"samples\":[-9,22,-27,44]}");
}

#[test]
fn live_cpp_fixed_array_struct_field_round_trips_by_value() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program =
        parse_human(include_str!("../../examples/capsule/fixed_samples_cpp.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(invocation.output, "{\"id\":7,\"samples\":[-9,22,-27,44]}");
}

#[test]
fn live_rust_fixed_array_struct_field_round_trips_by_value() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!(
        "../../examples/capsule/fixed_samples_rust.fgx"
    ))
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(invocation.output, "{\"id\":7,\"samples\":[-9,22,-27,44]}");
}

#[test]
fn fixed_array_zig_runner_uses_exact_array_type_and_leaf_order() {
    let program =
        parse_human(include_str!("../../examples/capsule/fixed_samples_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains(".samples = .{ try parseArg(i16"));
    assert!(runner.contains("result.samples[0]"));
    assert!(runner.contains("result.samples[3]"));
}

#[test]
fn live_zig_fixed_array_struct_field_round_trips_by_value() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program =
        parse_human(include_str!("../../examples/capsule/fixed_samples_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(7),
            CapsuleValue::Int(-10),
            CapsuleValue::Int(20),
            CapsuleValue::Int(-30),
            CapsuleValue::Int(40),
        ])],
    )
    .unwrap();
    assert_eq!(invocation.output, "{\"id\":7,\"samples\":[-9,22,-27,44]}");
}

fn point_batch_values() -> Vec<NativeValue> {
    vec![NativeValue::Struct(vec![
        CapsuleValue::Int(7),
        CapsuleValue::Int(-10),
        CapsuleValue::Int(20),
        CapsuleValue::Int(-30),
        CapsuleValue::Int(40),
        CapsuleValue::Int(-50),
        CapsuleValue::Int(60),
    ])]
}

const SHIFTED_POINT_BATCH: &str =
    "{\"id\":7,\"points\":[{\"x\":-9,\"y\":19},{\"x\":-28,\"y\":38},{\"x\":-47,\"y\":57}]}";

#[test]
fn live_c_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/struct_array_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_batch_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_BATCH);
}

#[test]
fn live_cpp_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/struct_array_cpp.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_batch_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_BATCH);
}

#[test]
fn live_rust_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program =
        parse_human(include_str!("../../examples/capsule/struct_array_rust.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_batch_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_BATCH);
}

#[test]
fn struct_array_zig_runner_uses_exact_nested_layout_and_leaf_order() {
    let program = parse_human(include_str!("../../examples/capsule/struct_array_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains(".points = .{"));
    assert!(runner.contains("capsule.NativePoint {"));
    assert!(runner.contains("result.points[0].x"));
    assert!(runner.contains("result.points[2].y"));
}

#[test]
fn live_zig_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/struct_array_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_batch_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_BATCH);
}

fn point_grid_values() -> Vec<NativeValue> {
    vec![NativeValue::Struct(vec![
        CapsuleValue::Int(9),
        CapsuleValue::Int(-10),
        CapsuleValue::Int(20),
        CapsuleValue::Int(-30),
        CapsuleValue::Int(40),
        CapsuleValue::Int(-50),
        CapsuleValue::Int(60),
        CapsuleValue::Int(-70),
        CapsuleValue::Int(80),
    ])]
}

const SHIFTED_POINT_GRID: &str =
    "{\"id\":9,\"points\":[[{\"x\":-9,\"y\":19},{\"x\":-28,\"y\":38}],[{\"x\":-47,\"y\":57},{\"x\":-66,\"y\":76}]]}";

#[test]
fn live_c_nested_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/nested_array_c.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_grid_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_GRID);
}

#[test]
fn live_cpp_nested_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_cpp_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/nested_array_cpp.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_grid_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_GRID);
}

#[test]
fn live_rust_nested_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_rust_toolchain().is_none() {
        return;
    }
    let program =
        parse_human(include_str!("../../examples/capsule/nested_array_rust.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_grid_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_GRID);
}

#[test]
fn nested_struct_array_zig_runner_uses_exact_dimensions_and_leaf_order() {
    let program = parse_human(include_str!("../../examples/capsule/nested_array_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let runner = super::super::ffi::generate_zig_live_runner(&program, capsule).unwrap();
    assert!(runner.contains(".points = .{"));
    assert!(runner.contains("result.points[0][0].x"));
    assert!(runner.contains("result.points[1][1].y"));
}

#[test]
fn live_zig_nested_struct_array_field_round_trips_by_value() {
    if crate::native_build::detect_zig_toolchain().is_none() {
        return;
    }
    let program = parse_human(include_str!("../../examples/capsule/nested_array_zig.fgx")).unwrap();
    let capsule = program.capsules().next().unwrap();
    let invocation = invoke_live(&program, capsule, &point_grid_values()).unwrap();
    assert_eq!(invocation.output, SHIFTED_POINT_GRID);
}

#[test]
fn zig_nested_struct_array_requires_the_exact_foreign_dimensions() {
    let program = parse_human(
        "@repr(C)\nstruct Point:\n    x: i16\n@repr(C)\nstruct Grid:\n    points: [[Point; 2]; 2]\n\nforeign bad zig version \"0.14\":\n    input grid: Grid\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub const Point = extern struct { x: i16, };\n        pub const Grid = extern struct { points: [2][3]Point, };\n        pub export fn bad(grid: Grid) i32 { return grid.points[0][0].x; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
            CapsuleValue::Int(3),
            CapsuleValue::Int(4),
        ])],
    )
    .unwrap_err();
    assert!(
        error.contains("Zig struct 'Grid' fields must exactly match"),
        "got: {error}"
    );
}

#[test]
fn rust_struct_array_requires_the_exact_foreign_array_layout() {
    let program = parse_human(
        "@repr(C)\nstruct Point:\n    x: i16\n    y: i16\n@repr(C)\nstruct Batch:\n    points: [Point; 3]\n\nforeign bad rust version \"2021\":\n    input batch: Batch\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #[repr(C)]\n        pub struct Point { pub x: i16, pub y: i16, }\n        #[repr(C)]\n        pub struct Batch { pub points: [Point; 2], }\n        #[unsafe(no_mangle)]\n        pub extern \"C\" fn bad(batch: Batch) -> i32 { batch.points[0].x as i32 }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
            CapsuleValue::Int(3),
            CapsuleValue::Int(4),
            CapsuleValue::Int(5),
            CapsuleValue::Int(6),
        ])],
    )
    .unwrap_err();
    assert!(
        error.contains("Rust struct 'Batch' fields must exactly match"),
        "got: {error}"
    );
}

#[test]
fn zig_struct_array_requires_the_exact_foreign_array_layout() {
    let program = parse_human(
        "@repr(C)\nstruct Point:\n    x: i16\n    y: i16\n@repr(C)\nstruct Batch:\n    points: [Point; 3]\n\nforeign bad zig version \"0.14\":\n    input batch: Batch\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub const Point = extern struct { x: i16, y: i16, };\n        pub const Batch = extern struct { points: [2]Point, };\n        pub export fn bad(batch: Batch) i32 { return batch.points[0].x; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
            CapsuleValue::Int(3),
            CapsuleValue::Int(4),
            CapsuleValue::Int(5),
            CapsuleValue::Int(6),
        ])],
    )
    .unwrap_err();
    assert!(
        error.contains("Zig struct 'Batch' fields must exactly match"),
        "got: {error}"
    );
}

#[test]
fn recursively_array_embedded_native_struct_is_rejected_without_recursing_forever() {
    let program = parse_human(
        "@repr(C)\nstruct Loop:\n    next: [Loop; 1]\n\nforeign bad c version \"c11\":\n    input value: Loop\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        int32_t bad(Loop value) { return 0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(&program, capsule, &[NativeValue::Struct(Vec::new())]).unwrap_err();
    assert!(error.contains("recursively embedded"), "got: {error}");
}

#[test]
fn zig_fixed_array_struct_requires_the_exact_foreign_array_layout() {
    let program = parse_human(
        "@repr(C)\nstruct Window:\n    samples: [i16; 4]\n\nforeign bad zig version \"0.14\":\n    input window: Window\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        pub const Window = extern struct { samples: [3]i16, };\n        pub export fn bad(window: Window) i32 { return window.samples[0]; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(
        &program,
        capsule,
        &[NativeValue::Struct(vec![
            CapsuleValue::Int(1),
            CapsuleValue::Int(2),
            CapsuleValue::Int(3),
            CapsuleValue::Int(4),
        ])],
    )
    .unwrap_err();
    assert!(error.contains("fields must exactly match"), "got: {error}");
}

#[test]
fn native_struct_fixed_arrays_reject_zero_length_nested_array_and_excessive_shapes() {
    let cases = [
        (
            "@repr(C)\nstruct Bad:\n    values: [i16; 0]\n",
            "zero-length array",
        ),
        (
            "@repr(C)\nstruct Bad:\n    values: [[i16; 0]; 2]\n",
            "zero-length nested array",
        ),
        (
            "@repr(C)\nstruct Point:\n    x: i16\n    y: i16\n@repr(C)\nstruct Bad:\n    values: [Point; 513]\n",
            "scalar leaf limit",
        ),
    ];
    for (declarations, expected) in cases {
        let source = format!(
            "{declarations}\nforeign bad c version \"c11\":\n    input value: Bad\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        int32_t bad(Bad value) {{ return 0; }}\n"
        );
        let program = parse_human(&source).unwrap();
        let capsule = program.capsules().next().unwrap();
        let error = invoke_live(&program, capsule, &[NativeValue::Struct(Vec::new())]).unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn native_struct_nested_arrays_reject_excessive_array_depth() {
    let mut ty = "i16".to_string();
    for _ in 0..17 {
        ty = format!("[{ty}; 1]");
    }
    let source = format!(
        "@repr(C)\nstruct Bad:\n    values: {ty}\n\nforeign bad c version \"c11\":\n    input value: Bad\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        int32_t bad(Bad value) {{ return 0; }}\n"
    );
    let program = parse_human(&source).unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(&program, capsule, &[NativeValue::Struct(Vec::new())]).unwrap_err();
    assert!(
        error.contains("array nesting depth limit 16"),
        "got: {error}"
    );
}

#[test]
fn recursively_embedded_native_struct_is_rejected_without_recursing_forever() {
    let program = parse_human(
        "@repr(C)\nstruct Loop:\n    next: Loop\n\nforeign bad c version \"c11\":\n    input value: Loop\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        #include \"types.h\"\n        int32_t bad(Loop value) { return 0; }\n",
    )
    .unwrap();
    let capsule = program.capsules().next().unwrap();
    let error = invoke_live(&program, capsule, &[NativeValue::Struct(Vec::new())]).unwrap_err();
    assert!(error.contains("recursively embedded"), "got: {error}");
}
