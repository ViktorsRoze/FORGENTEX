//! Capsule sandbox policy model and effect reconciliation (v2.3).
//!
//! v2.3 makes the constrained environment explicit and checkable without
//! invoking a foreign runtime. A capsule's `sandbox` rules are classified
//! into capability grants (filesystem, network, process, environment,
//! chain, gpu), resource limits (`limit.*`), and explicit `.none`
//! denials. Each declared effect must be covered by a matching grant, so a
//! capsule cannot claim `network.read` while sandboxed to `net.none`.
//!
//! Real OS-level isolation and process execution remain deferred; v2.3
//! owns the policy contract and the enforcement plan that a future
//! sandbox executor will apply, and it gates mock execution on approval.

use crate::ast::{Capsule, Program};
use crate::diag::Diagnostic;
use crate::json;

/// Categories that require an explicit sandbox grant before a capsule may
/// declare a matching effect.
const GRANTED_CATEGORIES: &[&str] = &["fs", "net", "proc", "env", "chain", "gpu"];

const LIMIT_KEYS: &[&str] = &["cpu_ms", "mem_mb", "wall_ms", "fs_bytes", "net_bytes"];

/// The capability category a sandbox rule grants, or `None` for `.none`
/// denials and `limit.*` rules (which grant no access).
fn grant_category(capability: &str) -> Option<String> {
    if capability.ends_with(".none") {
        return None;
    }
    let head = capability.split('.').next().unwrap_or(capability);
    if head == "limit" {
        return None;
    }
    Some(head.to_string())
}

/// The capability category an effect requires. Compute-only effects
/// (`alloc`, `cpu.*`, `crypto.*`, `memory.*`) need no grant.
fn effect_category(effect: &str) -> String {
    let head = effect.split('.').next().unwrap_or(effect);
    match head {
        "network" | "net" => "net".to_string(),
        "io" | "fs" => "fs".to_string(),
        "proc" | "process" | "subprocess" => "proc".to_string(),
        "env" => "env".to_string(),
        "chain" => "chain".to_string(),
        "gpu" => "gpu".to_string(),
        _ => "compute".to_string(),
    }
}

