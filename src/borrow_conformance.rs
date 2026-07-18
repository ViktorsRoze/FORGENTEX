use std::collections::BTreeSet;
use std::path::Path;

use crate::lifetimes::analyze_contracts;
use crate::parser::parse_file;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowConformanceReport {
    pub schema: String,
    pub status: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub reference_model: BorrowReferenceSummary,
    pub cases: Vec<BorrowConformanceCase>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowReferenceSummary {
    pub schema: String,
    pub scope: String,
    pub generated: usize,
    pub move_generated: usize,
    pub alias_generated: usize,
    pub lifetime_generated: usize,
    pub property_generated: usize,
    pub persisted_regression_generated: usize,
    pub campaign_seed: String,
    pub campaign_cases_per_family: usize,
    pub campaign_generated: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub move_mismatched: usize,
    pub alias_mismatched: usize,
    pub lifetime_mismatched: usize,
    pub property_mismatched: usize,
    pub persisted_regression_mismatched: usize,
    pub campaign_mismatched: usize,
    pub campaign_failure_artifacts: Vec<BorrowCampaignFailureArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowCampaignFailureArtifact {
    pub case_id: String,
    pub replay: String,
    pub original_complexity: usize,
    pub minimized_complexity: usize,
    pub minimized_source: String,
    pub expected_diagnostics: Vec<String>,
    pub found_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowConformanceCase {
    pub id: String,
    pub category: String,
    pub description: String,
    pub expected_status: String,
    pub actual_status: String,
    pub expected_diagnostics: Vec<String>,
    pub found_diagnostics: Vec<String>,
    pub expected_returns: Vec<String>,
    pub found_returns: Vec<String>,
    pub expected_mutates: Vec<String>,
    pub found_mutates: Vec<String>,
    pub passed: bool,
}

pub fn run_borrow_conformance() -> BorrowConformanceReport {
    run_borrow_conformance_with_campaign(
        crate::borrow_reference::DEFAULT_SAFETY_CAMPAIGN_SEED,
        crate::borrow_reference::DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY,
    )
}

pub fn run_borrow_conformance_with_campaign(
    campaign_seed: u64,
    campaign_cases_per_family: usize,
) -> BorrowConformanceReport {
    let mut cases = conformance_cases()
        .into_iter()
        .map(run_case)
        .collect::<Vec<_>>();
    let reference_start = cases.len();
    let persisted_regression_specs = persisted_regression_cases();
    let persisted_regression_generated = persisted_regression_specs.len();
    cases.extend(persisted_regression_specs.into_iter().map(|case| {
        run_diagnostic_case(
            case.id,
            "persisted_regressions",
            case.description,
            case.source,
            case.expected_diagnostics,
        )
    }));
    let move_start = cases.len();
    let move_generated = crate::borrow_reference::generated_move_cases().len();
    cases.extend(
        crate::borrow_reference::generated_move_cases()
            .into_iter()
            .map(|case| {
                let expected = if case.expects_use_after_move {
                    &["use_after_move"][..]
                } else {
                    &[][..]
                };
                run_diagnostic_case(
                    &case.id,
                    "reference_moves",
                    &case.description,
                    &case.source,
                    expected,
                )
            }),
    );
    let alias_start = cases.len();
    let alias_generated = crate::borrow_reference::generated_alias_cases().len();
    cases.extend(
        crate::borrow_reference::generated_alias_cases()
            .into_iter()
            .map(|case| {
                let expected = if case.expects_mutable_borrow_alias {
                    &["mutable_borrow_alias"][..]
                } else {
                    &[][..]
                };
                run_diagnostic_case(
                    &case.id,
                    "reference_aliases",
                    &case.description,
                    &case.source,
                    expected,
                )
            }),
    );
    let lifetime_start = cases.len();
    let lifetime_generated = crate::borrow_reference::generated_lifetime_cases().len();
    cases.extend(
        crate::borrow_reference::generated_lifetime_cases()
            .into_iter()
            .map(|case| {
                let expected = if case.expects_escaping_reference {
                    &["escaping_reference"][..]
                } else {
                    &[][..]
                };
                run_diagnostic_case(
                    &case.id,
                    "reference_lifetimes",
                    &case.description,
                    &case.source,
                    expected,
                )
            }),
    );
    let property_start = cases.len();
    let property_generated = crate::borrow_reference::generated_safety_property_cases().len();
    cases.extend(
        crate::borrow_reference::generated_safety_property_cases()
            .into_iter()
            .map(|case| {
                let expected = case.expected_diagnostic.into_iter().collect::<Vec<_>>();
                run_diagnostic_case(
                    &case.id,
                    "reference_properties",
                    &case.description,
                    &case.source,
                    &expected,
                )
            }),
    );
    let campaign_start = cases.len();
    let campaign_specs = crate::borrow_reference::generated_safety_campaign(
        campaign_seed,
        campaign_cases_per_family,
    );
    let campaign_generated = campaign_specs.len();
    let mut campaign_failure_artifacts = Vec::new();
    for campaign_case in campaign_specs {
        let (case, artifact) = run_campaign_case(campaign_case);
        cases.push(case);
        if let Some(artifact) = artifact {
            campaign_failure_artifacts.push(artifact);
        }
    }
    let reference_cases = &cases[reference_start..];
    let persisted_regression_cases = &cases[reference_start..move_start];
    let move_cases = &cases[move_start..alias_start];
    let alias_cases = &cases[alias_start..lifetime_start];
    let lifetime_cases = &cases[lifetime_start..property_start];
    let property_cases = &cases[property_start..campaign_start];
    let campaign_cases = &cases[campaign_start..];
    let reference_model = BorrowReferenceSummary {
        schema: "forgentex.borrow_reference.v1".to_string(),
        scope: "persisted minimized regression fixtures; owned Pair root/field move availability across linear flow, branch joins, and two-iteration loops; mutable alias/reborrow liveness across descendant loans, projections, joins, and loops; parameter/unproven lifetime provenance across aliases, control flow, selective helper contracts, transitive wrappers, field/index projections, and recursive cycles; seeded long-form properties; runtime-seeded move, alias, projection, and lifetime campaigns with mismatch shrinking and replay artifacts"
            .to_string(),
        generated: reference_cases.len(),
        move_generated,
        alias_generated,
        lifetime_generated,
        property_generated,
        persisted_regression_generated,
        campaign_seed: format!("0x{campaign_seed:016x}"),
        campaign_cases_per_family,
        campaign_generated,
        matched: reference_cases.iter().filter(|case| case.passed).count(),
        mismatched: reference_cases.iter().filter(|case| !case.passed).count(),
        move_mismatched: move_cases.iter().filter(|case| !case.passed).count(),
        alias_mismatched: alias_cases.iter().filter(|case| !case.passed).count(),
        lifetime_mismatched: lifetime_cases.iter().filter(|case| !case.passed).count(),
        property_mismatched: property_cases.iter().filter(|case| !case.passed).count(),
        persisted_regression_mismatched: persisted_regression_cases
            .iter()
            .filter(|case| !case.passed)
            .count(),
        campaign_mismatched: campaign_cases.iter().filter(|case| !case.passed).count(),
        campaign_failure_artifacts,
    };
    let passed = cases.iter().filter(|case| case.passed).count();
    let total = cases.len();
    let failed = total.saturating_sub(passed);
    let status = if failed == 0 { "pass" } else { "fail" };
    BorrowConformanceReport {
        schema: "forgentex.borrow_conformance.v1".to_string(),
        status: status.to_string(),
        total,
        passed,
        failed,
        reference_model,
        cases,
    }
}

impl BorrowConformanceReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn persist_campaign_failures(&self, directory: &Path) -> Result<usize, String> {
        let artifacts = &self.reference_model.campaign_failure_artifacts;
        if artifacts.is_empty() {
            return Ok(0);
        }
        std::fs::create_dir_all(directory).map_err(|error| {
            format!(
                "failed to create borrow campaign failure directory {}: {error}",
                directory.display()
            )
        })?;
        for artifact in artifacts {
            let stem = safe_artifact_stem(&artifact.case_id);
            let source_path = directory.join(format!("{stem}.fgx"));
            let metadata_path = directory.join(format!("{stem}.json"));
            std::fs::write(&source_path, &artifact.minimized_source).map_err(|error| {
                format!(
                    "failed to write borrow campaign source {}: {error}",
                    source_path.display()
                )
            })?;
            std::fs::write(&metadata_path, campaign_failure_artifact_json(artifact)).map_err(
                |error| {
                    format!(
                        "failed to write borrow campaign metadata {}: {error}",
                        metadata_path.display()
                    )
                },
            )?;
        }
        Ok(artifacts.len())
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex borrow conformance suite\n");
        out.push_str(&format!(
            "schema={} status={} total={} passed={} failed={}\n",
            self.schema, self.status, self.total, self.passed, self.failed
        ));
        out.push_str(&format!(
            "reference_schema={} generated={} matched={} mismatched={} scope={}\n",
            self.reference_model.schema,
            self.reference_model.generated,
            self.reference_model.matched,
            self.reference_model.mismatched,
            self.reference_model.scope
        ));
        out.push_str(&format!(
            "persisted_regressions={} mismatched={}\n",
            self.reference_model.persisted_regression_generated,
            self.reference_model.persisted_regression_mismatched
        ));
        out.push_str(&format!(
            "campaign_seed={} cases_per_family={} generated={} mismatched={} failure_artifacts={}\n",
            self.reference_model.campaign_seed,
            self.reference_model.campaign_cases_per_family,
            self.reference_model.campaign_generated,
            self.reference_model.campaign_mismatched,
            self.reference_model.campaign_failure_artifacts.len()
        ));
        for artifact in &self.reference_model.campaign_failure_artifacts {
            out.push_str(&format!(
                "  replay={} original_complexity={} minimized_complexity={}\n",
                artifact.replay, artifact.original_complexity, artifact.minimized_complexity
            ));
            out.push_str(&artifact.minimized_source);
            if !artifact.minimized_source.ends_with('\n') {
                out.push('\n');
            }
        }
        for case in &self.cases {
            out.push_str(&format!(
                "  - {} category={} expected={} actual={} passed={} diagnostics=[{}]",
                case.id,
                case.category,
                case.expected_status,
                case.actual_status,
                case.passed,
                case.found_diagnostics.join(",")
            ));
            if !case.found_returns.is_empty() || !case.found_mutates.is_empty() {
                out.push_str(&format!(
                    " returns=[{}] mutates=[{}]",
                    case.found_returns.join(","),
                    case.found_mutates.join(",")
                ));
            }
            out.push('\n');
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_prop(&mut out, 1, "total", &self.total.to_string(), true);
        push_prop(&mut out, 1, "passed", &self.passed.to_string(), true);
        push_prop(&mut out, 1, "failed", &self.failed.to_string(), true);
        push_line(&mut out, 1, "\"reference_model\": {");
        push_prop(
            &mut out,
            2,
            "schema",
            &quoted(&self.reference_model.schema),
            true,
        );
        push_prop(
            &mut out,
            2,
            "persisted_regression_generated",
            &self
                .reference_model
                .persisted_regression_generated
                .to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "scope",
            &quoted(&self.reference_model.scope),
            true,
        );
        push_prop(
            &mut out,
            2,
            "persisted_regression_mismatched",
            &self
                .reference_model
                .persisted_regression_mismatched
                .to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "generated",
            &self.reference_model.generated.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "move_generated",
            &self.reference_model.move_generated.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "alias_generated",
            &self.reference_model.alias_generated.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "lifetime_generated",
            &self.reference_model.lifetime_generated.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "property_generated",
            &self.reference_model.property_generated.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "campaign_seed",
            &quoted(&self.reference_model.campaign_seed),
            true,
        );
        push_prop(
            &mut out,
            2,
            "campaign_cases_per_family",
            &self.reference_model.campaign_cases_per_family.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "campaign_generated",
            &self.reference_model.campaign_generated.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "matched",
            &self.reference_model.matched.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "mismatched",
            &self.reference_model.mismatched.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "move_mismatched",
            &self.reference_model.move_mismatched.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "alias_mismatched",
            &self.reference_model.alias_mismatched.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "lifetime_mismatched",
            &self.reference_model.lifetime_mismatched.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "property_mismatched",
            &self.reference_model.property_mismatched.to_string(),
            true,
        );
        push_prop(
            &mut out,
            2,
            "campaign_mismatched",
            &self.reference_model.campaign_mismatched.to_string(),
            true,
        );
        push_line(&mut out, 2, "\"campaign_failure_artifacts\": [");
        for (index, artifact) in self
            .reference_model
            .campaign_failure_artifacts
            .iter()
            .enumerate()
        {
            if index > 0 {
                out.push_str(",\n");
            }
            push_campaign_failure_artifact(&mut out, artifact);
        }
        if !self.reference_model.campaign_failure_artifacts.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 2, "]");
        push_line(&mut out, 1, "},");
        push_line(&mut out, 1, "\"cases\": [");
        for (idx, case) in self.cases.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_case(&mut out, case);
        }
        if !self.cases.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "]");
        out.push('}');
        out
    }
}

