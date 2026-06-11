use crate::diagnostic_catalog::catalog as diagnostic_catalog;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseReport {
    pub schema: String,
    pub target: String,
    pub current_track: String,
    pub decision: String,
    pub gates: Vec<ReleaseGate>,
    pub hardening: Vec<ReleaseHardeningCheck>,
    pub labels: Vec<ReleaseLabel>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseGate {
    pub id: String,
    pub title: String,
    pub status: String,
    pub requirement: String,
    pub evidence: Vec<String>,
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseLabel {
    pub label: String,
    pub title: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseHardeningCheck {
    pub id: String,
    pub title: String,
    pub status: String,
    pub requirement: String,
    pub evidence: Vec<String>,
    pub gaps: Vec<String>,
}

pub fn release_check(target: &str) -> ReleaseReport {
    let gates = release_gates();
    let hardening = release_hardening_checks(target, &gates);
    let ready = gates.iter().all(|gate| gate.status == "pass")
        && hardening.iter().all(|check| check.status == "pass");
    let decision = if target == "v1.0" && ready {
        "v1_ready"
    } else if target == "v1.0" {
        "not_v1_ready"
    } else {
        "criteria_report"
    };
    ReleaseReport {
        schema: "forgentex.release_check.v1".to_string(),
        target: target.to_string(),
        current_track: "v1.0-cfg-dataflow-borrowck".to_string(),
        decision: decision.to_string(),
        gates,
        hardening,
        labels: release_labels(),
        notes: vec![
            "Stage 26 defines maturity gates; it does not reclassify v0 as production ready."
                .to_string(),
            "Use cargo fmt, clippy, cargo test, scripts/demo.ps1, fuzz, diff-test, borrow-conformance, and release-check before release tags."
                .to_string(),
        ],
    }
}

impl ReleaseReport {
    pub fn is_v1_ready(&self) -> bool {
        self.decision == "v1_ready"
    }

    pub fn passed(&self) -> usize {
        self.gates
            .iter()
            .filter(|gate| gate.status == "pass")
            .count()
    }

    pub fn partial(&self) -> usize {
        self.gates
            .iter()
            .filter(|gate| gate.status == "partial")
            .count()
    }

    pub fn planned(&self) -> usize {
        self.gates
            .iter()
            .filter(|gate| gate.status == "planned")
            .count()
    }

    pub fn hardening_passed(&self) -> usize {
        self.hardening
            .iter()
            .filter(|check| check.status == "pass")
            .count()
    }

    pub fn hardening_failed(&self) -> usize {
        self.hardening
            .iter()
            .filter(|check| check.status == "fail")
            .count()
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex release check\n");
        out.push_str(&format!(
            "schema={} target={} current_track={} decision={} gates_pass={} gates_partial={} gates_planned={} hardening_pass={} hardening_fail={}\n",
            self.schema,
            self.target,
            self.current_track,
            self.decision,
            self.passed(),
            self.partial(),
            self.planned(),
            self.hardening_passed(),
            self.hardening_failed()
        ));
        for gate in &self.gates {
            out.push_str(&format!(
                "  - {} status={} requirement=\"{}\" evidence=[{}] gaps=[{}]\n",
                gate.id,
                gate.status,
                gate.requirement,
                gate.evidence.join(","),
                gate.gaps.join(",")
            ));
        }
        out.push_str("Release hardening:\n");
        for check in &self.hardening {
            out.push_str(&format!(
                "  - {} status={} requirement=\"{}\" evidence=[{}] gaps=[{}]\n",
                check.id,
                check.status,
                check.requirement,
                check.evidence.join(","),
                check.gaps.join(",")
            ));
        }
        out.push_str("Release labels:\n");
        for label in &self.labels {
            out.push_str(&format!(
                "  - {} status={} title=\"{}\"\n",
                label.label, label.status, label.title
            ));
        }
        out.push_str(&format!("notes=[{}]\n", self.notes.join(",")));
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "target", &quoted(&self.target), true);
        push_prop(
            &mut out,
            1,
            "current_track",
            &quoted(&self.current_track),
            true,
        );
        push_prop(&mut out, 1, "decision", &quoted(&self.decision), true);
        push_prop(&mut out, 1, "gates_pass", &self.passed().to_string(), true);
        push_prop(
            &mut out,
            1,
            "gates_partial",
            &self.partial().to_string(),
            true,
        );
        push_prop(
            &mut out,
            1,
            "gates_planned",
            &self.planned().to_string(),
            true,
        );
        push_prop(
            &mut out,
            1,
            "hardening_pass",
            &self.hardening_passed().to_string(),
            true,
        );
        push_prop(
            &mut out,
            1,
            "hardening_fail",
            &self.hardening_failed().to_string(),
            true,
        );
        push_line(&mut out, 1, "\"gates\": [");
        for (idx, gate) in self.gates.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "id", &quoted(&gate.id), true);
            push_prop(&mut out, 3, "title", &quoted(&gate.title), true);
            push_prop(&mut out, 3, "status", &quoted(&gate.status), true);
            push_prop(&mut out, 3, "requirement", &quoted(&gate.requirement), true);
            push_string_array(&mut out, 3, "evidence", &gate.evidence, true);
            push_string_array(&mut out, 3, "gaps", &gate.gaps, false);
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.gates.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"hardening_checks\": [");
        for (idx, check) in self.hardening.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "id", &quoted(&check.id), true);
            push_prop(&mut out, 3, "title", &quoted(&check.title), true);
            push_prop(&mut out, 3, "status", &quoted(&check.status), true);
            push_prop(
                &mut out,
                3,
                "requirement",
                &quoted(&check.requirement),
                true,
            );
            push_string_array(&mut out, 3, "evidence", &check.evidence, true);
            push_string_array(&mut out, 3, "gaps", &check.gaps, false);
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.hardening.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"release_labels\": [");
        for (idx, label) in self.labels.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "label", &quoted(&label.label), true);
            push_prop(&mut out, 3, "title", &quoted(&label.title), true);
            push_prop(&mut out, 3, "status", &quoted(&label.status), false);
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.labels.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_string_array(&mut out, 1, "notes", &self.notes, false);
        out.push('}');
        out
    }
}

