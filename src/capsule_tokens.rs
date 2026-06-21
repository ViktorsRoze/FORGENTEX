//! Polyglot token efficiency measurement (capsule boundary vs body).
//!
//! Foreign capsule code is verbatim — it must stay valid in its own language,
//! so none of Forgentex's encoding tricks (dense `.fgai`, prelude aliases,
//! compression) can shrink it. The only token lever for foreign code is
//! **avoidance**: keep the agent's working set to each capsule's typed
//! *boundary card*, not its body, and pull the body only when editing it.
//!
//! This module quantifies that. For each capsule it measures the verbatim
//! body against a compact boundary card; the `equalizer` (body / boundary)
//! is how much the card saves, and is largest exactly where a language is
//! most verbose. It also measures the shared-namespace dedup: a type declared
//! once in Forgentex versus restating it in every language that binds it.

use crate::ast::{Capsule, Program, ShareDecl};
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleTokens {
    pub name: String,
    pub language: String,
    pub body_lines: usize,
    pub body_tokens: usize,
    pub boundary_tokens: usize,
}

impl CapsuleTokens {
    /// Body tokens per boundary token: how much the card saves over the body.
    pub fn equalizer(&self) -> f64 {
        if self.boundary_tokens == 0 {
            0.0
        } else {
            self.body_tokens as f64 / self.boundary_tokens as f64
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SharedTypeTokens {
    pub name: String,
    pub ty: String,
    pub bindings: usize,
    pub declared_once_tokens: usize,
    pub restated_estimate_tokens: usize,
    pub saved_tokens: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleTokenReport {
    pub schema: String,
    pub file: String,
    pub capsules: Vec<CapsuleTokens>,
    pub total_body_tokens: usize,
    pub total_boundary_tokens: usize,
    pub shared_types: Vec<SharedTypeTokens>,
    pub shared_saved_tokens: usize,
}

/// A compact, language-agnostic boundary card — the agent's working-set view.
pub fn boundary_card(capsule: &Capsule) -> String {
    let render = |bindings: &[crate::ast::CapsuleBinding]| {
        bindings
            .iter()
            .map(|b| format!("{} {}", b.name, b.ty.to_agent()))
            .collect::<Vec<_>>()
            .join(",")
    };
    let effects = capsule
        .effects
        .iter()
        .map(|e| format!("!{e}"))
        .collect::<Vec<_>>()
        .join(" ");
    let deps = if capsule.deps.is_empty() {
        String::new()
    } else {
        format!(" deps#{}", capsule.deps.len())
    };
    format!(
        "@{} {} in=({}) out=({}) {}{}",
        capsule.name,
        capsule.language,
        render(&capsule.inputs),
        render(&capsule.outputs),
        effects,
        deps
    )
}

fn shared_declaration(share: &ShareDecl) -> String {
    format!("share {}: {}", share.name, share.ty.to_agent())
}

pub fn report(file: &str, program: &Program) -> CapsuleTokenReport {
    let mut capsules = Vec::new();
    let mut total_body_tokens = 0;
    let mut total_boundary_tokens = 0;
    for capsule in program.capsules() {
        let body = capsule.code.join("\n");
        let body_tokens = estimate(&body);
        let boundary_tokens = estimate(&boundary_card(capsule));
        total_body_tokens += body_tokens;
        total_boundary_tokens += boundary_tokens;
        capsules.push(CapsuleTokens {
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            body_lines: capsule.code.len(),
            body_tokens,
            boundary_tokens,
        });
    }

    // Shared-namespace dedup: a type declared once in Forgentex vs restated
    // in every language that binds it. The per-language restatement is at
    // least as large as the Forgentex declaration (languages are more
    // verbose), so this estimate is conservative.
    let mut shared_types = Vec::new();
    let mut shared_saved_tokens = 0;
    for share in program.shares() {
        let bindings = program
            .capsules()
            .filter(|c| c.bindings.iter().any(|b| b.name == share.name))
            .count();
        let declared_once_tokens = estimate(&shared_declaration(share));
        let restated_estimate_tokens = bindings * declared_once_tokens;
        let saved = restated_estimate_tokens.saturating_sub(declared_once_tokens);
        shared_saved_tokens += saved;
        shared_types.push(SharedTypeTokens {
            name: share.name.clone(),
            ty: share.ty.to_agent(),
            bindings,
            declared_once_tokens,
            restated_estimate_tokens,
            saved_tokens: saved,
        });
    }

    CapsuleTokenReport {
        schema: "forgentex.capsule_tokens.v1".to_string(),
        file: file.to_string(),
        capsules,
        total_body_tokens,
        total_boundary_tokens,
        shared_types,
        shared_saved_tokens,
    }
}

impl CapsuleTokenReport {
    /// Boundary working set as a fraction of reading every body.
    pub fn working_set_ratio(&self) -> f64 {
        if self.total_body_tokens == 0 {
            1.0
        } else {
            self.total_boundary_tokens as f64 / self.total_body_tokens as f64
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex polyglot token efficiency (boundary vs body)\n");
        out.push_str(&format!("schema={} file={}\n", self.schema, self.file));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  @{:<22} {:<10} body={:>4} ({:>2} lines)  card={:>3}  equalizer={:.1}x\n",
                capsule.name,
                capsule.language,
                capsule.body_tokens,
                capsule.body_lines,
                capsule.boundary_tokens,
                capsule.equalizer()
            ));
        }
        out.push_str(&format!(
            "  totals: body={} boundary={} working_set_ratio={:.3} ({:.0}% saved)\n",
            self.total_body_tokens,
            self.total_boundary_tokens,
            self.working_set_ratio(),
            (1.0 - self.working_set_ratio()) * 100.0
        ));
        if !self.shared_types.is_empty() {
            out.push_str("  shared types (declare-once vs restate-per-language):\n");
            for share in &self.shared_types {
                out.push_str(&format!(
                    "    {} : {} bound by {} -> declared={} restated~{} saved~{}\n",
                    share.name,
                    share.ty,
                    share.bindings,
                    share.declared_once_tokens,
                    share.restated_estimate_tokens,
                    share.saved_tokens
                ));
            }
            out.push_str(&format!(
                "    shared total saved~{}\n",
                self.shared_saved_tokens
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let capsules = self
            .capsules
            .iter()
            .map(|c| {
                format!(
                    "{{ \"name\": {}, \"language\": {}, \"body_lines\": {}, \"body_tokens\": {}, \"boundary_tokens\": {}, \"equalizer\": {:.4} }}",
                    crate::json::string(&c.name),
                    crate::json::string(&c.language),
                    c.body_lines,
                    c.body_tokens,
                    c.boundary_tokens,
                    c.equalizer()
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        let shared = self
            .shared_types
            .iter()
            .map(|s| {
                format!(
                    "{{ \"name\": {}, \"type\": {}, \"bindings\": {}, \"declared_once_tokens\": {}, \"restated_estimate_tokens\": {}, \"saved_tokens\": {} }}",
                    crate::json::string(&s.name),
                    crate::json::string(&s.ty),
                    s.bindings,
                    s.declared_once_tokens,
                    s.restated_estimate_tokens,
                    s.saved_tokens
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsules\": [\n    {}\n  ],\n  \"total_body_tokens\": {},\n  \"total_boundary_tokens\": {},\n  \"working_set_ratio\": {:.4},\n  \"shared_types\": [\n    {}\n  ],\n  \"shared_saved_tokens\": {}\n}}",
            crate::json::string(&self.schema),
            crate::json::string(&self.file),
            capsules,
            self.total_body_tokens,
            self.total_boundary_tokens,
            self.working_set_ratio(),
            shared,
            self.shared_saved_tokens,
        )
    }
}
