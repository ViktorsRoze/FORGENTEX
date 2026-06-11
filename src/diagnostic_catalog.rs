#![allow(clippy::too_many_arguments)]

use std::fs;
use std::path::Path;

use crate::diag::Diagnostic;
use crate::parser::parse_file_detailed;
use crate::project::check_project;
use crate::simd::check_simd_fallbacks;
use crate::span::{escape, ParseDiagnostic, SourceSpan};
use crate::targets::check_program_target;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticCatalogEntry {
    pub code: &'static str,
    pub canonical_code: &'static str,
    pub family: &'static str,
    pub family_title: &'static str,
    pub rule: &'static str,
    pub summary: &'static str,
    pub safety: &'static str,
    pub suggested_patch_kinds: &'static [&'static str],
    pub review_gates: &'static [&'static str],
    pub target_consequences: &'static [&'static str],
    pub related: &'static [&'static str],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub code: String,
    pub canonical_code: String,
    pub family: String,
    pub rule: String,
    pub node: Option<String>,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub expected: Option<String>,
    pub found: Option<String>,
    pub suggested_patch_kinds: Vec<String>,
    pub fix_safety: String,
    pub review_gates: Vec<String>,
    pub target_consequences: Vec<String>,
    pub related_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairPlan {
    pub id: String,
    pub diagnostics_fixed: Vec<String>,
    pub safety: String,
    pub confidence: String,
    pub operation: String,
    pub patch_preview: String,
    pub review_gates: Vec<String>,
    pub target_impact: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnplannedDiagnostic {
    pub code: String,
    pub node: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixPlanReport {
    pub schema: String,
    pub source: String,
    pub target: Option<String>,
    pub ok: bool,
    pub diagnostics: Vec<DiagnosticRecord>,
    pub plans: Vec<RepairPlan>,
    pub unplanned_diagnostics: Vec<UnplannedDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixPlanOptions {
    pub source: String,
    pub target: Option<String>,
}

pub fn catalog() -> Vec<DiagnosticCatalogEntry> {
    vec![
        entry(
            "PAR001",
            "missing_colon",
            "PAR",
            "parser",
            "block_header_colon",
            "A block header is missing the trailing colon required by .fgx syntax.",
            "local-edit",
            &["insert_colon"],
            &["parse", "roundtrip"],
            &["source-only"],
            &["PAR005"],
        ),
        entry(
            "PAR002",
            "missing_closing_brace",
            "PAR",
            "parser",
            "agent_block_brace",
            "An agent-syntax block is missing a closing brace.",
            "local-edit",
            &["insert_closing_brace"],
            &["parse", "roundtrip"],
            &["source-only"],
            &["PAR005"],
        ),
        entry(
            "PAR003",
            "invalid_token",
            "PAR",
            "parser",
            "valid_token_stream",
            "The lexer or parser found a token outside the supported grammar.",
            "local-edit",
            &["replace_invalid_token"],
            &["parse", "roundtrip"],
            &["source-only"],
            &["PAR006"],
        ),
        entry(
            "PAR004",
            "unknown_item",
            "PAR",
            "parser",
            "top_level_item",
            "The top-level declaration is not a supported Forgentex item.",
            "local-edit",
            &["rewrite_declaration"],
            &["parse", "roundtrip"],
            &["source-only"],
            &["PAR006"],
        ),
        entry(
            "PAR005",
            "unexpected_token",
            "PAR",
            "parser",
            "expected_token",
            "The parser saw a token that does not fit the current production.",
            "local-edit",
            &["insert_expected_token", "rewrite_expression"],
            &["parse", "roundtrip"],
            &["source-only"],
            &["PAR001", "PAR002"],
        ),
        entry(
            "PAR006",
            "parse_error",
            "PAR",
            "parser",
            "v1_grammar",
            "The source does not match the v1 parser grammar.",
            "human-review",
            &["rewrite_syntax"],
            &["parse", "roundtrip", "human-review"],
            &["source-only"],
            &["PAR003", "PAR004"],
        ),
        entry(
            "NAM001",
            "unresolved_import",
            "NAM",
            "names/imports",
            "module_import_resolution",
            "A module imports another module that is not present in the project graph.",
            "project-edit",
            &["add_module_file", "fix_import_name"],
            &["project-check", "map"],
            &["project-graph"],
            &["NAM002", "NAM003"],
        ),
        entry(
            "NAM002",
            "import_cycle",
            "NAM",
            "names/imports",
            "acyclic_import_graph",
            "The project import graph contains a cycle.",
            "project-edit",
            &["remove_or_invert_import"],
            &["project-check", "map", "human-review"],
            &["project-graph"],
            &["NAM001"],
        ),
        entry(
            "NAM003",
            "duplicate_symbol",
            "NAM",
            "names/imports",
            "unique_symbol_namespace",
            "Two modules publish the same kind/name pair.",
            "api-impact",
            &["rename_symbol_or_split_namespace"],
            &["project-check", "api-review", "human-review"],
            &["public-api"],
            &["NAM004"],
        ),
        entry(
            "NAM004",
            "duplicate_module",
            "NAM",
            "names/imports",
            "unique_module_namespace",
            "A module name appears more than once in the same project graph.",
            "project-edit",
            &["rename_module_or_remove_duplicate"],
            &["project-check", "map"],
            &["project-graph"],
            &["NAM003"],
        ),
        entry(
            "NAM005",
            "unknown_symbol",
            "NAM",
            "names/imports",
            "visible_local_or_parameter",
            "A local, parameter, or assignment target is not visible at this program point.",
            "local-edit",
            &["add_local_or_parameter"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["NAM006"],
        ),
        entry(
            "NAM006",
            "unknown_function",
            "NAM",
            "names/imports",
            "call_target_resolution",
            "A function call refers to a function that is not declared in the checked program.",
            "api-impact",
            &["add_function_declaration"],
            &["typecheck", "api-review", "roundtrip"],
            &["public-api"],
            &["NAM005"],
        ),
        entry(
            "TYP001",
            "return_type_mismatch",
            "TYP",
            "type system",
            "return_assignability",
            "A return expression does not match the function return type.",
            "local-edit",
            &[
                "replace_return_type",
                "replace_return_expression",
                "insert_explicit_cast",
            ],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP003", "TYP004", "TYP005"],
        ),
        entry(
            "TYP002",
            "local_type_required",
            "TYP",
            "type system",
            "explicit_local_type",
            "Local bindings must declare their type so agent edits remain reviewable.",
            "local-edit",
            &["add_local_type_annotation"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP007"],
        ),
        entry(
            "TYP003",
            "integer_literal_out_of_range",
            "TYP",
            "type system",
            "integer_width_range",
            "An integer literal is outside the destination integer type range.",
            "local-edit",
            &["replace_literal", "change_destination_type"],
            &["typecheck", "roundtrip"],
            &["all-targets"],
            &["TYP001", "TYP005"],
        ),
        entry(
            "TYP004",
            "arithmetic_type_mismatch",
            "TYP",
            "type system",
            "numeric_operand_compatibility",
            "Arithmetic operands must be numeric and compatible.",
            "local-edit",
            &["insert_explicit_cast", "replace_arithmetic_operand"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP001", "TYP005"],
        ),
        entry(
            "TYP005",
            "argument_type_mismatch",
            "TYP",
            "type system",
            "call_argument_assignability",
            "A call argument is not assignable to the callee parameter type.",
            "local-edit",
            &["insert_explicit_cast", "replace_call_argument"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP001", "TYP004"],
        ),
        entry(
            "TYP006",
            "index_not_integer",
            "TYP",
            "type system",
            "slice_index_integer",
            "Slice, array, and pointer indices must be integer values.",
            "local-edit",
            &["insert_explicit_cast", "replace_index_expression"],
            &["typecheck", "roundtrip"],
            &["bounds-check-policy"],
            &["TYP003"],
        ),
        entry(
            "TYP007",
            "type_mismatch",
            "TYP",
            "type system",
            "assignment_assignability",
            "A let binding or assignment value does not match its declared type.",
            "local-edit",
            &["insert_explicit_cast", "replace_local_type"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP002", "TYP005"],
        ),
        entry(
            "TYP008",
            "constructor_type_mismatch",
            "TYP",
            "type system",
            "constructor_field_assignability",
            "A constructor argument does not match the corresponding field type.",
            "local-edit",
            &["insert_explicit_cast", "replace_constructor_argument"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP003", "TYP005"],
        ),
        entry(
            "TYP009",
            "constructor_arity",
            "TYP",
            "type system",
            "constructor_field_count",
            "A constructor call has the wrong number of field arguments.",
            "local-edit",
            &["fix_constructor_arguments"],
            &["typecheck", "roundtrip"],
            &["all-frontends"],
            &["TYP008"],
        ),
        entry(
            "TYP010",
            "index_type",
            "TYP",
            "type system",
            "indexable_base",
            "Only arrays, slices, and pointers can be indexed.",
            "local-edit",
            &["replace_index_base"],
            &["typecheck", "roundtrip"],
            &["bounds-check-policy"],
            &["TYP006"],
        ),
        entry(
            "TYP011",
            "immutable_write",
            "TYP",
            "type system",
            "mutable_index_write",
            "Indexed writes require a mutable slice or pointer target.",
            "api-impact",
            &["change_parameter_to_mutable_slice"],
            &["typecheck", "api-review", "roundtrip"],
            &["public-api"],
            &["BOR002"],
        ),
        entry(
            "BOR001",
            "use_after_move",
            "BOR",
            "borrow/ownership",
            "moved_value_unavailable",
            "A moved owned value is read again later.",
            "local-edit",
            &["reorder_use_before_move", "pass_by_reference"],
            &["borrow-report", "typecheck", "human-review"],
            &["safe-code"],
            &["BOR002"],
        ),
        entry(
            "BOR002",
            "mutable_borrow_alias",
            "BOR",
            "borrow/ownership",
            "unique_mutable_borrow",
            "A call or indexed write aliases a mutable borrow through the same root value.",
            "human-review",
            &[
                "split_slice_or_copy_input",
                "avoid_aliasing_mutable_argument",
                "reassign_borrow_alias_before_write",
            ],
            &["borrow-report", "typecheck", "human-review"],
            &["safe-code"],
            &["TYP011"],
        ),
        entry(
            "BOR003",
            "escaping_reference",
            "BOR",
            "borrow/ownership",
            "reference_return_origin",
            "A returned reference is not proven to outlive the function.",
            "api-impact",
            &["return_owned_value", "add_lifetime_parameter_later"],
            &["borrow-report", "api-review", "human-review"],
            &["safe-code"],
            &["BOR001"],
        ),
        entry(
            "BOR004",
            "raw_pointer_requires_unsafe",
            "BOR",
            "borrow/ownership",
            "raw_pointer_unsafe_boundary",
            "Raw pointer values require an unsafe function boundary or a safer API type.",
            "human-review",
            &["mark_function_unsafe", "replace_pointer_with_slice"],
            &["unsafe-review", "borrow-report", "human-review"],
            &["unsafe-boundary"],
            &["BOR005", "ASM001"],
        ),
        entry(
            "BOR005",
            "raw_pointer_dereference_requires_unsafe",
            "BOR",
            "borrow/ownership",
            "raw_pointer_deref_unsafe_boundary",
            "Raw pointer indexing is a dereference and requires unsafe code.",
            "human-review",
            &["mark_function_unsafe", "replace_pointer_with_slice"],
            &["unsafe-review", "borrow-report", "human-review"],
            &["unsafe-boundary"],
            &["BOR004"],
        ),
        entry(
            "EFF001",
            "unknown_effect",
            "EFF",
            "effects/capabilities",
            "known_effect_vocabulary",
            "The declared effect is not part of the current effect vocabulary.",
            "local-edit",
            &["replace_effect_name"],
            &["effects", "typecheck", "roundtrip"],
            &["capability-model"],
            &["EFF002"],
        ),
        entry(
            "EFF002",
            "undeclared_effect",
            "EFF",
            "effects/capabilities",
            "inferred_effect_declared",
            "A function performs an effect that is missing from its declared effect list.",
            "capability-change",
            &["add_effect_declaration"],
            &["effects", "typecheck", "human-review"],
            &["capability-model"],
            &["SEC002"],
        ),
        entry(
            "EFF003",
            "unsafe_effect_requires_unsafe",
            "EFF",
            "effects/capabilities",
            "unsafe_effect_boundary",
            "An effect such as raw memory or assembly requires an unsafe function boundary.",
            "human-review",
            &["mark_function_unsafe", "replace_raw_memory_effect"],
            &["unsafe-review", "effects", "human-review"],
            &["unsafe-boundary"],
            &["ASM001", "BOR004"],
        ),
        entry(
            "EFF004",
            "secret_leak",
            "EFF",
            "effects/capabilities",
            "secret_safe_api_boundary",
            "A secret value flows to logging, serialization, copying, or another secret sink.",
            "human-review",
            &[
                "use_approved_crypto_api",
                "mark_function_unsafe_for_secret_export",
            ],
            &["security-review", "effects", "human-review"],
            &["secret-material"],
            &["SEC002"],
        ),
        entry(
            "ASM001",
            "unsafe_required",
            "ASM",
            "assembly",
            "assembly_requires_unsafe",
            "Structured assembly is present in a safe function.",
            "human-review",
            &["mark_function_unsafe"],
            &["unsafe-review", "asm-check", "typecheck", "human-review"],
            &["unsafe-boundary", "architecture-specific"],
            &["EFF003", "BOR004"],
        ),
        entry(
            "ASM002",
            "asm_invalid_register",
            "ASM",
            "assembly",
            "register_belongs_to_dialect",
            "An assembly register is not valid for the selected dialect.",
            "architecture-review",
            &["replace_register_for_target"],
            &["asm-check", "target-check", "human-review"],
            &["architecture-specific"],
            &["ASM003", "TAR002"],
        ),
        entry(
            "ASM003",
            "asm_target_mismatch",
            "ASM",
            "assembly",
            "asm_target_matches_selected_target",
            "The assembly dialect does not match the selected target.",
            "architecture-review",
            &[
                "choose_matching_target",
                "move_assembly_to_matching_arch_path",
            ],
            &["asm-check", "target-check", "human-review"],
            &["architecture-specific"],
            &["TAR002"],
        ),
        entry(
            "ASM004",
            "asm_feature_required",
            "ASM",
            "assembly",
            "asm_feature_declared",
            "An assembly instruction needs an explicit CPU feature effect.",
            "architecture-review",
            &[
                "add_cpu_feature_effect",
                "replace_feature_specific_instruction",
            ],
            &["asm-check", "effects", "human-review"],
            &["architecture-specific"],
            &["TAR004"],
        ),
        entry(
            "ABI001",
            "abi_missing_repr_c",
            "ABI",
            "layout and headers",
            "public_layout_repr",
            "A public ABI-facing struct needs an explicit C representation.",
            "api-impact",
            &["add_repr_c"],
            &["abi", "header", "api-review"],
            &["public-abi"],
            &["ABI002"],
        ),
        entry(
            "ABI002",
            "abi_invalid_alignment",
            "ABI",
            "layout and headers",
            "valid_alignment_attribute",
            "An alignment attribute is invalid for the target ABI model.",
            "api-impact",
            &["replace_alignment"],
            &["abi", "header", "api-review"],
            &["public-abi"],
            &["ABI001"],
        ),
        entry(
            "ABI003",
            "abi_invalid_endian",
            "ABI",
            "layout and headers",
            "valid_endian_attribute",
            "An endian annotation must be little, big, or native.",
            "local-edit",
            &["choose_little_big_or_native"],
            &["abi", "header", "roundtrip"],
            &["public-abi"],
            &["ABI002"],
        ),
        entry(
            "TAR001",
            "unknown_target",
            "TAR",
            "target architecture",
            "known_target_triple",
            "The selected target triple is not in the Forgentex target catalog.",
            "command-choice",
            &["run_fgx_targets"],
            &["targets"],
            &["architecture-specific"],
            &["TAR002"],
        ),
        entry(
            "TAR002",
            "target_arch_mismatch",
            "TAR",
            "target architecture",
            "target_family_compatibility",
            "Target-specific code does not match the selected target family.",
            "architecture-review",
            &["choose_matching_target", "guard_target_specific_path"],
            &["target-check", "human-review"],
            &["architecture-specific"],
            &["ASM003"],
        ),
        entry(
            "TAR003",
            "unknown_target_family",
            "TAR",
            "target architecture",
            "known_target_family",
            "A target family label is not part of the current catalog.",
            "local-edit",
            &["choose_known_target_family", "run_fgx_targets"],
            &["targets", "target-check"],
            &["architecture-specific"],
            &["TAR001"],
        ),
        entry(
            "TAR004",
            "unknown_target_feature",
            "TAR",
            "target architecture",
            "known_target_feature",
            "A target feature label is not known for any supported family.",
            "local-edit",
            &["choose_known_target_feature", "run_fgx_targets"],
            &["targets", "target-check"],
            &["architecture-specific"],
            &["ASM004"],
        ),
        entry(
            "TAR005",
            "target_guard_required",
            "TAR",
            "target architecture",
            "feature_guarded_call",
            "A call to feature-specific code lacks a runtime or compile-time feature guard.",
            "architecture-review",
            &["wrap_call_in_cpu_feature_guard", "add_scalar_fallback"],
            &["target-check", "simd-review", "human-review"],
            &["architecture-specific"],
            &["SIMD001"],
        ),
        entry(
            "SIMD001",
            "simd_fallback_required",
            "PERF",
            "performance budgets",
            "simd_scalar_fallback",
            "SIMD code needs a scalar fallback unless it is explicitly target-only.",
            "architecture-review",
            &["add_scalar_fallback", "mark_target_only"],
            &["simd-review", "perf-report", "human-review"],
            &["architecture-specific", "performance-claim"],
            &["TAR005"],
        ),
        entry(
            "SEC001",
            "undeclared_permission",
            "SEC",
            "security/sandboxing",
            "manifest_permission_for_action",
            "An untrusted package action is not covered by its permission manifest.",
            "manifest-edit",
            &["add_permission_manifest_entry", "deny_action"],
            &["audit", "verify", "security-review"],
            &["supply-chain"],
            &["SEC002"],
        ),
        entry(
            "SEC002",
            "missing_effect_permission",
            "SEC",
            "security/sandboxing",
            "manifest_permission_for_effect",
            "An untrusted package uses an effect without a matching manifest permission.",
            "manifest-edit",
            &["add_permission_manifest_entry", "remove_effect"],
            &["audit", "verify", "security-review"],
            &["supply-chain", "capability-model"],
            &["EFF002"],
        ),
    ]
}

pub fn diagnostic_catalog_json() -> String {
    let mut out = String::new();
    out.push_str("{\n  \"schema\": \"forgentex.diagnostic_catalog.v1\",\n  \"stage\": 29,\n  \"diagnostics\": [");
    for (idx, entry) in catalog().iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('\n');
        push_entry_json(&mut out, entry, 4);
    }
    if !catalog().is_empty() {
        out.push('\n');
    }
    out.push_str("  ]\n}");
    out
}

pub fn diagnostic_catalog_text() -> String {
    let mut out = String::new();
    out.push_str("Forgentex diagnostic catalog v1 (Stage 29)\n");
    for entry in catalog() {
        out.push_str(&format!(
            "  - {} {} family={} safety={} rule={}\n",
            entry.code, entry.canonical_code, entry.family, entry.safety, entry.rule
        ));
    }
    out
}

pub fn explain_json(query: &str) -> Result<String, String> {
    let entry = find_entry(query)?;
    let mut out = String::new();
    out.push_str("{\n  \"schema\": \"forgentex.diagnostic_explain.v1\",\n");
    out.push_str(&format!("  \"query\": {},\n", quoted(query)));
    out.push_str("  \"found\": true,\n  \"diagnostic\": ");
    push_entry_json(&mut out, &entry, 2);
    out.push('\n');
    out.push('}');
    Ok(out)
}

pub fn explain_text(query: &str) -> Result<String, String> {
    let entry = find_entry(query)?;
    let mut out = String::new();
    out.push_str(&format!(
        "{} ({})\nfamily={} rule={} safety={}\n{}\n",
        entry.code, entry.canonical_code, entry.family, entry.rule, entry.safety, entry.summary
    ));
    out.push_str(&format!(
        "suggested_patch_kinds=[{}]\nreview_gates=[{}]\ntarget_consequences=[{}]\n",
        entry.suggested_patch_kinds.join(","),
        entry.review_gates.join(","),
        entry.target_consequences.join(",")
    ));
    Ok(out)
}

pub fn canonical_code_for(query: &str) -> Option<String> {
    find_entry(query)
        .ok()
        .map(|entry| entry.canonical_code.to_string())
        .or_else(|| Some(query.to_string()))
}

pub fn build_fix_plan(options: &FixPlanOptions) -> Result<FixPlanReport, String> {
    let path = Path::new(&options.source);
    let diagnostics = if path.is_dir() {
        check_project(path)?
            .into_iter()
            .map(record_from_diagnostic)
            .collect()
    } else {
        collect_file_diagnostics(path, options.target.as_deref())?
    };
    let mut plans = Vec::new();
    let mut unplanned = Vec::new();
    for diagnostic in &diagnostics {
        if let Some(mut plan) = plan_for_diagnostic(diagnostic, options.target.as_deref()) {
            plan.id = format!("plan-{:03}", plans.len() + 1);
            plans.push(plan);
        } else {
            unplanned.push(UnplannedDiagnostic {
                code: diagnostic.code.clone(),
                node: diagnostic.node.clone(),
                reason: "no safe semantic patch candidate is registered for this diagnostic"
                    .to_string(),
            });
        }
    }
    Ok(FixPlanReport {
        schema: "forgentex.fix_plan.v1".to_string(),
        source: options.source.clone(),
        target: options.target.clone(),
        ok: diagnostics.is_empty(),
        diagnostics,
        plans,
        unplanned_diagnostics: unplanned,
    })
}

impl FixPlanReport {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex fix plan\n");
        out.push_str(&format!(
            "schema={} source={} ok={} diagnostics={} plans={}\n",
            self.schema,
            self.source,
            bool_text(self.ok),
            self.diagnostics.len(),
            self.plans.len()
        ));
        for plan in &self.plans {
            out.push_str(&format!(
                "  - {} fixes=[{}] safety={} confidence={} op={} gates=[{}]\n",
                plan.id,
                plan.diagnostics_fixed.join(","),
                plan.safety,
                plan.confidence,
                plan.operation,
                plan.review_gates.join(",")
            ));
        }
        for item in &self.unplanned_diagnostics {
            out.push_str(&format!(
                "  - unplanned {} node={} reason=\"{}\"\n",
                item.code,
                item.node.as_deref().unwrap_or("null"),
                item.reason
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "source", &quoted(&self.source), true);
        if let Some(target) = &self.target {
            push_prop(&mut out, 1, "target", &quoted(target), true);
        } else {
            push_prop(&mut out, 1, "target", "null", true);
        }
        push_prop(&mut out, 1, "ok", bool_json(self.ok), true);
        push_line(&mut out, 1, "\"diagnostics\": [");
        for (idx, diagnostic) in self.diagnostics.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_diagnostic_record_json(&mut out, diagnostic, 2);
        }
        if !self.diagnostics.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"plans\": [");
        for (idx, plan) in self.plans.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_plan_json(&mut out, plan, 2);
        }
        if !self.plans.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"unplanned_diagnostics\": [");
        for (idx, item) in self.unplanned_diagnostics.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "code", &quoted(&item.code), true);
            push_prop(
                &mut out,
                3,
                "node",
                &optional_quoted(item.node.as_deref()),
                true,
            );
            push_prop(&mut out, 3, "reason", &quoted(&item.reason), false);
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.unplanned_diagnostics.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "]");
        out.push('}');
        out
    }
}

fn collect_file_diagnostics(
    path: &Path,
    target: Option<&str>,
) -> Result<Vec<DiagnosticRecord>, String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let output = parse_file_detailed(path, &source);
    let mut records = output
        .diagnostics
        .iter()
        .map(record_from_parse_diagnostic)
        .collect::<Vec<_>>();
    if let Some(program) = output.program {
        let mut diagnostics = check(&program);
        if let Some(triple) = target {
            diagnostics.extend(check_program_target(&program, triple));
            diagnostics.extend(check_simd_fallbacks(&program, triple));
        }
        records.extend(diagnostics.into_iter().map(record_from_diagnostic));
    }
    Ok(records)
}

fn record_from_parse_diagnostic(diagnostic: &ParseDiagnostic) -> DiagnosticRecord {
    let entry = find_entry(&diagnostic.code).ok();
    let suggested_patch_kinds = if diagnostic.suggested_patches.is_empty() {
        entry
            .as_ref()
            .map(|entry| string_vec(entry.suggested_patch_kinds))
            .unwrap_or_default()
    } else {
        diagnostic.suggested_patches.clone()
    };
    record(
        entry.as_ref(),
        &diagnostic.code,
        None,
        diagnostic.message.clone(),
        diagnostic.span.clone(),
        None,
        None,
        suggested_patch_kinds,
    )
}

fn record_from_diagnostic(diagnostic: Diagnostic) -> DiagnosticRecord {
    let entry = find_entry(&diagnostic.code).ok();
    let suggested_patch_kinds = if diagnostic.suggested_patches.is_empty() {
        entry
            .as_ref()
            .map(|entry| string_vec(entry.suggested_patch_kinds))
            .unwrap_or_default()
    } else {
        diagnostic.suggested_patches
    };
    record(
        entry.as_ref(),
        &diagnostic.code,
        Some(diagnostic.node),
        diagnostic.message,
        None,
        diagnostic.expected,
        diagnostic.found,
        suggested_patch_kinds,
    )
}

fn record(
    entry: Option<&DiagnosticCatalogEntry>,
    canonical_code: &str,
    node: Option<String>,
    message: String,
    span: Option<SourceSpan>,
    expected: Option<String>,
    found: Option<String>,
    suggested_patch_kinds: Vec<String>,
) -> DiagnosticRecord {
    if let Some(entry) = entry {
        DiagnosticRecord {
            code: entry.code.to_string(),
            canonical_code: entry.canonical_code.to_string(),
            family: entry.family.to_string(),
            rule: entry.rule.to_string(),
            node,
            message,
            span,
            expected,
            found,
            suggested_patch_kinds,
            fix_safety: entry.safety.to_string(),
            review_gates: string_vec(entry.review_gates),
            target_consequences: string_vec(entry.target_consequences),
            related_diagnostics: string_vec(entry.related),
        }
    } else {
        DiagnosticRecord {
            code: family_alias_for_unknown(canonical_code).to_string(),
            canonical_code: canonical_code.to_string(),
            family: "UNK".to_string(),
            rule: "uncataloged_diagnostic".to_string(),
            node,
            message,
            span,
            expected,
            found,
            suggested_patch_kinds,
            fix_safety: "human-review".to_string(),
            review_gates: vec!["check".to_string(), "human-review".to_string()],
            target_consequences: vec!["unknown".to_string()],
            related_diagnostics: Vec::new(),
        }
    }
}

fn plan_for_diagnostic(diagnostic: &DiagnosticRecord, target: Option<&str>) -> Option<RepairPlan> {
    let node = diagnostic.node.as_deref().unwrap_or("@parse");
    let function_node = enclosing_function_node(node);
    let gates = if diagnostic.review_gates.is_empty() {
        vec!["typecheck".to_string(), "roundtrip".to_string()]
    } else {
        diagnostic.review_gates.clone()
    };
    let target_impact = target_impact(diagnostic, target);
    let plan = match diagnostic.canonical_code.as_str() {
        "missing_colon" => simple_plan(
            diagnostic,
            "insert_colon",
            "local-edit",
            "medium",
            patch_preview(node, "OP INSERT_TOKEN token=\":\""),
            gates,
            target_impact,
        ),
        "missing_closing_brace" => simple_plan(
            diagnostic,
            "insert_closing_brace",
            "local-edit",
            "medium",
            patch_preview(node, "OP INSERT_TOKEN token=\"}\""),
            gates,
            target_impact,
        ),
        "local_type_required" => simple_plan(
            diagnostic,
            "add_local_type_annotation",
            "local-edit",
            "high",
            patch_preview(
                node,
                &format!(
                    "OP ADD_LOCAL_TYPE_ANNOTATION type={}",
                    diagnostic.found.as_deref().unwrap_or("_")
                ),
            ),
            gates,
            target_impact,
        ),
        "return_type_mismatch" => simple_plan(
            diagnostic,
            "replace_return_type",
            "api-impact",
            "medium",
            patch_preview(
                function_node,
                &format!(
                    "OP REPLACE_RETURN_TYPE type={}",
                    diagnostic.found.as_deref().unwrap_or("_")
                ),
            ),
            gates,
            target_impact,
        ),
        "integer_literal_out_of_range" => simple_plan(
            diagnostic,
            "replace_literal",
            "local-edit",
            "medium",
            patch_preview(
                node,
                &format!(
                    "OP REPLACE_LITERAL expected_type={}",
                    diagnostic.expected.as_deref().unwrap_or("_")
                ),
            ),
            gates,
            target_impact,
        ),
        "arithmetic_type_mismatch" => simple_plan(
            diagnostic,
            "insert_explicit_cast",
            "local-edit",
            "medium",
            patch_preview(
                node,
                &format!(
                    "OP INSERT_CAST target_type={}",
                    diagnostic.expected.as_deref().unwrap_or("_")
                ),
            ),
            gates,
            target_impact,
        ),
        "argument_type_mismatch" => simple_plan(
            diagnostic,
            "replace_call_argument",
            "local-edit",
            "medium",
            patch_preview(
                node,
                &format!(
                    "OP INSERT_CAST target_type={}",
                    diagnostic.expected.as_deref().unwrap_or("_")
                ),
            ),
            gates,
            target_impact,
        ),
        "index_not_integer" => simple_plan(
            diagnostic,
            "replace_index_expression",
            "local-edit",
            "high",
            patch_preview(node, "OP INSERT_CAST target_type=usize"),
            gates,
            target_impact,
        ),
        "type_mismatch" => simple_plan(
            diagnostic,
            "insert_explicit_cast",
            "local-edit",
            "medium",
            patch_preview(
                node,
                &format!(
                    "OP INSERT_CAST target_type={}",
                    diagnostic.expected.as_deref().unwrap_or("_")
                ),
            ),
            gates,
            target_impact,
        ),
        "undeclared_effect" => simple_plan(
            diagnostic,
            "add_effect_declaration",
            "capability-change",
            "high",
            patch_preview(
                function_node,
                &format!(
                    "OP ADD_EFFECT effect={}",
                    extract_quoted_value(&diagnostic.message).unwrap_or_else(|| "_".to_string())
                ),
            ),
            gates,
            target_impact,
        ),
        "unsafe_required" => simple_plan(
            diagnostic,
            "mark_function_unsafe",
            "human-review",
            "medium",
            patch_preview(function_node, "OP MARK_FUNCTION_UNSAFE"),
            ensure_gate(gates, "human-review"),
            target_impact,
        ),
        "asm_invalid_register" => simple_plan(
            diagnostic,
            "replace_register_for_target",
            "architecture-review",
            "medium",
            patch_preview(
                node,
                "OP REPLACE_ASM_REGISTER target=selected_target_register",
            ),
            ensure_gate(gates, "human-review"),
            target_impact,
        ),
        "target_guard_required" => simple_plan(
            diagnostic,
            "wrap_call_in_cpu_feature_guard",
            "architecture-review",
            "medium",
            patch_preview(node, "OP WRAP_CALL_IN_CPU_FEATURE_GUARD"),
            ensure_gate(gates, "simd-review"),
            target_impact,
        ),
        "unresolved_import" => simple_plan(
            diagnostic,
            "fix_import_name",
            "project-edit",
            "medium",
            patch_preview(node, "OP FIX_IMPORT_NAME or OP ADD_MODULE_FILE"),
            gates,
            target_impact,
        ),
        "mutable_borrow_alias" => simple_plan(
            diagnostic,
            "split_slice_or_copy_input",
            "human-review",
            "medium",
            patch_preview(node, "OP SPLIT_MUTABLE_BORROW_OR_COPY_INPUT"),
            ensure_gate(gates, "human-review"),
            target_impact,
        ),
        _ => return None,
    };
    Some(plan)
}

fn simple_plan(
    diagnostic: &DiagnosticRecord,
    operation: &str,
    safety: &str,
    confidence: &str,
    patch_preview: String,
    review_gates: Vec<String>,
    target_impact: Vec<String>,
) -> RepairPlan {
    RepairPlan {
        id: String::new(),
        diagnostics_fixed: vec![diagnostic.code.clone()],
        safety: safety.to_string(),
        confidence: confidence.to_string(),
        operation: operation.to_string(),
        patch_preview,
        review_gates,
        target_impact,
    }
}

fn patch_preview(target: &str, operation: &str) -> String {
    format!(
        "VERSION 1\nAUTHOR fgx.fix\nTARGET {target}\nPRECONDITION NODE_EXISTS {target}\n{operation}\nREVIEW generated_by=stage29_fix_plan\n"
    )
}

fn target_impact(diagnostic: &DiagnosticRecord, target: Option<&str>) -> Vec<String> {
    if let Some(target) = target {
        vec![target.to_string()]
    } else if diagnostic.target_consequences.is_empty() {
        vec!["all-frontends".to_string()]
    } else {
        diagnostic.target_consequences.clone()
    }
}

fn enclosing_function_node(node: &str) -> &str {
    if let Some(index) = node.find(".stmt.") {
        &node[..index]
    } else if node.starts_with("@function.") {
        node
    } else {
        "@program"
    }
}

fn extract_quoted_value(message: &str) -> Option<String> {
    let start = message.find('\'')?;
    let rest = &message[start + 1..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn ensure_gate(mut gates: Vec<String>, gate: &str) -> Vec<String> {
    if !gates.iter().any(|candidate| candidate == gate) {
        gates.push(gate.to_string());
    }
    gates
}

fn find_entry(query: &str) -> Result<DiagnosticCatalogEntry, String> {
    catalog()
        .into_iter()
        .find(|entry| entry.code.eq_ignore_ascii_case(query) || entry.canonical_code == query)
        .ok_or_else(|| format!("unknown diagnostic code '{query}'"))
}

fn entry(
    code: &'static str,
    canonical_code: &'static str,
    family: &'static str,
    family_title: &'static str,
    rule: &'static str,
    summary: &'static str,
    safety: &'static str,
    suggested_patch_kinds: &'static [&'static str],
    review_gates: &'static [&'static str],
    target_consequences: &'static [&'static str],
    related: &'static [&'static str],
) -> DiagnosticCatalogEntry {
    DiagnosticCatalogEntry {
        code,
        canonical_code,
        family,
        family_title,
        rule,
        summary,
        safety,
        suggested_patch_kinds,
        review_gates,
        target_consequences,
        related,
    }
}

