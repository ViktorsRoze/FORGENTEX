//! Shared typed namespace validation and reporting (v2.2).
//!
//! `share` declarations are Forgentex-owned typed values; capsules `bind`
//! them by name under read or rw mode with snapshot-in / write-back-out
//! semantics. v2.2 owns the contract rules: bindings must resolve to a
//! declared share, a shared name may have at most one writer (until a real
//! concurrency model exists), and a share default must match its type.
//! Shared mutable state is a high-signal review event.

use std::collections::BTreeMap;

use crate::ast::{Program, ShareMode};
use crate::diag::Diagnostic;
use crate::json;

/// Blocking shared-namespace diagnostics, surfaced through `fgx check`.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let shares: BTreeMap<&str, &crate::ast::ShareDecl> = program
        .shares()
        .map(|share| (share.name.as_str(), share))
        .collect();

    // Defaults must match the declared type.
    for share in program.shares() {
        if let Some(default) = &share.default {
            if !default.matches_type(&share.ty) {
                diagnostics.push(
                    Diagnostic::new(
                        "share_default_type",
                        &share.id,
                        format!(
                            "shared '{}' default is {} but the declared type is {}",
                            share.name,
                            default.type_name(),
                            share.ty.to_agent()
                        ),
                    )
                    .suggest("use a default literal matching the declared type"),
                );
            }
        }
    }

    // Bindings must resolve to a declared share.
    let mut writers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for capsule in program.capsules() {
        for binding in &capsule.bindings {
            if !shares.contains_key(binding.name.as_str()) {
                diagnostics.push(
                    Diagnostic::new(
                        "share_unknown_binding",
                        &capsule.id,
                        format!(
                            "capsule '{}' binds unknown shared value '{}'",
                            capsule.name, binding.name
                        ),
                    )
                    .suggest("declare the shared value with `share <name>: <type>`"),
                );
            }
            if binding.mode.is_writer() {
                writers
                    .entry(binding.name.clone())
                    .or_default()
                    .push(capsule.name.clone());
            }
        }
    }

    // At most one writer per shared name.
    for (name, capsules) in &writers {
        if capsules.len() > 1 {
            if let Some(share) = shares.get(name.as_str()) {
                diagnostics.push(
                    Diagnostic::new(
                        "shared_multiple_writers",
                        &share.id,
                        format!(
                            "shared '{}' is written (rw) by multiple capsules: {}",
                            name,
                            capsules.join(", ")
                        ),
                    )
                    .suggest("keep a single rw binding until a concurrency model is defined"),
                );
            }
        }
    }

    diagnostics
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespaceReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub shared: Vec<SharedEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedEntry {
    pub id: String,
    pub name: String,
    pub ty: String,
    pub default: Option<String>,
    pub readers: Vec<String>,
    pub writers: Vec<String>,
    pub diagnostics: Vec<String>,
}

pub fn analyze(file: &str, program: &Program) -> NamespaceReport {
    let all_diagnostics = validate(program);
    let mut shared = Vec::new();
    for share in program.shares() {
        let mut readers = Vec::new();
        let mut writers = Vec::new();
        for capsule in program.capsules() {
            for binding in &capsule.bindings {
                if binding.name != share.name {
                    continue;
                }
                match binding.mode {
                    ShareMode::Read => readers.push(capsule.name.clone()),
                    ShareMode::Rw => writers.push(capsule.name.clone()),
                }
            }
        }
        let diagnostics = all_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.node == share.id)
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>();
        shared.push(SharedEntry {
            id: share.id.clone(),
            name: share.name.clone(),
            ty: share.ty.to_agent(),
            default: share.default.as_ref().map(|value| value.to_literal()),
            readers,
            writers,
            diagnostics,
        });
    }
    let status = if !all_diagnostics.is_empty() {
        "rejected"
    } else if shared.iter().any(|entry| !entry.writers.is_empty()) {
        // Mutable shared state always warrants review.
        "review_required"
    } else {
        "pass"
    };
    NamespaceReport {
        schema: "forgentex.shared_namespace.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        shared,
    }
}

impl NamespaceReport {
    pub fn is_rejected(&self) -> bool {
        self.status == "rejected"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex shared namespace\n");
        out.push_str(&format!(
            "schema={} file={} status={} shared={}\n",
            self.schema,
            self.file,
            self.status,
            self.shared.len()
        ));
        for entry in &self.shared {
            out.push_str(&format!(
                "  - {} {} type={} default={}\n",
                entry.name,
                entry.id,
                entry.ty,
                entry.default.as_deref().unwrap_or("none")
            ));
            out.push_str(&format!(
                "      readers=[{}] writers=[{}]\n",
                entry.readers.join(","),
                entry.writers.join(",")
            ));
            if !entry.diagnostics.is_empty() {
                out.push_str(&format!(
                    "      diagnostics=[{}]\n",
                    entry.diagnostics.join(",")
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
        out.push_str("  \"shared\": [");
        for (idx, entry) in self.shared.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {\n");
            out.push_str(&format!("      \"id\": {},\n", json::string(&entry.id)));
            out.push_str(&format!("      \"name\": {},\n", json::string(&entry.name)));
            out.push_str(&format!("      \"type\": {},\n", json::string(&entry.ty)));
            out.push_str(&format!(
                "      \"default\": {},\n",
                json::opt_string(entry.default.as_deref())
            ));
            out.push_str(&format!(
                "      \"readers\": {},\n",
                json::string_array(&entry.readers)
            ));
            out.push_str(&format!(
                "      \"writers\": {},\n",
                json::string_array(&entry.writers)
            ));
            out.push_str(&format!(
                "      \"diagnostics\": {}\n",
                json::string_array(&entry.diagnostics)
            ));
            out.push_str("    }");
        }
        if !self.shared.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}
