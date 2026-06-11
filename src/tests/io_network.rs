use std::path::Path;

use crate::effects::{analyze, effects_to_json};
use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::patch::apply_patch;
use crate::typecheck::check;

use super::parse_fixture;

#[test]
fn filesystem_example_checks_and_runs_with_mock_runtime() {
    let program = parse_fixture("examples/io/file_copy.fgx");
    assert_eq!(check(&program), Vec::new());

    let json = effects_to_json(&analyze(&program));
    assert!(json.contains("\"fs.read\""));
    assert!(json.contains("\"fs.write\""));

    let output = run_entry_json(
        &program,
        "file_copy_demo",
        include_str!("../../examples/run/file_copy.json"),
    )
    .unwrap();

    assert!(output.contains("\"status\": \"pass\""));
    assert!(output.contains("\"Ok\": 29"));
}

#[test]
fn network_example_checks_and_runs_with_mock_runtime() {
    let program = parse_fixture("examples/network/agent_ping.fgx");
    assert_eq!(check(&program), Vec::new());

    let json = effects_to_json(&analyze(&program));
    assert!(json.contains("\"network.connect\""));
    assert!(json.contains("\"network.read\""));
    assert!(json.contains("\"network.http\""));

    let output = run_entry_json(
        &program,
        "agent_ping",
        include_str!("../../examples/run/agent_ping.json"),
    )
    .unwrap();

    assert!(output.contains("\"status\": \"pass\""));
    assert!(output.contains("pong:demo:https://agent.local/ping"));
}

#[test]
fn io_and_network_builtins_require_explicit_effects() {
    let source = r#"
fn bad_fs(scope: str, path: str) -> Result[str,str]:
    return core_fs_read_text(scope, path)

fn bad_net(scope: str, endpoint: str) -> Result[str,str]:
    return core_net_http_get(scope, endpoint)
"#;
    let program = parse_file(Path::new("missing_effects.fgx"), source).unwrap();
    let diagnostics = check(&program);

    let text = format!("{diagnostics:?}");
    assert!(text.contains("fs.read"));
    assert!(text.contains("network.connect"));
    assert!(text.contains("network.read"));
}

#[test]
fn string_literals_roundtrip_and_execute() {
    let source = r#"
fn size() -> usize effects [stream]:
    return core_stream_count_bytes("abc")
"#;
    let program = parse_file(Path::new("strings.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let output = run_entry_json(
        &program,
        "size",
        r#"{ "args": {}, "expect": { "return": 3 } }"#,
    )
    .unwrap();

    assert!(output.contains("\"status\": \"pass\""));
}

#[test]
fn patch_reviews_flag_filesystem_and_network_access() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let patch = "VERSION 1\n\
                 TARGET @function.mix_samples\n\
                 OP ADD_EFFECT effect=fs.read\n\
                 OP ADD_EFFECT effect=network.connect\n";

    let result = apply_patch(program, patch).unwrap();
    let review = result.review.render();
    let json = result.review.render_json(&check(&result.program));

    assert!(result.review.filesystem_changed);
    assert!(result.review.network_changed);
    assert!(review.contains("Filesystem access changed: yes"));
    assert!(review.contains("Network access changed: yes"));
    assert!(json.contains("\"filesystem\": true"));
    assert!(json.contains("\"network\": true"));
}
