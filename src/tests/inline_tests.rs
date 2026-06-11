use std::fs;
use std::path::{Path, PathBuf};

use crate::format::{format_agent, format_human};
use crate::inline_tests::{run_inline_tests, InlineTestOptions};
use crate::parser::parse_file;

use super::{fixture, manifest_path};

#[test]
fn inline_tests_parse_format_and_roundtrip() {
    let program = parse_file(
        Path::new("examples/smoke/inline_tests.fgx"),
        fixture("examples/smoke/inline_tests.fgx"),
    )
    .unwrap();
    assert_eq!(program.tests().count(), 3);
    let human = format_human(&program);
    assert!(human.contains("test \"clamp high\":"));
    assert!(human.contains("xfail test \"safe assembly rejected\":"));
    let agent = format_agent(&program);
    assert!(agent.contains("TEST \"clamp high\" {"));
    assert!(agent.contains("XTEST \"safe assembly rejected\" {"));

    let reparsed_agent = parse_file(Path::new("inline_tests.fgai"), &agent).unwrap();
    assert!(reparsed_agent.canonical_eq(&program));
}

#[test]
fn inline_test_runner_reports_pass_and_xfail() {
    let report = run_inline_tests(&InlineTestOptions {
        input: manifest_path("examples/smoke/inline_tests.fgx"),
        target: Some("x86_64-pc-windows-msvc".to_string()),
        filter: None,
    })
    .unwrap();
    assert_eq!(report.schema, "forgentex.test_report.v1");
    assert_eq!(report.status, "pass");
    assert_eq!(report.passed, 2);
    assert_eq!(report.xfailed, 1);
    let json = report.to_json();
    assert!(json.contains("\"status\": \"xfail\""));
    assert!(json.contains("\"expected\": \"ASM001\""));
}

#[test]
fn inline_test_runner_can_filter_and_report_failures() {
    let path = temp_path("stage30_inline_fail.fgx");
    fs::write(
        &path,
        "fn value() -> i32:\n    return 1\n\ntest \"bad math\":\n    expect value() == 2\n\ntest \"good math\":\n    expect value() == 1\n",
    )
    .unwrap();
    let report = run_inline_tests(&InlineTestOptions {
        input: path.clone(),
        target: None,
        filter: Some("bad".to_string()),
    })
    .unwrap();
    assert_eq!(report.status, "fail");
    assert_eq!(report.tests.len(), 1);
    assert_eq!(report.failed, 1);
    assert!(report.to_text().contains("bad math"));
    let _ = fs::remove_file(path);
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_{}", std::process::id(), name))
}
