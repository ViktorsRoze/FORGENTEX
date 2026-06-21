//! Agent workflow integration for capsules (v2.9).
//!
//! Agents need to search, slice, and review foreign capsules without
//! reading whole projects. This module provides two compact,
//! machine-readable surfaces:
//!
//! - an **inventory** of every capsule in a file (name, language, risk
//!   class, effect/dependency counts, and an aggregated decision), and
//! - a **context packet** for one capsule that merges every v2.0-v2.8
//!   review — contract validity, sandbox decision, dependency risk, and
//!   the class-specific native/shell/contract review — into a single
//!   decision plus a human-readable risk summary.
//!
//! It is read-only aggregation over the existing analyzers; the blocking
//! rules live in `fgx check`.

use crate::ast::{Capsule, Program};
use crate::json;

/// reject > review > allow; the worst signal wins.
fn worse<'a>(a: &'a str, b: &'a str) -> &'a str {
    let rank = |d: &str| match d {
        "reject" => 2,
        "review" => 1,
        _ => 0,
    };
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

fn capsule_dependency_risk(capsule: &Capsule) -> &'static str {
    let mut risk = "low";
    for dep in &capsule.deps {
        let parsed = crate::capsule_lock::parse_dependency(dep);
        risk = match (risk, parsed.risk()) {
            (_, "high") => "high",
            ("low", "medium") => "medium",
            (current, _) => current,
        };
    }
    risk
}

fn risk_to_decision(risk: &str) -> &'static str {
    match risk {
        "high" => "reject",
        "medium" => "review",
        _ => "allow",
    }
}

fn contract_status_to_decision(status: &str) -> &'static str {
    match status {
        "rejected" => "reject",
        "review_required" => "review",
        _ => "allow",
    }
}