struct CaseSpec {
    id: &'static str,
    category: &'static str,
    description: &'static str,
    source: &'static str,
    check: CaseCheck,
}

struct PersistedRegressionSpec {
    id: &'static str,
    description: &'static str,
    source: &'static str,
    expected_diagnostics: &'static [&'static str],
}

fn persisted_regression_cases() -> Vec<PersistedRegressionSpec> {
    vec![
        PersistedRegressionSpec {
            id: "regression.move_after_whole_move",
            description: "persisted minimal whole-value move regression",
            source: include_str!("../tests/borrow_regressions/move_after_whole_move.fgx"),
            expected_diagnostics: &["use_after_move"],
        },
        PersistedRegressionSpec {
            id: "regression.loop_parent_child_conflict",
            description: "persisted loop-carried parent/child loan regression",
            source: include_str!("../tests/borrow_regressions/loop_parent_child_conflict.fgx"),
            expected_diagnostics: &["mutable_borrow_alias"],
        },
        PersistedRegressionSpec {
            id: "regression.unknown_index_overlap",
            description: "persisted conservative unknown-index overlap regression",
            source: include_str!("../tests/borrow_regressions/unknown_index_overlap.fgx"),
            expected_diagnostics: &["mutable_borrow_alias"],
        },
        PersistedRegressionSpec {
            id: "regression.field_return_escape",
            description: "persisted unproven field-return provenance regression",
            source: include_str!("../tests/borrow_regressions/field_return_escape.fgx"),
            expected_diagnostics: &["escaping_reference"],
        },
    ]
}

