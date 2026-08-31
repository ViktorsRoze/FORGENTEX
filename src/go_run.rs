//! Real Go execution of Go capsules (v2.24).
//!
//! Cloud & network-services support. Go has the simplest model of the managed
//! runtimes: `go run` compiles and runs a single source file in one step.
//! Forgentex assembles a `package main` program (the body plus a generated
//! `main` that parses args, calls the entry, and prints the result) and runs
//! it per fixture, checking output against the capsule's `mock` fixtures.
//!
//! Honest scope: scalar typed boundary (int64/float64/bool/string). The entry
//! is the first `func`. Imports are emitted only when used (Go treats unused
//! imports as errors). Stdlib-only bodies need no `go.mod`. Missing Go
//! degrades gracefully to `runtime_unavailable`.

use std::process::Command;

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::capsule_tokens::boundary_card;
use crate::json;
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct GoCase {
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GoRun {
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
    pub cases: Vec<GoCase>,
    pub passed: usize,
    pub total: usize,
    pub message: Option<String>,
}

fn go_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(custom) = std::env::var("GO") {
        if !custom.trim().is_empty() {
            candidates.push(custom.trim().to_string());
        }
    }
    candidates.push("go".to_string());
    if cfg!(windows) {
        candidates.push("C:/Program Files/Go/bin/go.exe".to_string());
    }
    candidates
}