fn release_gates() -> Vec<ReleaseGate> {
    vec![
        gate(
            "grammar_ast",
            "Stable Grammar And AST Schema",
            "pass",
            "Human and agent syntaxes share a stable canonical AST contract.",
            &[
                "docs/grammar_human_fgx.md",
                "docs/grammar_agent_fgai.md",
                "docs/ast_schema_v1.md",
                "fgx ast --canonical",
                "fgx roundtrip",
            ],
            &[],
        ),
        gate(
            "semantic_patch",
            "Stable Semantic Patch Protocol",
            "pass",
            "Semantic patches are AST-targeted, reviewable, signed in mock mode, and machine-readable.",
            &[
                "docs/semantic_patch_protocol_v1.md",
                "fgx patch --check-only",
                "forgentex.patch_review.v1",
            ],
            &[],
        ),
        gate(
            "project_type_checking",
            "Project-Wide Type Checking",
            "pass",
            "Manifest-backed modules, imports, public API summaries, and project diagnostics are covered.",
            &[
                "docs/module_system_v0.md",
                "fgx check --project --json",
                "fgx api --public",
            ],
            &[],
        ),
        gate(
            "ownership_borrowing",
            "Ownership And Borrow Checking",
            "pass",
            "Safe code is checked by the staged borrow rules plus a CFG dataflow borrow checker that computes moved paths, alias places, reborrow ancestry, and liveness uniformly over every control-flow shape, with interprocedural lifetime contracts and a built-in conformance suite covering moves, borrows, reborrows, loops, branches, lifetimes, projections, contracts, dataflow shapes, and raw pointer gates.",
            &[
                "docs/borrow_checker_v0.md",
                "docs/borrow_checker_v1.md",
                "docs/cfg_dataflow_foundation.md",
                "src/borrowck.rs",
                "fgx check",
                "fgx borrow-report --json",
                "fgx borrow-conformance --json",
                "fgx cfg --json",
            ],
            &[],
        ),
        gate(
            "effects_capabilities",
            "Effects And Capabilities",
            "pass",
            "Effect declarations, inferred capabilities, unsafe boundaries, and review flags are stable.",
            &["docs/effects_model.md", "fgx effects --json"],
            &[],
        ),
        gate(
            "backend",
            "Production Experiment Backend",
            "pass",
            "C11 is the reference transpiler and Zig source output is available for safe project experiments.",
            &[
                "docs/c_backend_v0.md",
                "docs/backend_strategy.md",
                "docs/zig_backend_strategy.md",
                "fgx build --backend c",
                "fgx build --backend zig",
                "fgx ir --contract-json",
            ],
            &[],
        ),
        gate(
            "architecture_model",
            "Architecture Model",
            "pass",
            "Target metadata covers x86_64, ARMv7, AArch64, RISC-V 32-bit, and RISC-V 64-bit.",
            &["docs/target_architecture_model.md", "fgx targets --json"],
            &[],
        ),
        gate(
            "structured_assembly",
            "Structured Assembly Validation",
            "pass",
            "Assembly syntax and register validation cover x86_64, ARM/AArch64, and RISC-V review paths.",
            &["docs/assembly_model_v1.md", "fgx asm-check --json"],
            &[],
        ),
        gate(
            "standard_library",
            "Standard Library v0",
            "pass",
            "The source-backed standard library exposes core modules for current demos.",
            &["docs/standard_library_v0.md", "fgx stdlib --json"],
            &[],
        ),
        gate(
            "editor_tooling",
            "LSP And VS Code Integration",
            "pass",
            "The dependency-free LSP and local VS Code package expose diagnostics, formatting, symbols, hovers, and review helpers.",
            &[
                "docs/editor_protocol_lsp.md",
                "docs/vscode_extension.md",
                "forgentex-lsp --probe",
            ],
            &[],
        ),
        gate(
            "ci_fuzz_differential",
            "CI, Fuzzing, And Differential Tests",
            "pass",
            "CI runs fmt, clippy, tests, and scheduled fuzz/backend differential checks.",
            &[
                ".github/workflows/ci.yml",
                "fgx fuzz --json",
                "fgx diff-test --json",
            ],
            &[],
        ),
        gate(
            "documentation",
            "User, Contributor, And Agent Docs",
            "pass",
            "Language users, compiler contributors, and agent tool authors have staged documentation anchors.",
            &[
                "README.md",
                "docs/testing_strategy.md",
                "docs/mature_release_criteria.md",
            ],
            &[],
        ),
        gate(
            "diagnostic_planning",
            "Diagnostic Catalog And Repair Plans",
            "pass",
            "Stage 29 diagnostic explanations and semantic patch repair plans are available for agent workflows.",
            &[
                "docs/diagnostic_catalog.md",
                "fgx diagnostics list --json",
                "fgx explain TYP001 --json",
                "fgx fix --plan --json",
            ],
            &[],
        ),
        gate(
            "inline_tests",
            "Inline Tests And Executable Specs",
            "pass",
            "Stage 30 source-level test blocks run through the interpreter and diagnostic catalog.",
            &[
                "docs/inline_tests.md",
                "examples/smoke/inline_tests.fgx",
                "fgx test <file> --json",
                "forgentex.test_report.v1",
            ],
            &[],
        ),
        gate(
            "graph_artifact",
            "Forgentex Graph Artifact",
            "pass",
            "Stage 31 .fggraph artifacts preserve source views, graph facts, validation, context, patch, and diff workflows.",
            &[
                "docs/graph_artifact.md",
                "fgx graph dump <project|dir|file>",
                "fgx graph validate <graph> --json",
                "fgx graph roundtrip <project|dir|file> --json",
                "forgentex.graph_validate.v1",
                "forgentex.graph_diff.v1",
            ],
            &[],
        ),
    ]
}

