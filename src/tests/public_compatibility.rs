use std::path::Path;

use crate::diag::{diagnostics_to_json, Diagnostic};
use crate::diagnostic_catalog::catalog;
use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::public_compat::{
    compatibility_gaps, examples_evidence_gaps, public_0_1_evidence, standard_foundation_gaps,
    PUBLIC_0_1_LIMITS, PUBLIC_DIAGNOSTIC_CODES, PUBLIC_JSON_SCHEMAS, PUBLIC_STANDARD_FOUNDATION,
};
use crate::stdlib::standard_library_modules;

const CONTRACT: &str = include_str!("../../docs/public_0_1_compatibility.md");
const PORTABLE_CORE: &str = include_str!("../../examples/public_0_1/portable_core.fgx");
const PROFILE: &str = include_str!("../../docs/public_0_1_profile.md");

#[test]
fn public_0_1_diagnostic_catalog_is_an_exact_frozen_set() {
    let actual = catalog()
        .into_iter()
        .map(|entry| entry.code)
        .collect::<Vec<_>>();
    assert_eq!(actual, PUBLIC_DIAGNOSTIC_CODES);
    assert!(compatibility_gaps().is_empty());
    for code in PUBLIC_DIAGNOSTIC_CODES {
        assert!(
            CONTRACT.contains(code),
            "{code} is absent from the contract"
        );
    }
}

#[test]
fn public_0_1_schema_inventory_is_unique_documented_and_shaped() {
    let mut ids = PUBLIC_JSON_SCHEMAS
        .iter()
        .map(|schema| schema.id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), PUBLIC_JSON_SCHEMAS.len());

    for schema in PUBLIC_JSON_SCHEMAS {
        assert!(
            CONTRACT.contains(schema.id),
            "{} is undocumented",
            schema.id
        );
        assert!(
            CONTRACT.contains(schema.command),
            "{} has no command",
            schema.id
        );
        assert!(
            !schema.required_fields.is_empty(),
            "{} has no shape",
            schema.id
        );
        assert_eq!(schema.required_fields[0], "schema");
        for field in schema.required_fields {
            assert!(
                CONTRACT.contains(field),
                "{}.{} is undocumented",
                schema.id,
                field
            );
        }
    }
}

#[test]
fn public_checker_and_interpreter_reports_carry_versioned_schemas() {
    let diagnostic = Diagnostic::new("type_mismatch", "@function.demo", "mismatch")
        .suggest("replace_expression");
    let diagnostics = diagnostics_to_json(&[diagnostic]);
    assert!(diagnostics.contains("\"schema\": \"forgentex.diagnostics.v1\""));
    for field in ["error", "node", "message", "suggested_patches"] {
        assert!(
            diagnostics.contains(&format!("\"{field}\"")),
            "{diagnostics}"
        );
    }

    let program = parse_file(
        Path::new("examples/public_0_1/portable_core.fgx"),
        PORTABLE_CORE,
    )
    .unwrap();
    let run = run_entry_json(
        &program,
        "public_score",
        r#"{"args":{"limit":4},"expect":{"return":19}}"#,
    )
    .unwrap();
    assert!(run.contains("\"schema\": \"forgentex.run_result.v1\""));
    for field in ["entry", "status", "return", "bindings"] {
        assert!(run.contains(&format!("\"{field}\"")), "{run}");
    }
}

#[test]
fn public_0_1_standard_foundation_is_frozen_and_backed() {
    assert!(
        standard_foundation_gaps().is_empty(),
        "{:?}",
        standard_foundation_gaps()
    );
    let modules = standard_library_modules();
    for frozen in PUBLIC_STANDARD_FOUNDATION {
        let actual = modules
            .iter()
            .find(|module| module.name == frozen.name)
            .unwrap_or_else(|| panic!("{} missing from live catalog", frozen.name));
        for function in frozen.functions {
            assert!(
                actual.functions.contains(function),
                "{}::{} missing",
                frozen.name,
                function
            );
        }
        assert!(
            PROFILE.contains(frozen.name),
            "{} is undocumented in the public profile",
            frozen.name
        );
    }
}

#[test]
fn public_0_1_examples_evidence_reproduces_every_recorded_run() {
    let report = public_0_1_evidence();
    assert!(examples_evidence_gaps().is_empty(), "{:?}", report.gaps);
    assert_eq!(report.status, "pass");
    assert_eq!(report.schema, "forgentex.public_0_1_evidence.v1");
    assert!(!report.fixtures.is_empty());
    for fixture in &report.fixtures {
        assert!(fixture.documented, "{} undocumented", fixture.path);
        assert_eq!(fixture.roundtrip, "pass", "{} roundtrip", fixture.path);
        assert_eq!(fixture.check, "pass", "{} check", fixture.path);
        assert!(!fixture.runs.is_empty(), "{} has no runs", fixture.path);
        for run in &fixture.runs {
            assert_eq!(run.status, "pass", "{}::{}", fixture.path, run.entry);
        }
    }
    for limit in PUBLIC_0_1_LIMITS {
        assert!(
            report.limits.iter().any(|value| value == limit),
            "{limit} missing from evidence limits"
        );
    }

    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.public_0_1_evidence.v1\""));
    for field in ["status", "fixtures", "limits", "gaps"] {
        assert!(json.contains(&format!("\"{field}\"")), "{json}");
    }
}