fn family_alias_for_unknown(code: &str) -> &str {
    if code.starts_with("asm_") {
        "ASM999"
    } else if code.starts_with("abi_") {
        "ABI999"
    } else if code.starts_with("target_") || code == "unknown_target" {
        "TAR999"
    } else {
        "UNK999"
    }
}

fn string_vec(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn push_entry_json(out: &mut String, entry: &DiagnosticCatalogEntry, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "code", &quoted(entry.code), true);
    push_prop(
        out,
        indent + 1,
        "canonical_code",
        &quoted(entry.canonical_code),
        true,
    );
    push_prop(out, indent + 1, "family", &quoted(entry.family), true);
    push_prop(
        out,
        indent + 1,
        "family_title",
        &quoted(entry.family_title),
        true,
    );
    push_prop(out, indent + 1, "rule", &quoted(entry.rule), true);
    push_prop(out, indent + 1, "summary", &quoted(entry.summary), true);
    push_prop(out, indent + 1, "safety", &quoted(entry.safety), true);
    push_str_array(
        out,
        indent + 1,
        "suggested_patch_kinds",
        entry.suggested_patch_kinds,
        true,
    );
    push_str_array(out, indent + 1, "review_gates", entry.review_gates, true);
    push_str_array(
        out,
        indent + 1,
        "target_consequences",
        entry.target_consequences,
        true,
    );
    push_str_array(out, indent + 1, "related_diagnostics", entry.related, false);
    push_indent(out, indent);
    out.push('}');
}

