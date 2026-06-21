//! Capsule dependency locking and supply-chain audit (v2.3).
//!
//! Capsule dependencies reference external package managers and GitHub
//! starting projects. v2.3 parses each reference into structured fields
//! (ecosystem, name, version or revision, checksum), produces a
//! deterministic lockfile, and grades supply-chain risk so capsules can
//! only depend on pinned, ideally checksummed, sources.
//!
//! Reference grammar (best effort, lossless `raw` retained):
//!
//! ```text
//! <ecosystem>:<name>[==<version>|@<revision>][#<algo>:<digest>]
//! pypi:numpy==2.2.0#sha256:...
//! github:org/repo@<40-hex-commit>
//! npm:preact@10.24.0#sha512:...
//! ```

use std::collections::BTreeMap;

use crate::ast::{CapsuleDep, Program};
use crate::json;

const CHECKSUM_ALGOS: &[&str] = &["sha256", "sha512", "blake3"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyRef {
    pub raw: String,
    pub ecosystem: String,
    pub name: String,
    pub version: Option<String>,
    pub revision: Option<String>,
    pub checksum_algo: Option<String>,
    pub checksum: Option<String>,
    pub pinned: bool,
}

impl DependencyRef {
    /// low: pinned with a checksum. medium: pinned without a checksum.
    /// high: unpinned or a floating reference.
    pub fn risk(&self) -> &'static str {
        if !self.pinned {
            "high"
        } else if self.checksum.is_some() {
            "low"
        } else {
            "medium"
        }
    }
}

