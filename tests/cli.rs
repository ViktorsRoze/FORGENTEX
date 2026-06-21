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
    assert!(out.contains("\"diagnostics\": ["));

    let project = assert_success(fgx(&["check", "--project", "--json"]));
    assert!(project.contains("\"diagnostics\": ["));
    assert!(!project.contains("unresolved_import"));

    let err = assert_failure(fgx(&["check", "examples/missing.fgx", "--json"]));
    assert!(err.contains("failed to read"));
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
    assert!(json.contains("\"id\": \"moves.use_after_move\""));
    assert!(json.contains("\"id\": \"loops.loop_carried_alias_rejected\""));
    assert!(json.contains("\"id\": \"branches.conditional_alias_reassign_rejected\""));
    assert!(json.contains("\"id\": \"contracts.report_wrapper_mutates\""));
    assert!(json.contains("\"id\": \"raw.raw_pointer_param_safe_fn_rejected\""));
    assert!(json.contains("\"id\": \"raw.raw_pointer_deref_safe_fn_rejected\""));

    let text = assert_success(fgx(&["borrow-conformance"]));
    assert!(text.contains("Forgentex borrow conformance suite"));
    assert!(text.contains("status=pass"));
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
    let out = assert_success(fgx(&[
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
    let header = fs::read_to_string(out_dir.join("forgentex_demo_media.h")).unwrap();
    assert!(header.contains("void mix_samples(fgx_slice_i16 dst"));
    let _ = fs::remove_dir_all(out_dir);

    let err = assert_failure(fgx(&["build", ".", "--backend", "wat"]));
    assert!(err.contains("unknown backend"));

    let planned = assert_failure(fgx(&["build", ".", "--backend", "llvm"]));
    assert!(planned.contains("backend 'llvm' is planned for Stage 15"));
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
    let source = fs::read_to_string(out_dir.join("forgentex_demo_media.zig")).unwrap();
    assert!(source.contains("pub fn mix_samples(dst: []i16"));
    assert!(source.contains("fgx_bounds_index(i, dst.len)"));
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
    assert!(catalog.contains("forgentex.backend_ir.v1"));
    assert!(catalog.contains("c status=available"));
    assert!(catalog.contains("zig status=available"));
    assert!(catalog.contains("llvm status=planned"));

    let json = assert_success(fgx(&["backends", "--json"]));
    assert!(json.contains("\"schema\": \"forgentex.backends.v1\""));
    assert!(json.contains("\"name\": \"zig\""));
    assert!(json.contains("\"name\": \"mlir\""));

    let ir = assert_success(fgx(&["ir", "."]));
    assert!(ir.contains("FGIR schema=forgentex.backend_ir.v1"));
    assert!(ir.contains("F mix_samples @function.mix_samples visibility=public"));
    assert!(ir.contains("assign dst[i] core_math_clamp_i16(x)"));

    let ir_json = assert_success(fgx(&["ir", ".", "--json"]));
    assert!(ir_json.contains("\"schema\": \"forgentex.backend_ir.v1\""));
    assert!(ir_json.contains("\"op\": \"assign\""));

    let contract = assert_success(fgx(&["ir", ".", "--contract-json"]));
    assert!(contract.contains("\"schema\": \"forgentex.frontend_contract.v1\""));
    assert!(contract.contains("\"type_diagnostics\": ["));
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

    let err = assert_failure(fgx(&["run", "examples/media/popcount_x86.fgx"]));
    assert!(err.contains("no runnable demo entrypoint"));
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
}

#[test]
fn help_and_unknown_command_are_covered() {
    let out = assert_success(fgx(&["help"]));
    assert!(out.contains("Forgentex demo CLI"));
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
    assert!(out.contains("fgx ir <project>"));
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
