//! Real CPython execution of Python capsules (v2.17).
//!
//! The "special relationship": agents love generating Python, so Forgentex
//! runs it for real on the host interpreter — but behind a typed boundary.
//! The capsule's mock fixtures become real execution oracles (as the C-ABI
//! bridge did for native code): the host Python runs the body against each
//! input and the output is checked against the expectation.
//!
//! The token point: the agent works from the one-line **boundary card**, not
//! the body. A heavy `import numpy ...; def f(...)` body costs the agent
//! nothing to *use* — only its typed signature. This module reports both the
//! body and boundary token counts so the saving is measured, not assumed.
//!
//! Honest scope: CPython is not deterministic across versions, so the
//! interpreter version is detected and recorded, and the run transcript (not
//! the interpreter) is the oracle. Capsules whose body imports packages need
//! those packages installed; without them the run reports `error`. Missing
//! `python` degrades gracefully to `runtime_unavailable`.

use std::process::Command;

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::capsule_tokens::boundary_card;
use crate::json;
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct PyCase {
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PythonRun {
    pub schema: String,
    pub file: String,
    pub capsule: String,
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
    pub cases: Vec<PyCase>,
    pub passed: usize,
    pub total: usize,
    pub message: Option<String>,
}

/// Detect a host Python: `$PYTHON` first, then `python3`, `python`.
pub fn detect_python() -> Option<(String, String)> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(py) = std::env::var("PYTHON") {
        if !py.trim().is_empty() {
            candidates.push(py.trim().to_string());
        }
    }
    candidates.extend(["python3", "python"].iter().map(|s| s.to_string()));
    for name in candidates {
        if let Ok(output) = Command::new(&name).arg("--version").output() {
            if output.status.success() {
                let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    text = String::from_utf8_lossy(&output.stderr).trim().to_string();
                }
                return Some((name, text));
            }
        }
    }
    None
}

/// Strip the common leading indentation so the body is valid top-level
/// Python (the `code:` block is stored indented under the capsule).
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

/// The entry function is the first top-level `def` in the body.
pub(crate) fn find_entry(body: &str) -> Option<String> {
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("def ") {
            let name: String = rest
                .chars()
                .take_while(|c| *c != '(' && !c.is_whitespace())
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn json_inputs(values: &[CapsuleValue]) -> String {
    let parts: Vec<String> = values.iter().map(|v| v.to_literal()).collect();
    format!("[{}]", parts.join(", "))
}

/// Does the Python stdout match the expected boundary value?
fn matches(expected: &CapsuleValue, got: &str) -> bool {
    let got = got.trim();
    match expected {
        CapsuleValue::Int(i) => got
            .parse::<f64>()
            .map(|g| (g - *i as f64).abs() < 0.5)
            .unwrap_or(false),
        CapsuleValue::Float(f) => got
            .parse::<f64>()
            .map(|g| (g - *f).abs() < 1e-9)
            .unwrap_or(false),
        CapsuleValue::Bool(b) => got == if *b { "true" } else { "false" },
        CapsuleValue::Str(s) => got.trim_matches('"') == s,
    }
}

fn select<'a>(program: &'a Program, name: Option<&str>) -> Option<&'a Capsule> {
    program
        .capsules()
        .filter(|c| c.language == "python")
        .find(|c| name.map(|n| n == c.name).unwrap_or(true))
}

