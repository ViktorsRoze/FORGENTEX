//! Foreign code capsule contract validation and review reporting (v2.0).
//!
//! v2.0 is the capsule *specification* milestone: capsules are parsed,
//! their contract is validated, and a machine-readable review report is
//! produced. Capsules are not executed (deferred to v2.3) and are not
//! lowered by the native backends (review-only); the review report states
//! the `not_lowerable` backend stance explicitly rather than letting a
//! backend silently drop the code.
//!
//! Validation enforces the non-negotiable parts of the capsule contract
//! from `docs/foreign_capsule_spec.md`: a pinned dependency set, a declared
//! sandbox policy, a typed boundary, a runtime version, and a code body.

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::diag::Diagnostic;
use crate::json;

/// Contract diagnostics for every capsule in the program. These are
/// blocking: a project that contains an invalid capsule fails `fgx check`.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        validate_capsule(capsule, &mut diagnostics);
    }
    diagnostics
}

fn validate_capsule(capsule: &Capsule, out: &mut Vec<Diagnostic>) {
    if capsule.version.trim().is_empty() {
        out.push(
            Diagnostic::new(
                "capsule_missing_version",
                &capsule.id,
                format!("capsule '{}' must declare a runtime version", capsule.name),
            )
            .suggest("add a version, for example version \"3.13\""),
        );
    }
    if capsule.inputs.is_empty() && capsule.outputs.is_empty() {
        out.push(
            Diagnostic::new(
                "capsule_missing_typed_boundary",
                &capsule.id,
                format!(
                    "capsule '{}' must declare at least one typed input or output",
                    capsule.name
                ),
            )
            .suggest("add an input or output binding at the Forgentex boundary"),
        );
    }
    if capsule.sandbox.is_empty() {
        out.push(
            Diagnostic::new(
                "capsule_missing_sandbox",
                &capsule.id,
                format!("capsule '{}' must declare a sandbox policy", capsule.name),
            )
            .suggest("add a sandbox rule, for example sandbox \"fs.read /data\""),
        );
    }
    if capsule.code.is_empty() {
        out.push(Diagnostic::new(
            "capsule_empty_code",
            &capsule.id,
            format!("capsule '{}' has an empty code body", capsule.name),
        ));
    }
    for dep in &capsule.deps {
        if !dep.is_pinned() {
            out.push(
                Diagnostic::new(
                    "capsule_unpinned_dependency",
                    &capsule.id,
                    format!(
                        "capsule '{}' dependency '{}' is not pinned",
                        capsule.name, dep.source
                    ),
                )
                .suggest("pin with an exact version, commit, or checksum"),
            );
        }
    }
    for (index, fixture) in capsule.fixtures.iter().enumerate() {
        if fixture.inputs.len() != capsule.inputs.len()
            || fixture.outputs.len() != capsule.outputs.len()
        {
            out.push(
                Diagnostic::new(
                    "capsule_fixture_arity",
                    &capsule.id,
                    format!(
                        "capsule '{}' mock {} has {} input(s) and {} output(s) but the boundary declares {} and {}",
                        capsule.name,
                        index,
                        fixture.inputs.len(),
                        fixture.outputs.len(),
                        capsule.inputs.len(),
                        capsule.outputs.len()
                    ),
                )
                .suggest("match the mock arity to the declared inputs and outputs"),
            );
            continue;
        }
        for (value, binding) in fixture.inputs.iter().zip(&capsule.inputs) {
            if !value.matches_type(&binding.ty) {
                out.push(fixture_type_diagnostic(
                    capsule,
                    index,
                    "input",
                    &binding.name,
                    value,
                    &binding.ty,
                ));
            }
        }
        for (value, binding) in fixture.outputs.iter().zip(&capsule.outputs) {
            if !value.matches_type(&binding.ty) {
                out.push(fixture_type_diagnostic(
                    capsule,
                    index,
                    "output",
                    &binding.name,
                    value,
                    &binding.ty,
                ));
            }
        }
    }
}

