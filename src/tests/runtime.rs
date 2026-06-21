use crate::parser::parse_human;
use crate::runtime::{catalog, catalog_json, validate};

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn catalog_covers_managed_runtimes() {
    let languages: Vec<&str> = catalog().iter().map(|spec| spec.language).collect();
    for expected in ["python", "javascript", "typescript", "java", "csharp", "go"] {
        assert!(languages.contains(&expected), "missing runtime: {expected}");
    }
    let json = catalog_json();
    assert!(json.contains("\"schema\": \"forgentex.runtime_catalog.v1\""));
    assert!(json.contains("\"language\": \"typescript\""));
    assert!(json.contains("package-lock.json"));
}

#[test]
fn matching_ecosystem_passes() {
    let program = parse_fixture("examples/capsule/web_session.fgx");
    // python+pypi and typescript+npm both match their language ecosystem.
    assert!(validate(&program).is_empty());
}

#[test]
fn foreign_ecosystem_is_rejected() {
    let source = "\
foreign x python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    dep \"npm:leftpad@1.0.0#sha512:abc\"
    code:
        y = 1
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"capsule_ecosystem_mismatch".to_string()));
}

#[test]
fn generic_github_source_is_allowed_for_any_language() {
    let source = "\
foreign x go version \"1.23\":
    output v: i64
    sandbox \"fs.none\"
    dep \"github:org/repo@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2\"
    code:
        y := 1
";
    assert!(validate(&human(source)).is_empty());
}

#[test]
fn unknown_language_is_not_opined_on() {
    // Solidity is not a managed runtime in the catalog; its github dep is
    // left alone.
    let program = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    assert!(validate(&program).is_empty());
}

#[test]
fn typed_schema_is_shared_across_backend_and_frontend() {
    let program = parse_fixture("examples/capsule/web_session.fgx");
    let share = program.shares().next().expect("shared schema present");
    assert_eq!(share.name, "session");
    assert_eq!(share.ty.to_agent(), "Session");
    let backend = program
        .capsules()
        .find(|capsule| capsule.name == "backend")
        .unwrap();
    let frontend = program
        .capsules()
        .find(|capsule| capsule.name == "frontend")
        .unwrap();
    assert_eq!(backend.language, "python");
    assert_eq!(frontend.language, "typescript");
    assert!(backend.bindings.iter().any(|b| b.name == "session"));
    assert!(frontend.bindings.iter().any(|b| b.name == "session"));
}