fn push_diagnostic_record_json(out: &mut String, diagnostic: &DiagnosticRecord, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "code", &quoted(&diagnostic.code), true);
    push_prop(
        out,
        indent + 1,
        "canonical_code",
        &quoted(&diagnostic.canonical_code),
        true,
    );
    push_prop(out, indent + 1, "family", &quoted(&diagnostic.family), true);
    push_prop(out, indent + 1, "rule", &quoted(&diagnostic.rule), true);
    push_prop(
        out,
        indent + 1,
        "node",
        &optional_quoted(diagnostic.node.as_deref()),
        true,
    );
    if let Some(span) = &diagnostic.span {
        push_prop(out, indent + 1, "span", &span.to_json(), true);
    } else {
        push_prop(out, indent + 1, "span", "null", true);
    }
    push_prop(
        out,
        indent + 1,
        "message",
        &quoted(&diagnostic.message),
        true,
    );
    push_prop(
        out,
        indent + 1,
        "expected",
        &optional_quoted(diagnostic.expected.as_deref()),
        true,
    );
    push_prop(
        out,
        indent + 1,
        "found",
        &optional_quoted(diagnostic.found.as_deref()),
        true,
    );
    push_string_vec(
        out,
        indent + 1,
        "suggested_patch_kinds",
        &diagnostic.suggested_patch_kinds,
        true,
    );
    push_prop(
        out,
        indent + 1,
        "fix_safety",
        &quoted(&diagnostic.fix_safety),
        true,
    );
    push_string_vec(
        out,
        indent + 1,
        "review_gates",
        &diagnostic.review_gates,
        true,
    );
    push_string_vec(
        out,
        indent + 1,
        "target_consequences",
        &diagnostic.target_consequences,
        true,
    );
    push_string_vec(
        out,
        indent + 1,
        "related_diagnostics",
        &diagnostic.related_diagnostics,
        false,
    );
    push_indent(out, indent);
    out.push('}');
}