fn fixture_type_diagnostic(
    capsule: &Capsule,
    index: usize,
    role: &str,
    binding: &str,
    value: &crate::ast::CapsuleValue,
    ty: &crate::ast::Type,
) -> Diagnostic {
    Diagnostic::new(
        "capsule_fixture_type",
        &capsule.id,
        format!(
            "capsule '{}' mock {} {} '{}' is {} but the boundary type is {}",
            capsule.name,
            index,
            role,
            binding,
            value.type_name(),
            ty.to_agent()
        ),
    )
    .suggest("use a literal whose type matches the boundary binding")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub capsules: Vec<CapsuleReviewEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleReviewEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub version: String,
    pub env: Option<String>,
    pub risk_class: String,
    pub high_risk: bool,
    pub effects: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub dependencies: Vec<(String, bool)>,
    pub sandbox: Vec<String>,
    pub fixtures: usize,
    pub code_lines: usize,
    pub native_backend: String,
    pub status: String,
    pub diagnostics: Vec<String>,
    pub notes: Vec<String>,
}

pub fn analyze(file: &str, program: &Program) -> CapsuleReport {
    let mut capsules = Vec::new();
    for capsule in program.capsules() {
        let mut diagnostics = Vec::new();
        validate_capsule(capsule, &mut diagnostics);
        let codes: Vec<String> = diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect();
        let high_risk = capsule.risk_class() != "managed";
        let status = if !codes.is_empty() {
            "rejected"
        } else if high_risk {
            "review_required"
        } else {
            "pass"
        };
        let mut notes = Vec::new();
        if high_risk {
            notes.push(format!(
                "high-risk {} capsule: stricter review required",
                capsule.risk_class()
            ));
        }
        if matches!(capsule.risk_class(), "smart_contract") {
            notes.push(
                "smart-contract capsule: wallet/secret-key access denied by default".to_string(),
            );
        }
        notes.push("native backends: not lowerable (review-only)".to_string());
        capsules.push(CapsuleReviewEntry {
            id: capsule.id.clone(),
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            version: capsule.version.clone(),
            env: capsule.env.clone(),
            risk_class: capsule.risk_class().to_string(),
            high_risk,
            effects: capsule.effects.clone(),
            inputs: capsule
                .inputs
                .iter()
                .map(|binding| format!("{}: {}", binding.name, binding.ty.to_agent()))
                .collect(),
            outputs: capsule
                .outputs
                .iter()
                .map(|binding| format!("{}: {}", binding.name, binding.ty.to_agent()))
                .collect(),
            dependencies: capsule
                .deps
                .iter()
                .map(|dep| (dep.source.clone(), dep.is_pinned()))
                .collect(),
            sandbox: capsule.sandbox.clone(),
            fixtures: capsule.fixtures.len(),
            code_lines: capsule.code.len(),
            native_backend: "not_lowerable".to_string(),
            status: status.to_string(),
            diagnostics: codes,
            notes,
        });
    }
    let status = if capsules.iter().any(|c| c.status == "rejected") {
        "rejected"
    } else if capsules.iter().any(|c| c.status == "review_required") {
        "review_required"
    } else {
        "pass"
    };
    CapsuleReport {
        schema: "forgentex.capsule_review.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        capsules,
    }
}

