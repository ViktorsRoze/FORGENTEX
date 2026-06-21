//! Unified polyglot execution ABI (v2.18) — the capstone.
//!
//! One calling convention, one gate, one report across every backend. The
//! C-ABI bridge (v2.13) and the Python bridge (v2.17) are dispatched through
//! a single `fgx polyglot-run`, and the shared typed namespace (v2.2) is
//! surfaced as the **cross-language data plane**: a value declared once with
//! `share` flows to capsules in different languages by name.
//!
//! Honest scope: this unifies dispatch, marshaling, gating, and the
//! data-plane wiring, and executes the real C and Python backends against
//! their fixtures. Live in-process mutation of a shared value across
//! runtimes stays snapshot/orchestration-level (the v2.2 deferral); the data
//! plane here is the typed wiring (who binds what, in which language and
//! mode), which is what the ABI guarantees.

use std::collections::BTreeSet;
use std::path::Path;

use crate::ast::Program;
use crate::json;

#[derive(Clone, Debug, PartialEq)]
pub struct PolyglotCapsule {
    pub name: String,
    pub language: String,
    pub backend: String,
    pub gate_decision: String,
    pub execution: String,
    pub passed: usize,
    pub total: usize,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SharedBinding {
    pub name: String,
    pub ty: String,
    pub binders: Vec<(String, String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolyglotReport {
    pub schema: String,
    pub file: String,
    pub languages: Vec<String>,
    pub capsules: Vec<PolyglotCapsule>,
    pub data_plane: Vec<SharedBinding>,
    pub passed: usize,
    pub total: usize,
    pub status: String,
}

/// The single calling convention's backend router.
fn backend_for(language: &str) -> &'static str {
    match language {
        "c" | "cpp" | "rust" | "zig" => "ffi",
        "python" => "python",
        "javascript" | "typescript" => "node",
        "java" => "jvm",
        "csharp" => "dotnet",
        "go" => "go",
        _ => "not_wired",
    }
}

pub fn run(file: &str, program: &Program, out_dir: &Path) -> PolyglotReport {
    let mut capsules = Vec::new();
    let mut languages: BTreeSet<String> = BTreeSet::new();
    let mut passed = 0;
    let mut total = 0;

    for capsule in program.capsules() {
        languages.insert(capsule.language.clone());
        let backend = backend_for(&capsule.language);
        let (gate_decision, execution, p, t, ok, message) = match backend {
            "ffi" => match crate::ffi::run(file, program, Some(&capsule.name), out_dir) {
                Ok(plan) => (
                    plan.gate_decision.clone(),
                    plan.execution_status.clone(),
                    plan.passed,
                    plan.total,
                    plan.is_success(),
                    plan.message.clone(),
                ),
                Err(err) => ("deny".into(), "error".into(), 0, 0, false, Some(err)),
            },
            "python" => match crate::python_run::run(file, program, Some(&capsule.name)) {
                Ok(report) => (
                    report.sandbox_decision.clone(),
                    report.status.clone(),
                    report.passed,
                    report.total,
                    report.is_success(),
                    report.message.clone(),
                ),
                Err(err) => ("deny".into(), "error".into(), 0, 0, false, Some(err)),
            },
            "node" => match crate::node_run::run(file, program, Some(&capsule.name)) {
                Ok(report) => (
                    report.sandbox_decision.clone(),
                    report.status.clone(),
                    report.passed,
                    report.total,
                    report.is_success(),
                    report.message.clone(),
                ),
                Err(err) => ("deny".into(), "error".into(), 0, 0, false, Some(err)),
            },
            "jvm" => match crate::jvm_run::run(file, program, Some(&capsule.name)) {
                Ok(report) => (
                    report.sandbox_decision.clone(),
                    report.status.clone(),
                    report.passed,
                    report.total,
                    report.is_success(),
                    report.message.clone(),
                ),
                Err(err) => ("deny".into(), "error".into(), 0, 0, false, Some(err)),
            },
            "dotnet" => match crate::dotnet_run::run(file, program, Some(&capsule.name)) {
                Ok(report) => (
                    report.sandbox_decision.clone(),
                    report.status.clone(),
                    report.passed,
                    report.total,
                    report.is_success(),
                    report.message.clone(),
                ),
                Err(err) => ("deny".into(), "error".into(), 0, 0, false, Some(err)),
            },
            "go" => match crate::go_run::run(file, program, Some(&capsule.name)) {
                Ok(report) => (
                    report.sandbox_decision.clone(),
                    report.status.clone(),
                    report.passed,
                    report.total,
                    report.is_success(),
                    report.message.clone(),
                ),
                Err(err) => ("deny".into(), "error".into(), 0, 0, false, Some(err)),
            },
            _ => (
                "n/a".into(),
                "not_wired".into(),
                0,
                0,
                true,
                Some(format!(
                    "{} execution lands in a later milestone",
                    capsule.language
                )),
            ),
        };
        // Only wired backends count toward the pass tally.
        if backend != "not_wired" {
            passed += p;
            total += t;
        }
        capsules.push(PolyglotCapsule {
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            backend: backend.to_string(),
            gate_decision,
            execution,
            passed: p,
            total: t,
            ok,
            message,
        });
    }

    // The cross-language data plane: each shared value and who binds it.
    let mut data_plane = Vec::new();
    for share in program.shares() {
        let mut binders = Vec::new();
        for capsule in program.capsules() {
            for binding in &capsule.bindings {
                if binding.name == share.name {
                    binders.push((
                        capsule.name.clone(),
                        capsule.language.clone(),
                        binding.mode.as_str().to_string(),
                    ));
                }
            }
        }
        data_plane.push(SharedBinding {
            name: share.name.clone(),
            ty: share.ty.to_agent(),
            binders,
        });
    }

    let all_ok = capsules.iter().all(|c| c.ok);
    PolyglotReport {
        schema: "forgentex.polyglot_abi.v1".to_string(),
        file: file.to_string(),
        languages: languages.into_iter().collect(),
        capsules,
        data_plane,
        passed,
        total,
        status: if all_ok { "ok" } else { "partial" }.to_string(),
    }
}

impl PolyglotReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex unified polyglot ABI\n");
        out.push_str(&format!(
            "schema={} file={} status={} languages=[{}]\n",
            self.schema,
            self.file,
            self.status,
            self.languages.join(", ")
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  @{:<16} {:<10} backend={:<9} gate={:<5} exec={:<18} {}/{}\n",
                capsule.name,
                capsule.language,
                capsule.backend,
                capsule.gate_decision,
                capsule.execution,
                capsule.passed,
                capsule.total
            ));
        }
        if !self.data_plane.is_empty() {
            out.push_str("  data plane (declare once, flow by name):\n");
            for share in &self.data_plane {
                let binders = share
                    .binders
                    .iter()
                    .map(|(c, l, m)| format!("{c}[{l}:{m}]"))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "    {} : {} -> {}\n",
                    share.name, share.ty, binders
                ));
            }
        }
        out.push_str(&format!(
            "  wired fixtures: {}/{} passed\n",
            self.passed, self.total
        ));
        out
    }

    pub fn to_json(&self) -> String {
        let capsules = self
            .capsules
            .iter()
            .map(|c| {
                format!(
                    "{{ \"name\": {}, \"language\": {}, \"backend\": {}, \"gate\": {}, \"execution\": {}, \"passed\": {}, \"total\": {}, \"ok\": {} }}",
                    json::string(&c.name),
                    json::string(&c.language),
                    json::string(&c.backend),
                    json::string(&c.gate_decision),
                    json::string(&c.execution),
                    c.passed,
                    c.total,
                    c.ok
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        let plane = self
            .data_plane
            .iter()
            .map(|s| {
                let binders = s
                    .binders
                    .iter()
                    .map(|(c, l, m)| {
                        format!(
                            "{{ \"capsule\": {}, \"language\": {}, \"mode\": {} }}",
                            json::string(c),
                            json::string(l),
                            json::string(m)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{{ \"name\": {}, \"type\": {}, \"binders\": [{}] }}",
                    json::string(&s.name),
                    json::string(&s.ty),
                    binders
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"status\": {},\n  \"languages\": {},\n  \"capsules\": [\n    {}\n  ],\n  \"data_plane\": [\n    {}\n  ],\n  \"passed\": {},\n  \"total\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.status),
            json::string_array(&self.languages),
            capsules,
            plane,
            self.passed,
            self.total,
        )
    }
}
