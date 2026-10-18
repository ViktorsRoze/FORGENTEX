//! Native and GPU capsule review (v2.6).
//!
//! C, C++, and CUDA capsules are the highest-risk non-shell capsule
//! classes: they cross a C ABI boundary, may pass raw memory, and (for
//! CUDA) target a specific GPU architecture. v2.6 reviews them with
//! compiler/toolkit and GPU-target metadata, classifies the ABI boundary
//! (value vs raw-memory), and tracks deterministic fallback fixtures.
//!
//! These capsules are always at least `review` — they are never
//! auto-approved — and are reported `not_lowerable` until real native
//! execution exists. The blocking rule here is `gpu_capsule_no_target`: a
//! CUDA capsule must declare a GPU architecture in its target environment.

use crate::ast::{Capsule, Program, Type};
use crate::diag::Diagnostic;
use crate::json;

/// True for capsules whose risk class is native (C/C++) or gpu (CUDA).
fn is_native_or_gpu(capsule: &Capsule) -> bool {
    matches!(capsule.risk_class(), "native" | "gpu")
}

/// Extract a GPU architecture token (`sm_NN` or `compute_NN`) from the
/// capsule target environment, if present.
pub(crate) fn gpu_arch(env: Option<&str>) -> Option<String> {
    let env = env?;
    for token in env.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        if (token.starts_with("sm_") || token.starts_with("compute_"))
            && token.chars().any(|ch| ch.is_ascii_digit())
        {
            return Some(token.to_string());
        }
    }
    None
}

/// Whether a typed boundary binding passes raw memory across the ABI
/// (a pointer, slice, or array) rather than a scalar value.
fn is_memory_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Pointer { .. } | Type::Slice { .. } | Type::Array { .. }
    )
}

fn crosses_memory_boundary(capsule: &Capsule) -> bool {
    capsule
        .inputs
        .iter()
        .chain(&capsule.outputs)
        .any(|binding| is_memory_type(&binding.ty))
}

/// `gpu_capsule_no_target`: a CUDA capsule must declare a GPU architecture
/// (`sm_NN` / `compute_NN`) in its target environment.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        if capsule.risk_class() == "gpu" && gpu_arch(capsule.env.as_deref()).is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "gpu_capsule_no_target",
                    &capsule.id,
                    format!(
                        "GPU capsule '{}' must declare a GPU architecture (sm_NN or compute_NN) in its env",
                        capsule.name
                    ),
                )
                .suggest("set env to include a GPU arch, for example env \"cuda-12.4-sm_80\""),
            );
        }
    }
    diagnostics
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub risk_class: String,
    pub toolchain: String,
    pub target_env: Option<String>,
    pub gpu_arch: Option<String>,
    pub memory_boundary: bool,
    pub fallback_fixtures: usize,
    pub effects: Vec<String>,
    pub decision: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub capsules: Vec<NativeEntry>,
}

pub fn analyze(file: &str, program: &Program) -> NativeReport {
    let mut capsules = Vec::new();
    for capsule in program.capsules() {
        if !is_native_or_gpu(capsule) {
            continue;
        }
        let arch = gpu_arch(capsule.env.as_deref());
        let memory_boundary = crosses_memory_boundary(capsule);
        let mut notes = Vec::new();
        let mut reject = false;

        if capsule.risk_class() == "gpu" && arch.is_none() {
            notes.push("missing GPU architecture (sm_NN/compute_NN) in env".to_string());
            reject = true;
        }
        if memory_boundary {
            notes.push(
                "raw memory crosses the C ABI boundary: review ownership, bounds, and lifetime"
                    .to_string(),
            );
        } else {
            notes.push("value boundary: no raw memory crosses the ABI".to_string());
        }
        if capsule.fixtures.is_empty() {
            notes.push("no deterministic fallback fixtures declared".to_string());
        }
        notes.push("native backends: not lowerable (review-only)".to_string());

        let decision = if reject { "reject" } else { "review" };
        capsules.push(NativeEntry {
            id: capsule.id.clone(),
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            risk_class: capsule.risk_class().to_string(),
            toolchain: capsule.version.clone(),
            target_env: capsule.env.clone(),
            gpu_arch: arch,
            memory_boundary,
            fallback_fixtures: capsule.fixtures.len(),
            effects: capsule.effects.clone(),
            decision: decision.to_string(),
            notes,
        });
    }
    let status = if capsules.iter().any(|c| c.decision == "reject") {
        "reject"
    } else if capsules.is_empty() {
        "pass"
    } else {
        "review"
    };
    NativeReport {
        schema: "forgentex.native_review.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        capsules,
    }
}

impl NativeReport {
    pub fn is_rejected(&self) -> bool {
        self.status == "reject"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex native/GPU capsule review\n");
        out.push_str(&format!(
            "schema={} file={} status={} capsules={}\n",
            self.schema,
            self.file,
            self.status,
            self.capsules.len()
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  - {} {} lang={} risk={} toolchain={} target={} gpu_arch={} memory_boundary={} fallback_fixtures={} decision={}\n",
                capsule.name,
                capsule.id,
                capsule.language,
                capsule.risk_class,
                capsule.toolchain,
                capsule.target_env.as_deref().unwrap_or("-"),
                capsule.gpu_arch.as_deref().unwrap_or("-"),
                capsule.memory_boundary,
                capsule.fallback_fixtures,
                capsule.decision
            ));
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
                "      \"risk_class\": {},\n",
                json::string(&capsule.risk_class)
            ));
            out.push_str(&format!(
                "      \"toolchain\": {},\n",
                json::string(&capsule.toolchain)
            ));
            out.push_str(&format!(
                "      \"target_env\": {},\n",
                json::opt_string(capsule.target_env.as_deref())
            ));
            out.push_str(&format!(
                "      \"gpu_arch\": {},\n",
                json::opt_string(capsule.gpu_arch.as_deref())
            ));
            out.push_str(&format!(
                "      \"memory_boundary\": {},\n",
                capsule.memory_boundary
            ));
            out.push_str(&format!(
                "      \"fallback_fixtures\": {},\n",
                capsule.fallback_fixtures
            ));
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json::string_array(&capsule.effects)
            ));
            out.push_str(&format!(
                "      \"decision\": {},\n",
                json::string(&capsule.decision)
            ));
            out.push_str(&format!(
                "      \"notes\": {}\n",
                json::string_array(&capsule.notes)
            ));
            out.push_str("    }");
        }
        if !self.capsules.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}
