//! Real JS/TS execution of web capsules (v2.20).
//!
//! The web tier of the polyglot: JavaScript (and best-effort TypeScript)
//! capsules run for real on the host Node runtime, behind the same typed
//! boundary as every other backend. Mirrors the Python bridge (v2.17): the
//! body is dedented, the first named `function` is the entry, inputs are
//! marshaled as JSON, and the output is checked against the capsule's `mock`
//! fixtures (now real execution oracles).
//!
//! With this, the unified polyglot ABI (v2.18) runs a Python backend and a
//! JavaScript frontend under one roof, sharing one typed value by name.
//!
//! Honest scope: JavaScript via `node`; TypeScript via
//! `node --experimental-strip-types` (Node 22.6+), best-effort. Missing
//! `node` degrades gracefully to `runtime_unavailable`. The interpreter
//! version is recorded; the transcript is the oracle.

use std::process::Command;

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::capsule_tokens::boundary_card;
use crate::json;
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct WebCase {
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebRun {
    pub schema: String,
    pub file: String,
    pub capsule: String,
    pub language: String,
    pub runtime: Option<String>,
    pub entry: Option<String>,
    pub inputs: Vec<(String, String)>,
    pub outputs: Vec<(String, String)>,
    pub deps: Vec<String>,
    pub effects: Vec<String>,
    pub sandbox_decision: String,
    pub body_tokens: usize,
    pub boundary_tokens: usize,
    pub status: String,
    pub cases: Vec<WebCase>,
    pub passed: usize,
    pub total: usize,
    pub message: Option<String>,
}

/// Detect a host Node: `$NODE` first, then `node`.
pub fn detect_node() -> Option<(String, String)> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(node) = std::env::var("NODE") {
        if !node.trim().is_empty() {
            candidates.push(node.trim().to_string());
        }
    }
    candidates.push("node".to_string());
    for name in candidates {
        if let Ok(output) = Command::new(&name).arg("--version").output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Some((name, format!("node {text}")));
            }
        }
    }
    None
}

