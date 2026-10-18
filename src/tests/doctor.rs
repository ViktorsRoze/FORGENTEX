use crate::doctor::{classify_windows_version, doctor_report, doctor_status, DOCTOR_SCHEMA};

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
    assert!(json.contains("\"discovered_toolchains\": ["));
    assert!(json.contains("\"gpus\": ["));
    assert!(json.ends_with("}\n"));
    // Every discovered toolchain carries a version and a consistent reviewed
    // flag (languages limited to the native ones Phase 2 discovers).
    for toolchain in &report.discovered_toolchains {
        assert!(!toolchain.version.is_empty());
        assert!(matches!(
            toolchain.language.as_str(),
            "zig" | "rust" | "cuda" | "julia" | "moonbit"
        ));
    }

    let text = report.to_text();
    assert!(text.starts_with("Forgentex doctor\n"));
    assert!(text.contains("interpreter=ready"));
    for gpu in &report.gpus {
        assert!(text.contains(&format!("gpu: {} {}", gpu.index, gpu.name)));
    }
}

#[test]
fn windows_nt_versions_are_reported_as_the_right_product_generation() {
    assert_eq!(
        classify_windows_version("Microsoft Windows [Version 10.0.19045.5965]"),
        "Windows 10 build 19045.5965 (NT 10.0)"
    );
    assert_eq!(
        classify_windows_version("Microsoft Windows [Version 10.0.26200.7171]"),
        "Windows 11 build 26200.7171 (NT 10.0)"
    );
    assert_eq!(
        classify_windows_version("windows (version unavailable)"),
        "windows (version unavailable)"
    );
}
