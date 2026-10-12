use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fgx(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fgx"))
        .args(args)
        .current_dir(manifest_dir())
        .output()
        .expect("fgx should run")
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_{}", std::process::id(), name))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn assert_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "expected success\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    stdout(&output)
}

fn assert_failure(output: Output) -> String {
    assert!(
        !output.status.success(),
        "expected failure\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    format!("{}{}", stdout(&output), stderr(&output))
}

#[test]
fn ast_success_and_failure() {
    let out = assert_success(fgx(&["ast", "examples/hello.fgx", "--canonical"]));
    assert!(out.contains("\"schema\": \"forgentex.ast.v1\""));
    assert!(out.contains("\"id\": \"@function.area.stmt.0.expr.left\""));

    let err = assert_failure(fgx(&["ast", "examples/nope.fgx", "--canonical"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn diagnose_success_and_failure() {
    let out = assert_success(fgx(&["diagnose", "examples/hello.fgx", "--json"]));
    assert!(out.contains("\"schema\": \"forgentex.diagnose.v1\""));
    assert!(out.contains("\"tokens\":"));
    assert!(out.contains("\"source_map\":"));
    assert!(out.contains("\"canonical_ast\":"));
    assert!(out.contains("@function.area.stmt.0.expr.left"));

    let err = assert_failure(fgx(&["diagnose", "examples/missing.fgx", "--json"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn diagnostic_catalog_and_fix_plan_commands_are_available() {
    let catalog = assert_success(fgx(&["diagnostics", "list", "--json"]));
    assert!(catalog.contains("\"schema\": \"forgentex.diagnostic_catalog.v1\""));
    assert!(catalog.contains("\"code\": \"TYP001\""));
    assert!(catalog.contains("\"canonical_code\": \"return_type_mismatch\""));

    let explain = assert_success(fgx(&["explain", "TYP001", "--json"]));
    assert!(explain.contains("\"schema\": \"forgentex.diagnostic_explain.v1\""));
    assert!(explain.contains("\"rule\": \"return_assignability\""));

    let by_name = assert_success(fgx(&["explain", "unsafe_required"]));
    assert!(by_name.contains("ASM001"));
    assert!(by_name.contains("human-review"));

    let source_path = temp_file("stage29_fix_plan.fgx");
    fs::write(
        &source_path,
        "fn bad() -> i32:\n    let x = 1\n    return 1.0\n",
    )
    .unwrap();
    let source_path_text = source_path.to_string_lossy().to_string();
    let plan = assert_success(fgx(&["fix", "--plan", &source_path_text, "--json"]));
    assert!(plan.contains("\"schema\": \"forgentex.fix_plan.v1\""));
    assert!(plan.contains("\"code\": \"TYP002\""));
    assert!(plan.contains("\"operation\": \"add_local_type_annotation\""));
    assert!(plan.contains("\"operation\": \"replace_return_type\""));
    let _ = fs::remove_file(source_path);

    let asm_path = temp_file("stage29_fix_asm.fgx");
    fs::write(
        &asm_path,
        "fn bad(x: u64) -> u64:\n    asm x86_64:\n        in rax: x\n        out rax: y\n        op popcnt rax rax\n    return y\n",
    )
    .unwrap();
    let asm_path_text = asm_path.to_string_lossy().to_string();
    let asm_plan = assert_success(fgx(&[
        "fix",
        "--plan",
        &asm_path_text,
        "--target",
        "x86_64-pc-windows-msvc",
        "--json",
    ]));
    assert!(asm_plan.contains("\"code\": \"ASM001\""));
    assert!(asm_plan.contains("\"safety\": \"human-review\""));
    assert!(asm_plan.contains("OP MARK_FUNCTION_UNSAFE"));
    let _ = fs::remove_file(asm_path);

    let err = assert_failure(fgx(&["explain", "NOPE999"]));
    assert!(err.contains("unknown diagnostic code"));
}

#[test]
fn parse_success_and_failure() {
    let out = assert_success(fgx(&["parse", "examples/hello.fgx", "--emit-ast"]));
    assert!(out.contains("\"id\": \"@struct.Point\""));

    let err = assert_failure(fgx(&["parse", "examples/nope.fgx", "--emit-ast"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn fmt_success_and_failure() {
    let out = assert_success(fgx(&["fmt", "examples/hello.fgx", "--to", "fgai"]));
    assert!(out.contains("F area(width i32, height i32) -> i32"));

    let err = assert_failure(fgx(&["fmt", "examples/hello.fgx", "--to", "wat"]));
    assert!(err.contains("unknown format target"));
}

#[test]
fn roundtrip_success_and_failure() {
    let out = assert_success(fgx(&[
        "roundtrip",
        "examples/media/audio_mix.fgx",
        "--to",
        "fgai",
    ]));
    assert!(out.contains("roundtrip: ok"));

    let err = assert_failure(fgx(&["roundtrip", "examples/hello.fgx", "--to", "wat"]));
    assert!(err.contains("unknown roundtrip target"));
}

#[test]
fn check_success_and_failure() {
    let out = assert_success(fgx(&["check", "examples/media/audio_mix.fgx", "--json"]));
    assert!(out.contains("\"schema\": \"forgentex.diagnostics.v1\""));
    assert!(out.contains("\"diagnostics\": ["));

    let project = assert_success(fgx(&["check", "--project", "--json"]));
    assert!(project.contains("\"schema\": \"forgentex.diagnostics.v1\""));
    assert!(project.contains("\"diagnostics\": ["));
    assert!(!project.contains("unresolved_import"));

    let err = assert_failure(fgx(&["check", "examples/missing.fgx", "--json"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn public_0_1_versioned_reports_match_the_frozen_top_level_shapes() {
    let cases: &[(&str, &[&str])] = &[
        (
            "forgentex.ast.v1",
            &["ast", "examples/hello.fgx", "--canonical"],
        ),
        (
            "forgentex.diagnose.v1",
            &["diagnose", "examples/hello.fgx", "--json"],
        ),
        (
            "forgentex.diagnostics.v1",
            &["check", "examples/hello.fgx", "--json"],
        ),
        (
            "forgentex.diagnostic_catalog.v1",
            &["diagnostics", "list", "--json"],
        ),
        (
            "forgentex.diagnostic_explain.v1",
            &["explain", "TYP001", "--json"],
        ),
        (
            "forgentex.effects.v1",
            &["effects", "examples/hello.fgx", "--json"],
        ),
        (
            "forgentex.test_report.v1",
            &["test", "examples/smoke/inline_tests.fgx", "--json"],
        ),
        (
            "forgentex.run_result.v1",
            &[
                "run",
                "examples/media/audio_mix.fgx",
                "--entry",
                "mix_samples",
                "--json-input",
                "examples/run/audio_mix.json",
            ],
        ),
        (
            "forgentex.agent_context.v1",
            &[
                "context",
                "@function.mix_samples",
                "--deps",
                "types,effects,tests",
                "--json",
            ],
        ),
        (
            "forgentex.agent_review.v1",
            &[
                "review",
                "patches/add_fast_path.fgp",
                "--source",
                "examples/media/audio_mix.fgx",
                "--json",
            ],
        ),
        (
            "forgentex.release_check.v1",
            &["release-check", "--target", "0.1", "--json"],
        ),
    ];

    for (schema_id, args) in cases {
        let output = assert_success(fgx(args));
        let schema = forgentex::public_compat::PUBLIC_JSON_SCHEMAS
            .iter()
            .find(|schema| schema.id == *schema_id)
            .unwrap();
        assert!(
            output.contains(&format!("\"schema\": \"{schema_id}\"")),
            "{} did not emit {schema_id}:\n{output}",
            args.join(" ")
        );
        for field in schema.required_fields {
            assert!(
                output.contains(&format!("\"{field}\":")),
                "{} omitted required field {schema_id}.{field}:\n{output}",
                args.join(" ")
            );
        }
    }
}

#[test]
fn borrow_report_command_is_available() {
    let ok = assert_success(fgx(&[
        "borrow-report",
        "examples/media/audio_mix.fgx",
        "--json",
    ]));
    assert!(ok.contains("\"schema\": \"forgentex.borrow_report.v1\""));
    assert!(ok.contains("\"status\": \"pass\""));
    assert!(ok.contains("\"regions\": ["));
    assert!(ok.contains("\"id\": \"lifetime_region_graph\""));
    assert!(ok.contains("\"id\": \"loop_carried_borrow_liveness\""));
    assert!(ok.contains("\"id\": \"conditional_borrow_state_join\""));
    assert!(ok.contains("\"id\": \"borrow_alias_roots\""));

    let source_path = temp_file("borrow_report_bad.fgx");
    fs::write(
        &source_path,
        "fn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\nfn bad(xs: &mut [u8]) -> void:\n    let alias: &mut [u8] = xs\n    fill(xs, alias)\n",
    )
    .unwrap();
    let source_path_text = source_path.to_string_lossy().to_string();
    let err = assert_failure(fgx(&["borrow-report", &source_path_text, "--json"]));
    assert!(err.contains("\"status\": \"fail\""));
    assert!(err.contains("\"error\": \"mutable_borrow_alias\""));
    assert!(err.contains("borrow report found diagnostics"));
    let _ = fs::remove_file(source_path);
}

#[test]
fn borrow_conformance_command_is_available() {
    let json = assert_success(fgx(&["borrow-conformance", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.borrow_conformance.v1\""));
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"failed\": 0"));
    assert!(json.contains("\"persisted_regression_generated\": 8"));
    assert!(json.contains("\"persisted_acceptance_generated\": 4"));
    assert!(json.contains("\"persisted_regression_mismatched\": 0"));
    assert!(json.contains("\"persisted_acceptance_mismatched\": 0"));
    assert!(json.contains("\"id\": \"moves.use_after_move\""));
    assert!(json.contains("\"id\": \"loops.loop_carried_alias_rejected\""));
    assert!(json.contains("\"id\": \"branches.conditional_alias_reassign_rejected\""));
    assert!(json.contains("\"id\": \"contracts.report_wrapper_mutates\""));
    assert!(json.contains("\"id\": \"raw.raw_pointer_param_safe_fn_rejected\""));
    assert!(json.contains("\"id\": \"raw.raw_pointer_deref_safe_fn_rejected\""));

    let text = assert_success(fgx(&["borrow-conformance"]));
    assert!(text.contains("Forgentex borrow conformance suite"));
    assert!(text.contains("status=pass"));

    let custom = assert_success(fgx(&[
        "borrow-conformance",
        "--campaign-seed",
        "0x7",
        "--campaign-cases",
        "1",
        "--json",
    ]));
    assert!(custom.contains("\"campaign_seed\": \"0x0000000000000007\""));
    assert!(custom.contains("\"campaign_generated\": 6"));
    assert!(custom.contains("reference.campaign.v1.0000000000000007.moves.0000"));

    let failure_dir = temp_file("borrow_campaign_failures");
    if failure_dir.exists() {
        fs::remove_dir_all(&failure_dir).unwrap();
    }
    let persisted = assert_success(fgx(&[
        "borrow-conformance",
        "--campaign-seed",
        "11",
        "--campaign-cases",
        "1",
        "--failure-dir",
        failure_dir.to_str().unwrap(),
    ]));
    assert!(persisted.contains("status=pass"));
    assert!(!failure_dir.exists());

    let invalid = assert_failure(fgx(&["borrow-conformance", "--campaign-cases", "0"]));
    assert!(invalid.contains("expected 1..256"));
}

#[test]
fn targets_and_target_check_are_available() {
    let catalog = assert_success(fgx(&["targets"]));
    assert!(catalog.contains("x86_64-pc-windows-msvc"));
    assert!(catalog.contains("aarch64-unknown-linux-gnu"));
    assert!(catalog.contains("riscv64gc-unknown-linux-gnu"));

    let json = assert_success(fgx(&["targets", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.targets.v1\""));
    assert!(json.contains("\"family\": \"riscv32\""));

    let source_path = temp_file("target_check.fgx");
    fs::write(
        &source_path,
        "fn fast(x: u32) -> u32 effects [cpu.avx2]:\n    return x\n\nfn caller(x: u32) -> u32 effects [cpu.avx2]:\n    return fast(x)\n",
    )
    .unwrap();
    let source_path_text = source_path.to_string_lossy().to_string();
    // The program has a diagnostic, so `check` must fail — while still
    // printing the full JSON report on stdout for a wrapper to read.
    let out = assert_failure(fgx(&[
        "check",
        &source_path_text,
        "--target",
        "x86_64-pc-windows-msvc",
        "--json",
    ]));
    assert!(out.contains("\"error\": \"target_guard_required\""));
    let _ = fs::remove_file(source_path);
}

#[test]
fn asm_check_success_and_machine_diagnostics() {
    let out = assert_success(fgx(&[
        "asm-check",
        "examples/arch/x86_64_popcount.fgx",
        "--target",
        "x86_64-pc-windows-msvc",
    ]));
    assert!(out.contains("Assembly review for x86_64-pc-windows-msvc"));
    assert!(out.contains("changed=[rax]"));
    assert!(out.contains("asm-check: ok"));

    let source_path = temp_file("asm_invalid_register.fgx");
    fs::write(
        &source_path,
        "unsafe fn bad(x: u64) -> u64:\n    asm aarch64:\n        in rax: x\n        out x0: y\n        op add x0 x0 x0\n    return y\n",
    )
    .unwrap();
    let source_path_text = source_path.to_string_lossy().to_string();
    let json = assert_success(fgx(&[
        "asm-check",
        &source_path_text,
        "--target",
        "aarch64-unknown-linux-gnu",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.asm_check.v1\""));
    assert!(json.contains("\"error\": \"asm_invalid_register\""));
    let _ = fs::remove_file(source_path);
}

#[test]
fn simd_review_and_portable_target_checks_are_available() {
    let out = assert_success(fgx(&[
        "simd-review",
        "examples/media/audio_mix_simd.fgx",
        "--target",
        "aarch64-unknown-linux-gnu",
    ]));
    assert!(out.contains("SIMD review for aarch64-unknown-linux-gnu"));
    assert!(out.contains("aarch64.neon"));
    assert!(out.contains("scalar_fallback=yes"));
    assert!(out.contains("equivalence_tests=yes"));
    assert!(out.contains("simd-review: ok"));

    let json = assert_success(fgx(&[
        "simd-review",
        "examples/media/audio_mix_simd.fgx",
        "--target",
        "riscv64gc-unknown-linux-gnu",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.simd_review.v1\""));
    assert!(json.contains("riscv64.v-if-cpu.v"));

    let check = assert_success(fgx(&[
        "check",
        "examples/media/audio_mix_simd.fgx",
        "--target",
        "riscv64gc-unknown-linux-gnu",
        "--json",
    ]));
    assert!(!check.contains("simd_fallback_required"));

    let run = assert_success(fgx(&[
        "run",
        "examples/media/audio_mix_simd.fgx",
        "--entry",
        "mix_samples_simd",
        "--json-input",
        "examples/run/audio_mix_simd.json",
    ]));
    assert!(run.contains("\"entry\": \"mix_samples_simd\""));
    assert!(run.contains("\"status\": \"pass\""));
}

#[test]
fn abi_and_header_commands_are_available() {
    let out = assert_success(fgx(&[
        "abi",
        "examples/abi/layout_demo.fgx",
        "--target",
        "x86_64-unknown-linux-gnu",
    ]));
    assert!(out.contains("ABI report for x86_64-unknown-linux-gnu"));
    assert!(out.contains("struct AbiPacket"));
    assert!(out.contains("size=32"));
    assert!(out.contains("cc=sysv64"));

    let json = assert_success(fgx(&[
        "abi",
        "examples/abi/layout_demo.fgx",
        "--target",
        "armv7-unknown-linux-gnueabihf",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.abi.v1\""));
    assert!(json.contains("\"pointer_size\": 4"));
    assert!(json.contains("\"calling_convention\": \"aapcs32\""));

    let header = assert_success(fgx(&[
        "header",
        "examples/abi/layout_demo.fgx",
        "--target",
        "riscv64gc-unknown-linux-gnu",
    ]));
    assert!(header.contains("#define FGX_ABI_TARGET \"riscv64gc-unknown-linux-gnu\""));
    assert!(header.contains("typedef struct FGX_ALIGNAS(16) AbiPacket"));
    assert!(header.contains("FGX_EXTERN size_t inspect_packet"));

    let err = assert_failure(fgx(&[
        "header",
        "examples/abi/layout_demo.fgx",
        "--target",
        "mips64-demo",
    ]));
    assert!(err.contains("unknown target triple"));
}

#[test]
fn build_c_backend_success_and_failure() {
    let out_dir = temp_file("c_backend_out");
    let out_dir_text = out_dir.to_string_lossy().to_string();
    let out = assert_success(fgx(&[
        "build",
        ".",
        "--backend",
        "c",
        "--out",
        &out_dir_text,
    ]));
    assert!(out.contains("forgentex_demo_media.h"));
    assert!(out.contains("forgentex_demo_media.c"));
    assert!(out.contains("forgentex.fgir.json"));
    assert!(out.contains("forgentex.backend_build.json"));
    let header = fs::read_to_string(out_dir.join("forgentex_demo_media.h")).unwrap();
    assert!(header.contains("void mix_samples(fgx_slice_i16 dst"));
    assert!(header
        .contains("typedef struct { uint8_t *data; size_t len; size_t capacity; } fgx_vec_u8;"));
    assert!(header
        .contains("typedef struct { int16_t *data; size_t len; size_t capacity; } fgx_vec_i16;"));
    let receipt = fs::read_to_string(out_dir.join("forgentex.backend_build.json")).unwrap();
    assert!(receipt.contains("\"backend\": \"c\""));
    assert!(receipt.contains("\"fir_schema\": \"forgentex.backend_ir.v1\""));
    let _ = fs::remove_dir_all(out_dir);

    let err = assert_failure(fgx(&["build", ".", "--backend", "wat"]));
    assert!(err.contains("unknown backend"));

    let planned = assert_failure(fgx(&["build", ".", "--backend", "llvm"]));
    assert!(planned.contains("backend 'llvm' is planned for v3.0"));
    assert!(planned.contains("feature=backend-llvm"));
}

#[test]
fn build_zig_backend_success() {
    let out_dir = temp_file("zig_backend_out");
    let out_dir_text = out_dir.to_string_lossy().to_string();
    let out = assert_success(fgx(&[
        "build",
        ".",
        "--backend",
        "zig",
        "--out",
        &out_dir_text,
    ]));
    assert!(out.contains("forgentex_demo_media.zig"));
    assert!(out.contains("forgentex.fgir.json"));
    let source = fs::read_to_string(out_dir.join("forgentex_demo_media.zig")).unwrap();
    assert!(source.contains("pub fn mix_samples(dst: []i16"));
    assert!(source.contains("fgx_bounds_index(i, dst.len)"));
    assert!(source.contains("const FgxVecU8 = struct"));
    assert!(source.contains("const FgxVecI16 = struct"));
    let receipt = fs::read_to_string(out_dir.join("forgentex.backend_build.json")).unwrap();
    assert!(receipt.contains("\"backend\": \"zig\""));
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn package_manager_and_build_system_commands_are_available() {
    let lock = assert_success(fgx(&["lock", "examples/packages/codec_app", "--json"]));
    assert!(lock.contains("\"schema\": \"forgentex.lock.v1\""));
    assert!(lock.contains("\"name\": \"codec-app\""));
    assert!(lock.contains("\"name\": \"codec-common\""));

    let package_test = assert_success(fgx(&[
        "test",
        "--project",
        "examples/packages/codec_app",
        "--all-targets",
        "--json",
    ]));
    assert!(package_test.contains("\"schema\": \"forgentex.package_test.v1\""));
    assert!(package_test.contains("\"status\": \"pass\""));
    assert!(package_test.contains("aarch64-unknown-linux-gnu"));
    assert!(package_test.contains("riscv64gc-unknown-linux-gnu"));

    let out_dir = temp_file("stage23_build");
    let out_dir_text = out_dir.to_string_lossy().to_string();
    let build = assert_success(fgx(&[
        "build",
        "examples/packages/codec_app",
        "--backend",
        "c",
        "--target",
        "aarch64-unknown-linux-gnu",
        "--out",
        &out_dir_text,
        "--json",
    ]));
    assert!(build.contains("\"schema\": \"forgentex.package_build.v1\""));
    assert!(build.contains("\"triple\": \"aarch64-unknown-linux-gnu\""));
    assert!(build.contains("\"metadata_files\": ["));
    assert!(build.contains("forgentex.backend_build.json"));
    assert!(out_dir
        .join("dev")
        .join("aarch64-unknown-linux-gnu")
        .join("codec_app.h")
        .exists());
    let _ = fs::remove_dir_all(out_dir);

    let package_dir = temp_file("stage23_new");
    let package_dir_text = package_dir.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&package_dir);
    let new_output = assert_success(fgx(&["new", &package_dir_text, "--name", "cli-stage23"]));
    assert!(new_output.contains("Forgentex.toml"));
    assert!(package_dir.join("src").join("main.fgx").exists());
    let _ = fs::remove_dir_all(package_dir);

    let publish = assert_success(fgx(&[
        "publish",
        "examples/packages/codec_app",
        "--dry-run",
        "--json",
    ]));
    assert!(publish.contains("\"schema\": \"forgentex.publish_dry_run.v1\""));
    assert!(publish.contains("\"ready\": true"));
}

#[test]
fn security_audit_and_verify_commands_are_available() {
    let audit = assert_success(fgx(&["audit", "examples/packages/codec_app", "--json"]));
    assert!(audit.contains("\"schema\": \"forgentex.audit.v1\""));
    assert!(audit.contains("\"status\": \"pass\""));
    assert!(audit.contains("\"trust\": \"untrusted\""));
    assert!(audit.contains("build_scripts=deny"));

    let verify = assert_success(fgx(&["verify", "examples/packages/codec_app", "--json"]));
    assert!(verify.contains("\"schema\": \"forgentex.verify.v1\""));
    assert!(verify.contains("\"status\": \"pass\""));
    assert!(verify.contains("\"signature_status\": \"mock-accepted\""));
    assert!(verify.contains("\"reproducible\": true"));

    let package_dir = temp_file("stage24_audit_blocked");
    let src_dir = package_dir.join("src");
    let _ = fs::remove_dir_all(&package_dir);
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        package_dir.join("Forgentex.toml"),
        "[package]\nname = \"cli-stage24-blocked\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[security]\ntrust = \"untrusted\"\nsigned_by = \"agent-public-demo\"\nsignature_algorithm = \"Ed25519\"\nsignature = \"mock-sig-cli-stage24-blocked\"\nreproducible = true\nbuild_script = \"scripts/build.ps1\"\n\n[permissions]\nfs.read = [\"src\"]\n\n[sandbox]\nbuild_scripts = \"sandboxed\"\ncodegen = \"deny\"\ntests = \"sandboxed\"\nagent_tools = \"deny\"\n",
    )
    .unwrap();
    fs::write(
        src_dir.join("main.fgx"),
        "module cli.stage24\n\nexport fn hello() -> i32:\n    return 42\n",
    )
    .unwrap();
    let package_dir_text = package_dir.to_string_lossy().to_string();
    let err = assert_failure(fgx(&["audit", &package_dir_text, "--json"]));
    assert!(err.contains("\"status\": \"fail\""));
    assert!(err.contains("\"code\": \"undeclared_permission\""));
    assert!(err.contains("\"required_permission\": \"build.scripts\""));
    let _ = fs::remove_dir_all(package_dir);
}

#[test]
fn lock_command_can_write_lockfile() {
    let lock_path = temp_file("Forgentex.lock");
    let lock_path_text = lock_path.to_string_lossy().to_string();
    let output = assert_success(fgx(&[
        "lock",
        "examples/packages/codec_app",
        "--out",
        &lock_path_text,
    ]));
    assert!(output.contains("wrote"));
    let lock = fs::read_to_string(&lock_path).unwrap();
    assert!(lock.contains("schema = \"forgentex.lock.v1\""));
    assert!(lock.contains("name = \"codec-app\""));
    let _ = fs::remove_file(lock_path);
}

#[test]
fn backend_catalog_and_ir_commands_are_available() {
    let catalog = assert_success(fgx(&["backends"]));
    assert!(catalog.contains("forgentex.backend_info.v1"));
    assert!(catalog.contains("forgentex.backend_ir.v1"));
    assert!(catalog.contains("interp status=available interface=run"));
    assert!(catalog.contains("c status=available"));
    assert!(catalog.contains("zig status=available"));
    assert!(catalog.contains("feature=jit-cranelift"));
    assert!(catalog.contains("llvm status=planned"));

    let json = assert_success(fgx(&["backends", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.backend_info.v1\""));
    assert!(json.contains("\"default_zero_dependency\": true"));
    assert!(json.contains("\"name\": \"zig\""));
    assert!(json.contains("\"name\": \"mlir\""));
    assert!(json.contains("\"feature\": \"backend-mlir\""));

    let ir = assert_success(fgx(&["ir", "."]));
    assert!(ir.contains("FGIR schema=forgentex.backend_ir.v1"));
    assert!(ir.contains("F mix_samples @function.mix_samples visibility=public"));
    assert!(ir.contains("assign dst[i] core_math_clamp_i16(x)"));

    let file_ir = assert_success(fgx(&["ir", "examples/hello.fgx"]));
    assert!(file_ir.contains("package=single-file"));
    assert!(file_ir.contains("M hello path=examples/hello.fgx"));
    assert!(file_ir.contains("F area @function.area"));

    let ir_json = assert_success(fgx(&["ir", ".", "--json"]));
    assert!(ir_json.contains("\"schema\": \"forgentex.backend_ir.v1\""));
    assert!(ir_json.contains("\"op\": \"assign\""));

    let file_ir_json = assert_success(fgx(&["ir", "examples/hello.fgx", "--json"]));
    assert!(file_ir_json.contains("\"package\": \"single-file\""));
    assert!(file_ir_json.contains("\"name\": \"Point\""));

    let contract = assert_success(fgx(&["ir", ".", "--contract-json"]));
    assert!(contract.contains("\"schema\": \"forgentex.frontend_contract.v1\""));
    assert!(contract.contains("\"type_diagnostics\": ["));

    let file_contract = assert_success(fgx(&["ir", "examples/hello.fgx", "--contract-json"]));
    assert!(file_contract.contains("\"package\": \"single-file\""));
    assert!(file_contract.contains("\"schema\": \"forgentex.frontend_contract.v1\""));
}

#[test]
fn standard_library_catalog_is_available() {
    let catalog = assert_success(fgx(&["stdlib"]));
    assert!(catalog.contains("Forgentex standard library v0"));
    assert!(catalog.contains("core.math"));
    assert!(catalog.contains("core.target"));

    let json = assert_success(fgx(&["stdlib", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.stdlib.v1\""));
    assert!(json.contains("core_simd_load_i16x8"));
}

#[test]
fn api_public_success_and_failure() {
    let out = assert_success(fgx(&["api", "--public"]));
    assert!(out.contains("API forgentex-demo-media 0.1.0"));
    assert!(out.contains("fn mix_samples @function.mix_samples"));

    let err = assert_failure(fgx(&["api"]));
    assert!(err.contains("usage: fgx api --public"));
}

#[test]
fn effects_success_and_failure() {
    let out = assert_success(fgx(&[
        "effects",
        "examples/media/popcount_x86.fgx",
        "--json",
    ]));
    assert!(out.contains("\"schema\": \"forgentex.effects.v1\""));
    assert!(out.contains("\"inferred\": ["));
    assert!(out.contains("cpu.asm"));

    let err = assert_failure(fgx(&["effects", "examples/missing.fgx", "--json"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn patch_success_and_failure() {
    let out = assert_success(fgx(&[
        "patch",
        "examples/media/audio_mix.fgx",
        "patches/add_fast_path.fgp",
        "--review",
    ]));
    assert!(out.contains("Patch summary:"));
    assert!(out.contains("Type check after patch: ok"));

    let err = assert_failure(fgx(&[
        "patch",
        "examples/media/audio_mix.fgx",
        "patches/missing.fgp",
        "--review",
    ]));
    assert!(err.contains("failed to read patch"));
}

#[test]
fn patch_check_only_and_review_json_are_available() {
    let patch_path = temp_file("stage7_patch.fgp");
    fs::write(
        &patch_path,
        "VERSION 1\n\
         AUTHOR cli.test\n\
         MOCK_SIGNATURE cli-mock-key\n\
         TARGET @function.mix_samples\n\
         PRECONDITION NODE_EXISTS @function.mix_samples\n\
         REVIEW risk=low\n\
         OP ADD_EFFECT effect=alloc\n",
    )
    .unwrap();
    let patch_path_text = patch_path.to_string_lossy().to_string();

    let out = assert_success(fgx(&[
        "patch",
        "examples/media/audio_mix.fgx",
        &patch_path_text,
        "--check-only",
    ]));
    assert!(out.contains("Patch summary:"));
    assert!(out.contains("Check-only: yes"));
    assert!(out.contains("Patch preview only"));
    assert!(!out.contains("=== patched.fgx ==="));

    let json = assert_success(fgx(&[
        "patch",
        "examples/media/audio_mix.fgx",
        &patch_path_text,
        "--emit-review-json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.patch_review.v1\""));
    assert!(json.contains("\"signature_status\": \"mock-accepted\""));
    assert!(json.contains("\"operation_log\": ["));
    assert!(json.contains("\"allocation\": true"));

    let _ = fs::remove_file(patch_path);
}

#[test]
fn run_success_and_failure() {
    let out = assert_success(fgx(&["run", "examples/media/audio_mix.fgx"]));
    assert!(out.contains("run mix_samples: pass"));

    let json = assert_success(fgx(&[
        "run",
        "examples/media/audio_mix.fgx",
        "--entry",
        "mix_samples",
        "--json-input",
        "examples/run/audio_mix.json",
    ]));
    assert!(json.contains("\"entry\": \"mix_samples\""));
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"dst\": [5500, 0, -12000, 17000]"));
    assert!(!json.contains("\"concurrency\""));

    let concurrency = assert_success(fgx(&[
        "run",
        "examples/async/structured_task_group.fgx",
        "--entry",
        "structured_scores",
    ]));
    assert!(concurrency.contains("\"schema\": \"forgentex.concurrency_receipt.v1\""));
    assert!(concurrency.contains("\"operation\":\"task_group.await_all\""));
    assert!(concurrency.contains("\"decision\":\"completed\""));

    let backend_json = assert_success(fgx(&[
        "run",
        "examples/media/audio_mix.fgx",
        "--entry",
        "mix_samples",
        "--json-input",
        "examples/run/audio_mix.json",
        "--backend",
        "interp",
    ]));
    assert!(backend_json.contains("\"entry\": \"mix_samples\""));
    assert!(backend_json.contains("\"status\": \"pass\""));

    let source_backend = assert_failure(fgx(&[
        "run",
        "examples/media/audio_mix.fgx",
        "--entry",
        "mix_samples",
        "--json-input",
        "examples/run/audio_mix.json",
        "--backend",
        "c",
    ]));
    assert!(source_backend.contains("does not run entries in-process"));

    let blocked_path = temp_file("interp_fir_gate_cli.fgx");
    fs::write(
        &blocked_path,
        "unsafe fn blocked() -> i32:\n    return 1\n\nfn safe_entry() -> i32:\n    return 42\n",
    )
    .unwrap();
    let blocked_path_text = blocked_path.to_string_lossy().to_string();
    let blocked = assert_failure(fgx(&[
        "run",
        &blocked_path_text,
        "--entry",
        "blocked",
        "--backend",
        "interp",
    ]));
    assert!(blocked.contains("FIR contract blocks backend 'interp'"));
    assert!(blocked.contains("runtime::blocked blockers=[unsafe_function]"));
    let _ = fs::remove_file(blocked_path);

    let hosted_data = temp_file("hosted_live_data.txt");
    let hosted_input = temp_file("hosted_live_input.json");
    fs::write(&hosted_data, "payload").unwrap();
    let hosted_path = hosted_data
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    fs::write(
        &hosted_input,
        format!(
            r#"{{ "args": {{ "path": "{hosted_path}" }}, "expect": {{ "return": "payload" }} }}"#
        ),
    )
    .unwrap();
    let hosted_input_text = hosted_input.to_string_lossy().to_string();
    let live = assert_success(fgx(&[
        "run",
        "examples/io/real_copy.fgx",
        "--entry",
        "read_file",
        "--json-input",
        &hosted_input_text,
        "--live",
        "--grant-fs",
        "fs.read",
    ]));
    assert!(live.contains("\"status\": \"pass\""));
    assert!(live.contains("\"capsule_calls\": []"));
    assert!(live.contains("\"io_ops\": ["));
    assert!(live.contains("\"op\": \"read\""));
    assert!(live.contains("\"decision\": \"allow\""));
    assert!(live.contains("\"net_ops\": []"));
    let _ = fs::remove_file(hosted_data);
    let _ = fs::remove_file(hosted_input);

    let err = assert_failure(fgx(&["run", "examples/media/popcount_x86.fgx"]));
    assert!(err.contains("no runnable demo entrypoint"));
}

#[test]
fn io_and_net_run_commands_use_fir_backed_helpers() {
    let io_data = temp_file("cli_io_run_data.txt");
    fs::write(&io_data, "payload").unwrap();
    let io_data_text = io_data.to_string_lossy().to_string();
    let io = assert_success(fgx(&[
        "io-run",
        "examples/io/real_copy.fgx",
        "--entry",
        "read_file",
        "--grant",
        "fs.read",
        "--arg",
        &io_data_text,
        "--json",
    ]));
    assert!(io.contains("\"schema\": \"forgentex.io_run.v1\""));
    assert!(io.contains("\"status\": \"ok\""));
    assert!(io.contains("\"return_value\": \"payload\""));
    let _ = fs::remove_file(io_data);

    let net = assert_failure(fgx(&[
        "net-run",
        "examples/network/tcp_ping.fgx",
        "--entry",
        "ping",
        "--arg",
        "127.0.0.1:9",
        "--arg",
        "x",
        "--json",
    ]));
    assert!(net.contains("\"schema\": \"forgentex.net_run.v1\""));
    assert!(net.contains("\"status\": \"denied\""));
    assert!(net.contains("\"decision\": \"deny\""));
}

#[test]
fn forgepack_build_and_verify_commands_are_reproducible() {
    let first = manifest_dir().join("examples/apps/forgepack/output/cli-first.fpk");
    let second = manifest_dir().join("examples/apps/forgepack/output/cli-second.fpk");
    let _ = fs::remove_file(&first);
    let _ = fs::remove_file(&second);

    let built = assert_success(fgx(&[
        "forgepack",
        "build",
        "examples/apps/forgepack/build.json",
        "output/cli-first.fpk",
        "--json",
    ]));
    assert!(built.contains("\"schema\":\"forgentex.forgepack_build.v1\""));
    assert!(built.contains("\"entries\":2"));
    let inspected = assert_success(fgx(&[
        "forgepack",
        "inspect",
        "examples/apps/forgepack/output/cli-first.fpk",
        "--json",
    ]));
    assert!(inspected.contains("\"schema\":\"forgentex.forgepack_inspect.v1\""));
    assert!(inspected.contains("\"structure_valid\":true"));
    assert!(inspected.contains("\"hashes_verified\":false"));
    assert!(inspected.contains("\"path\":\"docs/readme.txt\""));
    let verified = assert_success(fgx(&[
        "forgepack",
        "verify",
        "examples/apps/forgepack/output/cli-first.fpk",
        "--json",
    ]));
    assert!(verified.contains("\"schema\":\"forgentex.forgepack_verify.v1\""));
    assert!(verified.contains("\"hashes_verified\":2"));

    let extract_root = manifest_dir().join("examples/apps/forgepack/output/cli-extracted");
    let _ = fs::remove_dir_all(&extract_root);
    fs::create_dir(&extract_root).unwrap();
    let extracted = assert_success(fgx(&[
        "forgepack",
        "extract",
        "examples/apps/forgepack/output/cli-first.fpk",
        "examples/apps/forgepack/output/cli-extracted",
        "--json",
    ]));
    assert!(extracted.contains("\"schema\":\"forgentex.forgepack_extract.v1\""));
    assert!(extracted.contains("\"publication\":\"create_new\""));
    assert_eq!(
        fs::read(extract_root.join("docs/readme.txt")).unwrap(),
        b"Forgentex forgepack command example.\n"
    );

    assert_success(fgx(&[
        "forgepack",
        "build",
        "examples/apps/forgepack/build.json",
        "output/cli-second.fpk",
    ]));
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());

    let refusal = assert_failure(fgx(&[
        "forgepack",
        "build",
        "examples/apps/forgepack/build.json",
        "output/cli-first.fpk",
    ]));
    assert!(refusal.contains("forgepack_publish"), "{refusal}");
    let evidence = assert_success(fgx(&["forgepack", "evidence", "--strict", "--json"]));
    assert!(evidence.contains("\"schema\":\"forgentex.forgepack_evidence.v1\""));
    assert!(evidence.contains("\"status\":\"pass\""));
    assert!(evidence.contains("\"checks_fail\":0"));
    fs::remove_dir_all(extract_root).unwrap();
    fs::remove_file(first).unwrap();
    fs::remove_file(second).unwrap();
}

#[test]
fn app_run_uses_manifest_entry_scopes_and_byte_budgets() {
    let root = temp_file("cli_application");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("input")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"cli-application\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"read_text\"\nruntime = \"hosted_io\"\neffects = [\"fs.read\"]\n\n[application.limits]\nmax_read_bytes = 1024\n\n[permissions]\nfs.read = [\"input\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn read_text(path: str) -> str effects [fs.read]:\n    return core_io_read_file(path)\n",
    )
    .unwrap();
    fs::write(root.join("input").join("data.txt"), "application payload").unwrap();

    let root_text = root.to_string_lossy().to_string();
    let report = assert_success(fgx(&[
        "app-run",
        &root_text,
        "--arg",
        "input/data.txt",
        "--json",
    ]));
    assert!(report.contains("\"schema\":\"forgentex.application_run.v1\""));
    assert!(report.contains("\"forgentex_required\":\"0.1\""));
    assert!(report.contains("\"forgentex_version\":\"0.1.0\""));
    assert!(report.contains("\"forgentex_compatibility\":\"compatible\""));
    assert!(report.contains("\"status\":\"ok\""));
    assert!(report.contains("\"result\":{\"type\":\"str\",\"value\":\"application payload\"}"));
    assert!(report.contains("\"error\":null"));
    assert!(report.contains("\"max_read_bytes\":1024"));
    assert!(report.contains("\"artifact_declarations\":[]"));
    assert!(report.contains("\"artifacts\":[]"));
    let manifest_path = root.join("Forgentex.toml");
    let incompatible = fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("forgentex = \"0.1\"", "forgentex = \"99.0\"");
    fs::write(&manifest_path, &incompatible).unwrap();
    let refusal = assert_failure(fgx(&[
        "app-run",
        &root_text,
        "--arg",
        "input/data.txt",
        "--json",
    ]));
    assert!(
        refusal.contains("application_version_incompatible"),
        "{refusal}"
    );
    assert!(refusal.contains("\"schema\":\"forgentex.application_error.v1\""));
    assert!(refusal.contains("\"phase\":\"compatibility\""));
    let typed_manifest = incompatible
        .replace("forgentex = \"99.0\"", "forgentex = \"0.1\"")
        .replace("entry = \"read_text\"", "entry = \"echo_number\"");
    fs::write(&manifest_path, typed_manifest).unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn echo_number(value: i16) -> i16:\n    return value\n",
    )
    .unwrap();
    let typed = assert_success(fgx(&["app-run", &root_text, "--arg", "-17", "--json"]));
    assert!(typed.contains("\"status\":\"ok\""));
    assert!(typed.contains("\"result\":{\"type\":\"i16\",\"value\":-17}"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn inline_test_command_is_available() {
    let json = assert_success(fgx(&[
        "test",
        "examples/smoke/inline_tests.fgx",
        "--target",
        "x86_64-pc-windows-msvc",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.test_report.v1\""));
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"xfailed\": 1"));
    assert!(json.contains("\"expected\": \"ASM001\""));

    let filtered = assert_success(fgx(&[
        "test",
        "examples/smoke/inline_tests.fgx",
        "--filter",
        "clamp high",
    ]));
    assert!(filtered.contains("Forgentex inline test report"));
    assert!(filtered.contains("passed=1"));

    let source_path = temp_file("stage30_inline_fail.fgx");
    fs::write(
        &source_path,
        "fn one() -> i32:\n    return 1\n\ntest \"fails\":\n    expect one() == 2\n",
    )
    .unwrap();
    let source_path_text = source_path.to_string_lossy().to_string();
    let err = assert_failure(fgx(&["test", &source_path_text, "--json"]));
    assert!(err.contains("\"schema\": \"forgentex.test_report.v1\""));
    assert!(err.contains("\"status\": \"fail\""));
    assert!(err.contains("inline tests failed"));
    let _ = fs::remove_file(source_path);
}

#[test]
fn map_success_and_failure() {
    let out_path = temp_file("project.fgmap");
    let out_path_text = out_path.to_string_lossy().to_string();
    let out = assert_success(fgx(&["map", "examples", "--out", &out_path_text]));
    assert!(out.contains("wrote"));
    let map = fs::read_to_string(&out_path).unwrap();
    assert!(map.contains("M hello.fgx"));
    assert!(!map.contains("M hello.fgai"));
    let _ = fs::remove_file(out_path);

    let target_map = assert_success(fgx(&["map", "examples", "--include-targets"]));
    assert!(target_map.contains("target.x86_64"));

    let err = assert_failure(fgx(&["map", "examples/missing"]));
    assert!(err.contains("failed to read directory"));
}

#[test]
fn project_map_success() {
    let out_path = temp_file("project_stage8.fgmap");
    let out_path_text = out_path.to_string_lossy().to_string();
    let out = assert_success(fgx(&["map", "--project", "--out", &out_path_text]));
    assert!(out.contains("wrote"));
    let map = fs::read_to_string(&out_path).unwrap();
    assert!(map.contains("P forgentex-demo-media 0.1.0"));
    assert!(map.contains("M media.audio"));
    assert!(map.contains("imports=core.math,core.slice,media.types"));
    assert!(map.contains("M core.math"));
    let _ = fs::remove_file(out_path);
}

#[test]
fn get_success_and_failure() {
    let out = assert_success(fgx(&[
        "get",
        "@function.mix_samples",
        "--from",
        "examples",
        "--deps",
        "types",
    ]));
    assert!(out.contains("fn mix_samples"));

    let err = assert_failure(fgx(&[
        "get",
        "@function.nope",
        "--from",
        "examples",
        "--deps",
        "types",
    ]));
    assert!(err.contains("symbol not found"));
}

#[test]
fn context_and_review_workbench_commands_are_available() {
    let context = assert_success(fgx(&[
        "context",
        "@function.mix_samples",
        "--deps",
        "types,effects,tests",
        "--json",
    ]));
    assert!(context.contains("\"schema\": \"forgentex.agent_context.v1\""));
    assert!(context.contains("\"symbol_source\":"));
    assert!(context.contains("examples/run/audio_mix.json"));
    assert!(context.contains("\"token_estimate\":"));
    assert!(context.contains("\"tokenizer\": \"heuristic-v1\""));
    assert!(context.contains("\"reason\": \"focused_symbol_only\""));

    let bounded = assert_success(fgx(&[
        "context",
        "@function.mix_samples",
        "--deps",
        "types,effects,tests",
        "--max-tokens",
        "100000",
        "--json",
    ]));
    assert!(bounded.contains("\"max_tokens\": 100000"));
    assert!(bounded.contains("\"decision\": \"within_budget\""));

    let full_packet = temp_file("context_full.json");
    let delta_packet = temp_file("context_delta.json");
    let full_path = full_packet.to_string_lossy().to_string();
    let delta_path = delta_packet.to_string_lossy().to_string();
    let written = assert_success(fgx(&[
        "context",
        "@function.mix_samples",
        "--deps",
        "types,effects,tests",
        "--out",
        &full_path,
    ]));
    assert!(written.contains("wrote"));
    assert!(fs::read_to_string(&full_packet)
        .unwrap()
        .contains("\"mode\": \"full\""));
    assert_success(fgx(&[
        "context",
        "@function.mix_samples",
        "--deps",
        "types,effects,tests",
        "--since",
        &full_path,
        "--out",
        &delta_path,
    ]));
    let delta = fs::read_to_string(&delta_packet).unwrap();
    assert!(delta.contains("\"mode\": \"delta\""));
    assert!(delta.contains("\"elided_sections\":"));
    assert!(!delta.contains("\n  \"symbol_source\":"));
    let _ = fs::remove_file(full_packet);
    let _ = fs::remove_file(delta_packet);

    let review = assert_success(fgx(&[
        "review",
        "patches/signed_add_crypto_effect.fgp",
        "--json",
    ]));
    assert!(review.contains("\"schema\": \"forgentex.agent_review.v1\""));
    assert!(review.contains("\"patch_review\": {"));
    assert!(review.contains("\"signature_status\": \"signed-mock-accepted\""));
    assert!(review.contains("\"check_only\": true"));

    let err = assert_failure(fgx(&["context", "@function.nope", "--json"]));
    assert!(err.contains("symbol not found in project context"));

    let bad_budget = assert_failure(fgx(&[
        "context",
        "@function.mix_samples",
        "--max-tokens",
        "0",
    ]));
    assert!(bad_budget.contains("needs a positive integer"));

    let bad_dep = assert_failure(fgx(&[
        "context",
        "@function.mix_samples",
        "--deps",
        "types,secrets",
    ]));
    assert!(bad_dep.contains("unknown context dependency 'secrets'"));
}

#[test]
fn graph_artifact_commands_are_available() {
    let graph_path = temp_file("stage31_inline.fggraph");
    let graph_path_text = graph_path.to_string_lossy().to_string();
    let out = assert_success(fgx(&[
        "graph",
        "dump",
        "examples/smoke/inline_tests.fgx",
        "--out",
        &graph_path_text,
    ]));
    assert!(out.contains("wrote"));

    let validation = assert_success(fgx(&["graph", "validate", &graph_path_text, "--json"]));
    assert!(validation.contains("\"schema\": \"forgentex.graph_validate.v1\""));
    assert!(validation.contains("\"status\": \"pass\""));
    assert!(validation.contains("\"graph_schema\": \"forgentex.graph.v1\""));

    let view = assert_success(fgx(&["graph", "view", &graph_path_text, "--to", "fgai"]));
    assert!(view.contains("TEST \"clamp high\""));

    let context = assert_success(fgx(&[
        "graph",
        "context",
        &graph_path_text,
        "@function.clamp_byte",
        "--deps",
        "effects,tests",
        "--json",
    ]));
    assert!(context.contains("\"schema\": \"forgentex.graph_context.v1\""));
    assert!(context.contains("@test.clamp_high"));

    let roundtrip = assert_success(fgx(&[
        "graph",
        "roundtrip",
        "examples/smoke/inline_tests.fgx",
        "--json",
    ]));
    assert!(roundtrip.contains("\"schema\": \"forgentex.graph_roundtrip.v1\""));
    assert!(roundtrip.contains("\"status\": \"pass\""));

    let audio_graph = temp_file("stage31_audio.fggraph");
    let patched_graph = temp_file("stage31_audio_patched.fggraph");
    let audio_graph_text = audio_graph.to_string_lossy().to_string();
    let patched_graph_text = patched_graph.to_string_lossy().to_string();
    assert_success(fgx(&[
        "graph",
        "dump",
        "examples/media/audio_mix.fgx",
        "--out",
        &audio_graph_text,
    ]));
    let patch = assert_success(fgx(&[
        "graph",
        "patch",
        &audio_graph_text,
        "patches/add_fast_path.fgp",
        "--out",
        &patched_graph_text,
        "--json",
    ]));
    assert!(patch.contains("\"schema\": \"forgentex.graph_patch.v1\""));
    assert!(patch.contains("\"canonical_ast_equal_source_patch\": true"));

    let diff = assert_success(fgx(&[
        "graph",
        "diff",
        &audio_graph_text,
        &patched_graph_text,
        "--json",
    ]));
    assert!(diff.contains("\"schema\": \"forgentex.graph_diff.v1\""));
    assert!(diff.contains("@function.mix_samples_avx2"));
    assert!(diff.contains("unsafe_changed"));

    let _ = fs::remove_file(graph_path);
    let _ = fs::remove_file(audio_graph);
    let _ = fs::remove_file(patched_graph);
}

#[test]
fn fuzz_and_backend_differential_commands_are_available() {
    let fuzz = assert_success(fgx(&["fuzz", "--iterations", "4", "--json"]));
    assert!(fuzz.contains("\"schema\": \"forgentex.fuzz.v1\""));
    assert!(fuzz.contains("\"failed\": 0"));
    assert!(fuzz.contains("patch-format-check"));
    assert!(fuzz.contains("target-validation"));

    let diff = assert_success(fgx(&["diff-test", "--json"]));
    assert!(diff.contains("\"schema\": \"forgentex.backend_differential.v1\""));
    assert!(diff.contains("\"interpreter_status\": \"pass\""));
    assert!(diff.contains("\"backend_entry_found\": true"));
    // The command earns its name only if it executed the generated C. On a
    // host without a compiler it must say so rather than pass silently.
    assert!(
        diff.contains("\"native_execution\": \"matched\"")
            || diff.contains("\"native_execution\": \"unavailable\""),
        "{diff}"
    );
    assert!(diff.contains("\"result\": \"pass\""));

    let err = assert_failure(fgx(&["fuzz", "--iterations", "nope"]));
    assert!(err.contains("invalid --iterations value"));
}

#[test]
fn performance_tooling_commands_are_available() {
    let bench = assert_success(fgx(&[
        "bench",
        "examples/media/audio_mix_simd.fgx",
        "--entry",
        "mix_samples_simd",
        "--json-input",
        "examples/bench/audio_mix.json",
        "--target",
        "aarch64-unknown-linux-gnu",
        "--iterations",
        "4",
        "--json",
    ]));
    assert!(bench.contains("\"schema\": \"forgentex.bench.v1\""));
    assert!(bench.contains("\"correctness\": \"pass\""));
    assert!(bench.contains("\"architecture_path\": \"aarch64.neon-review\""));
    assert!(bench.contains("\"bounds_report\""));
    assert!(bench.contains("\"allocation_report\""));

    let perf = assert_success(fgx(&[
        "perf-report",
        "--scalar",
        "examples/media/audio_mix.fgx",
        "--optimized",
        "examples/media/audio_mix_simd.fgx",
        "--entry",
        "mix_samples",
        "--optimized-entry",
        "mix_samples_simd",
        "--json-input",
        "examples/bench/audio_mix.json",
        "--json",
    ]));
    assert!(perf.contains("\"schema\": \"forgentex.perf_report.v1\""));
    assert!(perf.contains("\"patch_gate\": \"pass\""));
    assert!(perf.contains("x86_64.avx2-review"));
    assert!(perf.contains("aarch64.neon-review"));
    assert!(perf.contains("riscv.vector-review"));

    let err = assert_failure(fgx(&[
        "bench",
        "examples/media/audio_mix.fgx",
        "--entry",
        "mix_samples",
        "--json-input",
        "examples/bench/audio_mix.json",
        "--target",
        "mips64-demo",
    ]));
    assert!(err.contains("unknown target triple"));
}

#[test]
fn capsule_command_is_available() {
    let text = assert_success(fgx(&["capsule", "examples/capsule/normalize_python.fgx"]));
    assert!(text.contains("Forgentex capsule review"));
    assert!(text.contains("status=pass"));
    assert!(text.contains("risk=managed"));

    let json = assert_success(fgx(&[
        "capsule",
        "examples/capsule/price_oracle_solidity.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.capsule_review.v1\""));
    assert!(json.contains("\"risk_class\": \"smart_contract\""));
    assert!(json.contains("\"status\": \"review_required\""));

    let err = assert_failure(fgx(&["capsule", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn capsule_run_command_is_available() {
    let text = assert_success(fgx(&[
        "capsule-run",
        "examples/capsule/normalize_python.fgx",
    ]));
    assert!(text.contains("Forgentex capsule run"));
    assert!(text.contains("status=pass"));
    assert!(text.contains("passed=3"));

    let hook = assert_success(fgx(&[
        "capsule-run",
        "examples/capsule/normalize_python.fgx",
        "--input",
        "300",
    ]));
    assert!(hook.contains("outputs: [255]"));

    let json = assert_success(fgx(&[
        "capsule-run",
        "examples/capsule/normalize_python.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.capsule_run.v1\""));

    let miss = assert_failure(fgx(&[
        "capsule-run",
        "examples/capsule/normalize_python.fgx",
        "--input",
        "7",
    ]));
    assert!(miss.contains("no_matching_fixture"));
}

#[test]
fn pipeline_run_exposes_pipeline_wide_budget_controls() {
    let exhausted = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-stages",
        "1",
        "--schedule",
        "graph",
        "--json",
    ]));
    assert!(exhausted.contains("\"schema\":\"forgentex.pipeline_run.v1\""));
    assert!(exhausted.contains("\"schema\":\"forgentex.pipeline_budget.v1\""));
    assert!(exhausted.contains("\"status\":\"budget_exhausted\""));
    assert!(exhausted.contains("\"max_stages\":1"));
    assert!(exhausted.contains("planned stage count 2"));

    let invalid = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-wall-ms",
        "0",
    ]));
    assert!(invalid.contains("--max-wall-ms needs a positive integer"));

    let invalid_cpu = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-cpu-ms",
        "0",
    ]));
    assert!(invalid_cpu.contains("--max-cpu-ms needs a positive integer"));

    let invalid_memory = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-memory-bytes",
        "0",
    ]));
    assert!(invalid_memory.contains("--max-memory-bytes needs a positive integer"));

    let invalid_fs = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-fs-bytes",
        "0",
    ]));
    assert!(invalid_fs.contains("--max-fs-bytes needs a positive integer"));

    let invalid_net = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-net-bytes",
        "0",
    ]));
    assert!(invalid_net.contains("--max-net-bytes needs a positive integer"));

    let unavailable_net = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--max-net-bytes",
        "1",
        "--json",
    ]));
    assert!(unavailable_net.contains("aggregate network accounting"));
    assert!(unavailable_net.contains("\"stages\":[]"));
    assert!(unavailable_net.contains("\"max_net_bytes\":1"));
    assert!(unavailable_net.contains("\"used_net_bytes\":null"));

    let invalid_stop = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--stop-after-stages",
        "0",
    ]));
    assert!(invalid_stop.contains("--stop-after-stages needs a positive integer"));

    let stopped = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--stop-after-stages",
        "1",
        "--allow-effects",
        "cpu",
        "--json",
    ]));
    assert!(stopped.contains("\"status\":\"cancelled\""));
    assert!(stopped.contains("\"schema\":\"forgentex.pipeline_cancellation.v1\""));
    assert!(stopped.contains("\"source\":\"stage_limit\""));
    assert!(stopped.contains("\"committed_stages\":1"));

    let schedule = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--schedule",
        "random",
    ]));
    assert!(schedule.contains("must be declaration, graph, or source"));

    let source_graph = assert_success(fgx(&[
        "pipeline-run",
        "examples/pipeline/source_graph.fgx",
        "--schedule",
        "source",
        "--allow-effects",
        "cpu",
        "--json",
    ]));
    assert!(source_graph.contains("\"mode\":\"deterministic_source_graph\""));
    assert!(source_graph.contains("\"origin\":\"source_declaration\""));
    assert!(source_graph.contains("\"declaration\":\"preview\""));

    let move_handoff = assert_success(fgx(&[
        "pipeline-run",
        "examples/pipeline/move_handoff.fgx",
        "--schedule",
        "graph",
        "--allow-effects",
        "alloc,cpu",
        "--json",
    ]));
    assert!(move_handoff.contains("\"move_sources\":[\"samples\"]"));
    assert!(move_handoff.contains("\"handoff_committed\":true"));
    assert!(!move_handoff.contains("\"name\":\"samples\",\"type\""));

    let update_handoff = assert_success(fgx(&[
        "pipeline-run",
        "examples/pipeline/update_handoff.fgx",
        "--schedule",
        "graph",
        "--allow-effects",
        "alloc,cpu",
        "--json",
    ]));
    assert!(update_handoff.contains("\"update_sources\":[\"samples\"]"));
    assert!(update_handoff.contains("\"handoff_committed\":true"));
    assert!(update_handoff.contains("\"before_sha256\""));
    assert!(update_handoff.contains("\"after_sha256\""));
    assert!(update_handoff.contains("\"name\":\"sample_sum\",\"type\":\"i32\",\"value\":-400"));
}