impl CapsuleReport {
    pub fn is_rejected(&self) -> bool {
        self.status == "rejected"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule review\n");
        out.push_str(&format!(
            "schema={} file={} status={} capsules={}\n",
            self.schema,
            self.file,
            self.status,
            self.capsules.len()
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  - {} {} lang={} version={} risk={} status={} backend={}\n",
                capsule.name,
                capsule.id,
                capsule.language,
                capsule.version,
                capsule.risk_class,
                capsule.status,
                capsule.native_backend
            ));
            out.push_str(&format!(
                "      effects=[{}] inputs=[{}] outputs=[{}]\n",
                capsule.effects.join(","),
                capsule.inputs.join(","),
                capsule.outputs.join(",")
            ));
            for (source, pinned) in &capsule.dependencies {
                out.push_str(&format!("      dep {source} pinned={pinned}\n"));
            }
            for rule in &capsule.sandbox {
                out.push_str(&format!("      sandbox {rule}\n"));
            }
            if !capsule.diagnostics.is_empty() {
                out.push_str(&format!(
                    "      diagnostics=[{}]\n",
                    capsule.diagnostics.join(",")
                ));
            }
            for note in &capsule.notes {
                out.push_str(&format!("      note: {note}\n"));
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str(&format!("  \"status\": {},\n", json::string(&self.status)));
        out.push_str("  \"capsules\": [");
        for (idx, capsule) in self.capsules.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {\n");
            out.push_str(&format!("      \"id\": {},\n", json::string(&capsule.id)));
            out.push_str(&format!(
                "      \"name\": {},\n",
                json::string(&capsule.name)
            ));
            out.push_str(&format!(
                "      \"language\": {},\n",
                json::string(&capsule.language)
            ));
            out.push_str(&format!(
                "      \"version\": {},\n",
                json::string(&capsule.version)
            ));
            out.push_str(&format!(
                "      \"env\": {},\n",
                json::opt_string(capsule.env.as_deref())
            ));
            out.push_str(&format!(
                "      \"risk_class\": {},\n",
                json::string(&capsule.risk_class)
            ));
            out.push_str(&format!("      \"high_risk\": {},\n", capsule.high_risk));
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json::string_array(&capsule.effects)
            ));
            out.push_str(&format!(
                "      \"inputs\": {},\n",
                json::string_array(&capsule.inputs)
            ));
            out.push_str(&format!(
                "      \"outputs\": {},\n",
                json::string_array(&capsule.outputs)
            ));
            out.push_str("      \"dependencies\": [");
            for (dep_idx, (source, pinned)) in capsule.dependencies.iter().enumerate() {
                if dep_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"source\": {}, \"pinned\": {}}}",
                    json::string(source),
                    pinned
                ));
            }
            out.push_str("],\n");
            out.push_str(&format!(
                "      \"sandbox\": {},\n",
                json::string_array(&capsule.sandbox)
            ));
            out.push_str(&format!("      \"fixtures\": {},\n", capsule.fixtures));
            out.push_str(&format!("      \"code_lines\": {},\n", capsule.code_lines));
            out.push_str(&format!(
                "      \"native_backend\": {},\n",
                json::string(&capsule.native_backend)
            ));
            out.push_str(&format!(
                "      \"status\": {},\n",
                json::string(&capsule.status)
            ));
            out.push_str(&format!(
                "      \"diagnostics\": {},\n",
                json::string_array(&capsule.diagnostics)
            ));
            out.push_str(&format!(
                "      \"notes\": {}\n",
                json::string_array(&capsule.notes)
            ));
            out.push_str("    }");
        }
        if !self.capsules.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

// ---- Deterministic mock execution (v2.1) --------------------------------
//
// Capsules are not executed for real in v2; the mock executor exercises the
// declared fixtures against the typed boundary so the contract is testable
// and reproducible without a foreign runtime.

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleRunReport {
    pub schema: String,
    pub file: String,
    pub capsule: String,
    pub status: String,
    pub passed: usize,
    pub failed: usize,
    pub cases: Vec<CapsuleRunCase>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleRunCase {
    pub index: usize,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub status: String,
    pub detail: String,
}

fn select_capsule<'a>(program: &'a Program, name: Option<&str>) -> Result<&'a Capsule, String> {
    let mut capsules = program.capsules();
    match name {
        Some(name) => program
            .capsules()
            .find(|capsule| capsule.name == name)
            .ok_or_else(|| format!("no capsule named '{name}'")),
        None => {
            let first = capsules
                .next()
                .ok_or_else(|| "no capsule in file".to_string())?;
            if capsules.next().is_some() {
                return Err(
                    "file has multiple capsules; pass --capsule <name> to choose one".to_string(),
                );
            }
            Ok(first)
        }
    }
}