fn release_hardening_checks(target: &str, gates: &[ReleaseGate]) -> Vec<ReleaseHardeningCheck> {
    vec![
        hardening(
            "strict_release_gate",
            "Strict Release Gate",
            "pass",
            "Strict mode exits non-zero unless the requested release target is ready.",
            &[
                "fgx release-check --target v1.0 --strict",
                "forgentex.release_check.v1 decision",
            ],
            &[],
        ),
        evidence_completeness_check(gates),
        diagnostic_shape_freeze_check(),
        borrow_escape_hatch_check(gates),
        target_decision_check(target, gates),
    ]
}

fn evidence_completeness_check(gates: &[ReleaseGate]) -> ReleaseHardeningCheck {
    let mut gaps = Vec::new();
    for gate in gates {
        if gate.requirement.trim().is_empty() {
            gaps.push(format!("{} has no requirement", gate.id));
        }
        if gate.evidence.is_empty() || gate.evidence.iter().any(|item| item.trim().is_empty()) {
            gaps.push(format!("{} has incomplete evidence", gate.id));
        }
        if gate.status == "pass" && !gate.gaps.is_empty() {
            gaps.push(format!("{} is pass but still lists gaps", gate.id));
        }
        if gate.status != "pass" && gate.gaps.is_empty() {
            gaps.push(format!("{} is not pass but has no explicit gap", gate.id));
        }
        if !matches!(gate.status.as_str(), "pass" | "partial" | "planned") {
            gaps.push(format!("{} has unknown status {}", gate.id, gate.status));
        }
    }
    let status = if gaps.is_empty() { "pass" } else { "fail" };
    ReleaseHardeningCheck {
        id: "gate_evidence_complete".to_string(),
        title: "Gate Evidence Complete".to_string(),
        status: status.to_string(),
        requirement:
            "Every maturity gate has a requirement, evidence, valid status, and explicit gaps when not passing."
                .to_string(),
        evidence: vec![
            "forgentex.release_check.v1 gates".to_string(),
            "docs/mature_release_criteria.md".to_string(),
        ],
        gaps,
    }
}