#[test]
fn pipeline_run_reuses_an_explicit_persistent_cache_directory() {
    let cache_dir = temp_file("persistent_pipeline_cache");
    let _ = fs::remove_dir_all(&cache_dir);
    fs::create_dir(&cache_dir).unwrap();
    let cache_dir_text = cache_dir.to_str().unwrap();
    let args = [
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--allow-effects",
        "cpu",
        "--cache-dir",
        cache_dir_text,
        "--json",
    ];
    let first = assert_success(fgx(&args));
    assert!(first.contains("stored_process_local_and_integrity_checked_file"));
    assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 2);

    let second = assert_success(fgx(&args));
    assert!(second.contains("\"decision\":\"cache_hit\""));
    assert!(second.contains("\"storage\":\"integrity_checked_content_addressed_file\""));

    fs::remove_dir_all(&cache_dir).unwrap();
    let missing = assert_failure(fgx(&args));
    assert!(missing.contains("pipeline cache directory"));
    assert!(missing.contains("is unavailable"));
}

#[test]
fn pipeline_run_authenticates_persistent_cache_across_processes() {
    let cache_dir = temp_file("authenticated_pipeline_cache");
    let key_file = temp_file("authenticated_pipeline_cache.key");
    let wrong_key_file = temp_file("authenticated_pipeline_cache_wrong.key");
    let short_key_file = temp_file("authenticated_pipeline_cache_short.key");
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::remove_file(&key_file);
    let _ = fs::remove_file(&wrong_key_file);
    let _ = fs::remove_file(&short_key_file);
    fs::create_dir(&cache_dir).unwrap();
    fs::write(&key_file, b"forgentex-authenticated-cache-key-01").unwrap();
    fs::write(&wrong_key_file, b"forgentex-authenticated-cache-key-02").unwrap();
    fs::write(&short_key_file, b"short").unwrap();
    let cache_dir_text = cache_dir.to_str().unwrap();
    let key_file_text = key_file.to_str().unwrap();
    let args = [
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--allow-effects",
        "cpu",
        "--cache-dir",
        cache_dir_text,
        "--cache-key-file",
        key_file_text,
        "--json",
    ];
    let first = assert_success(fgx(&args));
    assert!(first.contains("stored_process_local_and_authenticated_file"));
    assert!(!first.contains("forgentex-authenticated-cache-key"));

    let second = assert_success(fgx(&args));
    assert!(second.contains("\"decision\":\"cache_hit\""));
    assert!(second.contains("\"storage\":\"authenticated_hmac_sha256_content_addressed_file\""));

    let artifact_args = [
        "pipeline-run",
        "examples/pipeline/artifact_preview.fgx",
        "--allow-effects",
        "alloc,cpu",
        "--cache-dir",
        cache_dir_text,
        "--cache-key-file",
        key_file_text,
        "--json",
    ];
    let artifact_first = assert_success(fgx(&artifact_args));
    assert!(artifact_first.contains("forgentex.native_artifact.v1"));
    assert!(!artifact_first.contains("\"decision\":\"cache_hit\""));
    let artifact_second = assert_success(fgx(&artifact_args));
    assert!(artifact_second.contains("\"decision\":\"cache_hit\""));
    assert!(artifact_second.contains(
        "\"artifact\":{\"schema\":\"forgentex.native_artifact.v1\",\"media_type\":\"application/octet-stream\",\"bytes\":4"
    ));

    let wrong_args = [
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--allow-effects",
        "cpu",
        "--cache-dir",
        cache_dir_text,
        "--cache-key-file",
        wrong_key_file.to_str().unwrap(),
        "--json",
    ];
    let wrong = assert_success(fgx(&wrong_args));
    assert!(!wrong.contains("\"decision\":\"cache_hit\""));
    assert!(wrong.contains("\"native\":true"));

    let missing_root = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--cache-key-file",
        key_file_text,
    ]));
    assert!(missing_root.contains("--cache-key-file requires --cache-dir"));
    let short = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--cache-dir",
        cache_dir_text,
        "--cache-key-file",
        short_key_file.to_str().unwrap(),
    ]));
    assert!(short.contains("between 32 and 4096 bytes"));

    let link_key_file = temp_file("authenticated_pipeline_cache_link.key");
    let _ = fs::remove_file(&link_key_file);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&key_file, &link_key_file).unwrap();
        let linked = assert_failure(fgx(&[
            "pipeline-run",
            "examples/pipeline/music_preview.fgx",
            "--cache-dir",
            cache_dir_text,
            "--cache-key-file",
            link_key_file.to_str().unwrap(),
        ]));
        assert!(linked.contains("must be a non-symlink regular file"));
        fs::remove_file(&link_key_file).unwrap();
    }
    #[cfg(windows)]
    if std::os::windows::fs::symlink_file(&key_file, &link_key_file).is_ok() {
        let linked = assert_failure(fgx(&[
            "pipeline-run",
            "examples/pipeline/music_preview.fgx",
            "--cache-dir",
            cache_dir_text,
            "--cache-key-file",
            link_key_file.to_str().unwrap(),
        ]));
        assert!(linked.contains("must be a non-symlink regular file"));
        fs::remove_file(&link_key_file).unwrap();
    }

    let inside_key = cache_dir.join("inside.key");
    fs::write(&inside_key, b"forgentex-authenticated-cache-key-03").unwrap();
    let inside = assert_failure(fgx(&[
        "pipeline-run",
        "examples/pipeline/music_preview.fgx",
        "--cache-dir",
        cache_dir_text,
        "--cache-key-file",
        inside_key.to_str().unwrap(),
    ]));
    assert!(inside.contains("must be outside the cache directory"));

    fs::remove_dir_all(cache_dir).unwrap();
    fs::remove_file(key_file).unwrap();
    fs::remove_file(wrong_key_file).unwrap();
    fs::remove_file(short_key_file).unwrap();
}

