use std::path::PathBuf;

use crate::polyglot::run;

use super::parse_fixture;

fn out_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_polyglot_{tag}", std::process::id()))
}

#[test]
fn one_command_routes_every_capsule_through_one_abi() {
    let program = parse_fixture("examples/polyglot/pipeline.fgx");
    let report = run("pipeline.fgx", &program, &out_dir("route"));

    assert_eq!(report.schema, "forgentex.polyglot_abi.v1");
    assert_eq!(
        report.languages,
        vec!["c".to_string(), "python".to_string()]
    );
    assert_eq!(report.capsules.len(), 2);

    let scale = report.capsules.iter().find(|c| c.name == "scale").unwrap();
    assert_eq!(scale.backend, "ffi");
    let label = report.capsules.iter().find(|c| c.name == "label").unwrap();
    assert_eq!(label.backend, "python");

    // Every capsule either executed or degraded gracefully (no compiler /
    // no python on CI); neither is a Forgentex failure.
    assert!(report.capsules.iter().all(|c| c.ok), "report: {report:?}");
    assert_eq!(report.status, "ok");
    let _ = std::fs::remove_dir_all(out_dir("route"));
}

#[test]
fn shared_value_is_the_cross_language_data_plane() {
    let program = parse_fixture("examples/polyglot/pipeline.fgx");
    let report = run("pipeline.fgx", &program, &out_dir("plane"));

    assert_eq!(report.data_plane.len(), 1);
    let gain = &report.data_plane[0];
    assert_eq!(gain.name, "gain");
    assert_eq!(gain.ty, "i32");
    // One value, declared once, bound by capsules in two languages by name.
    assert!(gain
        .binders
        .iter()
        .any(|(c, l, m)| c == "scale" && l == "c" && m == "read"));
    assert!(gain
        .binders
        .iter()
        .any(|(c, l, m)| c == "label" && l == "python" && m == "read"));
    let _ = std::fs::remove_dir_all(out_dir("plane"));
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/polyglot/pipeline.fgx");
    let json = run("pipeline.fgx", &program, &out_dir("json")).to_json();
    assert!(json.contains("\"schema\": \"forgentex.polyglot_abi.v1\""));
    assert!(json.contains("\"data_plane\""));
    assert!(json.contains("\"binders\""));
    let _ = std::fs::remove_dir_all(out_dir("json"));
}
