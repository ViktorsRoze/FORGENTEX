use crate::doctor::{doctor_report, doctor_status, DOCTOR_SCHEMA};

#[test]
fn doctor_status_requires_a_supported_host_and_path_install() {
    assert_eq!(doctor_status(true, true), "pass");
    assert_eq!(doctor_status(true, false), "action_required");
    assert_eq!(doctor_status(false, true), "unsupported_host");
    assert_eq!(doctor_status(false, false), "unsupported_host");
}

#[test]
fn doctor_report_has_stable_machine_and_human_shapes() {
    let report = doctor_report();
    assert_eq!(report.schema, DOCTOR_SCHEMA);
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
    assert!(report.interpreter_ready);
    assert_eq!(report.tools.len(), 5);

    let json = report.to_json();
    assert!(json.starts_with("{\n  \"schema\": \"forgentex.doctor.v1\""));
    assert!(json.contains("\"installed_on_path\":"));
    assert!(json.contains("\"native_c_ready\":"));
    assert!(json.ends_with("}\n"));

    let text = report.to_text();
    assert!(text.starts_with("Forgentex doctor\n"));
    assert!(text.contains("interpreter=ready"));
}
