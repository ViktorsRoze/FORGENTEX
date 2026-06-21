use crate::parser::parse_human;
use crate::sandbox::{analyze, analyze_capsule, is_approved, validate};

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn covered_effect_is_allowed() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let backend = program
        .capsules()
        .find(|capsule| capsule.name == "backend")
        .unwrap();
    let plan = analyze_capsule(backend);
    assert!(plan.uncovered_effects.is_empty());
    assert_eq!(plan.decision, "allow");
    assert!(is_approved(backend));
}

#[test]
fn resource_limits_are_parsed() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let backend = program
        .capsules()
        .find(|capsule| capsule.name == "backend")
        .unwrap();
    let plan = analyze_capsule(backend);
    assert!(plan
        .limits
        .iter()
        .any(|(key, value)| key == "wall_ms" && value == "1000"));
    assert!(plan
        .limits
        .iter()
        .any(|(key, value)| key == "mem_mb" && value == "256"));
}

#[test]
fn uncovered_effect_is_denied_and_rejected() {
    let source = "\
foreign leaky python version \"3.13\" effects [network.read]:
    output r: i64
    sandbox \"fs.none\"
    code:
        x = 1
";
    let program = human(source);
    let capsule = program.capsules().next().unwrap();
    let plan = analyze_capsule(capsule);
    assert_eq!(plan.uncovered_effects, vec!["network.read".to_string()]);
    assert_eq!(plan.decision, "deny");
    assert!(!is_approved(capsule));

    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"sandbox_effect_uncovered".to_string()));
}

#[test]
fn compute_effects_need_no_grant() {
    let source = "\
foreign calc python version \"3.13\" effects [alloc]:
    output r: i64
    sandbox \"fs.none\"
    code:
        x = 1
";
    let capsule_program = human(source);
    let capsule = capsule_program.capsules().next().unwrap();
    assert!(analyze_capsule(capsule).uncovered_effects.is_empty());
    assert!(validate(&capsule_program).is_empty());
}

#[test]
fn invalid_limit_is_rejected() {
    let source = "\
foreign t python version \"3.13\":
    output r: i64
    sandbox \"fs.none\"
    sandbox \"limit.cpu_ms fast\"
    code:
        x = 1
";
    let program = human(source);
    let plan = analyze_capsule(program.capsules().next().unwrap());
    assert!(!plan.invalid_limits.is_empty());
    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"sandbox_invalid_limit".to_string()));
}

#[test]
fn smart_contract_capsule_needs_review() {
    let program = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    let capsule = program.capsules().next().unwrap();
    let plan = analyze_capsule(capsule);
    // chain.read effect is covered by the chain.read grant, but the
    // high-risk class still requires review rather than auto-allow.
    assert!(plan.uncovered_effects.is_empty());
    assert_eq!(plan.decision, "review");
}

#[test]
fn unused_grant_is_flagged_for_review() {
    let source = "\
foreign over python version \"3.13\":
    output r: i64
    sandbox \"net.connect example.com:443\"
    code:
        x = 1
";
    let plan = analyze_capsule(human(source).capsules().next().unwrap());
    assert_eq!(plan.unused_grants, vec!["net".to_string()]);
    assert_eq!(plan.decision, "review");
}

#[test]
fn sandbox_plan_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/web_counter.fgx");
    let plan = analyze("web_counter.fgx", &program);
    let json = plan.to_json();
    assert!(json.contains("\"schema\": \"forgentex.sandbox_plan.v1\""));
    assert!(json.contains("\"decision\": \"allow\""));
    assert!(json.contains("\"capability\": \"net.listen\""));
    assert!(json.contains("\"key\": \"wall_ms\""));
}