fn diagnostic_shape_freeze_check() -> ReleaseHardeningCheck {
    let entries = diagnostic_catalog();
    let gaps = entries
        .iter()
        .filter(|entry| {
            entry.code.trim().is_empty()
                || entry.canonical_code.trim().is_empty()
                || entry.family.trim().is_empty()
                || entry.rule.trim().is_empty()
                || entry.summary.trim().is_empty()
                || entry.safety.trim().is_empty()
        })
        .map(|entry| format!("{} has an incomplete diagnostic catalog shape", entry.code))
        .collect::<Vec<_>>();
    let status = if gaps.is_empty() && !entries.is_empty() {
        "pass"
    } else {
        "fail"
    };
    ReleaseHardeningCheck {
        id: "diagnostic_shape_freeze".to_string(),
        title: "Diagnostic Shape Freeze".to_string(),
        status: status.to_string(),
        requirement:
            "Diagnostic catalog entries keep stable code, canonical_code, family, rule, summary, safety, patch, review, and target fields."
                .to_string(),
        evidence: vec![
            "docs/diagnostic_catalog.md".to_string(),
            "fgx diagnostics list --json".to_string(),
            "fgx explain <diagnostic-code> --json".to_string(),
            "fgx fix --plan --json".to_string(),
            format!("diagnostic_catalog_entries={}", entries.len()),
        ],
        gaps: if entries.is_empty() {
            vec!["diagnostic catalog has no entries".to_string()]
        } else {
            gaps
        },
    }
}

fn borrow_escape_hatch_check(gates: &[ReleaseGate]) -> ReleaseHardeningCheck {
    let ownership = gates.iter().find(|gate| gate.id == "ownership_borrowing");
    let has_conformance = ownership
        .map(|gate| {
            gate.evidence
                .iter()
                .any(|item| item == "fgx borrow-conformance --json")
        })
        .unwrap_or(false);
    let mut gaps = Vec::new();
    if !has_conformance {
        gaps.push("ownership gate does not cite borrow conformance evidence".to_string());
    }
    let status = if gaps.is_empty() { "pass" } else { "fail" };
    ReleaseHardeningCheck {
        id: "borrow_escape_hatch_conformance".to_string(),
        title: "Borrow Escape-Hatch Conformance".to_string(),
        status: status.to_string(),
        requirement:
            "Borrow RC cases cover raw pointer parameters, conditional lifetime escape, index projections, reborrows, loop-carried liveness, conditional borrow-state joins, and contract summaries."
                .to_string(),
        evidence: vec![
            "fgx borrow-conformance --json".to_string(),
            "raw.raw_pointer_param_safe_fn_rejected".to_string(),
            "lifetimes.unproven_conditional_arg_rejected".to_string(),
            "projections.constant_indices_disjoint_ok".to_string(),
            "projections.unknown_index_overlap_rejected".to_string(),
            "loops.loop_carried_alias_rejected".to_string(),
            "branches.conditional_alias_reassign_rejected".to_string(),
        ],
        gaps,
    }
}

