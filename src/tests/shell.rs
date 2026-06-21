use crate::parser::parse_human;
use crate::shell::{analyze, validate};

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn allowlisted_shell_capsule_reviews_cleanly() {
    let program = parse_fixture("examples/capsule/backup_bash.fgx");
    assert!(validate(&program).is_empty());
    let report = analyze("backup_bash.fgx", &program);
    assert_eq!(report.status, "review");
    let entry = &report.capsules[0];
    assert_eq!(
        entry.allowed_commands,
        vec!["tar".to_string(), "gzip".to_string()]
    );
    assert_eq!(entry.host_paths.len(), 2);
    assert_eq!(entry.decision, "review");
}

#[test]
fn shell_capsule_without_allowlist_is_rejected() {
    let source = "\
foreign x bash version \"5.2\":
    output ok: bool
    sandbox \"proc.spawn\"
    code:
        rm -rf /
";
    let program = human(source);
    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"shell_capsule_no_allowlist".to_string()));
    assert_eq!(analyze("x.fgx", &program).status, "reject");
}

#[test]
fn wildcard_allowlist_is_rejected() {
    let source = "\
foreign x bash version \"5.2\":
    output ok: bool
    sandbox \"proc.spawn\"
    sandbox \"exec.allow *\"
    code:
        echo hi
";
    let program = human(source);
    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"shell_command_wildcard".to_string()));
}

#[test]
fn powershell_is_classified_shell() {
    let source = "\
foreign deploy powershell version \"7.4\":
    output ok: bool
    sandbox \"proc.spawn\"
    sandbox \"exec.allow Copy-Item\"
    code:
        Copy-Item a b
";
    let report = analyze("deploy.fgx", &human(source));
    let entry = &report.capsules[0];
    assert_eq!(entry.language, "powershell");
    assert_eq!(entry.allowed_commands, vec!["Copy-Item".to_string()]);
    assert_eq!(entry.decision, "review");
}

#[test]
fn shell_review_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/backup_bash.fgx");
    let json = analyze("backup_bash.fgx", &program).to_json();
    assert!(json.contains("\"schema\": \"forgentex.shell_review.v1\""));
    assert!(json.contains("\"allowed_commands\": [\"tar\", \"gzip\"]"));
    assert!(json.contains("\"decision\": \"review\""));
}

#[test]
fn non_shell_capsules_are_not_reviewed() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = analyze("normalize_python.fgx", &program);
    assert!(report.capsules.is_empty());
    assert_eq!(report.status, "pass");
}
