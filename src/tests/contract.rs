use crate::contract::{analyze, validate};
use crate::parser::parse_human;

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn compliant_contract_reviews_cleanly() {
    let program = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    assert!(validate(&program).is_empty());
    let report = analyze("price_oracle.fgx", &program);
    assert_eq!(report.status, "review");
    let entry = &report.capsules[0];
    assert_eq!(entry.compiler, "0.8.24");
    assert_eq!(entry.evm_target.as_deref(), Some("evm-shanghai"));
    assert!(entry.abi_hash.is_some());
    assert_eq!(entry.gas_limit.as_deref(), Some("120000"));
    assert!(entry.secret_access.is_empty());
}

#[test]
fn contract_without_abi_hash_is_rejected() {
    let source = "\
foreign c solidity version \"0.8.24\" env \"evm-shanghai\" effects [chain.read]:
    output ok: bool
    sandbox \"chain.read mainnet\"
    code:
        // contract
";
    let program = human(source);
    let codes: Vec<String> = validate(&program).into_iter().map(|d| d.code).collect();
    assert!(codes.contains(&"contract_capsule_no_abi_hash".to_string()));
    assert_eq!(analyze("c.fgx", &program).status, "reject");
}

#[test]
fn contract_with_secret_key_access_is_rejected() {
    let source = "\
foreign c vyper version \"0.3.10\" env \"evm-shanghai\" effects [chain.write, crypto.sign]:
    output ok: bool
    sandbox \"chain.write mainnet\"
    sandbox \"abi.hash sha256:abc\"
    sandbox \"wallet.access default\"
    code:
        # vyper
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(codes.contains(&"contract_capsule_secret_key".to_string()));
}

#[test]
fn missing_gas_is_a_note_not_a_rejection() {
    let source = "\
foreign c solidity version \"0.8.24\" env \"evm-shanghai\" effects [chain.read]:
    output ok: bool
    sandbox \"chain.read mainnet\"
    sandbox \"abi.hash sha256:abc\"
    code:
        // contract
";
    let program = human(source);
    assert!(validate(&program).is_empty());
    let report = analyze("c.fgx", &program);
    assert_eq!(report.status, "review");
    assert!(report.capsules[0].notes.iter().any(|n| n.contains("gas")));
}

#[test]
fn contract_review_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/price_oracle_solidity.fgx");
    let json = analyze("price_oracle.fgx", &program).to_json();
    assert!(json.contains("\"schema\": \"forgentex.contract_review.v1\""));
    assert!(json.contains("\"compiler\": \"0.8.24\""));
    assert!(json.contains("\"gas_limit\": \"120000\""));
}
