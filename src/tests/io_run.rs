use std::fs;
use std::path::PathBuf;

use crate::io_run::run;

use super::parse_fixture;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_io_{tag}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn grants(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}

#[test]
fn copy_with_grants_performs_real_copy_and_records_allow() {
    let dir = scratch("copy_ok");
    let src = dir.join("src.txt");
    let dst = dir.join("dst.txt");
    fs::write(&src, "hello forgentex io\n").unwrap();

    let program = parse_fixture("examples/io/real_copy.fgx");
    let report = run(
        "real_copy.fgx",
        &program,
        "copy_file",
        vec![src.display().to_string(), dst.display().to_string()],
        grants(&["fs.read", "fs.write"]),
    );

    assert_eq!(report.status, "ok");
    assert!(dst.exists(), "destination should have been written");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "hello forgentex io\n");
    assert_eq!(report.ops.len(), 1);
    assert_eq!(report.ops[0].op, "copy");
    assert_eq!(report.ops[0].decision, "allow");
    assert_eq!(report.bytes_total, 19);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn copy_without_grants_is_denied_and_writes_nothing() {
    let dir = scratch("copy_deny");
    let src = dir.join("src.txt");
    let dst = dir.join("dst.txt");
    fs::write(&src, "secret\n").unwrap();

    let program = parse_fixture("examples/io/real_copy.fgx");
    let report = run(
        "real_copy.fgx",
        &program,
        "copy_file",
        vec![src.display().to_string(), dst.display().to_string()],
        Vec::new(),
    );

    assert_eq!(report.status, "denied");
    assert!(!dst.exists(), "denied copy must not create the destination");
    assert_eq!(report.ops[0].decision, "deny");
    assert_eq!(report.bytes_total, 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_is_gated_by_fs_read() {
    let dir = scratch("read");
    let path = dir.join("data.txt");
    fs::write(&path, "payload").unwrap();
    let program = parse_fixture("examples/io/real_copy.fgx");

    let granted = run(
        "real_copy.fgx",
        &program,
        "read_file",
        vec![path.display().to_string()],
        grants(&["fs.read"]),
    );
    assert_eq!(granted.status, "ok");
    assert_eq!(granted.return_value.as_deref(), Some("payload"));

    let denied = run(
        "real_copy.fgx",
        &program,
        "read_file",
        vec![path.display().to_string()],
        Vec::new(),
    );
    assert_eq!(denied.status, "denied");
    assert_eq!(denied.ops[0].decision, "deny");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn report_json_is_well_formed() {
    let dir = scratch("json");
    let path = dir.join("data.txt");
    fs::write(&path, "x").unwrap();
    let program = parse_fixture("examples/io/real_copy.fgx");
    let json = run(
        "real_copy.fgx",
        &program,
        "read_file",
        vec![path.display().to_string()],
        grants(&["fs.read"]),
    )
    .to_json();
    assert!(json.contains("\"schema\": \"forgentex.io_run.v1\""));
    assert!(json.contains("\"transcript_hash\""));
    assert!(json.contains("\"decision\""));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_io_outside_io_run_is_unavailable() {
    // The plain interpreter has no I/O host, so core_io_* must not run.
    use crate::ast::Expr;
    let program = parse_fixture("examples/io/real_copy.fgx");
    let call = Expr::Call {
        name: "core_io_read_file".to_string(),
        args: vec![Expr::String("x".to_string())],
    };
    let err = crate::interp::eval_expr_for_test(&program, &call)
        .err()
        .unwrap_or_default();
    assert!(
        err.contains("io"),
        "expected io unavailable error, got: {err}"
    );
}