fn is_commitish(value: &str) -> bool {
    value.len() >= 7 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn parse_dependency(dep: &CapsuleDep) -> DependencyRef {
    let raw = dep.source.clone();
    let pinned = dep.is_pinned();

    let (head, checksum_part) = match raw.split_once('#') {
        Some((head, checksum)) => (head, Some(checksum)),
        None => (raw.as_str(), None),
    };
    let (checksum_algo, checksum) = match checksum_part {
        Some(part) => match part.split_once(':') {
            Some((algo, digest)) if CHECKSUM_ALGOS.contains(&algo) => {
                (Some(algo.to_string()), Some(digest.to_string()))
            }
            _ => (None, Some(part.to_string())),
        },
        None => (None, None),
    };

    let (ecosystem, rest) = match head.split_once(':') {
        Some((ecosystem, rest)) => (ecosystem.to_string(), rest),
        None => ("unknown".to_string(), head),
    };

    let (name, version, revision) = if let Some((name, version)) = rest.split_once("==") {
        (name.to_string(), Some(version.to_string()), None)
    } else if let Some((name, at)) = rest.split_once('@') {
        if ecosystem == "github" || is_commitish(at) {
            (name.to_string(), None, Some(at.to_string()))
        } else {
            (name.to_string(), Some(at.to_string()), None)
        }
    } else {
        (rest.to_string(), None, None)
    };

    DependencyRef {
        raw,
        ecosystem,
        name,
        version,
        revision,
        checksum_algo,
        checksum,
        pinned,
    }
}

/// Collect every capsule dependency, deduplicated by raw reference, with
/// the set of capsules that require it (sorted for determinism).
fn collect(program: &Program) -> Vec<(DependencyRef, Vec<String>)> {
    let mut by_raw: BTreeMap<String, (DependencyRef, Vec<String>)> = BTreeMap::new();
    for capsule in program.capsules() {
        for dep in &capsule.deps {
            let parsed = parse_dependency(dep);
            let entry = by_raw
                .entry(parsed.raw.clone())
                .or_insert_with(|| (parsed, Vec::new()));
            if !entry.1.contains(&capsule.name) {
                entry.1.push(capsule.name.clone());
            }
        }
    }
    by_raw.into_values().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockReport {
    pub schema: String,
    pub file: String,
    pub dependencies: Vec<(DependencyRef, Vec<String>)>,
}

pub fn lock(file: &str, program: &Program) -> LockReport {
    LockReport {
        schema: "forgentex.capsule_lock.v1".to_string(),
        file: file.to_string(),
        dependencies: collect(program),
    }
}

impl LockReport {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule lock\n");
        out.push_str(&format!(
            "schema={} file={} dependencies={}\n",
            self.schema,
            self.file,
            self.dependencies.len()
        ));
        for (dep, required_by) in &self.dependencies {
            out.push_str(&format!(
                "  - {} ecosystem={} name={} version={} revision={} checksum={} pinned={} required_by=[{}]\n",
                dep.raw,
                dep.ecosystem,
                dep.name,
                dep.version.as_deref().unwrap_or("-"),
                dep.revision.as_deref().unwrap_or("-"),
                dep.checksum_algo.as_deref().unwrap_or("-"),
                dep.pinned,
                required_by.join(",")
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str("  \"dependencies\": [");
        for (idx, (dep, required_by)) in self.dependencies.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {\n");
            push_dependency_fields(&mut out, dep);
            out.push_str(&format!(
                "      \"required_by\": {}\n",
                json::string_array(required_by)
            ));
            out.push_str("    }");
        }
        if !self.dependencies.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}

fn push_dependency_fields(out: &mut String, dep: &DependencyRef) {
    out.push_str(&format!(
        "      \"reference\": {},\n",
        json::string(&dep.raw)
    ));
    out.push_str(&format!(
        "      \"ecosystem\": {},\n",
        json::string(&dep.ecosystem)
    ));
    out.push_str(&format!("      \"name\": {},\n", json::string(&dep.name)));
    out.push_str(&format!(
        "      \"version\": {},\n",
        json::opt_string(dep.version.as_deref())
    ));
    out.push_str(&format!(
        "      \"revision\": {},\n",
        json::opt_string(dep.revision.as_deref())
    ));
    out.push_str(&format!(
        "      \"checksum_algo\": {},\n",
        json::opt_string(dep.checksum_algo.as_deref())
    ));
    out.push_str(&format!(
        "      \"checksum\": {},\n",
        json::opt_string(dep.checksum.as_deref())
    ));
    out.push_str(&format!("      \"pinned\": {},\n", dep.pinned));
    out.push_str(&format!("      \"risk\": {},\n", json::string(dep.risk())));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub dependencies: Vec<(DependencyRef, Vec<String>)>,
}

pub fn audit(file: &str, program: &Program) -> AuditReport {
    let dependencies = collect(program);
    let status = if dependencies.iter().any(|(dep, _)| dep.risk() == "high") {
        "high"
    } else if dependencies.iter().any(|(dep, _)| dep.risk() == "medium") {
        "medium"
    } else {
        "low"
    };
    AuditReport {
        schema: "forgentex.capsule_audit.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        dependencies,
    }
}

impl AuditReport {
    pub fn has_high_risk(&self) -> bool {
        self.status == "high"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule supply-chain audit\n");
        out.push_str(&format!(
            "schema={} file={} status={} dependencies={}\n",
            self.schema,
            self.file,
            self.status,
            self.dependencies.len()
        ));
        for (dep, required_by) in &self.dependencies {
            out.push_str(&format!(
                "  - {} risk={} pinned={} checksum={} required_by=[{}]\n",
                dep.raw,
                dep.risk(),
                dep.pinned,
                dep.checksum_algo.as_deref().unwrap_or("none"),
                required_by.join(",")
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str(&format!("  \"status\": {},\n", json::string(&self.status)));
        out.push_str("  \"dependencies\": [");
        for (idx, (dep, required_by)) in self.dependencies.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {\n");
            push_dependency_fields(&mut out, dep);
            out.push_str(&format!(
                "      \"required_by\": {}\n",
                json::string_array(required_by)
            ));
            out.push_str("    }");
        }
        if !self.dependencies.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}