fn push_plan_json(out: &mut String, plan: &RepairPlan, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "id", &quoted(&plan.id), true);
    push_string_vec(
        out,
        indent + 1,
        "diagnostics_fixed",
        &plan.diagnostics_fixed,
        true,
    );
    push_prop(out, indent + 1, "safety", &quoted(&plan.safety), true);
    push_prop(
        out,
        indent + 1,
        "confidence",
        &quoted(&plan.confidence),
        true,
    );
    push_prop(out, indent + 1, "operation", &quoted(&plan.operation), true);
    push_prop(
        out,
        indent + 1,
        "patch_preview",
        &quoted(&plan.patch_preview),
        true,
    );
    push_string_vec(out, indent + 1, "review_gates", &plan.review_gates, true);
    push_string_vec(out, indent + 1, "target_impact", &plan.target_impact, false);
    push_indent(out, indent);
    out.push('}');
}

fn push_str_array(out: &mut String, indent: usize, key: &str, values: &[&str], trailing: bool) {
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

fn push_string_vec(out: &mut String, indent: usize, key: &str, values: &[String], trailing: bool) {
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
    format!("\"{}\"", escape(value))
}

fn optional_quoted(value: Option<&str>) -> String {
    value.map(quoted).unwrap_or_else(|| "null".to_string())
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
