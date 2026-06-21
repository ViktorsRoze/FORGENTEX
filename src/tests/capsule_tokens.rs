use crate::capsule_tokens::report;

use super::parse_fixture;

#[test]
fn boundary_is_cheaper_than_body_and_scales_with_verbosity() {
    let program = parse_fixture("examples/capsule/web_session.fgx");
    let r = report("web_session.fgx", &program);

    assert_eq!(r.capsules.len(), 2);
    assert!(r.total_boundary_tokens < r.total_body_tokens);
    assert!(r.working_set_ratio() < 1.0);

    // The verbose TypeScript capsule should equalize more than Python.
    let python = r.capsules.iter().find(|c| c.language == "python").unwrap();
    let ts = r
        .capsules
        .iter()
        .find(|c| c.language == "typescript")
        .unwrap();
    assert!(ts.equalizer() > python.equalizer());
}

#[test]
fn verbose_business_language_has_a_large_equalizer() {
    let program = parse_fixture("examples/capsule/order_total_java.fgx");
    let r = report("order_total_java.fgx", &program);
    let java = &r.capsules[0];
    assert_eq!(java.language, "java");
    // A ~20-line Java body collapses to a one-line card: a big avoidance win.
    assert!(
        java.equalizer() > 3.0,
        "expected a large equalizer, got {:.1}",
        java.equalizer()
    );
    assert!(java.boundary_tokens < java.body_tokens);
}

#[test]
fn shared_type_declared_once_saves_per_language_restatement() {
    let program = parse_fixture("examples/capsule/web_session.fgx");
    let r = report("web_session.fgx", &program);
    let session = r.shared_types.iter().find(|s| s.name == "session").unwrap();
    assert_eq!(session.bindings, 2);
    assert!(session.saved_tokens > 0);
    assert!(r.shared_saved_tokens > 0);
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/order_total_java.fgx");
    let json = report("order_total_java.fgx", &program).to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_tokens.v1\""));
    assert!(json.contains("\"equalizer\""));
    assert!(json.contains("\"working_set_ratio\""));
}
