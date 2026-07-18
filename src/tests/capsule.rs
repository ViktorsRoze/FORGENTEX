use crate::ast::Item;
use crate::capsule::{analyze, run, run_input, validate};
use crate::format::{format_agent, format_human};
use crate::parser::{parse_agent, parse_capsule_values, parse_human};

use super::{fixture, parse_fixture};

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("human capsule should parse")
}

#[test]
fn capsule_parses_contract_and_code_in_human_syntax() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let capsule = program.capsules().next().expect("capsule present");
    assert_eq!(capsule.id, "@capsule.normalize_py");
    assert_eq!(capsule.language, "python");
    assert_eq!(capsule.version, "3.13");
    assert_eq!(capsule.env.as_deref(), Some("cpython-3.13-linux-x86_64"));
    assert_eq!(capsule.effects, vec!["alloc".to_string()]);
    assert_eq!(capsule.inputs.len(), 1);
    assert_eq!(capsule.outputs.len(), 1);
    assert_eq!(capsule.deps.len(), 1);
    assert!(capsule.deps[0].is_pinned());
    assert!(!capsule.code.is_empty());
    // The Python comment line keeps its `#`.
    assert!(capsule.code.iter().any(|line| line.contains("# clamp")));
}

#[test]
fn capsule_roundtrips_between_human_and_agent_views() {
    let from_human = parse_fixture("examples/capsule/normalize_python.fgx");
    let from_agent =
        parse_agent(fixture("examples/capsule/normalize_python.fgai")).expect("agent parse");
    assert!(
        from_human.canonical_eq(&from_agent),
        "human and agent capsule views must share one canonical AST"
    );
}

#[test]
fn capsule_formatting_is_idempotent_in_both_syntaxes() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");

    let human_once = format_human(&program);
    let human_twice = format_human(&human(&human_once));
    assert_eq!(human_once, human_twice);

    let agent_once = format_agent(&program);
    let agent_twice = format_agent(&parse_agent(&agent_once).expect("reparse agent"));
    assert_eq!(agent_once, agent_twice);
}

#[test]
fn dependency_pinning_detection() {
    let pinned = "\
foreign demo python version \"3.13\":
    output v: i32
    dep \"pypi:numpy==2.2.0#sha256:abc\"
    sandbox \"fs.none\"
    code:
        x = 1
";
    let program = human(pinned);
    let capsule = program.capsules().next().unwrap();
    assert!(capsule.deps[0].is_pinned());

    let floating = "\
foreign demo python version \"3.13\":
    output v: i32
    dep \"pypi:numpy\"
    dep \"github:org/repo@main\"
    sandbox \"fs.none\"
    code:
        x = 1
";
    let program = human(floating);
    let capsule = program.capsules().next().unwrap();
    assert!(!capsule.deps[0].is_pinned());
    assert!(!capsule.deps[1].is_pinned());
}

#[test]
fn validation_rejects_incomplete_contracts() {
    // Unpinned dependency, no sandbox, no boundary, empty code.
    let source = "\
foreign bad python version \"3.13\":
    dep \"pypi:requests\"
    code:
";
    let program = human(source);
    let codes: Vec<String> = validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"capsule_unpinned_dependency".to_string()));
    assert!(codes.contains(&"capsule_missing_sandbox".to_string()));
    assert!(codes.contains(&"capsule_missing_typed_boundary".to_string()));
    assert!(codes.contains(&"capsule_empty_code".to_string()));
}

#[test]
fn valid_capsule_has_no_contract_diagnostics() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    assert!(validate(&program).is_empty());
}

#[test]
fn artifact_policy_requires_typed_owned_bytes_and_a_bounded_media_contract() {
    let missing_limit = human(
        "foreign render c version \"c11\":\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    code:\n        int render(void) { return 0; }\n",
    );
    let diagnostics = validate(&missing_limit);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "capsule_invalid_artifact_policy"));

    let wrong_output = human(
        "foreign render c version \"c11\":\n    output value: i32\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 1024\"\n    code:\n        int render(void) { return 0; }\n",
    );
    assert!(validate(&wrong_output).iter().any(|diagnostic| diagnostic
        .message
        .contains("requires exactly one owned Vec[u8]")));

    let unsafe_path = human(
        "foreign render c version \"c11\" effects [fs.write]:\n    output bytes: Vec[u8]\n    sandbox \"fs.write build/artifacts\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 1024\"\n    sandbox \"artifact.path ../escape.png\"\n    code:\n        int render(void) { return 0; }\n",
    );
    assert!(validate(&unsafe_path)
        .iter()
        .any(|diagnostic| diagnostic.message.contains("without parent traversal")));

    let uncovered_path = human(
        "foreign render c version \"c11\":\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    sandbox \"artifact.media_type image/png\"\n    sandbox \"artifact.max_bytes 1024\"\n    sandbox \"artifact.path build/render.png\"\n    code:\n        int render(void) { return 0; }\n",
    );
    assert!(validate(&uncovered_path)
        .iter()
        .any(|diagnostic| diagnostic.message.contains("requires the fs.write effect")));
}