/// Run every declared fixture through the mock boundary check.
pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<CapsuleRunReport, String> {
    let capsule = select_capsule(program, name)?;
    let mut cases = Vec::new();
    for (index, fixture) in capsule.fixtures.iter().enumerate() {
        let inputs: Vec<String> = fixture
            .inputs
            .iter()
            .map(CapsuleValue::to_literal)
            .collect();
        let outputs: Vec<String> = fixture
            .outputs
            .iter()
            .map(CapsuleValue::to_literal)
            .collect();
        let (status, detail) = evaluate_fixture(capsule, fixture);
        cases.push(CapsuleRunCase {
            index,
            inputs,
            outputs,
            status,
            detail,
        });
    }
    let passed = cases.iter().filter(|case| case.status == "pass").count();
    let failed = cases.len() - passed;
    let status = if capsule.fixtures.is_empty() {
        "no_fixtures"
    } else if failed == 0 {
        "pass"
    } else {
        "fail"
    };
    Ok(CapsuleRunReport {
        schema: "forgentex.capsule_run.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.id.clone(),
        status: status.to_string(),
        passed,
        failed,
        cases,
    })
}

/// Execution hook: given input literals, deterministically return the
/// outputs of the matching fixture, or an error trap.
pub fn run_input(
    program: &Program,
    name: Option<&str>,
    inputs: &[CapsuleValue],
) -> Result<Vec<String>, String> {
    let capsule = select_capsule(program, name)?;
    if inputs.len() != capsule.inputs.len() {
        return Err(format!(
            "input arity {} does not match boundary arity {}",
            inputs.len(),
            capsule.inputs.len()
        ));
    }
    for fixture in &capsule.fixtures {
        if fixture.inputs == inputs {
            return Ok(fixture
                .outputs
                .iter()
                .map(CapsuleValue::to_literal)
                .collect());
        }
    }
    Err("no_matching_fixture: no declared mock matches these inputs".to_string())
}

fn evaluate_fixture(capsule: &Capsule, fixture: &crate::ast::CapsuleFixture) -> (String, String) {
    if fixture.inputs.len() != capsule.inputs.len()
        || fixture.outputs.len() != capsule.outputs.len()
    {
        return ("arity".to_string(), "fixture arity mismatch".to_string());
    }
    for (value, binding) in fixture.inputs.iter().zip(&capsule.inputs) {
        if !value.matches_type(&binding.ty) {
            return (
                "type_mismatch".to_string(),
                format!("input '{}' type mismatch", binding.name),
            );
        }
    }
    for (value, binding) in fixture.outputs.iter().zip(&capsule.outputs) {
        if !value.matches_type(&binding.ty) {
            return (
                "type_mismatch".to_string(),
                format!("output '{}' type mismatch", binding.name),
            );
        }
    }
    ("pass".to_string(), String::new())
}

impl CapsuleRunReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule run\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} status={} passed={} failed={}\n",
            self.schema, self.file, self.capsule, self.status, self.passed, self.failed
        ));
        for case in &self.cases {
            out.push_str(&format!(
                "  - mock {} [{}] -> [{}] {}{}\n",
                case.index,
                case.inputs.join(", "),
                case.outputs.join(", "),
                case.status,
                if case.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", case.detail)
                }
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str(&format!(
            "  \"capsule\": {},\n",
            json::string(&self.capsule)
        ));
        out.push_str(&format!("  \"status\": {},\n", json::string(&self.status)));
        out.push_str(&format!("  \"passed\": {},\n", self.passed));
        out.push_str(&format!("  \"failed\": {},\n", self.failed));
        out.push_str("  \"cases\": [");
        for (idx, case) in self.cases.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {");
            out.push_str(&format!("\"index\": {}, ", case.index));
            out.push_str(&format!(
                "\"inputs\": {}, ",
                json::string_array(&case.inputs)
            ));
            out.push_str(&format!(
                "\"outputs\": {}, ",
                json::string_array(&case.outputs)
            ));
            out.push_str(&format!("\"status\": {}, ", json::string(&case.status)));
            out.push_str(&format!("\"detail\": {}", json::string(&case.detail)));
            out.push('}');
        }
        if !self.cases.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}
