//! Real JVM execution of Java capsules (v2.22).
//!
//! Business-language support: Java capsules run for real on the host JDK.
//! Unlike the interpreted runtimes, Java is compiled first (`javac`) then run
//! (`java`). The body is a Java class; Forgentex parses its class and entry
//! method, generates a tiny typed driver, compiles both, and runs the driver
//! per fixture, checking the output against the capsule's `mock` fixtures.
//!
//! Honest scope: scalar typed boundary (int/long/double/boolean/String). The
//! entry is the first `public static` method of the body class. Missing JDK
//! degrades gracefully to `runtime_unavailable`. Marshaling is via process
//! arguments and `System.out.println`, so no JSON dependency is required.

use std::process::Command;

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::capsule_tokens::boundary_card;
use crate::json;
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct JvmCase {
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JvmRun {
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
    pub cases: Vec<JvmCase>,
    pub passed: usize,
    pub total: usize,
    pub message: Option<String>,
}

struct Jdk {
    java: String,
    javac: String,
    version: String,
}

/// Detect a host JDK: `$JAVA_HOME/bin` first, then `java`/`javac` on PATH.
fn detect_jdk() -> Option<Jdk> {
    let (java, javac) = match std::env::var("JAVA_HOME") {
        Ok(home) if !home.trim().is_empty() => {
            let home = home.trim();
            (format!("{home}/bin/java"), format!("{home}/bin/javac"))
        }
        _ => ("java".to_string(), "javac".to_string()),
    };
    let output = Command::new(&javac).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        version = String::from_utf8_lossy(&output.stderr).trim().to_string();
    }
    Some(Jdk {
        java,
        javac,
        version,
    })
}

fn dedent(lines: &[String]) -> String {
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

fn find_class(body: &str) -> Option<String> {
    let idx = body.find("class ")?;
    let rest = &body[idx + "class ".len()..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The entry is the first `public static` method: the identifier just before
/// the first `(` after `public static`.
fn find_method(body: &str) -> Option<String> {
    let idx = body.find("public static")?;
    let rest = &body[idx + "public static".len()..];
    let paren = rest.find('(')?;
    let head = &rest[..paren];
    head.split_whitespace().last().map(|s| s.to_string())
}

fn java_parse(ty: &str, idx: usize) -> String {
    match ty {
        "i64" | "isize" | "u64" | "usize" => format!("Long.parseLong(args[{idx}])"),
        "i8" | "i16" | "i32" | "u8" | "u16" | "u32" => format!("Integer.parseInt(args[{idx}])"),
        "f32" => format!("Float.parseFloat(args[{idx}])"),
        "f64" => format!("Double.parseDouble(args[{idx}])"),
        "bool" => format!("Boolean.parseBoolean(args[{idx}])"),
        _ => format!("args[{idx}]"),
    }
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
        .filter(|c| c.language == "java")
        .find(|c| name.map(|n| n == c.name).unwrap_or(true))
}

pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<JvmRun, String> {
    let capsule = select(program, name).ok_or_else(|| "no java capsule found".to_string())?;

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

    let class = find_class(&body);
    let method = find_method(&body);
    let entry = match (&class, &method) {
        (Some(c), Some(m)) => Some(format!("{c}.{m}")),
        _ => None,
    };

    let mut report = JvmRun {
        schema: "forgentex.jvm_run.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.name.clone(),
        runtime: None,
        entry,
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
    let (class, method) = match (class, method) {
        (Some(c), Some(m)) => (c, m),
        _ => {
            report.status = "error".to_string();
            report.message = Some("no public static method found in capsule body".to_string());
            return Ok(report);
        }
    };
    let jdk = match detect_jdk() {
        Some(jdk) => jdk,
        None => {
            report.status = "runtime_unavailable".to_string();
            report.message =
                Some("no host JDK found (looked for $JAVA_HOME, java/javac)".to_string());
            return Ok(report);
        }
    };
    report.runtime = Some(jdk.version.clone());

    use std::sync::atomic::{AtomicU64, Ordering};
    static JVM_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = JVM_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_jvm", std::process::id(), seq));
    if std::fs::create_dir_all(&dir).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to create scratch dir".to_string());
        return Ok(report);
    }

    let arg_exprs = inputs
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| java_parse(ty, i))
        .collect::<Vec<_>>()
        .join(", ");
    let driver = format!(
        "public class ForgentexDriver {{\n    public static void main(String[] args) {{\n        System.out.println({class}.{method}({arg_exprs}));\n    }}\n}}\n"
    );
    let class_path = dir.join(format!("{class}.java"));
    let driver_path = dir.join("ForgentexDriver.java");
    if std::fs::write(&class_path, &body).is_err() || std::fs::write(&driver_path, driver).is_err()
    {
        report.status = "error".to_string();
        report.message = Some("failed to write java sources".to_string());
        return Ok(report);
    }

    // Compile both sources together.
    let compile = Command::new(&jdk.javac)
        .arg("-d")
        .arg(&dir)
        .arg(&class_path)
        .arg(&driver_path)
        .output();
    match compile {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            report.status = "error".to_string();
            report.message = Some(String::from_utf8_lossy(&out.stderr).trim().to_string());
            let _ = std::fs::remove_dir_all(&dir);
            return Ok(report);
        }
        Err(err) => {
            report.status = "error".to_string();
            report.message = Some(format!("failed to launch javac: {err}"));
            let _ = std::fs::remove_dir_all(&dir);
            return Ok(report);
        }
    }

    for fixture in &capsule.fixtures {
        let mut command = Command::new(&jdk.java);
        command.arg("-cp").arg(&dir).arg("ForgentexDriver");
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
                report.cases.push(JvmCase {
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
                report.message = Some(format!("failed to launch java: {err}"));
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

impl JvmRun {
    pub fn is_success(&self) -> bool {
        self.status == "ok" || self.status == "runtime_unavailable"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex JVM capsule run\n");
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
