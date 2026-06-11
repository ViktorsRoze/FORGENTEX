use std::fs;

use crate::borrow::analyze_borrows;

use super::manifest_path;

#[test]
fn borrow_report_lists_rules_and_filtered_diagnostics() {
    let good = analyze_borrows(&manifest_path("examples/media/audio_mix.fgx")).unwrap();
    assert_eq!(good.schema, "forgentex.borrow_report.v1");
    assert_eq!(good.status, "pass");
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "borrow_alias_roots"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "lifetime_region_graph"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "non_lexical_borrow_endings"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "loop_carried_borrow_liveness"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "conditional_borrow_state_join"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "reborrows_and_two_phase_access"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "place_projection_alias_analysis"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "interprocedural_lifetime_contracts"));
    assert!(good.rules.iter().any(|rule| rule.id == "field_move_paths"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "borrow_return_provenance"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "conditional_borrow_return_join"));
    assert!(good
        .rules
        .iter()
        .any(|rule| rule.id == "lexical_borrow_alias_locks"));
    assert!(good
        .regions
        .iter()
        .any(|region| region.function == "@function.mix_samples"
            && region.root == "dst"
            && region.mutable));
    assert!(good.regions.iter().any(|region| region
        .uses
        .iter()
        .any(|borrow_use| borrow_use.kind == "index_write" && borrow_use.root == "dst")));

    let path = std::env::temp_dir().join(format!(
        "forgentex_borrow_report_{}.fgx",
        std::process::id()
    ));
    fs::write(
        &path,
        "fn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\nfn bad(xs: &mut [u8]) -> void:\n    let alias: &mut [u8] = xs\n    fill(xs, alias)\n",
    )
    .unwrap();
    let bad = analyze_borrows(&path).unwrap();
    let json = bad.to_json();

    assert_eq!(bad.status, "fail");
    assert!(json.contains("\"schema\": \"forgentex.borrow_report.v1\""));
    assert!(json.contains("\"error\": \"mutable_borrow_alias\""));
    assert!(json.contains("\"id\": \"conditional_move_merge\""));

    let _ = fs::remove_file(path);
}

#[test]
fn borrow_report_emits_lifetime_region_graph_evidence() {
    let path = std::env::temp_dir().join(format!(
        "forgentex_region_report_{}.fgx",
        std::process::id()
    ));
    fs::write(
        &path,
        "fn demo(xs: &mut [u8], ys: &[u8]) -> void:\n    let alias: &mut [u8] = xs\n    alias[0] = ys[0]\n    let view: &[u8] = ys\n    return\n",
    )
    .unwrap();
    let report = analyze_borrows(&path).unwrap();
    let json = report.to_json();

    assert_eq!(report.status, "pass");
    let xs_region = report
        .regions
        .iter()
        .find(|region| region.function == "@function.demo" && region.root == "xs")
        .unwrap();
    assert!(xs_region.mutable);
    assert!(xs_region
        .loans
        .iter()
        .any(|loan| loan.binding == "xs" && loan.origin == "parameter"));
    assert!(xs_region
        .loans
        .iter()
        .any(|loan| loan.binding == "alias" && loan.origin == "let"));
    assert!(xs_region
        .uses
        .iter()
        .any(|borrow_use| borrow_use.kind == "index_write" && borrow_use.place == "alias"));

    let ys_region = report
        .regions
        .iter()
        .find(|region| region.function == "@function.demo" && region.root == "ys")
        .unwrap();
    assert!(!ys_region.mutable);
    assert!(ys_region
        .loans
        .iter()
        .any(|loan| loan.binding == "view" && loan.source == "ys"));
    assert!(ys_region
        .uses
        .iter()
        .any(|borrow_use| borrow_use.kind == "index_read" && borrow_use.place == "ys"));

    assert!(json.contains("\"regions\": ["));
    assert!(json.contains("\"id\": \"lifetime_region_graph\""));
    assert!(json.contains("\"binding\": \"alias\""));
    assert!(json.contains("\"kind\": \"index_write\""));

    let _ = fs::remove_file(path);
}

#[test]
fn borrow_report_marks_reborrow_loans() {
    let path = std::env::temp_dir().join(format!(
        "forgentex_reborrow_report_{}.fgx",
        std::process::id()
    ));
    fs::write(
        &path,
        "fn demo(xs: &mut [u8]) -> void:\n    let alias: &mut [u8] = xs\n    let child: &mut [u8] = alias\n    child[0] = 1\n    alias[0] = 2\n",
    )
    .unwrap();
    let report = analyze_borrows(&path).unwrap();
    let json = report.to_json();

    assert_eq!(report.status, "pass");
    let region = report
        .regions
        .iter()
        .find(|region| region.function == "@function.demo" && region.root == "xs")
        .unwrap();
    assert!(region
        .loans
        .iter()
        .any(|loan| loan.binding == "child" && loan.origin == "reborrow"));
    assert!(json.contains("\"id\": \"reborrows_and_two_phase_access\""));
    assert!(json.contains("\"origin\": \"reborrow\""));

    let _ = fs::remove_file(path);
}

#[test]
fn borrow_report_lists_place_projection_alias_rule() {
    let path = std::env::temp_dir().join(format!(
        "forgentex_projection_report_{}.fgx",
        std::process::id()
    ));
    fs::write(
        &path,
        "struct Pair:\n    left: &mut [u8]\n    right: &mut [u8]\n\nfn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\nfn ok(pair: Pair) -> void:\n    fill(pair.left, pair.right)\n",
    )
    .unwrap();
    let report = analyze_borrows(&path).unwrap();
    let json = report.to_json();

    assert!(report
        .rules
        .iter()
        .any(|rule| rule.id == "place_projection_alias_analysis"));
    assert!(json.contains("\"id\": \"place_projection_alias_analysis\""));

    let _ = fs::remove_file(path);
}

#[test]
fn borrow_report_emits_interprocedural_lifetime_contracts() {
    let path = std::env::temp_dir().join(format!(
        "forgentex_lifetime_contract_report_{}.fgx",
        std::process::id()
    ));
    fs::write(
        &path,
        "fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:\n    if flag:\n        return xs\n    return ys\n\nfn write(xs: &mut [u8]) -> void:\n    xs[0] = 1\n\nfn wrapper(xs: &mut [u8]) -> void:\n    write(xs)\n",
    )
    .unwrap();
    let report = analyze_borrows(&path).unwrap();
    let json = report.to_json();

    assert!(report
        .rules
        .iter()
        .any(|rule| rule.id == "interprocedural_lifetime_contracts"));
    let choose = report
        .lifetime_contracts
        .iter()
        .find(|contract| contract.function == "@function.choose")
        .unwrap();
    assert_eq!(
        choose
            .returns
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["xs", "ys"]
    );
    let wrapper = report
        .lifetime_contracts
        .iter()
        .find(|contract| contract.function == "@function.wrapper")
        .unwrap();
    assert_eq!(wrapper.mutates.len(), 1);
    assert_eq!(wrapper.mutates[0].name, "xs");
    assert!(json.contains("\"id\": \"interprocedural_lifetime_contracts\""));
    assert!(json.contains("\"lifetime_contracts\": ["));
    assert!(json.contains("\"function\": \"@function.choose\""));
    assert!(json.contains("\"mutates\": ["));

    let _ = fs::remove_file(path);
}