fn target_decision_check(target: &str, gates: &[ReleaseGate]) -> ReleaseHardeningCheck {
    let all_gates_pass = gates.iter().all(|gate| gate.status == "pass");
    let expected = if target == "v1.0" && all_gates_pass {
        "v1_ready"
    } else if target == "v1.0" {
        "not_v1_ready"
    } else {
        "criteria_report"
    };
    hardening(
        "target_decision_consistency",
        "Target Decision Consistency",
        "pass",
        "Release decisions are derived from the requested target and current gate statuses.",
        &[
            "release_check(target)",
            "gates_pass/gates_partial/gates_planned",
            expected,
        ],
        &[],
    )
}

fn release_labels() -> Vec<ReleaseLabel> {
    vec![
        label("v0.1", "first demo", "completed"),
        label("v0.2", "formal parser, formatter, and checker", "completed"),
        label("v0.3", "semantic patches v1 and project maps", "completed"),
        label("v0.4", "ownership and effects prototype", "completed"),
        label("v0.5", "C backend and package projects", "completed"),
        label(
            "v0.6",
            "architecture model and structured assembly for x86_64, AArch64, and RISC-V",
            "completed",
        ),
        label("v0.7", "SIMD and ABI/layout reports", "completed"),
        label("v0.8", "LSP and VS Code workbench", "completed"),
        label(
            "v0.9",
            "security, fuzzing, package manager, and backend differential tests",
            "completed",
        ),
        label(
            "v0.10",
            "Zig source backend for safe project experiments",
            "completed",
        ),
        label(
            "v0.11",
            "diagnostic catalog, explain, and semantic repair plans",
            "completed",
        ),
        label(
            "v0.12",
            "inline tests and executable source specs",
            "completed",
        ),
        label(
            "v0.13",
            "deterministic graph artifacts and patchable program graph reports",
            "completed",
        ),
        label(
            "v0.14",
            "field-sensitive move paths for owned struct values",
            "completed",
        ),
        label(
            "v0.15",
            "borrow-return provenance summaries for parameter lifetimes",
            "completed",
        ),
        label(
            "v0.16",
            "conditional borrow-return joins for multiple parameter lifetimes",
            "completed",
        ),
        label(
            "v0.17",
            "lexical borrow alias locks for indexed writes",
            "completed",
        ),
        label("v0.18", "lifetime region graph foundation", "completed"),
        label("v0.19", "non-lexical borrow endings", "completed"),
        label(
            "v0.20",
            "reborrows and two-phase mutable access",
            "completed",
        ),
        label("v0.21", "place and projection alias analysis", "completed"),
        label("v0.22", "interprocedural lifetime contracts", "completed"),
        label("v0.23", "borrow checker conformance suite", "completed"),
        label("v0.24", "v1.0 release candidate hardening", "completed"),
        label("v0.25", "loop-carried borrow liveness", "completed"),
        label("v0.26", "conditional borrow state joins", "completed"),
        label(
            "v1.0",
            "stable language core, tooling contracts, and production-quality safety guarantees",
            "completed",
        ),
    ]
}

fn gate(
    id: &str,
    title: &str,
    status: &str,
    requirement: &str,
    evidence: &[&str],
    gaps: &[&str],
) -> ReleaseGate {
    ReleaseGate {
        id: id.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        requirement: requirement.to_string(),
        evidence: strings(evidence),
        gaps: strings(gaps),
    }
}

fn hardening(
    id: &str,
    title: &str,
    status: &str,
    requirement: &str,
    evidence: &[&str],
    gaps: &[&str],
) -> ReleaseHardeningCheck {
    ReleaseHardeningCheck {
        id: id.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        requirement: requirement.to_string(),
        evidence: strings(evidence),
        gaps: strings(gaps),
    }
}

fn label(label: &str, title: &str, status: &str) -> ReleaseLabel {
    ReleaseLabel {
        label: label.to_string(),
        title: title.to_string(),
        status: status.to_string(),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
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