enum CaseCheck {
    Diagnostics {
        expected_diagnostics: &'static [&'static str],
    },
    LifetimeContract {
        function: &'static str,
        expected_returns: &'static [&'static str],
        expected_mutates: &'static [&'static str],
    },
}

fn conformance_cases() -> Vec<CaseSpec> {
    vec![
        diagnostic_case(
            "moves.field_sibling_ok",
            "moves",
            "moving one owned struct field preserves unread sibling fields",
            "\
struct Packet:
    value: u8

struct Pair:
    left: Packet
    right: Packet

fn ok(pair: Pair) -> u8:
    let left: Packet = pair.left
    return pair.right.value
",
            &[],
        ),
        diagnostic_case(
            "moves.use_after_move",
            "moves",
            "reading an owned value after move is rejected",
            "\
struct Packet:
    value: u8

fn bad(packet: Packet) -> u8:
    let moved: Packet = packet
    return packet.value
",
            &["use_after_move"],
        ),
        diagnostic_case(
            "borrows.reborrow_child_then_parent_ok",
            "reborrows",
            "parent mutable borrow becomes usable after child reborrow's last use",
            "\
fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    child[0] = 1
    alias[0] = 2
",
            &[],
        ),
        diagnostic_case(
            "borrows.parent_before_child_rejected",
            "reborrows",
            "parent mutable borrow cannot activate while child reborrow remains live",
            "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    alias[0] = 1
    child[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "borrows.mutable_call_alias_rejected",
            "borrows",
            "mutable call arguments must not alias the same borrow root",
            "\
fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    fill(xs, alias)
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "borrows.live_alias_blocks_write",
            "borrows",
            "a live mutable alias blocks writes through another handle to the same root",
            "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = 1
    alias[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "loops.loop_carried_alias_rejected",
            "loops",
            "outer borrow aliases remain live across loop back-edges",
            "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    for i in 0 .. 2:
        alias[0] = 1
        xs[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "loops.loop_local_alias_ok",
            "loops",
            "loop-local borrow aliases can end before a later root write in the same iteration",
            "\
fn ok(xs: &mut [u8]) -> void:
    for i in 0 .. 2:
        let alias: &mut [u8] = xs
        alias[0] = 1
        xs[0] = 2
",
            &[],
        ),
        diagnostic_case(
            "branches.conditional_alias_reassign_rejected",
            "branches",
            "conditional borrow alias reassignment joins possible roots",
            "\
fn bad(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    xs[0] = 1
    alias[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "branches.conditional_alias_call_rejected",
            "branches",
            "conditional borrow alias roots are checked at call boundaries",
            "\
fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    fill(xs, alias)
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "branches.conditional_alias_last_use_ok",
            "branches",
            "conditional borrow alias joins still allow root writes after the alias last use",
            "\
fn ok(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    alias[0] = 1
    xs[0] = 2
",
            &[],
        ),
        diagnostic_case(
            "lifetimes.conditional_return_ok",
            "lifetimes",
            "conditional borrow returns preserve multiple parameter origins",
            "\
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys

fn caller(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    return choose(flag, xs, ys)
",
            &[],
        ),
        diagnostic_case(
            "lifetimes.unproven_field_return_rejected",
            "lifetimes",
            "returned borrows from unproven field projections are rejected",
            "\
struct Holder:
    view: &[u8]

fn bad(holder: Holder) -> &[u8]:
    return holder.view
",
            &["escaping_reference"],
        ),
        diagnostic_case(
            "lifetimes.unproven_conditional_arg_rejected",
            "lifetimes",
            "conditional helper returns reject any unproven returned argument",
            "\
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys

fn bad(flag: bool, xs: &[u8]) -> &[u8]:
    let local: &[u8] = unknown_borrow(xs)
    return choose(flag, xs, local)
",
            &["escaping_reference"],
        ),
        diagnostic_case(
            "projections.disjoint_fields_ok",
            "projections",
            "disjoint field projections may be mutably borrowed together",
            "\
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn ok(pair: Pair) -> void:
    fill(pair.left, pair.right)
",
            &[],
        ),
        diagnostic_case(
            "projections.overlapping_field_alias_rejected",
            "projections",
            "overlapping field projections are still treated as aliases",
            "\
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(pair: Pair) -> void:
    fill(pair.left, pair.left)
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "projections.constant_indices_disjoint_ok",
            "projections",
            "constant index projections can prove obvious disjointness",
            "\
struct Slots:
    items: [&mut [u8]; 2]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn ok(slots: Slots) -> void:
    fill(slots.items[0], slots.items[1])
",
            &[],
        ),
        diagnostic_case(
            "projections.unknown_index_overlap_rejected",
            "projections",
            "unknown index projections stay conservative at mutable call boundaries",
            "\
struct Slots:
    items: [&mut [u8]; 2]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(slots: Slots, i: usize) -> void:
    fill(slots.items[i], slots.items[i])
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "contracts.read_only_mut_helper_ok",
            "contracts",
            "read-only helpers accepting mutable borrows do not activate parent writes",
            "\
fn view(xs: &mut [u8]) -> u8:
    return xs[0]

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    let value: u8 = view(alias)
    child[0] = value
",
            &[],
        ),
        diagnostic_case(
            "contracts.mutating_wrapper_conflict",
            "contracts",
            "mutating callee contracts propagate through wrapper calls",
            "\
fn write(xs: &mut [u8]) -> void:
    xs[0] = 1

fn wrapper(xs: &mut [u8]) -> void:
    write(xs)

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    wrapper(alias)
    child[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        contract_case(
            "contracts.report_choose_returns",
            "contracts",
            "lifetime contract summaries record multi-parameter return provenance",
            "\
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys
",
            "@function.choose",
            &["xs", "ys"],
            &[],
        ),
        contract_case(
            "contracts.report_wrapper_mutates",
            "contracts",
            "lifetime contract summaries record transitive mutable parameters",
            "\
fn write(xs: &mut [u8]) -> void:
    xs[0] = 1

fn wrapper(xs: &mut [u8]) -> void:
    write(xs)
",
            "@function.wrapper",
            &[],
            &["xs"],
        ),
        diagnostic_case(
            "dataflow.loop_remove_rejected",
            "dataflow",
            "moving the same owned value on every loop iteration is rejected across the back edge",
            "\
struct Packet:
    value: u8

fn bad(packet: Packet) -> void:
    for i in 0 .. 2:
        let moved: Packet = packet
",
            &["use_after_move"],
        ),
        diagnostic_case(
            "dataflow.joined_alias_reborrow_rejected",
            "dataflow",
            "reborrows of a conditionally-joined alias keep every possible root",
            "\
fn bad(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    let child: &mut [u8] = alias
    xs[0] = 1
    child[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "dataflow.nested_loop_alias_rejected",
            "dataflow",
            "outer borrow aliases stay live across nested loop back edges",
            "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    for i in 0 .. 2:
        for j in 0 .. 2:
            xs[0] = 1
        alias[0] = 2
",
            &["mutable_borrow_alias"],
        ),
        diagnostic_case(
            "dataflow.branch_write_after_last_use_ok",
            "dataflow",
            "branch-local root writes after the alias last use stay allowed",
            "\
fn ok(flag: bool, xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = 1
    if flag:
        xs[0] = 2
",
            &[],
        ),
        diagnostic_case(
            "dataflow.joined_alias_last_use_ok",
            "dataflow",
            "joined aliases end at their last use before later root writes",
            "\
fn ok(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    alias[0] = 1
    ys[0] = 2
",
            &[],
        ),
        diagnostic_case(
            "raw.raw_pointer_deref_safe_fn_rejected",
            "raw",
            "raw pointer indexing requires an unsafe function boundary",
            "\
fn bad(ptr: *const u8) -> u8:
    return ptr[0]
",
            &[
                "raw_pointer_dereference_requires_unsafe",
                "raw_pointer_requires_unsafe",
            ],
        ),
        diagnostic_case(
            "raw.raw_pointer_param_safe_fn_rejected",
            "raw",
            "raw pointer parameters require an unsafe function boundary",
            "\
fn bad(ptr: *const u8) -> void:
    return
",
            &["raw_pointer_requires_unsafe"],
        ),
        diagnostic_case(
            "raw.raw_pointer_return_safe_fn_rejected",
            "raw",
            "returning a raw pointer requires an unsafe function boundary",
            "\
fn bad(ptr: *const u8) -> *const u8:
    return ptr
",
            &["raw_pointer_requires_unsafe"],
        ),
    ]
}

fn diagnostic_case(
    id: &'static str,
    category: &'static str,
    description: &'static str,
    source: &'static str,
    expected_diagnostics: &'static [&'static str],
) -> CaseSpec {
    CaseSpec {
        id,
        category,
        description,
        source,
        check: CaseCheck::Diagnostics {
            expected_diagnostics,
        },
    }
}

fn contract_case(
    id: &'static str,
    category: &'static str,
    description: &'static str,
    source: &'static str,
    function: &'static str,
    expected_returns: &'static [&'static str],
    expected_mutates: &'static [&'static str],
) -> CaseSpec {
    CaseSpec {
        id,
        category,
        description,
        source,
        check: CaseCheck::LifetimeContract {
            function,
            expected_returns,
            expected_mutates,
        },
    }
}

fn run_case(spec: CaseSpec) -> BorrowConformanceCase {
    match spec.check {
        CaseCheck::Diagnostics {
            expected_diagnostics,
        } => run_diagnostic_case(
            spec.id,
            spec.category,
            spec.description,
            spec.source,
            expected_diagnostics,
        ),
        CaseCheck::LifetimeContract {
            function,
            expected_returns,
            expected_mutates,
        } => run_contract_case(
            spec.id,
            spec.category,
            spec.description,
            spec.source,
            function,
            expected_returns,
            expected_mutates,
        ),
    }
}

fn run_campaign_case(
    case: crate::borrow_reference::ReferenceCampaignCase,
) -> (BorrowConformanceCase, Option<BorrowCampaignFailureArtifact>) {
    let expected = case.expected_diagnostic.into_iter().collect::<Vec<_>>();
    let result = run_diagnostic_case(
        &case.id,
        "reference_campaign",
        &case.description,
        &case.source,
        &expected,
    );
    if result.passed {
        return (result, None);
    }

    let mut minimized_complexity = case.complexity;
    let mut minimized_source = case.source.clone();
    for candidate in &case.shrink_candidates {
        let candidate_expected = candidate
            .expected_diagnostic
            .into_iter()
            .collect::<Vec<_>>();
        let candidate_result = run_diagnostic_case(
            &format!("{}.shrink.{}", case.id, candidate.complexity),
            "reference_campaign_shrink",
            &candidate.description,
            &candidate.source,
            &candidate_expected,
        );
        if !candidate_result.passed {
            minimized_complexity = candidate.complexity;
            minimized_source = candidate.source.clone();
            break;
        }
    }
    let artifact = BorrowCampaignFailureArtifact {
        case_id: case.id.clone(),
        replay: case.replay,
        original_complexity: case.complexity,
        minimized_complexity,
        minimized_source,
        expected_diagnostics: result.expected_diagnostics.clone(),
        found_diagnostics: result.found_diagnostics.clone(),
    };
    (result, Some(artifact))
}

fn run_diagnostic_case(
    id: &str,
    category: &str,
    description: &str,
    source: &str,
    expected_diagnostics: &[&str],
) -> BorrowConformanceCase {
    let expected = sorted_strings(expected_diagnostics);
    let found = parse_file(Path::new(&format!("{id}.fgx")), source)
        .map(|program| {
            check(&program)
                .into_iter()
                .filter(|diagnostic| is_conformance_diagnostic(&diagnostic.code))
                .map(|diagnostic| diagnostic.code)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| vec!["parse_error".to_string()]);
    let expected_status = if expected.is_empty() { "pass" } else { "fail" };
    let actual_status = if found.is_empty() { "pass" } else { "fail" };
    let passed = expected == found;
    BorrowConformanceCase {
        id: id.to_string(),
        category: category.to_string(),
        description: description.to_string(),
        expected_status: expected_status.to_string(),
        actual_status: actual_status.to_string(),
        expected_diagnostics: expected,
        found_diagnostics: found,
        expected_returns: Vec::new(),
        found_returns: Vec::new(),
        expected_mutates: Vec::new(),
        found_mutates: Vec::new(),
        passed,
    }
}

fn run_contract_case(
    id: &str,
    category: &str,
    description: &str,
    source: &str,
    function: &str,
    expected_returns: &[&str],
    expected_mutates: &[&str],
) -> BorrowConformanceCase {
    let expected_returns = sorted_strings(expected_returns);
    let expected_mutates = sorted_strings(expected_mutates);
    let (found_returns, found_mutates) = parse_file(Path::new(&format!("{id}.fgx")), source)
        .map(|program| {
            analyze_contracts(&program)
                .into_iter()
                .find(|contract| contract.function == function)
                .map(|contract| {
                    let returns = contract
                        .returns
                        .into_iter()
                        .map(|param| param.name)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    let mutates = contract
                        .mutates
                        .into_iter()
                        .map(|param| param.name)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    (returns, mutates)
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let passed = expected_returns == found_returns && expected_mutates == found_mutates;
    BorrowConformanceCase {
        id: id.to_string(),
        category: category.to_string(),
        description: description.to_string(),
        expected_status: "pass".to_string(),
        actual_status: if passed { "pass" } else { "fail" }.to_string(),
        expected_diagnostics: Vec::new(),
        found_diagnostics: Vec::new(),
        expected_returns,
        found_returns,
        expected_mutates,
        found_mutates,
        passed,
    }
}

fn sorted_strings(values: &[&str]) -> Vec<String> {
    values
        .iter()
        .map(|value| (*value).to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_conformance_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "use_after_move"
            | "mutable_borrow_alias"
            | "escaping_reference"
            | "raw_pointer_requires_unsafe"
            | "raw_pointer_dereference_requires_unsafe"
            | "immutable_write"
    )
}

fn push_campaign_failure_artifact(out: &mut String, artifact: &BorrowCampaignFailureArtifact) {
    push_line(out, 3, "{");
    push_prop(out, 4, "case_id", &quoted(&artifact.case_id), true);
    push_prop(out, 4, "replay", &quoted(&artifact.replay), true);
    push_prop(
        out,
        4,
        "original_complexity",
        &artifact.original_complexity.to_string(),
        true,
    );
    push_prop(
        out,
        4,
        "minimized_complexity",
        &artifact.minimized_complexity.to_string(),
        true,
    );
    push_prop(
        out,
        4,
        "minimized_source",
        &quoted(&artifact.minimized_source),
        true,
    );
    push_string_array(
        out,
        4,
        "expected_diagnostics",
        &artifact.expected_diagnostics,
        true,
    );
    push_string_array(
        out,
        4,
        "found_diagnostics",
        &artifact.found_diagnostics,
        false,
    );
    push_indent(out, 3);
    out.push('}');
}

fn campaign_failure_artifact_json(artifact: &BorrowCampaignFailureArtifact) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    push_prop(
        &mut out,
        1,
        "schema",
        &quoted("forgentex.borrow_regression.v1"),
        true,
    );
    push_prop(&mut out, 1, "case_id", &quoted(&artifact.case_id), true);
    push_prop(&mut out, 1, "replay", &quoted(&artifact.replay), true);
    push_prop(
        &mut out,
        1,
        "original_complexity",
        &artifact.original_complexity.to_string(),
        true,
    );
    push_prop(
        &mut out,
        1,
        "minimized_complexity",
        &artifact.minimized_complexity.to_string(),
        true,
    );
    push_string_array(
        &mut out,
        1,
        "expected_diagnostics",
        &artifact.expected_diagnostics,
        true,
    );
    push_string_array(
        &mut out,
        1,
        "found_diagnostics",
        &artifact.found_diagnostics,
        false,
    );
    out.push('}');
    out
}

fn safe_artifact_stem(case_id: &str) -> String {
    case_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn push_case(out: &mut String, case: &BorrowConformanceCase) {
    push_line(out, 2, "{");
    push_prop(out, 3, "id", &quoted(&case.id), true);
    push_prop(out, 3, "category", &quoted(&case.category), true);
    push_prop(out, 3, "description", &quoted(&case.description), true);
    push_prop(
        out,
        3,
        "expected_status",
        &quoted(&case.expected_status),
        true,
    );
    push_prop(out, 3, "actual_status", &quoted(&case.actual_status), true);
    push_string_array(
        out,
        3,
        "expected_diagnostics",
        &case.expected_diagnostics,
        true,
    );
    push_string_array(out, 3, "found_diagnostics", &case.found_diagnostics, true);
    push_string_array(out, 3, "expected_returns", &case.expected_returns, true);
    push_string_array(out, 3, "found_returns", &case.found_returns, true);
    push_string_array(out, 3, "expected_mutates", &case.expected_mutates, true);
    push_string_array(out, 3, "found_mutates", &case.found_mutates, true);
    push_prop(out, 3, "passed", bool_json(case.passed), false);
    push_indent(out, 2);
    out.push('}');
}

fn push_string_array(
    out: &mut String,
    indent: usize,
    key: &str,
    values: &[String],
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_prop(out: &mut String, indent: usize, key: &str, value: &str, trailing: bool) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": {value}"));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_line(out: &mut String, indent: usize, line: &str) {
    push_indent(out, indent);
    out.push_str(line);
    out.push('\n');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

#[cfg(test)]
mod tests {
    use super::{run_borrow_conformance_with_campaign, run_campaign_case};

    #[test]
    fn campaign_mismatch_emits_a_smaller_replay_artifact() {
        let mut case = crate::borrow_reference::generated_safety_campaign(7, 32)
            .into_iter()
            .find(|case| case.expected_diagnostic.is_some() && !case.shrink_candidates.is_empty())
            .expect("campaign should include a rejected shrinkable case");
        let original_complexity = case.complexity;

        // Simulate a reference/checker disagreement so the failure-only path is
        // covered even while the real reference and production checker agree.
        case.expected_diagnostic = None;
        for candidate in &mut case.shrink_candidates {
            candidate.expected_diagnostic = None;
        }

        let (result, artifact) = run_campaign_case(case);
        assert!(!result.passed);
        let artifact = artifact.expect("mismatch should emit a replay artifact");
        assert!(artifact.replay.contains("--campaign-seed 0x"));
        assert!(artifact.minimized_complexity < original_complexity);
        assert!(!artifact.minimized_source.is_empty());
        assert!(artifact.expected_diagnostics.is_empty());
        assert!(!artifact.found_diagnostics.is_empty());
    }

    #[test]
    fn campaign_failure_artifacts_persist_as_source_and_metadata() {
        let mut case = crate::borrow_reference::generated_safety_campaign(11, 32)
            .into_iter()
            .find(|case| case.expected_diagnostic.is_some() && !case.shrink_candidates.is_empty())
            .expect("campaign should include a rejected shrinkable case");
        case.expected_diagnostic = None;
        for candidate in &mut case.shrink_candidates {
            candidate.expected_diagnostic = None;
        }
        let (_, artifact) = run_campaign_case(case);
        let artifact = artifact.expect("simulated mismatch should produce an artifact");
        let mut report = run_borrow_conformance_with_campaign(11, 1);
        report.reference_model.campaign_failure_artifacts = vec![artifact.clone()];

        let directory = std::env::temp_dir().join(format!(
            "forgentex-borrow-regression-{}-{}",
            std::process::id(),
            artifact.minimized_complexity
        ));
        if directory.exists() {
            std::fs::remove_dir_all(&directory)
                .expect("stale fixture directory should be removable");
        }
        assert_eq!(report.persist_campaign_failures(&directory).unwrap(), 1);
        let stem = super::safe_artifact_stem(&artifact.case_id);
        let source = std::fs::read_to_string(directory.join(format!("{stem}.fgx"))).unwrap();
        let metadata = std::fs::read_to_string(directory.join(format!("{stem}.json"))).unwrap();
        assert_eq!(source, artifact.minimized_source);
        assert!(metadata.contains("\"schema\": \"forgentex.borrow_regression.v1\""));
        assert!(metadata.contains("\"expected_diagnostics\": []"));
        assert!(metadata.contains("\"found_diagnostics\": ["));
        std::fs::remove_dir_all(directory).expect("fixture directory should be removable");
    }
}