#[test]
fn preview_render_policy_requires_seed_artifact_effect_and_budgets() {
    let source = include_str!("../../examples/pipeline/render_preview.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let renderer = program
        .capsules()
        .find(|capsule| capsule.name == "render_ppm_preview")
        .unwrap();
    let policy = crate::capsule::render_policy(renderer)
        .unwrap()
        .expect("render policy");
    assert_eq!(policy.profile, "preview");
    assert_eq!(policy.seed_field, "seed");

    let missing_budget = human(&source.replacen("    sandbox \"limit.mem_mb 64\"\n", "", 1));
    assert!(validate(&missing_budget).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic.message.contains("requires limit.mem_mb")
    }));

    let wrong_media =
        human(&source.replacen("image/x-portable-pixmap", "application/octet-stream", 1));
    assert!(validate(&wrong_media).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_render_policy"
            && diagnostic
                .message
                .contains("must be an image/* or application/x-forgentex-rgb")
    }));
}

#[test]
fn image_encode_policy_requires_raw_input_image_output_effect_and_budgets() {
    let source = include_str!("../../examples/pipeline/render_encode_preview.fgx");
    let program = human(source);
    assert!(validate(&program).is_empty());
    let encoder = program
        .capsules()
        .find(|capsule| capsule.name == "encode_ppm_preview")
        .unwrap();
    let policy = crate::capsule::encode_policy(encoder)
        .unwrap()
        .expect("encode policy");
    assert_eq!(policy.profile, "ppm");

    let missing_budget = human(&source.replacen("    sandbox \"limit.mem_mb 64\"\n", "", 2));
    assert!(validate(&missing_budget).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic.message.contains("requires limit.mem_mb")
    }));

    let wrong_input =
        human(&source.replacen("    input frame: &[u8]", "    input frame: &mut [u8]", 1));
    assert!(validate(&wrong_input).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic
                .message
                .contains("exactly one immutable &[u8] input")
    }));

    let wrong_output = human(&source.replacen(
        "artifact.media_type image/x-portable-pixmap",
        "artifact.media_type application/octet-stream",
        1,
    ));
    assert!(validate(&wrong_output).iter().any(|diagnostic| {
        diagnostic.code == "capsule_invalid_encode_policy"
            && diagnostic.message.contains("must start with image/")
    }));
}

#[test]
fn review_report_classifies_managed_and_high_risk() {
    let managed = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = analyze("normalize_python.fgx", &managed);
    assert_eq!(report.status, "pass");
    assert_eq!(report.capsules[0].risk_class, "managed");
    assert!(!report.capsules[0].high_risk);
    assert_eq!(report.capsules[0].native_backend, "not_lowerable");

    let contract = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    let report = analyze("price_oracle_solidity.fgx", &contract);
    assert_eq!(report.status, "review_required");
    assert_eq!(report.capsules[0].risk_class, "smart_contract");
    assert!(report.capsules[0].high_risk);
    assert!(report
        .capsules
        .iter()
        .any(|c| c.notes.iter().any(|n| n.contains("secret-key"))));
}

#[test]
fn review_report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let json = analyze("normalize_python.fgx", &program).to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_review.v1\""));
    assert!(json.contains("\"risk_class\": \"managed\""));
    assert!(json.contains("\"native_backend\": \"not_lowerable\""));
    assert!(json.contains("\"pinned\": true"));
}

#[test]
fn fixtures_parse_and_validate_against_boundary() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let capsule = program.capsules().next().unwrap();
    assert_eq!(capsule.fixtures.len(), 3);
    assert!(validate(&program).is_empty());
}

#[test]
fn fixture_type_and_arity_are_rejected() {
    let source = "\
foreign t python version \"3.13\":
    input value: i32
    output normalized: i32
    sandbox \"fs.none\"
    mock 300 -> \"oops\"
    mock 1, 2 -> 3
    code:
        x = 1
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"capsule_fixture_type".to_string()));
    assert!(codes.contains(&"capsule_fixture_arity".to_string()));
}

#[test]
fn mock_runner_executes_all_fixtures() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = run("normalize_python.fgx", &program, None).expect("run");
    assert_eq!(report.status, "pass");
    assert_eq!(report.passed, 3);
    assert_eq!(report.failed, 0);
    assert!(report.to_json().contains("forgentex.capsule_run.v1"));
}

#[test]
fn mock_execution_hook_returns_matching_outputs() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let inputs = parse_capsule_values("300").unwrap();
    let outputs = run_input(&program, None, &inputs).expect("matched fixture");
    assert_eq!(outputs, vec!["255".to_string()]);

    let missing = parse_capsule_values("7").unwrap();
    assert!(run_input(&program, None, &missing).is_err());
}

#[test]
fn fixtures_survive_roundtrip() {
    let from_human = parse_fixture("examples/capsule/normalize_python.fgx");
    let agent = format_agent(&from_human);
    let from_agent = parse_agent(&agent).expect("agent parse");
    assert!(from_human.canonical_eq(&from_agent));
    let capsule = from_agent.capsules().next().unwrap();
    assert_eq!(capsule.fixtures.len(), 3);
}

#[test]
fn capsule_appears_in_canonical_ast() {
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"kind\": \"capsule\""));
    assert!(canonical.contains("\"id\": \"@capsule.normalize_py\""));
    assert!(matches!(program.items.last(), Some(Item::Capsule(_))));
}
