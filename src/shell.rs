//! Shell capsule hardening (v2.7).
//!
//! Bash and PowerShell are the highest-risk capsule class: a shell capsule
//! can run arbitrary host commands. v2.7 requires shell capsules to declare
//! an explicit command allowlist (`exec.allow <cmd>` sandbox rules), forbids
//! wildcard allowlists, and reviews host-path, environment, and network
//! exposure. Shell capsules are always at least `review` and never
//! auto-approved.
//!
//! The command allowlist refines the `proc.*` effect grant from the sandbox
//! model (v2.4): the effect says "this capsule spawns processes"; the
//! allowlist says "only these commands".

use crate::ast::{Capsule, Program};
use crate::diag::Diagnostic;
use crate::json;

fn is_shell(capsule: &Capsule) -> bool {
    capsule.risk_class() == "shell"
}

/// Commands explicitly allowed via `exec.allow <cmd>` sandbox rules.
fn allowed_commands(capsule: &Capsule) -> Vec<String> {
    capsule
        .sandbox
        .iter()
        .filter_map(|rule| rule.trim().strip_prefix("exec.allow "))
        .map(|cmd| cmd.trim().to_string())
        .filter(|cmd| !cmd.is_empty())
        .collect()
}

fn is_wildcard(cmd: &str) -> bool {
    cmd == "*" || cmd == "all" || cmd == "any"
}

/// Sandbox grant scopes for a capability prefix (e.g. all `fs.read`/`fs.write`
/// host paths, or all `env.read` variables).
fn grant_scopes(capsule: &Capsule, prefixes: &[&str]) -> Vec<String> {
    capsule
        .sandbox
        .iter()
        .filter_map(|rule| {
            let (cap, scope) = match rule.trim().split_once(char::is_whitespace) {
                Some((cap, scope)) => (cap, scope.trim()),
                None => (rule.trim(), ""),
            };
            if prefixes.iter().any(|p| cap.starts_with(p)) {
                Some(if scope.is_empty() {
                    cap.to_string()
                } else {
                    format!("{cap} {scope}")
                })
            } else {
                None
            }
        })
        .collect()
}

/// Blocking shell diagnostics, surfaced through `fgx check`.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        if !is_shell(capsule) {
            continue;
        }
        let commands = allowed_commands(capsule);
        if commands.is_empty() {
            diagnostics.push(
                Diagnostic::new(
                    "shell_capsule_no_allowlist",
                    &capsule.id,
                    format!(
                        "shell capsule '{}' must declare an explicit command allowlist",
                        capsule.name
                    ),
                )
                .suggest("add sandbox rules such as sandbox \"exec.allow git\""),
            );
        }
        for cmd in &commands {
            if is_wildcard(cmd) {
                diagnostics.push(
                    Diagnostic::new(
                        "shell_command_wildcard",
                        &capsule.id,
                        format!(
                            "shell capsule '{}' uses a wildcard command allowlist '{}'",
                            capsule.name, cmd
                        ),
                    )
                    .suggest("list each allowed command explicitly instead of a wildcard"),
                );
            }
        }
    }
    diagnostics
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellEntry {
    pub id: String,
    pub name: String,
    pub language: String,
    pub allowed_commands: Vec<String>,
    pub host_paths: Vec<String>,
    pub env_reads: Vec<String>,
    pub network: Vec<String>,
    pub effects: Vec<String>,
    pub decision: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub capsules: Vec<ShellEntry>,
}

pub fn analyze(file: &str, program: &Program) -> ShellReport {
    let mut capsules = Vec::new();
    for capsule in program.capsules() {
        if !is_shell(capsule) {
            continue;
        }
        let commands = allowed_commands(capsule);
        let host_paths = grant_scopes(capsule, &["fs."]);
        let env_reads = grant_scopes(capsule, &["env."]);
        let network = grant_scopes(capsule, &["net."]);

        let mut notes = Vec::new();
        let mut reject = false;
        if commands.is_empty() {
            notes.push("no command allowlist: shell capsule rejected".to_string());
            reject = true;
        }
        if commands.iter().any(|cmd| is_wildcard(cmd)) {
            notes.push("wildcard command allowlist is not permitted".to_string());
            reject = true;
        }
        if !network.is_empty() {
            notes.push("shell capsule requests network access: extra review".to_string());
        }
        if host_paths
            .iter()
            .any(|p| p.ends_with(" /") || p.ends_with(" /*"))
        {
            notes.push("broad host-path grant: restrict to specific directories".to_string());
        }
        notes.push("shell capsule: highest-risk class, human approval required".to_string());

        let decision = if reject { "reject" } else { "review" };
        capsules.push(ShellEntry {
            id: capsule.id.clone(),
            name: capsule.name.clone(),
            language: capsule.language.clone(),
            allowed_commands: commands,
            host_paths,
            env_reads,
            network,
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
    ShellReport {
        schema: "forgentex.shell_review.v1".to_string(),
        file: file.to_string(),
        status: status.to_string(),
        capsules,
    }
}

impl ShellReport {
    pub fn is_rejected(&self) -> bool {
        self.status == "reject"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex shell capsule review\n");
        out.push_str(&format!(
            "schema={} file={} status={} capsules={}\n",
            self.schema,
            self.file,
            self.status,
            self.capsules.len()
        ));
        for capsule in &self.capsules {
            out.push_str(&format!(
                "  - {} {} lang={} decision={} commands=[{}]\n",
                capsule.name,
                capsule.id,
                capsule.language,
                capsule.decision,
                capsule.allowed_commands.join(",")
            ));
            out.push_str(&format!(
                "      host_paths=[{}] env_reads=[{}] network=[{}]\n",
                capsule.host_paths.join(","),
                capsule.env_reads.join(","),
                capsule.network.join(",")
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
                "      \"allowed_commands\": {},\n",
                json::string_array(&capsule.allowed_commands)
            ));
            out.push_str(&format!(
                "      \"host_paths\": {},\n",
                json::string_array(&capsule.host_paths)
            ));
            out.push_str(&format!(
                "      \"env_reads\": {},\n",
                json::string_array(&capsule.env_reads)
            ));
            out.push_str(&format!(
                "      \"network\": {},\n",
                json::string_array(&capsule.network)
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
