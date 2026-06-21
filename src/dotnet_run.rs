//! Real .NET execution of C# capsules (v2.23).
//!
//! Business-language support. C# is compiled then run: Forgentex scaffolds a
//! minimal console project (a driver `Main` plus the capsule's class in one
//! `Program.cs`), builds it once with `dotnet build`, and runs the produced
//! assembly per fixture, checking output against the capsule's `mock`
//! fixtures.
//!
//! Honest scope: scalar typed boundary (int/long/double/bool/string). The
//! entry is the first `public static` method (skipping `public static class`).
//! The target framework is detected from the installed SDK. Missing .NET
//! degrades gracefully to `runtime_unavailable`. Marshaling is via process
//! arguments and `Console.WriteLine`, so no JSON dependency is required.

use std::process::Command;

use crate::ast::{Capsule, CapsuleValue, Program};
use crate::capsule_tokens::boundary_card;
use crate::json;
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct DotnetCase {
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DotnetRun {
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
    pub cases: Vec<DotnetCase>,
    pub passed: usize,
    pub total: usize,
    pub message: Option<String>,
}

struct Dotnet {
    exe: String,
    version: String,
    tfm: String,
}

fn dotnet_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(custom) = std::env::var("DOTNET") {
        if !custom.trim().is_empty() {
            candidates.push(custom.trim().to_string());
        }
    }
    candidates.push("dotnet".to_string());
    if cfg!(windows) {
        candidates.push("C:/Program Files/dotnet/dotnet.exe".to_string());
    }
    candidates
}

/// Map the highest installed SDK major version to a target framework moniker.
fn tfm_for_sdk(exe: &str) -> String {
    let major = Command::new(exe)
        .arg("--list-sdks")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .and_then(|text| {
            text.lines()
                .filter_map(|l| {
                    l.split('.')
                        .next()
                        .and_then(|m| m.trim().parse::<u32>().ok())
                })
                .max()
        })
        .unwrap_or(0);
    match major {
        0 | 2 => "netcoreapp2.1".to_string(),
        3 => "netcoreapp3.1".to_string(),
        m => format!("net{m}.0"),
    }
}

fn detect_dotnet() -> Option<Dotnet> {
    for exe in dotnet_candidates() {
        if let Ok(output) = Command::new(&exe).arg("--version").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let tfm = tfm_for_sdk(&exe);
                return Some(Dotnet { exe, version, tfm });
            }
        }
    }
    None
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

/// First `public static` method, skipping `public static class` declarations.
fn find_method(body: &str) -> Option<String> {
    let mut offset = 0;
    while let Some(rel) = body[offset..].find("public static") {
        let after = offset + rel + "public static".len();
        let rest = &body[after..];
        if rest.trim_start().starts_with("class") {
            offset = after;
            continue;
        }
        let paren = rest.find('(')?;
        return rest[..paren]
            .split_whitespace()
            .last()
            .map(|s| s.to_string());
    }
    None
}

fn csharp_parse(ty: &str, idx: usize) -> String {
    match ty {
        "i64" | "isize" | "u64" | "usize" => format!("long.Parse(args[{idx}])"),
        "i8" | "i16" | "i32" | "u8" | "u16" | "u32" => format!("int.Parse(args[{idx}])"),
        "f32" => {
            format!("float.Parse(args[{idx}], System.Globalization.CultureInfo.InvariantCulture)")
        }
        "f64" => {
            format!("double.Parse(args[{idx}], System.Globalization.CultureInfo.InvariantCulture)")
        }
        "bool" => format!("bool.Parse(args[{idx}])"),
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
        .filter(|c| c.language == "csharp")
        .find(|c| name.map(|n| n == c.name).unwrap_or(true))
}

pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<DotnetRun, String> {
    let capsule = select(program, name).ok_or_else(|| "no csharp capsule found".to_string())?;

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

    let mut report = DotnetRun {
        schema: "forgentex.dotnet_run.v1".to_string(),
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
    let dotnet = match detect_dotnet() {
        Some(found) => found,
        None => {
            report.status = "runtime_unavailable".to_string();
            report.message = Some("no host .NET found (looked for $DOTNET, dotnet)".to_string());
            return Ok(report);
        }
    };
    report.runtime = Some(format!("dotnet {} ({})", dotnet.version, dotnet.tfm));

    use std::sync::atomic::{AtomicU64, Ordering};
    static DOTNET_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = DOTNET_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_dotnet", std::process::id(), seq));
    if std::fs::create_dir_all(&dir).is_err() {
        report.status = "error".to_string();
        report.message = Some("failed to create scratch dir".to_string());
        return Ok(report);
    }

    let arg_exprs = inputs
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| csharp_parse(ty, i))
        .collect::<Vec<_>>()
        .join(", ");
    let program_cs = format!(
        "class ForgentexDriver {{\n    static void Main(string[] args) {{\n        System.Console.WriteLine({class}.{method}({arg_exprs}));\n    }}\n}}\n{body}\n"
    );
    let csproj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>{}</TargetFramework></PropertyGroup></Project>\n",
        dotnet.tfm
    );
    if std::fs::write(dir.join("Program.cs"), program_cs).is_err()
        || std::fs::write(dir.join("app.csproj"), csproj).is_err()
    {
        report.status = "error".to_string();
        report.message = Some("failed to write csharp sources".to_string());
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(report);
    }

    let out_dir = dir.join("out");
    let build = Command::new(&dotnet.exe)
        .arg("build")
        .arg(dir.join("app.csproj"))
        .arg("-c")
        .arg("Release")
        .arg("-o")
        .arg(&out_dir)
        .output();
    match build {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            report.status = "error".to_string();
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            report.message = Some(format!("{}{}", stderr.trim(), stdout.trim()));
            let _ = std::fs::remove_dir_all(&dir);
            return Ok(report);
        }
        Err(err) => {
            report.status = "error".to_string();
            report.message = Some(format!("failed to launch dotnet: {err}"));
            let _ = std::fs::remove_dir_all(&dir);
            return Ok(report);
        }
    }

    let dll = out_dir.join("app.dll");
    for fixture in &capsule.fixtures {
        let mut command = Command::new(&dotnet.exe);
        command.arg(&dll);
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
                report.cases.push(DotnetCase {
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
                report.message = Some(format!("failed to launch app: {err}"));
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

impl DotnetRun {
    pub fn is_success(&self) -> bool {
        self.status == "ok" || self.status == "runtime_unavailable"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex .NET capsule run\n");
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