pub fn detect_go() -> Option<(String, String)> {
    for exe in go_candidates() {
        if let Ok(output) = Command::new(&exe).arg("version").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Some((exe, version));
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

pub(crate) fn find_entry(body: &str) -> Option<String> {
    let idx = body.find("func ")?;
    let rest = &body[idx + "func ".len()..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The Go type a boundary input unmarshals into.
pub(crate) fn go_json_type(ty: &str) -> &'static str {
    match ty {
        "f32" | "f64" => "float64",
        "bool" => "bool",
        t if is_numeric(t) => "int64",
        _ => "string",
    }
}

/// Invoke a go capsule's entry with live, JSON-encoded positional arguments (a
/// JSON array string), returning the entry's JSON result on stdout. Go has a
/// stdlib JSON codec, so the boundary stays uniform with python/node: the
/// driver unmarshals each argument into its typed parameter, calls the entry,
/// and marshals the result back to JSON. Gated by the capsule's sandbox
/// approval (the live capsule runtime, rt.1).
pub fn invoke_live(capsule: &Capsule, json_args: &str) -> Result<String, String> {
    if !crate::sandbox::is_approved(capsule) {
        return Err("capsule effects are not covered by its sandbox".to_string());
    }
    let body = dedent(&capsule.code);
    let entry = find_entry(&body).ok_or_else(|| "no func found in capsule body".to_string())?;
    let (go, _version) =
        detect_go().ok_or_else(|| "no host Go found (looked for $GO, go)".to_string())?;

    let mut binds = Vec::new();
    let mut call_args = Vec::new();
    for (i, input) in capsule.inputs.iter().enumerate() {
        binds.push(format!(
            "    var a{i} {}\n    json.Unmarshal(_args[{i}], &a{i})",
            go_json_type(&input.ty.to_agent())
        ));
        call_args.push(format!("a{i}"));
    }
    let needs_os = !capsule.inputs.is_empty();
    let mut imports = vec!["\t\"encoding/json\"", "\t\"fmt\""];
    if needs_os {
        imports.push("\t\"os\"");
    }
    let read_args = if needs_os {
        "    var _args []json.RawMessage\n    json.Unmarshal([]byte(os.Args[1]), &_args)\n"
    } else {
        ""
    };
    let main_src = format!(
        "package main\n\nimport (\n{}\n)\n\n{body}\n\nfunc main() {{\n{read_args}{}\n    _r := {entry}({})\n    _b, _ := json.Marshal(_r)\n    fmt.Println(string(_b))\n}}\n",
        imports.join("\n"),
        binds.join("\n"),
        call_args.join(", ")
    );

    use std::sync::atomic::{AtomicU64, Ordering};
    static LIVE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_golive", std::process::id(), seq));
    std::fs::create_dir_all(&dir).map_err(|err| format!("failed to create scratch dir: {err}"))?;
    let main_path = dir.join("main.go");
    std::fs::write(&main_path, main_src)
        .map_err(|err| format!("failed to write go source: {err}"))?;

    let output = Command::new(&go)
        .arg("run")
        .arg(&main_path)
        .arg(json_args)
        .output()
        .map_err(|err| format!("failed to spawn go: {err}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    if !output.status.success() {
        return Err(format!(
            "go capsule '{}' failed: {}",
            capsule.name,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_numeric(ty: &str) -> bool {
    matches!(
        ty,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "f32"
            | "f64"
    )
}

/// (binding statement, argument identifier) for one input.
fn go_bind(ty: &str, i: usize) -> (String, String) {
    let id = format!("a{i}");
    let idx = i + 1;
    let line = match ty {
        "f32" | "f64" => format!("{id}, _ := strconv.ParseFloat(os.Args[{idx}], 64)"),
        "bool" => format!("{id}, _ := strconv.ParseBool(os.Args[{idx}])"),
        t if is_numeric(t) => format!("{id}, _ := strconv.ParseInt(os.Args[{idx}], 10, 64)"),
        _ => format!("{id} := os.Args[{idx}]"),
    };
    (line, id)
}

fn arg_value(value: &CapsuleValue) -> String {
    match value {
        CapsuleValue::Str(s) => s.clone(),
        other => other.to_literal(),
    }
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
        CapsuleValue::Bool(b) => got.eq_ignore_ascii_case(if *b { "true" } else { "false" }),
        CapsuleValue::Str(s) => got.trim_matches('"') == s,
    }
}

fn select<'a>(program: &'a Program, name: Option<&str>) -> Option<&'a Capsule> {
    program
        .capsules()
        .filter(|c| c.language == "go")
        .find(|c| name.map(|n| n == c.name).unwrap_or(true))
}

pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<GoRun, String> {
    let capsule = select(program, name).ok_or_else(|| "no go capsule found".to_string())?;

    let inputs: Vec<(String, String)> = capsule
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

    let mut report = GoRun {
        schema: "forgentex.go_run.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.name.clone(),
        runtime: None,
        entry: find_entry(&body),
        inputs: inputs.clone(),
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
            report.message = Some("no func found in capsule body".to_string());
            return Ok(report);
        }
    };
    let (go, version) = match detect_go() {
        Some(found) => found,
        None => {
            report.status = "runtime_unavailable".to_string();
            report.message = Some("no host Go found (looked for $GO, go)".to_string());
            return Ok(report);
        }
    };
    report.runtime = Some(version);

    // Assemble a package main program; emit imports only when used (Go treats
    // unused imports as compile errors).
    let mut binds = Vec::new();
    let mut call_args = Vec::new();
    for (i, (_, ty)) in inputs.iter().enumerate() {
        let (line, id) = go_bind(ty, i);
        binds.push(format!("    {line}"));
        call_args.push(id);
    }
    let needs_os = !inputs.is_empty();
    let needs_strconv = inputs.iter().any(|(_, ty)| is_numeric(ty) || ty == "bool");
    let mut imports = vec!["\t\"fmt\"".to_string()];
    if needs_os {
        imports.push("\t\"os\"".to_string());
    }
    if needs_strconv {
        imports.push("\t\"strconv\"".to_string());
    }
    let main_src = format!(
        "package main\n\nimport (\n{}\n)\n\n{body}\n\nfunc main() {{\n{}\n    fmt.Println({entry}({}))\n}}\n",
        imports.join("\n"),
        binds.join("\n"),
        call_args.join(", ")
    );

    use std::sync::atomic::{AtomicU64, Ordering};
    static GO_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = GO_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_go", std::process::id(), seq));
    if std::fs::create_dir_all(&dir).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to create scratch dir".to_string());
        return Ok(report);
    }
    let main_path = dir.join("main.go");
    if std::fs::write(&main_path, main_src).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to write go source".to_string());
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(report);
    }

    for fixture in &capsule.fixtures {
        let mut command = Command::new(&go);
        command.arg("run").arg(&main_path);
        for value in &fixture.inputs {
            command.arg(arg_value(value));
        }
        match command.output() {
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
                report.cases.push(GoCase {
                    inputs: fixture.inputs.iter().map(arg_value).collect(),
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
                report.message = Some(format!("failed to launch go: {err}"));
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

impl GoRun {
    pub fn is_success(&self) -> bool {
        self.status == "ok" || self.status == "runtime_unavailable"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex Go capsule run\n");
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
        out.push_str(&format!(
            "  effects=[{}] sandbox={} tokens: body={} boundary={}\n",
            self.effects.join(", "),
            self.sandbox_decision,
            self.body_tokens,
            self.boundary_tokens
        ));
        for case in &self.cases {
            out.push_str(&format!(
                "  {} ({}) -> {} (expected {})\n",
                if case.pass { "pass" } else { "FAIL" },
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
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsule\": {},\n  \"runtime\": {},\n  \"entry\": {},\n  \"effects\": {},\n  \"sandbox_decision\": {},\n  \"body_tokens\": {},\n  \"boundary_tokens\": {},\n  \"status\": {},\n  \"cases\": [\n    {}\n  ],\n  \"passed\": {},\n  \"total\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.capsule),
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