/// The class-specific review decision (native/shell/contract), if any.
fn class_review(file: &str, program: &Program, capsule: &Capsule) -> Option<(String, String)> {
    match capsule.risk_class() {
        "native" | "gpu" => crate::native_capsule::analyze(file, program)
            .capsules
            .into_iter()
            .find(|c| c.name == capsule.name)
            .map(|c| ("native".to_string(), c.decision)),
        "shell" => crate::shell::analyze(file, program)
            .capsules
            .into_iter()
            .find(|c| c.name == capsule.name)
            .map(|c| ("shell".to_string(), c.decision)),
        "smart_contract" => crate::contract::analyze(file, program)
            .capsules
            .into_iter()
            .find(|c| c.name == capsule.name)
            .map(|c| ("contract".to_string(), c.decision)),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub risk_class: String,
    pub effects: usize,
    pub dependencies: usize,
    pub bindings: usize,
    pub fixtures: usize,
    pub decision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleInventory {
    pub schema: String,
    pub file: String,
    pub capsules: Vec<InventoryEntry>,
}

pub fn inventory(file: &str, program: &Program) -> CapsuleInventory {
    let capsules = program
        .capsules()
        .map(|capsule| {
            let context = context(file, program, &capsule.name).expect("named capsule exists");
            InventoryEntry {
                id: capsule.id.clone(),
                name: capsule.name.clone(),
                language: capsule.language.clone(),
                risk_class: capsule.risk_class().to_string(),
                effects: capsule.effects.len(),
                dependencies: capsule.deps.len(),
                bindings: capsule.bindings.len(),
                fixtures: capsule.fixtures.len(),
                decision: context.overall_decision,
            }
        })
        .collect();
    CapsuleInventory {
        schema: "forgentex.capsule_inventory.v1".to_string(),
        file: file.to_string(),
        capsules,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleContext {
    pub schema: String,
    pub file: String,
    pub id: String,
    pub name: String,
    pub language: String,
    pub risk_class: String,
    pub effects: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub bindings: Vec<String>,
    pub contract_status: String,
    pub sandbox_decision: String,
    pub dependency_risk: String,
    pub class_review: Option<(String, String)>,
    pub overall_decision: String,
    pub risk_summary: Vec<String>,
}

pub fn context(file: &str, program: &Program, name: &str) -> Result<CapsuleContext, String> {
    let capsule = program
        .capsules()
        .find(|capsule| capsule.name == name)
        .ok_or_else(|| format!("no capsule named '{name}'"))?;

    let contract_status = crate::capsule::analyze(file, program)
        .capsules
        .into_iter()
        .find(|c| c.name == name)
        .map(|c| c.status)
        .unwrap_or_else(|| "pass".to_string());
    let sandbox_decision = crate::sandbox::analyze_capsule(capsule).decision;
    let dependency_risk = capsule_dependency_risk(capsule).to_string();
    let class = class_review(file, program, capsule);

    let mut overall = "allow";
    overall = worse(overall, contract_status_to_decision(&contract_status));
    overall = worse(overall, &sandbox_decision);
    overall = worse(overall, risk_to_decision(&dependency_risk));
    if let Some((_, decision)) = &class {
        overall = worse(overall, decision);
    }
    let overall_decision = overall.to_string();

    let mut risk_summary = Vec::new();
    risk_summary.push(format!("{} capsule", capsule.risk_class()));
    if capsule.bindings.iter().any(|b| b.mode.is_writer()) {
        risk_summary.push("writes shared state (rw binding)".to_string());
    }
    if dependency_risk != "low" {
        risk_summary.push(format!("dependency risk: {dependency_risk}"));
    }
    if sandbox_decision != "allow" {
        risk_summary.push(format!("sandbox: {sandbox_decision}"));
    }
    if let Some((kind, decision)) = &class {
        risk_summary.push(format!("{kind} review: {decision}"));
    }
    risk_summary.push("native backend: not lowerable".to_string());

    Ok(CapsuleContext {
        schema: "forgentex.capsule_context.v1".to_string(),
        file: file.to_string(),
        id: capsule.id.clone(),
        name: capsule.name.clone(),
        language: capsule.language.clone(),
        risk_class: capsule.risk_class().to_string(),
        effects: capsule.effects.clone(),
        inputs: capsule
            .inputs
            .iter()
            .map(|b| format!("{}: {}", b.name, b.ty.to_agent()))
            .collect(),
        outputs: capsule
            .outputs
            .iter()
            .map(|b| format!("{}: {}", b.name, b.ty.to_agent()))
            .collect(),
        bindings: capsule
            .bindings
            .iter()
            .map(|b| format!("{}:{}", b.name, b.mode.as_str()))
            .collect(),
        contract_status,
        sandbox_decision,
        dependency_risk,
        class_review: class,
        overall_decision,
        risk_summary,
    })
}

impl CapsuleInventory {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule inventory\n");
        out.push_str(&format!(
            "schema={} file={} capsules={}\n",
            self.schema,
            self.file,
            self.capsules.len()
        ));
        for c in &self.capsules {
            out.push_str(&format!(
                "  - {} {} lang={} risk={} effects={} deps={} bindings={} fixtures={} decision={}\n",
                c.name, c.id, c.language, c.risk_class, c.effects, c.dependencies, c.bindings, c.fixtures, c.decision
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str("  \"capsules\": [");
        for (idx, c) in self.capsules.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str("\n    {");
            out.push_str(&format!("\"id\": {}, ", json::string(&c.id)));
            out.push_str(&format!("\"name\": {}, ", json::string(&c.name)));
            out.push_str(&format!("\"language\": {}, ", json::string(&c.language)));
            out.push_str(&format!(
                "\"risk_class\": {}, ",
                json::string(&c.risk_class)
            ));
            out.push_str(&format!("\"effects\": {}, ", c.effects));
            out.push_str(&format!("\"dependencies\": {}, ", c.dependencies));
            out.push_str(&format!("\"bindings\": {}, ", c.bindings));
            out.push_str(&format!("\"fixtures\": {}, ", c.fixtures));
            out.push_str(&format!("\"decision\": {}", json::string(&c.decision)));
            out.push('}');
        }
        if !self.capsules.is_empty() {
            out.push_str("\n  ");
        }
        out.push_str("]\n}");
        out
    }
}

impl CapsuleContext {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex capsule context\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} lang={} risk={} decision={}\n",
            self.schema, self.file, self.id, self.language, self.risk_class, self.overall_decision
        ));
        out.push_str(&format!(
            "  effects=[{}] inputs=[{}] outputs=[{}] bindings=[{}]\n",
            self.effects.join(","),
            self.inputs.join(","),
            self.outputs.join(","),
            self.bindings.join(",")
        ));
        out.push_str(&format!(
            "  contract={} sandbox={} dependency_risk={}{}\n",
            self.contract_status,
            self.sandbox_decision,
            self.dependency_risk,
            self.class_review
                .as_ref()
                .map(|(k, d)| format!(" {k}={d}"))
                .unwrap_or_default()
        ));
        for note in &self.risk_summary {
            out.push_str(&format!("  risk: {note}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json::string(&self.schema)));
        out.push_str(&format!("  \"file\": {},\n", json::string(&self.file)));
        out.push_str(&format!("  \"capsule\": {},\n", json::string(&self.id)));
        out.push_str(&format!("  \"name\": {},\n", json::string(&self.name)));
        out.push_str(&format!(
            "  \"language\": {},\n",
            json::string(&self.language)
        ));
        out.push_str(&format!(
            "  \"risk_class\": {},\n",
            json::string(&self.risk_class)
        ));
        out.push_str(&format!(
            "  \"effects\": {},\n",
            json::string_array(&self.effects)
        ));
        out.push_str(&format!(
            "  \"inputs\": {},\n",
            json::string_array(&self.inputs)
        ));
        out.push_str(&format!(
            "  \"outputs\": {},\n",
            json::string_array(&self.outputs)
        ));
        out.push_str(&format!(
            "  \"bindings\": {},\n",
            json::string_array(&self.bindings)
        ));
        out.push_str(&format!(
            "  \"contract_status\": {},\n",
            json::string(&self.contract_status)
        ));
        out.push_str(&format!(
            "  \"sandbox_decision\": {},\n",
            json::string(&self.sandbox_decision)
        ));
        out.push_str(&format!(
            "  \"dependency_risk\": {},\n",
            json::string(&self.dependency_risk)
        ));
        out.push_str(&format!(
            "  \"class_review\": {},\n",
            self.class_review
                .as_ref()
                .map(|(k, d)| format!(
                    "{{\"kind\": {}, \"decision\": {}}}",
                    json::string(k),
                    json::string(d)
                ))
                .unwrap_or_else(|| "null".to_string())
        ));
        out.push_str(&format!(
            "  \"overall_decision\": {},\n",
            json::string(&self.overall_decision)
        ));
        out.push_str(&format!(
            "  \"risk_summary\": {}\n",
            json::string_array(&self.risk_summary)
        ));
        out.push('}');
        out
    }
}