#[test]
fn capsule_lock_and_audit_commands_are_available() {
    let lock = assert_success(fgx(&["capsule-lock", "examples/capsule/stats_python.fgx"]));
    assert!(lock.contains("Forgentex capsule lock"));
    assert!(lock.contains("dependencies=2"));

    let lock_json = assert_success(fgx(&[
        "capsule-lock",
        "examples/capsule/stats_python.fgx",
        "--json",
    ]));
    assert!(lock_json.contains("\"schema\": \"forgentex.capsule_lock.v1\""));
    assert!(lock_json.contains("\"ecosystem\": \"pypi\""));

    let audit = assert_success(fgx(&[
        "capsule-audit",
        "examples/capsule/stats_python.fgx",
        "--json",
    ]));
    assert!(audit.contains("\"schema\": \"forgentex.capsule_audit.v1\""));
    assert!(audit.contains("\"status\": \"medium\""));

    let err = assert_failure(fgx(&["capsule-lock", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn compress_command_is_available() {
    let text = assert_success(fgx(&["compress", "examples/audio/src/dsp.fgx"]));
    assert!(text.contains("Forgentex agent-view compression"));
    assert!(text.contains("roundtrip_ok=true"));

    let json = assert_success(fgx(&["compress", "examples/audio/src/dsp.fgx", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.capsule_compress.v1\""));
    assert!(json.contains("\"roundtrip_ok\": true"));

    let err = assert_failure(fgx(&["compress", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn capsule_context_command_is_available() {
    let inv = assert_success(fgx(&[
        "capsule-context",
        "examples/capsule/web_session.fgx",
    ]));
    assert!(inv.contains("Forgentex capsule inventory"));
    assert!(inv.contains("backend"));

    let ctx = assert_success(fgx(&[
        "capsule-context",
        "examples/capsule/saxpy_cuda.fgx",
        "--capsule",
        "saxpy",
        "--json",
    ]));
    assert!(ctx.contains("\"schema\": \"forgentex.capsule_context.v1\""));
    assert!(ctx.contains("\"overall_decision\": \"review\""));

    let err = assert_failure(fgx(&[
        "capsule-context",
        "examples/capsule/saxpy_cuda.fgx",
        "--capsule",
        "missing",
    ]));
    assert!(err.contains("no capsule named"));
}

#[test]
fn contract_review_command_is_available() {
    let text = assert_success(fgx(&[
        "contract-review",
        "examples/capsule/price_oracle_solidity.fgx",
    ]));
    assert!(text.contains("Forgentex smart-contract capsule review"));
    assert!(text.contains("compiler=0.8.24"));
    assert!(text.contains("decision=review"));

    let json = assert_success(fgx(&[
        "contract-review",
        "examples/capsule/price_oracle_solidity.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.contract_review.v1\""));
    assert!(json.contains("\"gas_limit\": \"120000\""));

    let err = assert_failure(fgx(&["contract-review", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn shell_review_command_is_available() {
    let text = assert_success(fgx(&["shell-review", "examples/capsule/backup_bash.fgx"]));
    assert!(text.contains("Forgentex shell capsule review"));
    assert!(text.contains("commands=[tar,gzip]"));
    assert!(text.contains("decision=review"));

    let json = assert_success(fgx(&[
        "shell-review",
        "examples/capsule/backup_bash.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.shell_review.v1\""));
    assert!(json.contains("\"allowed_commands\": [\"tar\", \"gzip\"]"));

    let err = assert_failure(fgx(&["shell-review", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn native_review_command_is_available() {
    let text = assert_success(fgx(&["native-review", "examples/capsule/saxpy_cuda.fgx"]));
    assert!(text.contains("Forgentex native/GPU capsule review"));
    assert!(text.contains("risk=gpu"));
    assert!(text.contains("gpu_arch=sm_80"));

    let json = assert_success(fgx(&[
        "native-review",
        "examples/capsule/clamp_c.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.native_review.v1\""));
    assert!(json.contains("\"risk_class\": \"native\""));

    let err = assert_failure(fgx(&["native-review", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn runtimes_command_is_available() {
    let text = assert_success(fgx(&["runtimes"]));
    assert!(text.contains("Forgentex managed runtime catalog"));
    assert!(text.contains("typescript"));

    let json = assert_success(fgx(&["runtimes", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.runtime_catalog.v1\""));
    assert!(json.contains("\"language\": \"go\""));
}

#[test]
fn sandbox_command_is_available() {
    let text = assert_success(fgx(&["sandbox", "examples/capsule/web_counter.fgx"]));
    assert!(text.contains("Forgentex sandbox plan"));
    assert!(text.contains("status=allow"));
    assert!(text.contains("limit wall_ms=1000"));

    let json = assert_success(fgx(&[
        "sandbox",
        "examples/capsule/web_counter.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.sandbox_plan.v1\""));
    assert!(json.contains("\"capability\": \"net.listen\""));

    let err = assert_failure(fgx(&["sandbox", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn namespace_command_is_available() {
    let text = assert_success(fgx(&["namespace", "examples/capsule/web_counter.fgx"]));
    assert!(text.contains("Forgentex shared namespace"));
    assert!(text.contains("status=review_required"));
    assert!(text.contains("visitor_count"));

    let json = assert_success(fgx(&[
        "namespace",
        "examples/capsule/web_counter.fgx",
        "--json",
    ]));
    assert!(json.contains("\"schema\": \"forgentex.shared_namespace.v1\""));
    assert!(json.contains("\"writers\": [\"backend\"]"));
    assert!(json.contains("\"readers\": [\"frontend\"]"));

    let err = assert_failure(fgx(&["namespace", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn cfg_command_is_available() {
    let text = assert_success(fgx(&["cfg", "examples/media/audio_mix.fgx"]));
    assert!(text.contains("@function.mix_samples blocks="));
    assert!(text.contains("back_edges=1"));

    let json = assert_success(fgx(&["cfg", "examples/media/audio_mix.fgx", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.cfg_report.v1\""));
    assert!(json.contains("\"schema\": \"forgentex.cfg.v1\""));
    assert!(json.contains("\"kind\": \"loop_header\""));
    assert!(json.contains("\"live_in\""));

    let err = assert_failure(fgx(&["cfg", "examples/nope.fgx"]));
    assert!(err.contains("failed to read"));
}

#[test]
fn release_check_command_is_available() {
    let text = assert_success(fgx(&["release-check"]));
    assert!(text.contains("Forgentex release check"));
    assert!(text.contains("decision=v1_ready"));
    assert!(text.contains("ownership_borrowing status=pass"));

    let json = assert_success(fgx(&["release-check", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.release_check.v1\""));
    assert!(json.contains("\"current_track\": \"v1.0-cfg-dataflow-borrowck\""));
    assert!(json.contains("\"decision\": \"v1_ready\""));
    assert!(json.contains("\"id\": \"ci_fuzz_differential\""));
    assert!(json.contains("\"hardening_checks\": ["));
    assert!(json.contains("\"id\": \"diagnostic_shape_freeze\""));
    assert!(json.contains("\"hardening_fail\": 0"));
    assert!(json.contains("\"label\": \"v0.25\""));
    assert!(json.contains("\"label\": \"v0.26\""));
    assert!(json.contains("\"label\": \"v1.0\""));
    assert!(json.contains("src/borrowck.rs"));

    let strict = assert_success(fgx(&["release-check", "--strict"]));
    assert!(strict.contains("decision=v1_ready"));

    let public = assert_success(fgx(&["release-check", "--target", "0.1", "--json"]));
    assert!(public.contains("\"current_track\": \"public-0.1-stabilization\""));
    assert!(public.contains("\"decision\": \"public_0_1_not_ready\""));
    assert!(public.contains("\"id\": \"public_0_1_profile\""));
    assert!(public.contains("\"id\": \"public_0_1_distribution\""));

    let public_strict = assert_failure(fgx(&["release-check", "--target", "0.1", "--strict"]));
    assert!(public_strict.contains("decision=public_0_1_not_ready"));
    assert!(public_strict.contains("release target '0.1' is not ready"));
}

#[test]
fn help_and_unknown_command_are_covered() {
    let out = assert_success(fgx(&["help"]));
    assert!(out.contains("Forgentex CLI"));
    assert!(out.contains("fgx --version"));
    // The writing verbs each carry their own flags, so help is usable for the
    // guarded edits rather than listing one merged `fs` line.
    assert!(out.contains("fgx fs set <file> --tag <node-id> --from <path> [--expect <hash>]"));
    assert!(out.contains("fgx fs changes <file> --rev <git-rev>|--against <file>|--since <iso>"));
    assert!(out.contains("fgx doctor [--json] [--strict]"));
    assert!(out.contains("fgx diagnose <file> --json"));
    assert!(out.contains("fgx diagnostics list"));
    assert!(out.contains("fgx explain <diagnostic-code>"));
    assert!(out.contains("fgx effects <file> --json"));
    assert!(out.contains("fgx check --project"));
    assert!(out.contains("fgx borrow-report"));
    assert!(out.contains("fgx targets"));
    assert!(out.contains("--target <triple>"));
    assert!(out.contains("fgx asm-check"));
    assert!(out.contains("fgx abi"));
    assert!(out.contains("fgx header"));
    assert!(out.contains("fgx simd-review"));
    assert!(out.contains("fgx backends"));
    assert!(out.contains("fgx ir <project|file>"));
    assert!(out.contains("fgx stdlib"));
    assert!(out.contains("fgx build <project> --backend c"));
    assert!(out.contains("fgx new <path>"));
    assert!(out.contains("fgx lock"));
    assert!(out.contains("fgx test <file>"));
    assert!(out.contains("fgx test [--project"));
    assert!(out.contains("fgx publish"));
    assert!(out.contains("fgx audit"));
    assert!(out.contains("fgx verify"));
    assert!(out.contains("fgx api --public"));
    assert!(out.contains("--json-input"));
    assert!(out.contains("--check-only"));
    assert!(out.contains("fgx context <symbol>"));
    assert!(out.contains("fgx review <patch>"));
    assert!(out.contains("fgx graph dump"));
    assert!(out.contains("fgx graph validate"));
    assert!(out.contains("fgx graph context"));
    assert!(out.contains("fgx fuzz"));
    assert!(out.contains("fgx diff-test"));
    assert!(out.contains("fgx bench"));
    assert!(out.contains("fgx perf-report"));
    assert!(out.contains("fgx fix --plan"));
    assert!(out.contains("fgx release-check"));

    let err = assert_failure(fgx(&["wat"]));
    assert!(err.contains("unknown command"));
}

#[test]
fn version_and_doctor_commands_are_available() {
    let version = assert_success(fgx(&["--version"]));
    assert_eq!(version.trim(), concat!("fgx ", env!("CARGO_PKG_VERSION")));

    let text = assert_success(fgx(&["doctor"]));
    assert!(text.contains("Forgentex doctor"));
    assert!(text.contains("interpreter=ready"));

    let json = assert_success(fgx(&["doctor", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.doctor.v1\""));
    assert!(json.contains("\"host\":"));
    assert!(json.contains("\"tools\":"));

    let error = assert_failure(fgx(&["doctor", "--wat"]));
    assert!(error.contains("unknown doctor option '--wat'"));
}

#[test]
fn media_visualizer_application_publishes_a_governed_frame() {
    let root = temp_file("media_visualizer_application");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("output")).unwrap();
    fs::copy(
        manifest_dir().join("examples/apps/media_visualizer/Forgentex.toml"),
        root.join("Forgentex.toml"),
    )
    .unwrap();
    fs::copy(
        manifest_dir().join("examples/apps/media_visualizer/src/main.fgx"),
        root.join("src/main.fgx"),
    )
    .unwrap();
    let root_text = root.to_string_lossy().to_string();

    let out = assert_success(fgx(&["app-run", &root_text, "--arg", "11", "--json"]));
    assert!(out.contains("\"schema\":\"forgentex.pipeline_application_run.v1\""));
    assert!(out.contains("\"origin\":\"source_declaration\""));
    assert!(out.contains("\"declaration\":\"visualizer\""));
    assert!(out.contains("\"decision\":\"within_budget\""));
    assert!(out.contains("\"op\":\"publish_pipeline_artifact\""));
    assert!(out.contains("\"media_type\":\"image/x-portable-pixmap\""));

    let artifact = fs::read(root.join("output/visualizer.ppm")).unwrap();
    assert_eq!(&artifact[..13], b"P6\n64 36\n255\n");
    assert_eq!(artifact.len(), 6925);

    let denied = assert_failure(fgx(&["app-run", &root_text, "--arg", "11", "--json"]));
    assert!(denied.contains("\"code\":\"io_target_exists\""));
    assert_eq!(
        fs::read(root.join("output/visualizer.ppm")).unwrap(),
        artifact
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffolded_application_template_runs_without_further_edits() {
    let root = temp_file("new_application_template");
    let _ = fs::remove_dir_all(&root);
    let root_text = root.to_string_lossy().to_string();

    let created = assert_success(fgx(&[
        "new",
        &root_text,
        "--name",
        "greeter",
        "--template",
        "app",
    ]));
    assert!(created.contains("Forgentex.toml"));
    assert!(created.contains("main.fgx"));
    assert!(created.contains("run: fgx app-run"));

    assert!(assert_success(fgx(&["check", "--project", &root_text])).contains("ok"));

    // The scaffold must run end to end with no manual edits.
    let out = assert_success(fgx(&[
        "app-run",
        &root_text,
        "--arg",
        "hello from forgentex",
        "--json",
    ]));
    assert!(out.contains("\"schema\":\"forgentex.application_run.v1\""));
    assert!(out.contains("\"status\":\"ok\""));
    assert!(out.contains("\"entry\":\"greet\""));
    assert!(out.contains("\"op\":\"publish_artifact_text\""));
    assert!(out.contains("\"media_type\":\"text/plain\""));
    assert_eq!(
        fs::read_to_string(root.join("output/greeting.txt")).unwrap(),
        "hello from forgentex"
    );

    // create_new publication still refuses to clobber on a rerun.
    let denied = assert_failure(fgx(&[
        "app-run",
        &root_text,
        "--arg",
        "second run",
        "--json",
    ]));
    assert!(denied.contains("\"code\":\"io_target_exists\""));
    assert_eq!(
        fs::read_to_string(root.join("output/greeting.txt")).unwrap(),
        "hello from forgentex"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn wav_waveform_application_renders_a_real_pcm_envelope() {
    let root = temp_file("wav_waveform_application");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("input")).unwrap();
    fs::create_dir_all(root.join("output")).unwrap();
    let source_root = manifest_dir().join("examples/apps/wav_waveform");
    fs::copy(
        source_root.join("Forgentex.toml"),
        root.join("Forgentex.toml"),
    )
    .unwrap();
    fs::copy(source_root.join("src/main.fgx"), root.join("src/main.fgx")).unwrap();
    fs::copy(
        source_root.join("input/loop_8bars_128bpm.wav"),
        root.join("input/loop.wav"),
    )
    .unwrap();
    let root_text = root.to_string_lossy().to_string();

    let out = assert_success(fgx(&[
        "app-run",
        &root_text,
        "--arg",
        "input/loop.wav",
        "--json",
    ]));
    assert!(out.contains("\"status\":\"ok\""));
    // 2.6 MB of PCM crosses the governed read boundary and is accounted.
    assert!(out.contains("\"read_bytes\":2649628"));
    assert!(out.contains("\"media_type\":\"image/x-portable-pixmap\""));

    let image = fs::read(root.join("output/waveform.ppm")).unwrap();
    assert_eq!(&image[..13], b"P6\n64 36\n255\n");
    assert_eq!(image.len(), 6925);

    // The envelope must track the audio, not be flat: 8 bars at 128 BPM is 32
    // beats across 64 columns, so on-beat and off-beat columns differ.
    let mut heights = Vec::new();
    for x in 0..64usize {
        let mut lit = 0;
        for y in 0..36usize {
            if image[13 + ((y * 64) + x) * 3] == 64 {
                lit += 1;
            }
        }
        heights.push(lit);
    }
    let low = heights.iter().min().copied().unwrap();
    let high = heights.iter().max().copied().unwrap();
    assert!(
        low < high,
        "waveform envelope is flat: {heights:?} (expected real amplitude variation)"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn new_rejects_an_unknown_template() {
    let root = temp_file("new_unknown_template");
    let _ = fs::remove_dir_all(&root);
    let out = assert_failure(fgx(&[
        "new",
        &root.to_string_lossy(),
        "--template",
        "service",
    ]));
    assert!(out.contains("expected 'lib' or 'app'"));
    assert!(!root.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_codebook_build_check_measure_and_stats_are_available() {
    let path = temp_file("waveform.codebook.json");
    let path_text = path.to_string_lossy().to_string();
    let project = "examples/apps/wav_waveform";

    let built = assert_success(fgx(&[
        "codebook", "build", project, "--out", &path_text, "--budget", "1024", "--json",
    ]));
    assert!(built.contains("\"schema\": \"forgentex.codebook.v1\""));
    assert!(built.contains("\"tier\": \"semantic\""));
    assert!(built.contains("\"break_even_interactions\":"));
    assert!(built.contains("\"generation\": 1"));
    assert!(path.exists());

    let unchanged = temp_file("waveform.unchanged.codebook.json");
    let unchanged_text = unchanged.to_string_lossy().to_string();
    assert_success(fgx(&[
        "codebook",
        "build",
        project,
        "--previous",
        &path_text,
        "--out",
        &unchanged_text,
        "--budget",
        "1024",
    ]));
    assert_eq!(
        fs::read_to_string(&unchanged).unwrap(),
        fs::read_to_string(&path).unwrap()
    );

    let checked = assert_success(fgx(&[
        "codebook",
        "check",
        project,
        "--codebook",
        &path_text,
        "--json",
    ]));
    assert!(checked.contains("\"status\":\"pass\""));

    let measured = assert_success(fgx(&[
        "codebook",
        "measure",
        project,
        "--codebook",
        &path_text,
        "--json",
    ]));
    assert!(measured.contains("\"schema\":\"forgentex.codebook_measure.v1\""));
    assert!(measured.contains("\"status\":\"current\""));

    let stats = assert_success(fgx(&["codebook", "stats", "--codebook", &path_text]));
    assert!(stats.contains("Forgentex project codebook"));
    assert!(stats.contains("break_even_interactions="));

    let guarded_map = temp_file("waveform.guarded.fgmap");
    let encoded_map = temp_file("waveform.encoded.fgmap");
    let decoded_map = temp_file("waveform.decoded.fgmap");
    let guarded_map_text = guarded_map.to_string_lossy().to_string();
    let encoded_map_text = encoded_map.to_string_lossy().to_string();
    let decoded_map_text = decoded_map.to_string_lossy().to_string();
    let guarded = assert_success(fgx(&[
        "map",
        "--project",
        project,
        "--codebook",
        &path_text,
        "--out",
        &guarded_map_text,
    ]));
    assert!(guarded.contains("codebook=skipped_not_smaller"));
    assert!(!fs::read_to_string(&guarded_map)
        .unwrap()
        .starts_with("#FGCB"));
    assert_success(fgx(&[
        "map",
        "--project",
        project,
        "--codebook",
        &path_text,
        "--force-codebook",
        "--out",
        &encoded_map_text,
    ]));
    assert!(fs::read_to_string(&encoded_map)
        .unwrap()
        .starts_with("#FGCB v1 hash="));
    assert_success(fgx(&[
        "codebook",
        "decode",
        &encoded_map_text,
        "--codebook",
        &path_text,
        "--out",
        &decoded_map_text,
    ]));
    assert!(fs::read_to_string(&decoded_map)
        .unwrap()
        .contains("render_waveform"));

    let guarded_context = assert_success(fgx(&[
        "context",
        "@function.render_waveform",
        "--project",
        project,
        "--deps",
        "types,effects,tests",
        "--codebook",
        &path_text,
        "--json",
    ]));
    assert!(guarded_context.contains("\"codebook_ref\": \""));
    assert!(guarded_context.contains('§'));

    let encoded_context = assert_success(fgx(&[
        "context",
        "@function.render_waveform",
        "--project",
        project,
        "--deps",
        "types,effects,tests",
        "--codebook",
        &path_text,
        "--force-codebook",
        "--json",
    ]));
    assert!(encoded_context.contains("\"codebook_ref\": \""));
    assert!(encoded_context.contains('§'));
    assert!(encoded_context.contains("\"saved_by_codebook\":"));

    let patch = temp_file("waveform.codebook_source.fgp");
    let guarded_patch = temp_file("waveform.codebook_guarded.fgp");
    let encoded_patch = temp_file("waveform.codebook_encoded.fgp");
    let patch_text = patch.to_string_lossy().to_string();
    let guarded_patch_text = guarded_patch.to_string_lossy().to_string();
    let encoded_patch_text = encoded_patch.to_string_lossy().to_string();
    fs::write(
        &patch,
        "VERSION 1\nTARGET @function.render_waveform\nPRECONDITION NODE_EXISTS @function.render_waveform\nREVIEW helper=read_u32_le\nOP ADD_EFFECT effect=io.write\n",
    )
    .unwrap();
    let guarded_patch_receipt = assert_success(fgx(&[
        "codebook",
        "encode",
        &patch_text,
        "--codebook",
        &path_text,
        "--out",
        &guarded_patch_text,
    ]));
    assert!(guarded_patch_receipt.contains("codebook=skipped_not_smaller"));
    assert!(!fs::read_to_string(&guarded_patch)
        .unwrap()
        .starts_with("#FGCB"));
    assert_success(fgx(&[
        "codebook",
        "encode",
        &patch_text,
        "--codebook",
        &path_text,
        "--force-codebook",
        "--out",
        &encoded_patch_text,
    ]));
    assert!(fs::read_to_string(&encoded_patch)
        .unwrap()
        .starts_with("#FGCB v1 hash="));
    let patched = assert_success(fgx(&[
        "patch",
        "examples/apps/wav_waveform/src/main.fgx",
        &encoded_patch_text,
        "--project",
        project,
        "--codebook",
        &path_text,
        "--check-only",
    ]));
    assert!(patched.contains("Type check after patch: ok"));
    let reviewed = assert_success(fgx(&[
        "review",
        &encoded_patch_text,
        "--source",
        "examples/apps/wav_waveform/src/main.fgx",
        "--project",
        project,
        "--codebook",
        &path_text,
        "--json",
    ]));
    assert!(reviewed.contains("\"codebook_ref\": \""));
    assert!(reviewed.contains("ADD_EFFECT @function.render_waveform io.write"));

    let bad_budget = assert_failure(fgx(&["codebook", "build", project, "--budget", "1"]));
    assert!(bad_budget.contains("budget must be between"));
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(unchanged);
    let _ = fs::remove_file(guarded_map);
    let _ = fs::remove_file(encoded_map);
    let _ = fs::remove_file(decoded_map);
    let _ = fs::remove_file(patch);
    let _ = fs::remove_file(guarded_patch);
    let _ = fs::remove_file(encoded_patch);
}

/// The VS Code extension's grammars, snippets, and providers are validated by
/// three Node scripts under `tools/vscode/forgentex/test`. They are run here so
/// the extension cannot drift from the language without a failing build.
///
/// The snippet and smoke suites drive the real `fgx` binary, so they need a
/// release build; the grammar suite is self-contained. Hosts without Node skip.
#[test]
fn vscode_extension_suites_pass() {
    let node = if cfg!(windows) { "node.exe" } else { "node" };
    let probe = std::process::Command::new(node).arg("--version").output();
    if probe.map(|out| !out.status.success()).unwrap_or(true) {
        eprintln!("skipping: node is not available on this host");
        return;
    }

    let extension_dir = manifest_dir().join("tools/vscode/forgentex");
    // The binary this test run built, not whatever is left in target/release:
    // a stale release binary rejects syntax the language has since gained, and
    // the suites would report that as a broken extension.
    let fgx_binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_fgx"));

    let suites = vec![
        "test/grammar-coverage.js",
        "test/snippet-check.js",
        "test/extension-smoke.js",
    ];

    for suite in suites {
        let mut command = std::process::Command::new(node);
        command
            .arg(suite)
            .arg(&fgx_binary)
            .current_dir(&extension_dir);
        let output = command
            .output()
            .unwrap_or_else(|err| panic!("failed to run {suite}: {err}"));
        assert!(
            output.status.success(),
            "{suite} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// `fgx debug --interactive` drives a suspended session from stdin. The run
/// blocks mid-execution on a real thread, so this also covers that the session
/// resumes and terminates rather than deadlocking.
#[test]
fn interactive_debug_session_steps_and_evaluates() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let root = temp_file("interactive_debug");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let file = root.join("sess.fgx");
    fs::write(
        &file,
        "fn helper(x: i32) -> i32:\n    let doubled: i32 = x * 2\n    return doubled\n\nfn run() -> i32:\n    let a: i32 = 21\n    let b: i32 = helper(a)\n    return b\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fgx"))
        .args([
            "debug",
            &file.to_string_lossy(),
            "--entry",
            "run",
            "--interactive",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fgx debug --interactive");

    // step, step (into helper), print in the callee's frame, step out, finish.
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"s\ns\np x * 2\no\nc\n")
        .unwrap();
    let out = child.wait_with_output().expect("session should terminate");
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Stepping in reaches the callee at depth 2.
    assert!(
        text.contains("@function.helper.stmt.0 (depth 2)"),
        "step-in should reach the callee: {text}"
    );
    // Evaluation happens in the paused frame, where `x` is bound.
    assert!(text.contains("x * 2 = 42"), "{text}");
    // Step-out returns to the caller.
    assert!(text.contains("@function.run.stmt.2"), "{text}");
    assert!(text.contains("session completed"), "{text}");
    let _ = fs::remove_dir_all(root);
}

/// Drive `fgx dap` with a scripted Debug Adapter Protocol client.
///
/// This covers the whole bridge end to end — framing, the configuration
/// handshake, line-to-node breakpoint resolution, stepping into a callee,
/// variable inspection, and running to a breakpoint — against the real
/// binary. Hosts without Python skip.
#[test]
fn dap_adapter_serves_a_full_debug_session() {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let probe = std::process::Command::new(python).arg("--version").output();
    if probe.map(|out| !out.status.success()).unwrap_or(true) {
        eprintln!("skipping: python is not available on this host");
        return;
    }

    let root = temp_file("dap_session");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let program = root.join("sess.fgx");
    fs::write(
        &program,
        "fn helper(x: i32) -> i32:\n    let doubled: i32 = x * 2\n    return doubled\n\nfn run() -> i32:\n    let a: i32 = 21\n    let b: i32 = helper(a)\n    return b\n",
    )
    .unwrap();

    let script = manifest_dir().join("tools/dap/session_check.py");
    let output = std::process::Command::new(python)
        .arg(&script)
        .arg(env!("CARGO_BIN_EXE_fgx"))
        .arg(&program)
        .output()
        .expect("dap session check should run");

    assert!(
        output.status.success(),
        "dap session failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("DAP session OK"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fs_carries_a_project_filesystem_in_one_file() {
    // The cycle a person actually performs: start from a program, add a text
    // payload and a binary one, look around, then materialise it all back to
    // real files and require the bytes to be identical.
    let root = temp_file("fs_project");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let project = root.join("proj.fgx");
    std::fs::write(
        &project,
        "fn total(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .unwrap();

    let csv = root.join("sales.csv");
    std::fs::write(&csv, "region,units\nnorth,120\n").unwrap();
    // Bytes that are not text and do not survive a lossy conversion.
    let binary: Vec<u8> = (0..=255u8).collect();
    let wav = root.join("tone.wav");
    std::fs::write(&wav, &binary).unwrap();

    let project_arg = project.display().to_string();
    assert_success(fgx(&[
        "fs",
        "add",
        &project_arg,
        "--from",
        &csv.display().to_string(),
        "--by",
        "agent:claude-opus-5",
        "--at",
        "2026-09-20T12:00:00Z",
    ]));
    assert_success(fgx(&[
        "fs",
        "add",
        &project_arg,
        "--from",
        &wav.display().to_string(),
        "--by",
        "human:viktor",
        "--at",
        "2026-09-20T12:01:00Z",
    ]));

    // The file still checks, and the code that was already there is untouched.
    assert!(assert_success(fgx(&["check", &project_arg])).contains("ok"));
    let written = std::fs::read_to_string(&project).unwrap();
    assert!(
        written.contains("fn total(a: i32, b: i32) -> i32:"),
        "{written}"
    );

    let listing = assert_success(fgx(&["fs", "ls", &project_arg]));
    assert!(listing.contains("@data.sales"), "{listing}");
    assert!(listing.contains("@blob.tone"), "{listing}");

    let stat = assert_success(fgx(&["fs", "stat", &project_arg, "--tag", "@blob.tone"]));
    assert!(stat.contains("bytes=256"), "{stat}");
    assert!(stat.contains("by=human:viktor"), "{stat}");

    let out = root.join("out");
    assert_success(fgx(&[
        "fs",
        "extract",
        &project_arg,
        "--out",
        &out.display().to_string(),
    ]));
    assert_eq!(
        std::fs::read(out.join("tone.wav")).unwrap(),
        binary,
        "a binary payload must come back byte for byte"
    );
    assert_eq!(
        std::fs::read_to_string(out.join("sales.csv")).unwrap(),
        "region,units\nnorth,120\n"
    );

    // Adding the same name twice would shadow a section rather than replace it.
    let duplicate = assert_failure(fgx(&[
        "fs",
        "add",
        &project_arg,
        "--from",
        &csv.display().to_string(),
    ]));
    assert!(duplicate.contains("already in this file"), "{duplicate}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn fs_edits_are_guarded_stamped_and_visible_in_the_change_feed() {
    // Two agents on one file: the first reads a section's hash and edits
    // against it; the second edits against the same stale hash and must be
    // refused. Then the change feed, from a git revision, must name exactly
    // what moved and who moved it.
    let root = temp_file("fs_feed");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let project = root.join("proj.fgx");
    std::fs::write(
        &project,
        "fn total(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .unwrap();
    let csv = root.join("sales.csv");
    std::fs::write(&csv, "region,units\nnorth,120\n").unwrap();
    let p = project.display().to_string();
    let csv_arg = csv.display().to_string();
    assert_success(fgx(&[
        "fs",
        "add",
        &p,
        "--from",
        &csv_arg,
        "--by",
        "human:viktor",
        "--at",
        "2026-09-20T09:00:00Z",
    ]));
    assert_success(fgx(&[
        "fs",
        "note",
        &p,
        "--on",
        "@data.sales",
        "--text",
        "North looks doubled.",
        "--by",
        "agent:claude-opus-5",
        "--at",
        "2026-09-20T10:00:00Z",
    ]));

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
    };
    let have_git = git(&["init", "-q"]).is_ok_and(|out| out.status.success());
    if have_git {
        for args in [
            &["add", "proj.fgx"][..],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "base",
            ][..],
        ] {
            assert!(git(args).unwrap().status.success(), "git {args:?}");
        }
    }

    let listing = assert_success(fgx(&["fs", "ls", &p, "--json"]));
    let marker = "\"id\": \"@data.sales\", \"hash\": \"";
    let start = listing.find(marker).expect(&listing) + marker.len();
    let read_hash = listing[start..start + 16].to_string();

    let fixed = root.join("fixed.csv");
    std::fs::write(&fixed, "region,units\nnorth,60\n").unwrap();
    let fixed_arg = fixed.display().to_string();
    assert_success(fgx(&[
        "fs",
        "set",
        &p,
        "--tag",
        "@data.sales",
        "--from",
        &fixed_arg,
        "--expect",
        &read_hash,
        "--by",
        "agent:claude-opus-5",
        "--at",
        "2026-09-21T08:00:00Z",
    ]));
    let stale = assert_failure(fgx(&[
        "fs",
        "set",
        &p,
        "--tag",
        "@data.sales",
        "--from",
        &csv_arg,
        "--expect",
        &read_hash,
        "--by",
        "agent:other",
    ]));
    assert!(stale.contains("has changed since it was read"), "{stale}");
    let written = std::fs::read_to_string(&project).unwrap();
    assert!(
        written.contains("north,60"),
        "the refused edit must not land: {written}"
    );
    assert!(
        written.contains("modified \"2026-09-21T08:00:00Z\" by \"agent:claude-opus-5\""),
        "{written}"
    );

    assert_success(fgx(&[
        "fs",
        "resolve",
        &p,
        "--tag",
        "@note.n1",
        "--text",
        "Halved at source.",
        "--by",
        "human:viktor",
        "--at",
        "2026-09-21T08:05:00Z",
    ]));
    assert!(assert_success(fgx(&["check", &p])).contains("ok"));

    let since = assert_success(fgx(&[
        "fs",
        "changes",
        &p,
        "--since",
        "2026-09-21T00:00:00Z",
    ]));
    assert!(since.contains("changed\t@data.sales"), "{since}");
    assert!(since.contains("added\t@note.n2"), "{since}");
    assert!(!since.contains("@function.total"), "{since}");

    if have_git {
        let feed = assert_success(fgx(&["fs", "changes", &p, "--rev", "HEAD", "--json"]));
        assert!(feed.contains("forgentex.project_changes.v1"), "{feed}");
        for expected in [
            "\"id\": \"@data.sales\", \"kind\": \"data section\", \"change\": \"changed\"",
            "\"id\": \"@note.n1\", \"kind\": \"note\", \"change\": \"changed\"",
            "\"id\": \"@note.n2\", \"kind\": \"note\", \"change\": \"added\"",
        ] {
            assert!(feed.contains(expected), "missing {expected} in {feed}");
        }
        assert!(!feed.contains("@function.total"), "{feed}");
    }

    let stranded = assert_failure(fgx(&["fs", "rm", &p, "--tag", "@data.sales"]));
    assert!(stranded.contains("--with-notes"), "{stranded}");
    let removed = assert_success(fgx(&[
        "fs",
        "rm",
        &p,
        "--tag",
        "@data.sales",
        "--with-notes",
    ]));
    assert!(removed.contains("removed @note.n2"), "{removed}");
    assert!(assert_success(fgx(&["check", &p])).contains("ok"));

    let _ = std::fs::remove_dir_all(&root);
}
