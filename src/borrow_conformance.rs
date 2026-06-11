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
    pub cases: Vec<BorrowConformanceCase>,
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
    let cases = conformance_cases()
        .into_iter()
        .map(run_case)
        .collect::<Vec<_>>();
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
        cases,
    }
}

impl BorrowConformanceReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex borrow conformance suite\n");
        out.push_str(&format!(
            "schema={} status={} total={} passed={} failed={}\n",
            self.schema, self.status, self.total, self.passed, self.failed
        ));
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
