use std::fs;
use std::path::{Path, PathBuf};

use crate::application::{run, ApplicationRunReport};
use crate::project::{check_project, load_project};

use super::manifest_path;

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_application_{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("input")).unwrap();
    fs::create_dir_all(root.join("output")).unwrap();
    root
}

fn result_text(report: &ApplicationRunReport) -> Option<&str> {
    report
        .result
        .as_ref()
        .map(|result| result.value_text.as_str())
}

fn error_message(report: &ApplicationRunReport) -> &str {
    report
        .error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or_default()
}

fn write_application(root: &Path, max_read_bytes: usize, effects: &str) {
    fs::write(
        root.join("Forgentex.toml"),
        format!(
            "[package]\nname = \"application-test\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"copy_text\"\nruntime = \"hosted_io\"\neffects = {effects}\n\n[application.limits]\nmax_read_bytes = {max_read_bytes}\nmax_write_bytes = 1024\n\n[permissions]\nfs.read = [\"input\"]\nfs.write = [\"output\"]\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn copy_text(source: str, destination: str) -> usize effects [fs.read, fs.write]:\n    return core_io_copy_file(source, destination)\n",
    )
    .unwrap();
}

fn write_binary_application(root: &Path, max_read_bytes: usize, max_write_bytes: usize) {
    fs::write(
        root.join("Forgentex.toml"),
        format!(
            "[package]\nname = \"binary-application-test\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"copy_binary\"\nruntime = \"hosted_io\"\neffects = [\"fs.read\", \"fs.write\"]\n\n[application.limits]\nmax_read_bytes = {max_read_bytes}\nmax_write_bytes = {max_write_bytes}\n\n[permissions]\nfs.read = [\"input\"]\nfs.write = [\"output\"]\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn copy_binary(source: str, destination: str) -> usize effects [fs.read, fs.write]:\n    let bytes: Vec[u8] = core_io_read_bytes(source)\n    return core_io_write_bytes_atomic(destination, bytes)\n",
    )
    .unwrap();
}

fn write_artifact_application(
    root: &Path,
    artifact_name_in_source: &str,
    artifact_root: &str,
    max_bytes: usize,
    required: bool,
) {
    fs::create_dir_all(root.join(artifact_root)).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        format!(
            "[package]\nname = \"artifact-application-test\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"publish\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"{artifact_root}\"\nmedia_type = \"application/vnd.forgentex.bundle\"\nmax_bytes = {max_bytes}\nrequired = {required}\npublication = \"create_new\"\n\n[permissions]\nfs.write = [\"output\"]\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        format!(
            "fn publish(destination: str, contents: str) -> usize effects [fs.write]:\n    let path: Path = core_path_parse(destination)\n    return core_io_publish_text_artifact(\"{artifact_name_in_source}\", path, contents)\n"
        ),
    )
    .unwrap();
}

fn write_directory_application(
    root: &Path,
    max_read_bytes: usize,
    max_directory_entries: Option<usize>,
) {
    let directory_limit = max_directory_entries
        .map(|limit| format!("max_directory_entries = {limit}\n"))
        .unwrap_or_default();
    fs::write(
        root.join("Forgentex.toml"),
        format!(
            "[package]\nname = \"directory-application-test\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"first_entry\"\nruntime = \"hosted_io\"\neffects = [\"fs.read\"]\n\n[application.limits]\nmax_read_bytes = {max_read_bytes}\n{directory_limit}\n[permissions]\nfs.read = [\"input\"]\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "import core.io\n\nfn first_entry(directory: str) -> str effects [fs.read]:\n    let path: Path = core_path_parse(directory)\n    let entries: Vec[DirEntry] = core_io_list_dir(path)\n    return core_path_to_str(entries[0].path)\n",
    )
    .unwrap();
}

fn write_pure_application(root: &Path, param_type: &str) {
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"typed-arguments\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"echo\"\neffects = []\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        format!("fn echo(value: {param_type}) -> {param_type}:\n    return value\n"),
    )
    .unwrap();
}

#[test]
fn application_manifest_requires_a_forgentex_compatibility_line() {
    let root = scratch("missing_forgentex_version");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"missing-version\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nentry = \"main\"\neffects = []\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main() -> i32:\n    return 0\n",
    )
    .unwrap();

    let error = load_project(&root).unwrap_err();
    assert!(
        error.contains("requires forgentex = \"MAJOR.MINOR\""),
        "{error}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_manifest_rejects_ambiguous_version_requirements() {
    for (index, requirement) in ["0", "0.1.0", ">=0.1", "00.1", "0.*"]
        .into_iter()
        .enumerate()
    {
        let root = scratch(&format!("bad_forgentex_version_{index}"));
        fs::write(
            root.join("Forgentex.toml"),
            format!(
                "[package]\nname = \"bad-version\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"{requirement}\"\nentry = \"main\"\neffects = []\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.fgx"),
            "fn main() -> i32:\n    return 0\n",
        )
        .unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(error.contains("compatibility"), "{requirement}: {error}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn incompatible_forgentex_line_is_rejected_before_dispatch() {
    let root = scratch("incompatible_forgentex_version");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"future-application\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"99.0\"\nentry = \"main\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main() -> usize effects [fs.write]:\n    return core_io_write_file(\"output/ran.txt\", \"ran\")\n",
    )
    .unwrap();

    let error = run(&root, Vec::new()).unwrap_err();
    assert!(
        error.contains("application_version_incompatible"),
        "{error}"
    );
    assert!(error.contains("requires Forgentex 99.0"), "{error}");
    assert_eq!(error.code, "application_version_incompatible");
    assert_eq!(error.phase, "compatibility");
    assert!(error
        .to_json()
        .contains("\"schema\":\"forgentex.application_error.v1\""));
    assert!(!root.join("output/ran.txt").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_arguments_decode_from_the_entry_signature() {
    let cases = [
        ("bool", "true", "true"),
        ("str", "raw value", "raw value"),
        ("Path", "input/data.txt", "input/data.txt"),
        ("u8", "255", "255"),
        ("u16", "65535", "65535"),
        ("u32", "4294967295", "4294967295"),
        ("u64", "9223372036854775807", "9223372036854775807"),
        ("usize", "42", "42"),
        ("i8", "-128", "-128"),
        ("i16", "-32768", "-32768"),
        ("i32", "-2147483648", "-2147483648"),
        ("i64", "-9223372036854775808", "-9223372036854775808"),
        ("isize", "-7", "-7"),
        ("f32", "1.25", "1.25"),
        ("f64", "-2.5e2", "-250"),
    ];
    for (index, (param_type, input, expected)) in cases.into_iter().enumerate() {
        let root = scratch(&format!("typed_argument_{index}"));
        write_pure_application(&root, param_type);
        let report = run(&root, vec![input.to_string()]).unwrap();
        assert_eq!(report.status, "ok", "{param_type}: {report:?}");
        assert_eq!(result_text(&report), Some(expected), "{param_type}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn application_arguments_reject_bad_syntax_ranges_and_paths() {
    let cases = [
        ("bool", "TRUE", "application_argument_invalid"),
        ("u8", "-1", "application_argument_invalid"),
        ("i8", "128", "application_argument_out_of_range"),
        (
            "u64",
            "9223372036854775808",
            "application_argument_out_of_range",
        ),
        ("i32", " 1", "application_argument_invalid"),
        ("f32", "3.5e38", "application_argument_out_of_range"),
        ("f64", "NaN", "application_argument_invalid"),
        ("Path", "../escape", "application_argument_invalid"),
    ];
    for (index, (param_type, input, code)) in cases.into_iter().enumerate() {
        let root = scratch(&format!("bad_typed_argument_{index}"));
        write_pure_application(&root, param_type);
        let error = run(&root, vec![input.to_string()]).unwrap_err();
        assert!(error.contains(code), "{param_type} {input}: {error}");
        assert!(error.contains("argument 1 'value'"), "{error}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn application_arguments_reject_unsupported_aggregate_types_before_execution() {
    let root = scratch("unsupported_typed_argument");
    write_pure_application(&root, "Vec[u8]");
    let error = run(&root, vec!["1,2,3".to_string()]).unwrap_err();
    assert!(
        error.contains("application_argument_type_unsupported"),
        "{error}"
    );
    assert!(error.contains("Vec[u8]"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_backed_application_copies_only_within_scopes_and_reports_budgets() {
    let root = scratch("copy");
    write_application(&root, 1024, "[\"fs.read\", \"fs.write\"]");
    fs::write(root.join("input").join("source.txt"), "scoped payload").unwrap();

    let report = run(
        &root,
        vec![
            "input/source.txt".to_string(),
            "output/copied.txt".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.schema, crate::application::APPLICATION_RUN_SCHEMA);
    assert_eq!(report.forgentex_required, "0.1");
    assert_eq!(report.forgentex_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(report.result.as_ref().unwrap().type_name, "usize");
    assert_eq!(result_text(&report), Some("14"));
    assert!(report.error.is_none());
    assert!(report
        .to_json()
        .contains("\"result\":{\"type\":\"usize\",\"value\":14}"));
    assert_eq!(report.read_bytes, 14);
    assert_eq!(report.write_bytes, 14);
    assert_eq!(report.ops.len(), 1);
    assert_eq!(report.ops[0].decision, "allow");
    assert_eq!(
        fs::read_to_string(root.join("output").join("copied.txt")).unwrap(),
        "scoped payload"
    );
    let json = report.to_json();
    assert!(json.contains("\"max_read_bytes\":1024"));
    assert!(json.contains("\"manifest_sha256\""));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_denies_paths_outside_manifest_permission_roots() {
    let root = scratch("scope_deny");
    write_application(&root, 1024, "[\"fs.read\", \"fs.write\"]");
    fs::write(root.join("secret.txt"), "not in input").unwrap();

    let report = run(
        &root,
        vec!["secret.txt".to_string(), "output/copied.txt".to_string()],
    )
    .unwrap();

    assert_eq!(report.status, "denied");
    assert_eq!(report.ops[0].decision, "deny");
    assert!(report.result.is_none());
    assert_eq!(report.error.as_ref().unwrap().code, "io_denied");
    assert_eq!(report.error.as_ref().unwrap().phase, "io");
    assert!(!root.join("output").join("copied.txt").exists());
    assert!(error_message(&report).contains("outside the application permission roots"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_rejects_parent_traversal_before_filesystem_access() {
    let root = scratch("traversal");
    write_application(&root, 1024, "[\"fs.read\", \"fs.write\"]");
    fs::write(root.join("secret.txt"), "secret").unwrap();

    let report = run(
        &root,
        vec![
            "input/../secret.txt".to_string(),
            "output/copied.txt".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "denied");
    assert_eq!(report.ops[0].decision, "deny");
    assert!(error_message(&report).contains("parent traversal"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_requires_runtime_paths_to_be_package_relative() {
    let root = scratch("absolute_path");
    write_application(&root, 1024, "[\"fs.read\", \"fs.write\"]");
    let source = root.join("input").join("source.txt");
    fs::write(&source, "payload").unwrap();

    let report = run(
        &root,
        vec![
            source.display().to_string(),
            "output/copied.txt".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "denied");
    assert_eq!(report.ops[0].decision, "deny");
    assert!(!root.join("output").join("copied.txt").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_exhausts_read_budget_before_copying_or_writing() {
    let root = scratch("budget");
    write_application(&root, 4, "[\"fs.read\", \"fs.write\"]");
    fs::write(root.join("input").join("source.txt"), "too large").unwrap();

    let report = run(
        &root,
        vec![
            "input/source.txt".to_string(),
            "output/copied.txt".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "budget_exhausted");
    assert_eq!(report.ops[0].decision, "budget_exhausted");
    assert!(!root.join("output").join("copied.txt").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_round_trips_non_utf8_bytes_through_atomic_publication() {
    let root = scratch("binary_atomic");
    write_binary_application(&root, 1024, 1024);
    let payload = [0, 255, 128, 10, 13, 1, 2, 3];
    fs::write(root.join("input").join("source.bin"), payload).unwrap();

    let report = run(
        &root,
        vec![
            "input/source.bin".to_string(),
            "output/copied.bin".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(report.read_bytes, payload.len());
    assert_eq!(report.write_bytes, payload.len());
    assert_eq!(report.ops.len(), 2);
    assert_eq!(report.ops[0].op, "read_bytes");
    assert_eq!(report.ops[1].op, "publish_bytes");
    assert_eq!(
        fs::read(root.join("output").join("copied.bin")).unwrap(),
        payload
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn declared_artifact_publication_is_hashed_bounded_and_reported() {
    let root = scratch("artifact_receipt");
    fs::create_dir_all(root.join("output").join("releases")).unwrap();
    write_artifact_application(&root, "bundle", "output/releases", 64, true);

    let report = run(
        &root,
        vec![
            "output/releases/demo.fgp".to_string(),
            "canonical payload".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "ok", "{report:?}");
    assert_eq!(report.schema, crate::application::APPLICATION_RUN_SCHEMA);
    assert_eq!(report.artifact_declarations.len(), 1);
    assert_eq!(report.artifacts.len(), 1);
    let artifact = &report.artifacts[0];
    assert_eq!(artifact.name, "bundle");
    assert_eq!(artifact.path, "output/releases/demo.fgp");
    assert_eq!(artifact.media_type, "application/vnd.forgentex.bundle");
    assert_eq!(artifact.bytes, 17);
    assert_eq!(
        artifact.sha256,
        crate::crypto::sha256_hex(b"canonical payload")
    );
    assert_eq!(artifact.publication, "create_new");
    assert_eq!(report.write_bytes, 17);
    assert_eq!(report.ops[0].op, "publish_artifact_text");
    let json = report.to_json();
    assert!(json.contains("\"artifact_declarations\":[{"));
    assert!(json.contains("\"artifacts\":[{"));
    assert!(json.contains(&format!("\"sha256\":\"{}\"", artifact.sha256)));
    assert_eq!(
        fs::read_to_string(root.join("output/releases/demo.fgp")).unwrap(),
        "canonical payload"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn declared_binary_artifact_preserves_non_utf8_bytes() {
    let root = scratch("artifact_binary");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"binary-artifact\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.read\", \"fs.write\"]\n\n[application.limits]\nmax_read_bytes = 1024\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n\n[permissions]\nfs.read = [\"input\"]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main(source: str, destination: str) -> usize effects [fs.read, fs.write]:\n    let bytes: Vec[u8] = core_io_read_bytes(source)\n    let path: Path = core_path_parse(destination)\n    return core_io_publish_bytes_artifact(\"bundle\", path, bytes)\n",
    )
    .unwrap();
    let payload = [0, 255, 128, 10, 13, 1, 2, 3];
    fs::write(root.join("input/source.bin"), payload).unwrap();

    let report = run(
        &root,
        vec![
            "input/source.bin".to_string(),
            "output/bundle.bin".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(report.status, "ok", "{report:?}");
    assert_eq!(report.artifacts[0].bytes, payload.len());
    assert_eq!(
        report.artifacts[0].sha256,
        crate::crypto::sha256_hex(&payload)
    );
    assert_eq!(fs::read(root.join("output/bundle.bin")).unwrap(), payload);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn artifact_publication_rejects_undeclared_names_and_narrow_root_escape() {
    let root = scratch("artifact_undeclared");
    fs::create_dir_all(root.join("output").join("releases")).unwrap();
    write_artifact_application(&root, "other", "output/releases", 64, true);
    let report = run(
        &root,
        vec![
            "output/releases/demo.fgp".to_string(),
            "payload".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(report.status, "denied");
    assert!(error_message(&report).contains("artifact_undeclared"));
    assert!(report.artifacts.is_empty());
    assert!(!root.join("output/releases/demo.fgp").exists());

    write_artifact_application(&root, "bundle", "output/releases", 64, true);
    let report = run(
        &root,
        vec!["output/escape.fgp".to_string(), "payload".to_string()],
    )
    .unwrap();
    assert_eq!(report.status, "denied");
    assert!(error_message(&report).contains("artifact_root_denied"));
    assert!(report.artifacts.is_empty());
    assert!(!root.join("output/escape.fgp").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn artifact_limit_is_checked_before_publication_and_run_budget_commit() {
    let root = scratch("artifact_limit");
    fs::create_dir_all(root.join("output").join("releases")).unwrap();
    write_artifact_application(&root, "bundle", "output/releases", 3, true);

    let report = run(
        &root,
        vec!["output/releases/demo.fgp".to_string(), "four".to_string()],
    )
    .unwrap();

    assert_eq!(report.status, "budget_exhausted");
    assert!(report.result.is_none());
    assert_eq!(
        report.error.as_ref().unwrap().code,
        "artifact_budget_exhausted"
    );
    assert_eq!(report.error.as_ref().unwrap().phase, "io");
    assert_eq!(report.write_bytes, 0);
    assert!(report.artifacts.is_empty());
    assert!(error_message(&report).contains("artifact_budget_exhausted"));
    assert!(!root.join("output/releases/demo.fgp").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn required_artifact_must_be_published_but_optional_artifact_may_be_absent() {
    for (required, expected_status) in [(true, "error"), (false, "ok")] {
        let root = scratch(&format!("artifact_required_{required}"));
        fs::write(
            root.join("Forgentex.toml"),
            format!(
                "[package]\nname = \"missing-artifact\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\nrequired = {required}\n\n[permissions]\nfs.write = [\"output\"]\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.fgx"),
            "fn main() -> usize effects [fs.write]:\n    return 0\n",
        )
        .unwrap();

        let report = run(&root, Vec::new()).unwrap();
        assert_eq!(report.status, expected_status, "{report:?}");
        assert!(report.artifacts.is_empty());
        if required {
            assert!(report.result.is_none());
            assert!(error_message(&report).contains("artifact_required_missing"));
        } else {
            assert_eq!(result_text(&report), Some("0"));
        }
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn ordinary_atomic_writes_never_become_artifact_receipts() {
    let root = scratch("ordinary_not_artifact");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"ordinary-write\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\nrequired = false\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main(path: str) -> usize effects [fs.write]:\n    return core_io_write_file_atomic(path, \"ordinary\")\n",
    )
    .unwrap();

    let report = run(&root, vec!["output/ordinary.txt".to_string()]).unwrap();
    assert_eq!(report.status, "ok");
    assert_eq!(report.ops[0].op, "publish_text");
    assert!(report.artifacts.is_empty());
    assert!(report.to_json().contains("\"artifacts\":[]"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn one_artifact_name_cannot_be_published_twice() {
    let root = scratch("artifact_twice");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"artifact-twice\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main() -> usize effects [fs.write]:\n    let first: Path = core_path_parse(\"output/first.bin\")\n    let second: Path = core_path_parse(\"output/second.bin\")\n    core_io_publish_text_artifact(\"bundle\", first, \"first\")\n    return core_io_publish_text_artifact(\"bundle\", second, \"second\")\n",
    )
    .unwrap();

    let report = run(&root, Vec::new()).unwrap();
    assert_eq!(report.status, "denied");
    assert_eq!(report.artifacts.len(), 1);
    assert_eq!(report.artifacts[0].path, "output/first.bin");
    assert!(error_message(&report).contains("artifact_already_published"));
    assert!(root.join("output/first.bin").exists());
    assert!(!root.join("output/second.bin").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn artifact_manifest_rejects_incomplete_or_unsafe_declarations() {
    let cases = [
        (
            "invalid_name",
            "[application.artifact.1bad]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n",
            "artifact name",
        ),
        (
            "missing_root",
            "[application.artifact.bundle]\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n",
            "requires a root",
        ),
        (
            "invalid_root",
            "[application.artifact.bundle]\nroot = \"output/../escape\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n",
            "invalid root",
        ),
        (
            "bad_media",
            "[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"Application Octets\"\nmax_bytes = 64\n",
            "media_type",
        ),
        (
            "bad_publication",
            "[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\npublication = \"replace\"\n",
            "expected create_new",
        ),
        (
            "over_run_limit",
            "[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 2048\n",
            "exceeds application.limits.max_write_bytes",
        ),
        (
            "duplicate_field",
            "[application.artifact.bundle]\nroot = \"output\"\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n",
            "more than once",
        ),
    ];
    for (tag, artifact, expected) in cases {
        let root = scratch(&format!("artifact_manifest_{tag}"));
        fs::write(
            root.join("Forgentex.toml"),
            format!(
                "[package]\nname = \"bad-artifact\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n{artifact}\n[permissions]\nfs.write = [\"output\"]\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.fgx"),
            "fn main() -> usize effects [fs.write]:\n    return 0\n",
        )
        .unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(error.contains(expected), "tag={tag}: {error}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn artifact_manifest_requires_the_write_effect_and_budget() {
    for (tag, application, expected) in [
        (
            "effect",
            "[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = []\n",
            "require the fs.write effect",
        ),
        (
            "budget",
            "[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n",
            "max_write_bytes",
        ),
    ] {
        let root = scratch(&format!("artifact_requires_{tag}"));
        fs::write(
            root.join("Forgentex.toml"),
            format!(
                "[package]\nname = \"artifact-requirement\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n{application}\n[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join("src/main.fgx"),
            "fn main() -> usize:\n    return 0\n",
        )
        .unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(error.contains(expected), "tag={tag}: {error}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn artifact_root_must_resolve_beneath_a_write_permission() {
    let root = scratch("artifact_permission_root");
    fs::create_dir(root.join("other")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"artifact-root\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"other\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main() -> usize effects [fs.write]:\n    return 0\n",
    )
    .unwrap();

    let error = run(&root, Vec::new()).unwrap_err();
    assert!(
        error.contains("outside the fs.write permission roots"),
        "{error}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn artifact_publication_requires_a_typed_portable_path() {
    let root = scratch("artifact_static_path");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"artifact-static\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn bad(path: str) -> usize effects [fs.write]:\n    return core_io_publish_text_artifact(\"bundle\", path, \"data\")\n",
    )
    .unwrap();

    let diagnostics = check_project(&root).unwrap();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "argument_type_mismatch"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn atomic_binary_publication_refuses_to_replace_an_existing_destination() {
    let root = scratch("atomic_no_clobber");
    write_binary_application(&root, 1024, 1024);
    fs::write(root.join("input").join("source.bin"), [1, 2, 3, 4]).unwrap();
    fs::write(root.join("output").join("copied.bin"), [9, 8, 7]).unwrap();

    let report = run(
        &root,
        vec![
            "input/source.bin".to_string(),
            "output/copied.bin".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "error");
    assert_eq!(report.ops[1].op, "publish_bytes");
    assert_eq!(report.ops[1].decision, "error");
    assert!(error_message(&report).contains("io_target_exists"));
    assert_eq!(
        fs::read(root.join("output").join("copied.bin")).unwrap(),
        [9, 8, 7]
    );
    assert!(fs::read_dir(root.join("output"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".fgx-publish-")));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn atomic_binary_publication_exhausts_write_budget_without_creating_a_target() {
    let root = scratch("binary_write_budget");
    write_binary_application(&root, 1024, 3);
    fs::write(root.join("input").join("source.bin"), [1, 2, 3, 4]).unwrap();

    let report = run(
        &root,
        vec![
            "input/source.bin".to_string(),
            "output/copied.bin".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(report.status, "budget_exhausted");
    assert_eq!(report.read_bytes, 4);
    assert_eq!(report.write_bytes, 0);
    assert_eq!(report.ops[1].decision, "budget_exhausted");
    assert!(!root.join("output").join("copied.bin").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn atomic_text_and_copy_intrinsics_publish_complete_new_files() {
    let root = scratch("atomic_text_copy");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"atomic-ops\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"publish\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn publish(path: str, contents: str) -> usize effects [fs.write]:\n    return core_io_write_file_atomic(path, contents)\n",
    )
    .unwrap();

    let report = run(
        &root,
        vec![
            "output/published.txt".to_string(),
            "complete text".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(report.status, "ok");
    assert_eq!(report.ops[0].op, "publish_text");
    assert_eq!(
        fs::read_to_string(root.join("output").join("published.txt")).unwrap(),
        "complete text"
    );

    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"atomic-ops\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"copy\"\nruntime = \"hosted_io\"\neffects = [\"fs.read\", \"fs.write\"]\n\n[application.limits]\nmax_read_bytes = 1024\nmax_write_bytes = 1024\n\n[permissions]\nfs.read = [\"input\"]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn copy(source: str, destination: str) -> usize effects [fs.read, fs.write]:\n    return core_io_copy_file_atomic(source, destination)\n",
    )
    .unwrap();
    fs::write(root.join("input").join("copy.bin"), [0, 254, 1]).unwrap();
    let report = run(
        &root,
        vec!["input/copy.bin".to_string(), "output/copy.bin".to_string()],
    )
    .unwrap();
    assert_eq!(report.status, "ok");
    assert_eq!(report.ops[0].op, "copy_atomic");
    assert_eq!(report.read_bytes, 3);
    assert_eq!(report.write_bytes, 3);
    assert_eq!(
        fs::read(root.join("output").join("copy.bin")).unwrap(),
        [0, 254, 1]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn binary_publication_builtin_is_statically_typed_as_vec_u8() {
    let root = scratch("binary_static_type");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"bad-bytes\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn bad(path: str) -> usize effects [fs.write]:\n    return core_io_write_bytes_atomic(path, \"not bytes\")\n",
    )
    .unwrap();

    let diagnostics = check_project(&root).unwrap();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "argument_type_mismatch"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn typed_paths_drive_sorted_bounded_directory_enumeration() {
    let root = scratch("directory_sorted");
    write_directory_application(&root, 1024, Some(3));
    fs::write(root.join("input").join("z-last.bin"), [1]).unwrap();
    fs::create_dir(root.join("input").join("middle")).unwrap();
    fs::write(root.join("input").join("a-first.bin"), [2]).unwrap();

    let report = run(&root, vec!["input".to_string()]).unwrap();

    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(result_text(&report), Some("input/a-first.bin"));
    assert_eq!(report.directory_entries, 3);
    assert_eq!(report.max_directory_entries, Some(3));
    assert_eq!(report.ops.len(), 1);
    assert_eq!(report.ops[0].op, "list_dir");
    assert_eq!(report.ops[0].decision, "allow");
    assert_eq!(report.read_bytes, report.ops[0].bytes);
    assert!(report.to_json().contains("\"directory_entries\":3"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_entry_budget_fails_atomically_without_returning_a_partial_list() {
    let root = scratch("directory_entry_budget");
    write_directory_application(&root, 1024, Some(2));
    for name in ["a.bin", "b.bin", "c.bin"] {
        fs::write(root.join("input").join(name), [1]).unwrap();
    }

    let report = run(&root, vec!["input".to_string()]).unwrap();

    assert_eq!(report.status, "budget_exhausted");
    assert_eq!(report.directory_entries, 0);
    assert_eq!(report.read_bytes, 0);
    assert_eq!(report.ops[0].decision, "budget_exhausted");
    assert!(report.result.is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_entry_budget_is_cumulative_across_snapshots() {
    let root = scratch("directory_entry_cumulative");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"directory-cumulative\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"list_twice\"\nruntime = \"hosted_io\"\neffects = [\"fs.read\"]\n\n[application.limits]\nmax_read_bytes = 1024\nmax_directory_entries = 3\n\n[permissions]\nfs.read = [\"input\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "import core.io\n\nfn list_twice(directory: str) -> usize effects [fs.read]:\n    let first_path: Path = core_path_parse(directory)\n    let second_path: Path = core_path_parse(directory)\n    let first: Vec[DirEntry] = core_io_list_dir(first_path)\n    let second: Vec[DirEntry] = core_io_list_dir(second_path)\n    return first.len + second.len\n",
    )
    .unwrap();
    fs::write(root.join("input").join("a.bin"), [1]).unwrap();
    fs::write(root.join("input").join("b.bin"), [2]).unwrap();

    let report = run(&root, vec!["input".to_string()]).unwrap();

    assert_eq!(report.status, "budget_exhausted");
    assert_eq!(report.directory_entries, 2);
    assert_eq!(report.ops.len(), 2);
    assert_eq!(report.ops[0].decision, "allow");
    assert_eq!(report.ops[1].decision, "budget_exhausted");
    assert_eq!(report.read_bytes, report.ops[0].bytes);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_path_bytes_are_charged_to_the_read_budget() {
    let root = scratch("directory_path_budget");
    write_directory_application(&root, 5, Some(10));
    fs::write(root.join("input").join("long-name.bin"), [1]).unwrap();

    let report = run(&root, vec!["input".to_string()]).unwrap();

    assert_eq!(report.status, "budget_exhausted");
    assert_eq!(report.directory_entries, 0);
    assert_eq!(report.read_bytes, 0);
    assert_eq!(report.ops[0].decision, "budget_exhausted");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_enumeration_requires_an_explicit_manifest_entry_limit() {
    let root = scratch("directory_limit_missing");
    write_directory_application(&root, 1024, None);
    fs::write(root.join("input").join("a.bin"), [1]).unwrap();

    let report = run(&root, vec!["input".to_string()]).unwrap();

    assert_eq!(report.status, "denied");
    assert_eq!(report.directory_entries, 0);
    assert_eq!(report.ops[0].decision, "deny");
    assert!(error_message(&report).contains("max_directory_entries"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn portable_paths_reject_traversal_backslashes_and_reserved_names_before_io() {
    for (tag, path) in [
        ("parent", "../input"),
        ("backslash", "input\\child"),
        ("absolute", "/input"),
        ("reserved", "input/CON"),
    ] {
        let root = scratch(&format!("path_{tag}"));
        write_directory_application(&root, 1024, Some(10));

        let report = run(&root, vec![path.to_string()]).unwrap();

        assert_eq!(report.status, "error", "path={path} report={report:?}");
        assert!(report.ops.is_empty());
        assert!(report.result.is_none());
        assert_eq!(report.error.as_ref().unwrap().code, "path_invalid");
        assert_eq!(report.error.as_ref().unwrap().phase, "runtime");
        assert!(error_message(&report).contains("path_invalid"));
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn application_rejects_an_unstable_aggregate_result_before_dispatch() {
    let root = scratch("unsupported_result");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"unsupported-result\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"read\"\neffects = [\"fs.read\"]\n\n[application.limits]\nmax_read_bytes = 1024\n\n[permissions]\nfs.read = [\"input\"]\n",
    )
    .unwrap();
    fs::write(root.join("input/data.bin"), [1, 2, 3]).unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "fn read(path: str) -> Vec[u8] effects [fs.read]:\n    return core_io_read_bytes(path)\n",
    )
    .unwrap();

    let error = run(&root, vec!["input/data.bin".to_string()]).unwrap_err();
    assert_eq!(error.code, "application_result_type_unsupported");
    assert_eq!(error.phase, "contract");
    assert!(error.message.contains("Vec[u8]"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_intrinsic_requires_a_typed_path_and_limit_requires_read_effect() {
    let root = scratch("directory_static_contract");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"bad-directory\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn bad(path: str) -> Vec[DirEntry] effects [fs.read]:\n    return core_io_list_dir(path)\n",
    )
    .unwrap();
    let diagnostics = check_project(&root).unwrap();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "argument_type_mismatch"));

    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"bad-directory\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\neffects = []\n\n[application.limits]\nmax_directory_entries = 10\n",
    )
    .unwrap();
    let error = load_project(&root).unwrap_err();
    assert!(error.contains("requires the fs.read effect"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_manifest_limit_is_positive_and_hard_bounded() {
    for limit in [0, 1_000_001] {
        let root = scratch(&format!("directory_limit_{limit}"));
        fs::write(
            root.join("Forgentex.toml"),
            format!(
                "[package]\nname = \"bad-directory-limit\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\neffects = [\"fs.read\"]\n\n[application.limits]\nmax_read_bytes = 1024\nmax_directory_entries = {limit}\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.fgx"),
            "fn main(path: str) -> str effects [fs.read]:\n    return path\n",
        )
        .unwrap();

        let error = load_project(&root).unwrap_err();
        assert!(error.contains("between 1 and 1000000"), "{error}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn pure_path_operations_preserve_component_boundaries() {
    let root = scratch("path_operations");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"path-operations\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"extension\"\nruntime = \"hosted_io\"\neffects = []\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "import core.path\n\nfn extension(base: str, child: str) -> str:\n    let root: Path = core_path_parse(base)\n    let joined: Path = core_path_join(root, child)\n    return core_path_extension(joined)\n",
    )
    .unwrap();

    let report = run(
        &root,
        vec!["assets".to_string(), "images/preview.fgp".to_string()],
    )
    .unwrap();
    assert_eq!(report.status, "ok");
    assert_eq!(result_text(&report), Some("fgp"));
    assert_eq!(report.directory_entries, 0);
    assert!(report.ops.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_manifest_requires_a_limit_for_each_io_effect() {
    let root = scratch("missing_limit");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"main\"\neffects = [\"fs.read\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn main(path: str) -> str effects [fs.read]:\n    return path\n",
    )
    .unwrap();

    let error = load_project(&root).unwrap_err();
    assert!(error.contains("max_read_bytes"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn application_entry_effects_must_be_declared_by_the_manifest() {
    let root = scratch("effect_mismatch");
    write_application(&root, 1024, "[\"fs.read\"]");
    fs::write(root.join("input").join("source.txt"), "payload").unwrap();

    let error = run(
        &root,
        vec![
            "input/source.txt".to_string(),
            "output/copied.txt".to_string(),
        ],
    )
    .unwrap_err();
    assert!(error.contains("entry effect 'fs.write'"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn checked_in_scoped_copy_is_a_valid_application_project() {
    let root = manifest_path("examples/apps/scoped_copy");
    let project = load_project(&root).unwrap();
    let application = project.manifest.application.expect("application contract");
    assert_eq!(application.entry, "copy_binary");
    assert_eq!(application.runtime, "hosted_io");
    assert_eq!(application.max_read_bytes, Some(1_048_576));
    assert_eq!(application.artifacts.len(), 1);
    assert_eq!(application.artifacts[0].name, "copy");
    assert_eq!(application.artifacts[0].root, "output");
    assert_eq!(
        application.artifacts[0].media_type,
        "application/octet-stream"
    );
    assert!(application.artifacts[0].required);
    assert!(check_project(&root).unwrap().is_empty());
}

#[test]
fn checked_in_directory_inventory_is_a_valid_bounded_application() {
    let root = manifest_path("examples/apps/directory_inventory");
    let project = load_project(&root).unwrap();
    let application = project.manifest.application.expect("application contract");
    assert_eq!(application.entry, "inventory_count");
    assert_eq!(application.max_directory_entries, Some(64));
    assert!(check_project(&root).unwrap().is_empty());
}

#[test]
fn hosted_io_admits_alloc_so_an_entry_can_build_the_bytes_it_publishes() {
    let root = scratch("alloc_effect");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"alloc-application-test\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"build\"\nruntime = \"hosted_io\"\neffects = [\"alloc\", \"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.bundle]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\nrequired = true\npublication = \"create_new\"\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    // Constructing an owned buffer requires `alloc`; without it a hosted entry
    // could only forward bytes it had already read.
    fs::write(
        root.join("src").join("main.fgx"),
        "import core.io\n\nfn build() -> usize effects [alloc, fs.write]:\n    let bytes: Vec[u8] = vec_new()\n    push(bytes, u8(70))\n    push(bytes, u8(71))\n    push(bytes, u8(88))\n    let path: Path = core_path_parse(\"output/built.bin\")\n    return core_io_publish_bytes_artifact(\"bundle\", path, bytes)\n",
    )
    .unwrap();
    assert!(check_project(&root).unwrap().is_empty());

    let report = run(&root, Vec::new()).expect("hosted_io must admit the alloc effect");
    assert_eq!(report.status, "ok", "report: {report:?}");
    assert_eq!(result_text(&report), Some("3"));
    assert_eq!(fs::read(root.join("output/built.bin")).unwrap(), b"FGX");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn hosted_io_still_refuses_native_execution_effects() {
    let root = scratch("cpu_effect");
    // `cpu` implies a native process tree this runtime cannot govern, so it
    // must stay rejected even though `alloc` is now admitted.
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"cpu-application-test\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"build\"\nruntime = \"hosted_io\"\neffects = [\"alloc\", \"cpu\"]\n\n[permissions]\nfs.read = [\"input\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "fn build() -> i32:\n    return 1\n",
    )
    .unwrap();
    let error = load_project(&root).expect_err("cpu must remain unsupported");
    assert!(
        error.contains("'cpu' is unsupported by the hosted_io runtime"),
        "unexpected error: {error}"
    );
    let _ = fs::remove_dir_all(&root);
}