pub(crate) fn dedent(lines: &[String]) -> String {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.len() >= indent {
                l[indent..].to_string()
            } else {
                l.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The entry is the first named `function` in the body.
pub(crate) fn find_entry(body: &str) -> Option<String> {
    let marker = "function ";
    let idx = body.find(marker)?;
    let rest = &body[idx + marker.len()..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn json_inputs(values: &[CapsuleValue]) -> String {
    let parts: Vec<String> = values.iter().map(|v| v.to_literal()).collect();
    format!("[{}]", parts.join(", "))
}

fn matches(expected: &CapsuleValue, got: &str) -> bool {
    let got = got.trim();
    match expected {
        CapsuleValue::Int(i) => got
            .parse::<f64>()
            .map(|g| (g - *i as f64).abs() < 0.5)
            .unwrap_or(false),
        CapsuleValue::UInt(i) => got.parse::<u64>().map(|g| g == *i).unwrap_or(false),
        CapsuleValue::Float(f) => got
            .parse::<f64>()
            .map(|g| (g - *f).abs() < 1e-9)
            .unwrap_or(false),
        CapsuleValue::Bool(b) => got == if *b { "true" } else { "false" },
        CapsuleValue::Str(s) => got.trim_matches('"') == s,
    }
}

/// Invoke a node/typescript capsule's entry with live, JSON-encoded positional
/// arguments (a JSON array string), returning the entry's JSON result printed on
/// stdout. The live capsule runtime (rt.1) path: real runtime values are
/// marshaled across the typed boundary instead of replaying mock fixtures, gated
/// by the capsule's sandbox approval.
/// Run a capsule live under the default managed policy: every enforceable limit
/// applied, and any capsule declaring a limit that cannot be enforced refused.
pub fn invoke_live(capsule: &Capsule, json_args: &str) -> Result<String, String> {
    invoke_live_with(
        capsule,
        json_args,
        crate::managed_exec::ManagedPolicy::default(),
    )
}

/// Run a capsule live under `policy`. The host runtime runs inside the managed
/// containment layer (`managed_exec`): wall deadline, CPU and output budgets,
/// declared memory ceiling, and process-tree cleanup.
pub fn invoke_live_with(
    capsule: &Capsule,
    json_args: &str,
    policy: crate::managed_exec::ManagedPolicy,
) -> Result<String, String> {
    let managed_plan = crate::managed_exec::plan(capsule, policy)?;
    if !crate::sandbox::is_approved(capsule) {
        return Err("capsule effects are not covered by its sandbox".to_string());
    }
    let body = dedent(&capsule.code);
    let entry = find_entry(&body)
        .ok_or_else(|| "no top-level function found in capsule body".to_string())?;
    let (node_name, _version) =
        detect_node().ok_or_else(|| "no host node found (looked for $NODE, node)".to_string())?;

    use std::sync::atomic::{AtomicU64, Ordering};
    static LIVE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_{}_nodelive", std::process::id(), seq));
    std::fs::create_dir_all(&dir).map_err(|err| format!("failed to create scratch dir: {err}"))?;
    let is_ts = capsule.language == "typescript";
    let driver_path = dir.join(if is_ts { "driver.ts" } else { "driver.mjs" });
    let nonce = crate::managed_protocol::nonce();
    let driver = format!(
        "{prelude}{body}\nconst _chunks = [];\nfor await (const _c of process.stdin) _chunks.push(_c);\nconst _args = JSON.parse(Buffer.concat(_chunks).toString('utf8') || '[]');\nconst _result = {entry}(..._args);\n{emit};\n",
        prelude = crate::managed_protocol::node_prelude(&nonce),
        emit = crate::managed_protocol::node_emit(&nonce, "_result"),
    );
    std::fs::write(&driver_path, driver).map_err(|err| format!("failed to write driver: {err}"))?;

    let mut argv = vec![node_name.to_string()];
    if is_ts {
        argv.push("--experimental-strip-types".to_string());
    }
    argv.push(driver_path.display().to_string());
    let output = match crate::managed_exec::execute_with_input(
        &argv,
        &managed_plan,
        Some(json_args.as_bytes()),
    ) {
        Ok(output) => output,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(err);
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    let framed = crate::managed_protocol::split(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        &nonce,
    );
    if !output.status.success() {
        return Err(format!(
            "node capsule '{}' raised: {}",
            capsule.name,
            if framed.diagnostics.is_empty() {
                "no output".to_string()
            } else {
                framed.diagnostics
            }
        ));
    }
    framed.result_or_error(&capsule.name)
}

fn select<'a>(program: &'a Program, name: Option<&str>) -> Option<&'a Capsule> {
    program
        .capsules()
        .filter(|c| c.language == "javascript" || c.language == "typescript")
        .find(|c| name.map(|n| n == c.name).unwrap_or(true))
}

pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<WebRun, String> {
    let capsule = select(program, name)
        .ok_or_else(|| "no javascript/typescript capsule found".to_string())?;

    let inputs = capsule
        .inputs
        .iter()
        .map(|b| (b.name.clone(), b.ty.to_agent()))
        .collect();
    let outputs = capsule
        .outputs
        .iter()
        .map(|b| (b.name.clone(), b.ty.to_agent()))
        .collect();
    let deps = capsule.deps.iter().map(|d| d.source.clone()).collect();
    let body = dedent(&capsule.code);
    let body_tokens = estimate(&capsule.code.join("\n"));
    let boundary_tokens = estimate(&boundary_card(capsule));
    let sandbox_decision = if crate::sandbox::is_approved(capsule) {
        "allow"
    } else {
        "deny"
    }
    .to_string();

    let mut report = WebRun {
        schema: "forgentex.web_run.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.name.clone(),
        language: capsule.language.clone(),
        runtime: None,
        entry: find_entry(&body),
        inputs,
        outputs,
        deps,
        effects: capsule.effects.clone(),
        sandbox_decision,
        body_tokens,
        boundary_tokens,
        status: "ok".to_string(),
        cases: Vec::new(),
        passed: 0,
        total: capsule.fixtures.len(),
        message: None,
    };

    if report.sandbox_decision == "deny" {
        report.status = "denied".to_string();
        report.message = Some("capsule effects are not covered by its sandbox".to_string());
        return Ok(report);
    }
    let entry = match &report.entry {
        Some(entry) => entry.clone(),
        None => {
            report.status = "error".to_string();
            report.message = Some("no named function found in capsule body".to_string());
            return Ok(report);
        }
    };
    let (node_name, version) = match detect_node() {
        Some(found) => found,
        None => {
            report.status = "runtime_unavailable".to_string();
            report.message = Some("no host node found (looked for $NODE, node)".to_string());
            return Ok(report);
        }
    };
    report.runtime = Some(version);

    use std::sync::atomic::{AtomicU64, Ordering};
    static WEBRUN_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = WEBRUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_webrun", std::process::id(), seq));
    if std::fs::create_dir_all(&dir).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to create scratch dir".to_string());
        return Ok(report);
    }
    let is_ts = capsule.language == "typescript";
    let driver_path = dir.join(if is_ts { "driver.ts" } else { "driver.mjs" });
    let nonce = crate::managed_protocol::nonce();
    let driver = format!(
        "{prelude}{body}\nconst _chunks = [];\nfor await (const _c of process.stdin) _chunks.push(_c);\nconst _args = JSON.parse(Buffer.concat(_chunks).toString('utf8') || '[]');\nconst _result = {entry}(..._args);\n{emit};\n",
        prelude = crate::managed_protocol::node_prelude(&nonce),
        emit = crate::managed_protocol::node_emit(&nonce, "_result"),
    );
    if std::fs::write(&driver_path, driver).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to write driver".to_string());
        return Ok(report);
    }

    for fixture in &capsule.fixtures {
        let args = json_inputs(&fixture.inputs);
        let mut command = Command::new(&node_name);
        if is_ts {
            command.arg("--experimental-strip-types");
        }
        command.arg(&driver_path);
        match crate::managed_protocol::run_with_stdin(&mut command, &args) {
            Ok(out) if out.status.success() => {
                let framed = crate::managed_protocol::split(
                    &String::from_utf8_lossy(&out.stdout),
                    &String::from_utf8_lossy(&out.stderr),
                    &nonce,
                );
                let got = framed.result.unwrap_or_default();
                let expected = fixture
                    .outputs
                    .first()
                    .map(|v| v.to_literal())
                    .unwrap_or_default();
                let pass = fixture
                    .outputs
                    .first()
                    .map(|v| matches(v, &got))
                    .unwrap_or(false);
                if pass {
                    report.passed += 1;
                }
                report.cases.push(WebCase {
                    inputs: fixture.inputs.iter().map(|v| v.to_literal()).collect(),
                    output: got,
                    expected,
                    pass,
                });
            }
            Ok(out) => {
                report.status = "error".to_string();
                report.message = Some(String::from_utf8_lossy(&out.stderr).trim().to_string());
                break;
            }
            Err(err) => {
                report.status = "error".to_string();
                report.message = Some(format!("failed to launch node: {err}"));
                break;
            }
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
    if report.status == "ok" && report.passed != report.total {
        report.status = "mismatch".to_string();
    }
    Ok(report)
}

impl WebRun {
    pub fn is_success(&self) -> bool {
        self.status == "ok" || self.status == "runtime_unavailable"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex web capsule run\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} language={} status={}\n",
            self.schema, self.file, self.capsule, self.language, self.status
        ));
        if let Some(runtime) = &self.runtime {
            out.push_str(&format!("  runtime: {runtime}\n"));
        }
        if let Some(entry) = &self.entry {
            out.push_str(&format!("  entry: {entry}()\n"));
        }
        out.push_str(&format!(
            "  effects=[{}] sandbox={} tokens: body={} boundary={}\n",
            self.effects.join(", "),
            self.sandbox_decision,
            self.body_tokens,
            self.boundary_tokens
        ));
        for case in &self.cases {
            out.push_str(&format!(
                "  {} {}({}) -> {} (expected {})\n",
                if case.pass { "pass" } else { "FAIL" },
                self.entry.as_deref().unwrap_or("f"),
                case.inputs.join(", "),
                case.output,
                case.expected
            ));
        }
        out.push_str(&format!(
            "  {}/{} fixtures passed\n",
            self.passed, self.total
        ));
        if let Some(message) = &self.message {
            out.push_str(&format!("  note: {message}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let cases = self
            .cases
            .iter()
            .map(|c| {
                format!(
                    "{{ \"inputs\": [{}], \"output\": {}, \"expected\": {}, \"pass\": {} }}",
                    c.inputs
                        .iter()
                        .map(|v| json::string(v))
                        .collect::<Vec<_>>()
                        .join(", "),
                    json::string(&c.output),
                    json::string(&c.expected),
                    c.pass
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsule\": {},\n  \"language\": {},\n  \"runtime\": {},\n  \"entry\": {},\n  \"effects\": {},\n  \"sandbox_decision\": {},\n  \"body_tokens\": {},\n  \"boundary_tokens\": {},\n  \"status\": {},\n  \"cases\": [\n    {}\n  ],\n  \"passed\": {},\n  \"total\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.capsule),
            json::string(&self.language),
            json::opt_string(self.runtime.as_deref()),
            json::opt_string(self.entry.as_deref()),
            json::string_array(&self.effects),
            json::string(&self.sandbox_decision),
            self.body_tokens,
            self.boundary_tokens,
            json::string(&self.status),
            cases,
            self.passed,
            self.total,
            json::opt_string(self.message.as_deref()),
        )
    }
}