fn split_rule(rule: &str) -> (String, String) {
    match rule.trim().split_once(char::is_whitespace) {
        Some((cap, scope)) => (cap.to_string(), scope.trim().to_string()),
        None => (rule.trim().to_string(), String::new()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleSandbox {
    pub id: String,
    pub name: String,
    pub risk_class: String,
    pub grants: Vec<(String, String)>,
    pub limits: Vec<(String, String)>,
    pub effects: Vec<String>,
    pub uncovered_effects: Vec<String>,
    pub unused_grants: Vec<String>,
    pub invalid_limits: Vec<String>,
    pub decision: String,
}

pub fn analyze_capsule(capsule: &Capsule) -> CapsuleSandbox {
    let mut grants = Vec::new();
    let mut limits = Vec::new();
    let mut invalid_limits = Vec::new();
    let mut grant_categories: Vec<String> = Vec::new();
    for rule in &capsule.sandbox {
        let (capability, scope) = split_rule(rule);
        if let Some(key) = capability.strip_prefix("limit.") {
            if LIMIT_KEYS.contains(&key) && scope.parse::<u64>().is_ok() {
                limits.push((key.to_string(), scope));
            } else {
                invalid_limits.push(rule.clone());
            }
            continue;
        }
        grants.push((capability.clone(), scope));
        if let Some(category) = grant_category(&capability) {
            if !grant_categories.contains(&category) {
                grant_categories.push(category);
            }
        }
    }

    let mut uncovered_effects = Vec::new();
    let mut required_categories: Vec<String> = Vec::new();
    for effect in &capsule.effects {
        let category = effect_category(effect);
        if !GRANTED_CATEGORIES.contains(&category.as_str()) {
            continue;
        }
        if !required_categories.contains(&category) {
            required_categories.push(category.clone());
        }
        if !grant_categories.contains(&category) {
            uncovered_effects.push(effect.clone());
        }
    }

    let unused_grants = grant_categories
        .iter()
        .filter(|category| {
            GRANTED_CATEGORIES.contains(&category.as_str())
                && !required_categories.contains(*category)
        })
        .cloned()
        .collect::<Vec<_>>();

    let high_risk = capsule.risk_class() != "managed";
    let decision = if !uncovered_effects.is_empty() || !invalid_limits.is_empty() {
        "deny"
    } else if high_risk || !unused_grants.is_empty() {
        "review"
    } else {
        "allow"
    };

    CapsuleSandbox {
        id: capsule.id.clone(),
        name: capsule.name.clone(),
        risk_class: capsule.risk_class().to_string(),
        grants,
        limits,
        effects: capsule.effects.clone(),
        uncovered_effects,
        unused_grants,
        invalid_limits,
        decision: decision.to_string(),
    }
}

/// Blocking sandbox diagnostics, surfaced through `fgx check`.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        let plan = analyze_capsule(capsule);
        for effect in &plan.uncovered_effects {
            diagnostics.push(
                Diagnostic::new(
                    "sandbox_effect_uncovered",
                    &capsule.id,
                    format!(
                        "capsule '{}' declares effect '{}' but its sandbox grants no matching {} access",
                        capsule.name,
                        effect,
                        effect_category(effect)
                    ),
                )
                .suggest("add a matching sandbox grant, for example sandbox \"net.connect <host>\""),
            );
        }
        for rule in &plan.invalid_limits {
            diagnostics.push(
                Diagnostic::new(
                    "sandbox_invalid_limit",
                    &capsule.id,
                    format!(
                        "capsule '{}' has an invalid resource limit '{}'",
                        capsule.name, rule
                    ),
                )
                .suggest("use limit.<cpu_ms|mem_mb|wall_ms|fs_bytes|net_bytes> <integer>"),
            );
        }
    }
    diagnostics
}

/// Whether a capsule is approved to run under its declared sandbox.
pub fn is_approved(capsule: &Capsule) -> bool {
    analyze_capsule(capsule).decision != "deny"
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPlan {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub capsules: Vec<CapsuleSandbox>,
}

pub fn analyze(file: &str, program: &Program) -> SandboxPlan {
    let capsules: Vec<CapsuleSandbox> = program.capsules().map(analyze_capsule).collect();
    let status = if capsules.iter().any(|c| c.decision == "deny") {
        "deny"
    } else if capsules.iter().any(|c| c.decision == "review") {
        "review"
    } else {
        "allow"
    };
    SandboxPlan {
        schema: "forgentex.sandbox_plan.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        capsules,
    }
}

impl SandboxPlan {
    pub fn is_denied(&self) -> bool {
        self.status == "deny"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex sandbox plan\n");
        out.push_str(&format!(
            "schema={} file={} status={} capsules={}\n",
            self.schema,
            self.file,
            self.status,
            self.capsules.len()
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  - {} {} risk={} decision={}\n",
                capsule.name, capsule.id, capsule.risk_class, capsule.decision
            ));
            for (capability, scope) in &capsule.grants {
                if scope.is_empty() {
                    out.push_str(&format!("      grant {capability}\n"));
                } else {
                    out.push_str(&format!("      grant {capability} {scope}\n"));
                }
            }
            for (key, value) in &capsule.limits {
                out.push_str(&format!("      limit {key}={value}\n"));
            }
            if !capsule.uncovered_effects.is_empty() {
                out.push_str(&format!(
                    "      uncovered_effects=[{}]\n",
                    capsule.uncovered_effects.join(",")
                ));
            }
            if !capsule.unused_grants.is_empty() {
                out.push_str(&format!(
                    "      unused_grants=[{}]\n",
                    capsule.unused_grants.join(",")
                ));
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
                "      \"risk_class\": {},\n",
                json::string(&capsule.risk_class)
            ));
            out.push_str(&format!(
                "      \"decision\": {},\n",
                json::string(&capsule.decision)
            ));
            out.push_str("      \"grants\": [");
            for (grant_idx, (capability, scope)) in capsule.grants.iter().enumerate() {
                if grant_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"capability\": {}, \"scope\": {}}}",
                    json::string(capability),
                    json::string(scope)
                ));
            }
            out.push_str("],\n");
            out.push_str("      \"limits\": [");
            for (limit_idx, (key, value)) in capsule.limits.iter().enumerate() {
                if limit_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"key\": {}, \"value\": {}}}",
                    json::string(key),
                    json::string(value)
                ));
            }
            out.push_str("],\n");
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json::string_array(&capsule.effects)
            ));
            out.push_str(&format!(
                "      \"uncovered_effects\": {},\n",
                json::string_array(&capsule.uncovered_effects)
            ));
            out.push_str(&format!(
                "      \"unused_grants\": {}\n",
                json::string_array(&capsule.unused_grants)
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
