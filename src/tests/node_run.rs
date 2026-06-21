use crate::node_run::run;

use super::parse_fixture;

#[test]
fn render_capsule_runs_on_real_node_or_degrades() {
    let program = parse_fixture("examples/web/one_roof.fgx");
    let report = run("one_roof.fgx", &program, Some("render")).unwrap();

    assert_eq!(report.schema, "forgentex.web_run.v1");
    assert_eq!(report.capsule, "render");
    assert_eq!(report.language, "javascript");
    assert_eq!(report.entry.as_deref(), Some("render"));
    assert_eq!(report.total, 2);
    assert!(report.boundary_tokens > 0 && report.body_tokens > 0);

    // With host node the fixtures are real oracles; without it the run
    // degrades gracefully. Both are a success for Forgentex.
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 2);
        assert!(report.runtime.is_some());
        for case in &report.cases {
            assert!(case.pass, "fixture failed: {case:?}");
        }
    } else {
        assert_eq!(report.status, "runtime_unavailable");
    }
}

#[test]
fn typescript_classifier_runs_on_real_node_or_degrades() {
    // TypeScript with real type annotations, stripped and run on host node.
    let program = parse_fixture("examples/ts/classify.fgx");
    let report = run("classify.fgx", &program, None).unwrap();

    assert_eq!(report.language, "typescript");
    assert_eq!(report.entry.as_deref(), Some("classify"));
    assert_eq!(report.total, 3);
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 3);
        for case in &report.cases {
            assert!(case.pass, "fixture failed: {case:?}");
        }
    } else {
        assert_eq!(report.status, "runtime_unavailable");
    }
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/web/one_roof.fgx");
    let json = run("one_roof.fgx", &program, Some("render"))
        .unwrap()
        .to_json();
    assert!(json.contains("\"schema\": \"forgentex.web_run.v1\""));
    assert!(json.contains("\"boundary_tokens\""));
    assert!(json.contains("\"language\": \"javascript\""));
}

#[test]
fn polyglot_covers_the_web_tier_under_one_roof() {
    let program = parse_fixture("examples/web/one_roof.fgx");
    let out = std::env::temp_dir().join(format!("forgentex_{}_oneroof", std::process::id()));
    let report = crate::polyglot::run("one_roof.fgx", &program, &out);

    assert_eq!(
        report.languages,
        vec!["javascript".to_string(), "python".to_string()]
    );
    let render = report.capsules.iter().find(|c| c.name == "render").unwrap();
    assert_eq!(render.backend, "node");

    // One value, declared once, flows across Python and JavaScript by name.
    let total = report
        .data_plane
        .iter()
        .find(|s| s.name == "total")
        .unwrap();
    assert!(total
        .binders
        .iter()
        .any(|(c, l, m)| c == "accumulate" && l == "python" && m == "rw"));
    assert!(total
        .binders
        .iter()
        .any(|(c, l, m)| c == "render" && l == "javascript" && m == "read"));
    let _ = std::fs::remove_dir_all(&out);
}
