//! Smart-contract capsule review (v2.8).
//!
//! Solidity and Vyper capsules (`smart_contract` risk class) deploy code to
//! a blockchain. v2.8 reviews them with compiler pins, EVM target metadata,
//! a required ABI hash, gas evidence, and the non-negotiable safety gate:
//! a contract capsule may not request secret-key or wallet access. Contract
//! capsules are always at least `review` and never auto-approved.

use crate::ast::{Capsule, Program};
use crate::diag::Diagnostic;
use crate::json;

fn is_contract(capsule: &Capsule) -> bool {
    capsule.risk_class() == "smart_contract"
}

/// A sandbox rule `<key> <value>` whose capability matches `prefix`.
fn sandbox_value(capsule: &Capsule, prefix: &str) -> Option<String> {
    capsule.sandbox.iter().find_map(|rule| {
        let rule = rule.trim();
        rule.strip_prefix(prefix)
            .map(|rest| rest.trim().to_string())
    })
}

/// Tokens that indicate secret-key / wallet access in an effect or grant.
const SECRET_TOKENS: &[&str] = &[
    "secret",
    "wallet",
    "privatekey",
    "private_key",
    "keystore",
    "crypto.sign",
    "signer",
];

fn mentions_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    SECRET_TOKENS.iter().any(|token| lower.contains(token))
}

fn secret_access(capsule: &Capsule) -> Vec<String> {
    let mut hits = Vec::new();
    for effect in &capsule.effects {
        if mentions_secret(effect) {
            hits.push(format!("effect {effect}"));
        }
    }
    for rule in &capsule.sandbox {
        if mentions_secret(rule) {
            hits.push(format!("sandbox {rule}"));
        }
    }
    hits
}

/// Blocking contract diagnostics, surfaced through `fgx check`.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        if !is_contract(capsule) {
            continue;
        }
        if sandbox_value(capsule, "abi.hash").is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "contract_capsule_no_abi_hash",
                    &capsule.id,
                    format!(
                        "smart-contract capsule '{}' must declare an ABI hash",
                        capsule.name
                    ),
                )
                .suggest("add a sandbox rule such as sandbox \"abi.hash sha256:<digest>\""),
            );
        }
        for hit in secret_access(capsule) {
            diagnostics.push(
                Diagnostic::new(
                    "contract_capsule_secret_key",
                    &capsule.id,
                    format!(
                        "smart-contract capsule '{}' requests secret-key/wallet access ({hit}); this is denied",
                        capsule.name
                    ),
                )
                .suggest("remove secret-key/wallet effects and grants from contract capsules"),
            );
        }
    }
    diagnostics
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub compiler: String,
    pub evm_target: Option<String>,
    pub abi_hash: Option<String>,
    pub gas_limit: Option<String>,
    pub effects: Vec<String>,
    pub secret_access: Vec<String>,
    pub decision: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub capsules: Vec<ContractEntry>,
}

pub fn analyze(file: &str, program: &Program) -> ContractReport {
    let mut capsules = Vec::new();
    for capsule in program.capsules() {
        if !is_contract(capsule) {
            continue;
        }
        let abi_hash = sandbox_value(capsule, "abi.hash");
        let gas_limit = sandbox_value(capsule, "gas.limit");
        let secret = secret_access(capsule);
        let mut notes = Vec::new();
        let mut reject = false;

        if abi_hash.is_none() {
            notes.push("missing ABI hash: contract capsule rejected".to_string());
            reject = true;
        }
        if !secret.is_empty() {
            notes.push("secret-key/wallet access requested: denied".to_string());
            reject = true;
        }
        if gas_limit.is_none() {
            notes.push(
                "no declared gas limit: add sandbox \"gas.limit <n>\" as gas evidence".to_string(),
            );
        }
        notes.push(
            "smart-contract capsule: on-chain deployment requires human approval".to_string(),
        );

        let decision = if reject { "reject" } else { "review" };
        capsules.push(ContractEntry {
            id: capsule.id.clone(),
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            compiler: capsule.version.clone(),
            evm_target: capsule.env.clone(),
            abi_hash,
            gas_limit,
            effects: capsule.effects.clone(),
            secret_access: secret,
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
    ContractReport {
        schema: "forgentex.contract_review.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        capsules,
    }
}

impl ContractReport {
    pub fn is_rejected(&self) -> bool {
        self.status == "reject"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex smart-contract capsule review\n");
        out.push_str(&format!(
            "schema={} file={} status={} capsules={}\n",
            self.schema,
            self.file,
            self.status,
            self.capsules.len()
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  - {} {} lang={} compiler={} evm={} abi_hash={} gas_limit={} decision={}\n",
                capsule.name,
                capsule.id,
                capsule.language,
                capsule.compiler,
                capsule.evm_target.as_deref().unwrap_or("-"),
                capsule.abi_hash.as_deref().unwrap_or("-"),
                capsule.gas_limit.as_deref().unwrap_or("-"),
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
                "      \"compiler\": {},\n",
                json::string(&capsule.compiler)
            ));
            out.push_str(&format!(
                "      \"evm_target\": {},\n",
                json::opt_string(capsule.evm_target.as_deref())
            ));
            out.push_str(&format!(
                "      \"abi_hash\": {},\n",
                json::opt_string(capsule.abi_hash.as_deref())
            ));
            out.push_str(&format!(
                "      \"gas_limit\": {},\n",
                json::opt_string(capsule.gas_limit.as_deref())
            ));
            out.push_str(&format!(
                "      \"effects\": {},\n",
                json::string_array(&capsule.effects)
            ));
            out.push_str(&format!(
                "      \"secret_access\": {},\n",
                json::string_array(&capsule.secret_access)
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
