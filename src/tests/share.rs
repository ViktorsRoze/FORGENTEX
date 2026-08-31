use crate::ast::{Item, ShareMode};
use crate::format::{format_agent, format_human};
use crate::parser::{parse_agent, parse_human};
use crate::share::{analyze, validate};

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn share_declaration_parses_with_type_and_default() {
    let program = human("share session_count: i64 = 0\n");
    let share = program.shares().next().expect("share present");
    assert_eq!(share.id, "@share.session_count");
    assert_eq!(share.name, "session_count");
    assert_eq!(share.ty.to_agent(), "i64");
    assert!(share.default.is_some());
}

#[test]
fn capsule_binds_shared_value_by_name() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let backend = program
        .capsules()
        .find(|capsule| capsule.name == "backend")
        .unwrap();
    assert_eq!(backend.bindings.len(), 1);
    assert_eq!(backend.bindings[0].name, "visitor_count");
    assert_eq!(backend.bindings[0].mode, ShareMode::Rw);

    let frontend = program
        .capsules()
        .find(|capsule| capsule.name == "frontend")
        .unwrap();
    assert_eq!(frontend.bindings[0].mode, ShareMode::Read);
}

#[test]
fn shares_and_bindings_roundtrip_between_views() {
    let from_human = parse_fixture("examples/capsule/web_counter.fgx");
    let from_agent = parse_agent(&format_agent(&from_human)).expect("agent view should parse back");
    assert!(
        from_human.canonical_eq(&from_agent),
        "shares and bindings must share one canonical AST"
    );
}

#[test]
fn move_binding_roundtrips_between_views() {
    let source = "share payload: Vec[i16]\nshare total: i32 = 0\n\nforeign consume c version \"c11\":\n    input payload: &[i16]\n    output total: i32\n    sandbox \"fs.none\"\n    bind payload move\n    bind total rw\n    code:\n        int consume(void) { return 0; }\n";
    let human = parse_human(source).unwrap();
    let binding = &human.capsules().next().unwrap().bindings[0];
    assert_eq!(binding.mode, ShareMode::Move);
    let agent = format_agent(&human);
    assert!(agent.contains("BIND payload move"), "{agent}");
    assert!(human.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(format_human(&human).contains("bind payload move"));
    let namespace = analyze("move.fgx", &human);
    let payload = namespace
        .shared
        .iter()
        .find(|entry| entry.name == "payload")
        .unwrap();
    assert_eq!(payload.movers, vec!["consume"]);
    assert!(namespace.to_json().contains("\"movers\": [\"consume\"]"));
}

#[test]
fn update_binding_roundtrips_and_is_reported() {
    let source = "share payload: Vec[i16]\nshare total: i32 = 0\n\nforeign mutate c version \"c11\":\n    input payload: &mut[i16]\n    output total: i32\n    sandbox \"fs.none\"\n    bind payload update\n    bind total rw\n    code:\n        int mutate(void) { return 0; }\n";
    let human = parse_human(source).unwrap();
    assert_eq!(
        human.capsules().next().unwrap().bindings[0].mode,
        ShareMode::Update
    );
    let agent = format_agent(&human);
    assert!(agent.contains("BIND payload update"), "{agent}");
    assert!(human.canonical_eq(&parse_agent(&agent).unwrap()));
    let namespace = analyze("update.fgx", &human);
    let payload = namespace
        .shared
        .iter()
        .find(|entry| entry.name == "payload")
        .unwrap();
    assert_eq!(payload.updaters, vec!["mutate"]);
    assert!(namespace.to_json().contains("\"updaters\": [\"mutate\"]"));
}

#[test]
fn explicit_share_versions_roundtrip_canonically() {
    let source = "share payload: Vec[i16]\nshare total: i32 = 0\n\nforeign mutate c version \"c11\":\n    input payload: &mut[i16]\n    output total: i32\n    sandbox \"fs.none\"\n    bind payload@7 update\n    bind total rw\n    code:\n        int mutate(void) { return 0; }\n";
    let human = parse_human(source).unwrap();
    assert_eq!(
        human.capsules().next().unwrap().bindings[0].name,
        "payload@7"
    );
    let agent = format_agent(&human);
    assert!(agent.contains("BIND payload@7 update"), "{agent}");
    assert!(human.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(format_human(&human).contains("bind payload@7 update"));
    assert!(parse_human(&source.replace("payload@7", "payload@07")).is_err());
    assert!(parse_human(&source.replace("payload@7", "payload@x")).is_err());
}

#[test]
fn share_formatting_is_idempotent() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let human_once = format_human(&program);
    let human_twice = format_human(&human(&human_once));
    assert_eq!(human_once, human_twice);

    let agent_once = format_agent(&program);
    let agent_twice = format_agent(&parse_agent(&agent_once).unwrap());
    assert_eq!(agent_once, agent_twice);
}

#[test]
fn unknown_binding_is_rejected() {
    let source = "\
foreign a python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    bind missing read
    code:
        x = 1
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"share_unknown_binding".to_string()));
}

#[test]
fn multiple_writers_are_rejected() {
    let source = "\
share counter: i64 = 0

foreign a python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    bind counter rw
    code:
        x = 1

foreign b python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    bind counter rw
    code:
        y = 2
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"shared_multiple_writers".to_string()));
}

#[test]
fn share_default_type_mismatch_is_rejected() {
    let codes: Vec<String> = validate(&human("share flag: bool = 3\n"))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"share_default_type".to_string()));
}

#[test]
fn namespace_report_classifies_readers_and_writers() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let report = analyze("web_counter.fgx", &program);
    assert_eq!(report.status, "review_required");
    let entry = &report.shared[0];
    assert_eq!(entry.name, "visitor_count");
    assert_eq!(entry.writers, vec!["backend".to_string()]);
    assert_eq!(entry.readers, vec!["frontend".to_string()]);

    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.shared_namespace.v1\""));
    assert!(json.contains("\"writers\": [\"backend\"]"));
}

#[test]
fn read_only_namespace_passes() {
    let source = "\
share config: i64 = 7

foreign a python version \"3.13\":
    output v: i64
    sandbox \"fs.none\"
    bind config read
    code:
        x = 1
";
    let report = analyze("read_only.fgx", &human(source));
    assert_eq!(report.status, "pass");
}

#[test]
fn share_appears_in_canonical_ast() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"kind\": \"share\""));
    assert!(canonical.contains("\"id\": \"@share.visitor_count\""));
    assert!(matches!(program.items.first(), Some(Item::Share(_))));
}