/// Invoke a python capsule's entry with live, JSON-encoded positional arguments
/// (a JSON array string), returning the entry's JSON result printed on stdout.
///
/// This is the live capsule runtime path (rt.1): the interpreter marshals real
/// runtime values across the typed boundary and dispatches to the host python,
/// instead of replaying mock fixtures. The call is gated by the capsule's
/// sandbox approval just like the fixture runner.
pub fn invoke_live(capsule: &Capsule, json_args: &str) -> Result<String, String> {
    if !crate::sandbox::is_approved(capsule) {
        return Err("capsule effects are not covered by its sandbox".to_string());
    }
    let body = dedent(&capsule.code);
    let entry =
        find_entry(&body).ok_or_else(|| "no top-level def found in capsule body".to_string())?;
    let (py_name, _version) = detect_python()
        .ok_or_else(|| "no host python found (looked for $PYTHON, python3, python)".to_string())?;

    use std::sync::atomic::{AtomicU64, Ordering};
    static LIVE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_pylive", std::process::id(), seq));
    std::fs::create_dir_all(&dir).map_err(|err| format!("failed to create scratch dir: {err}"))?;
    let driver_path = dir.join("driver.py");
    let driver = format!(
        "import json, sys\n{body}\n_inputs = json.loads(sys.argv[1])\n_result = {entry}(*_inputs)\nprint(json.dumps(_result))\n"
    );
    std::fs::write(&driver_path, driver).map_err(|err| format!("failed to write driver: {err}"))?;

    let output = Command::new(&py_name)
        .arg(&driver_path)
        .arg(json_args)
        .output()
        .map_err(|err| format!("failed to spawn python: {err}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    if !output.status.success() {
        return Err(format!(
            "python capsule '{}' raised: {}",
            capsule.name,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<PythonRun, String> {
    let capsule =
        select(program, name).ok_or_else(|| "no python capsule found in program".to_string())?;

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

    let mut report = PythonRun {
        schema: "forgentex.python_run.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.name.clone(),
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
            report.message = Some("no top-level def found in capsule body".to_string());
            return Ok(report);
        }
    };
    let (py_name, version) = match detect_python() {
        Some(found) => found,
        None => {
            report.status = "runtime_unavailable".to_string();
            report.message =
                Some("no host python found (looked for $PYTHON, python3, python)".to_string());
            return Ok(report);
        }
    };
    report.runtime = Some(version);

    // Write the driver once; run it per fixture with marshaled inputs. The
    // scratch dir is unique per call so concurrent runs never collide.
    use std::sync::atomic::{AtomicU64, Ordering};
    static PYRUN_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = PYRUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_pyrun", std::process::id(), seq));
    if std::fs::create_dir_all(&dir).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to create scratch dir".to_string());
        return Ok(report);
    }
    let driver_path = dir.join("driver.py");
    let driver = format!(
        "import json, sys\n{body}\n_inputs = json.loads(sys.argv[1])\n_result = {entry}(*_inputs)\nprint(json.dumps(_result))\n"
    );
    if std::fs::write(&driver_path, driver).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to write driver".to_string());
        return Ok(report);
    }

    for fixture in &capsule.fixtures {
        let args = json_inputs(&fixture.inputs);
        let output = Command::new(&py_name).arg(&driver_path).arg(&args).output();
        match output {
            Ok(out) if out.status.success() => {
                let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
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
                report.cases.push(PyCase {
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
                report.message = Some(format!("failed to launch python: {err}"));
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

impl PythonRun {
    pub fn is_success(&self) -> bool {
        self.status == "ok" || self.status == "runtime_unavailable"
    }

    fn marshal_line(label: &str, bindings: &[(String, String)]) -> String {
        let rendered = bindings
            .iter()
            .map(|(n, t)| format!("{n}:{t}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("  {label}: [{rendered}]\n")
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex python capsule run\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} status={}\n",
            self.schema, self.file, self.capsule, self.status
        ));
        if let Some(runtime) = &self.runtime {
            out.push_str(&format!("  runtime: {runtime}\n"));
        }
        if let Some(entry) = &self.entry {
            out.push_str(&format!("  entry: {entry}()\n"));
        }
        out.push_str(&Self::marshal_line("inputs", &self.inputs));
        out.push_str(&Self::marshal_line("outputs", &self.outputs));
        out.push_str(&format!(
            "  effects=[{}] sandbox={} deps={}\n",
            self.effects.join(", "),
            self.sandbox_decision,
            self.deps.len()
        ));
        out.push_str(&format!(
            "  tokens: body={} boundary={} (agent pays the boundary, not the body)\n",
            self.body_tokens, self.boundary_tokens
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
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsule\": {},\n  \"runtime\": {},\n  \"entry\": {},\n  \"effects\": {},\n  \"sandbox_decision\": {},\n  \"deps\": {},\n  \"body_tokens\": {},\n  \"boundary_tokens\": {},\n  \"status\": {},\n  \"cases\": [\n    {}\n  ],\n  \"passed\": {},\n  \"total\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.capsule),
            json::opt_string(self.runtime.as_deref()),
            json::opt_string(self.entry.as_deref()),
            json::string_array(&self.effects),
            json::string(&self.sandbox_decision),
            self.deps.len(),
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
