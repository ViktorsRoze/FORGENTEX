//! C-ABI FFI bridge (v2.13).
//!
//! Makes native-class capsules *execute* instead of mock. The typed capsule
//! boundary becomes a real `extern "C"` signature: the capsule body compiles
//! to an object, a generated harness compiles against the boundary header,
//! the two are linked, and the program runs each fixture for real — so the
//! fixtures become execution oracles, not just type checks. Linking/execution
//! is gated: the capsule must pass `check` (effects covered by sandbox grants,
//! dependencies pinned) or it is denied.
//!
//! First slice scope (honest): language `c`, a scalar boundary
//! (int/float/bool). C++/Rust/Zig reduce to the same C ABI and join later;
//! slices/structs join when their marshaling lands. The C compiler is the
//! v2.12 host toolchain, gated gracefully when absent.

use std::path::Path;

use crate::ast::{Capsule, CapsuleValue, Program, Type};
use crate::json;
use crate::native_build::{detect_toolchain, run_with_path, Toolchain};

#[derive(Clone, Debug, PartialEq)]
pub struct Marshal {
    pub name: String,
    pub fgx_type: String,
    pub c_type: String,
    pub direction: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureRun {
    pub index: usize,
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FfiPlan {
    pub schema: String,
    pub file: String,
    pub capsule: String,
    pub language: String,
    pub risk_class: String,
    pub extern_c_signature: String,
    pub marshaling: Vec<Marshal>,
    pub link_units: Vec<String>,
    pub effects: Vec<String>,
    pub grants: Vec<String>,
    pub deps_pinned: bool,
    pub gate_decision: String,
    pub gate_reasons: Vec<String>,
    pub execution_status: String,
    pub toolchain: Option<Toolchain>,
    pub passed: usize,
    pub total: usize,
    pub transcript: Vec<FixtureRun>,
    pub message: Option<String>,
}

/// Map a scalar Forgentex type to its C-ABI type. Non-scalar types are not
/// part of this slice's marshalable set.
fn c_type(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Bool => Some("int"),
        Type::Primitive(name) => match name.as_str() {
            "i8" => Some("int8_t"),
            "i16" => Some("int16_t"),
            "i32" => Some("int32_t"),
            "i64" => Some("int64_t"),
            "isize" => Some("ptrdiff_t"),
            "u8" => Some("uint8_t"),
            "u16" => Some("uint16_t"),
            "u32" => Some("uint32_t"),
            "u64" => Some("uint64_t"),
            "usize" => Some("size_t"),
            "f32" => Some("float"),
            "f64" => Some("double"),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum OutKind {
    Int,
    Float,
    Bool,
}

fn out_kind(ty: &Type) -> Option<OutKind> {
    match ty {
        Type::Bool => Some(OutKind::Bool),
        Type::Primitive(name) => match name.as_str() {
            "f32" | "f64" => Some(OutKind::Float),
            other if crate::ast::is_integer_primitive(other) => Some(OutKind::Int),
            _ => None,
        },
        _ => None,
    }
}

fn marshaling(capsule: &Capsule) -> Vec<Marshal> {
    let mut out = Vec::new();
    for binding in &capsule.inputs {
        out.push(Marshal {
            name: binding.name.clone(),
            fgx_type: binding.ty.to_agent(),
            c_type: c_type(&binding.ty).unwrap_or("?").to_string(),
            direction: "in".to_string(),
        });
    }
    for binding in &capsule.outputs {
        out.push(Marshal {
            name: binding.name.clone(),
            fgx_type: binding.ty.to_agent(),
            c_type: c_type(&binding.ty).unwrap_or("?").to_string(),
            direction: "out".to_string(),
        });
    }
    out
}

/// Derive the `extern "C"` signature from the typed boundary. Requires a
/// single scalar output and scalar inputs.
fn extern_c_signature(capsule: &Capsule) -> Result<String, String> {
    if capsule.outputs.len() != 1 {
        return Err(format!(
            "boundary has {} outputs; this slice supports exactly one scalar output",
            capsule.outputs.len()
        ));
    }
    let ret = c_type(&capsule.outputs[0].ty).ok_or_else(|| {
        format!(
            "output type '{}' is not scalar",
            capsule.outputs[0].ty.to_agent()
        )
    })?;
    let params = if capsule.inputs.is_empty() {
        "void".to_string()
    } else {
        capsule
            .inputs
            .iter()
            .map(|b| {
                c_type(&b.ty)
                    .map(|ct| format!("{ct} {}", b.name))
                    .ok_or_else(|| format!("input type '{}' is not scalar", b.ty.to_agent()))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    Ok(format!("{ret} {}({params})", capsule.name))
}

fn float_repr(value: f64) -> String {
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') || text.contains("inf") || text.contains("nan") {
        text
    } else {
        format!("{text}.0")
    }
}

/// A fixture input value as a C literal at the given boundary type.
fn c_input_literal(value: &CapsuleValue, ty: &Type) -> String {
    let is_f32 = matches!(ty, Type::Primitive(n) if n == "f32");
    let is_f64 = matches!(ty, Type::Primitive(n) if n == "f64");
    match value {
        CapsuleValue::Float(v) if is_f32 => format!("{}f", float_repr(*v)),
        CapsuleValue::Float(v) => float_repr(*v),
        CapsuleValue::Int(v) if is_f32 => format!("{v}.0f"),
        CapsuleValue::Int(v) if is_f64 => format!("{v}.0"),
        CapsuleValue::Int(v) => v.to_string(),
        CapsuleValue::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        CapsuleValue::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
    }
}

fn outputs_match(actual: &str, expected: &CapsuleValue) -> bool {
    match expected {
        CapsuleValue::Int(v) => actual
            .trim()
            .parse::<i64>()
            .map(|a| a == *v)
            .unwrap_or(false),
        CapsuleValue::Bool(b) => actual.trim() == if *b { "1" } else { "0" },
        CapsuleValue::Float(v) => actual
            .trim()
            .parse::<f64>()
            .map(|a| (a - *v).abs() <= 1e-6 * v.abs().max(1.0))
            .unwrap_or(false),
        CapsuleValue::Str(s) => actual.trim() == s,
    }
}

pub fn run(
    file: &str,
    program: &Program,
    name: Option<&str>,
    out_dir: &Path,
) -> Result<FfiPlan, String> {
    let capsule = select_capsule(program, name)?;
    if capsule.risk_class() != "native" {
        return Err(format!(
            "capsule '{}' is {} ({}); --native runs C-ABI capsules — use the managed runtime bridge for this language",
            capsule.name,
            capsule.risk_class(),
            capsule.language
        ));
    }

    let marshaling = marshaling(capsule);
    let signature = extern_c_signature(capsule);

    // Gate: the capsule must pass check (effects covered, deps pinned).
    let diagnostics = crate::typecheck::check(program);
    let deps_pinned = capsule.deps.iter().all(|d| d.is_pinned());
    let mut reasons = Vec::new();
    if !diagnostics.is_empty() {
        reasons.push(format!("check failed: {} diagnostic(s)", diagnostics.len()));
    }
    if !deps_pinned {
        reasons.push("capsule has unpinned dependencies".to_string());
    }
    let allowed = reasons.is_empty();

    let mut plan = FfiPlan {
        schema: "forgentex.ffi_plan.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.id.clone(),
        language: capsule.language.clone(),
        risk_class: capsule.risk_class().to_string(),
        extern_c_signature: signature.clone().unwrap_or_default(),
        marshaling,
        link_units: Vec::new(),
        effects: capsule.effects.clone(),
        grants: capsule.sandbox.clone(),
        deps_pinned,
        gate_decision: if allowed { "allow" } else { "deny" }.to_string(),
        gate_reasons: reasons,
        execution_status: "planned".to_string(),
        toolchain: None,
        passed: 0,
        total: capsule.fixtures.len(),
        transcript: Vec::new(),
        message: None,
    };

    if !allowed {
        plan.execution_status = "denied".to_string();
        return Ok(plan);
    }
    let signature = match signature {
        Ok(signature) => signature,
        Err(message) => {
            plan.execution_status = "unsupported_boundary".to_string();
            plan.message = Some(message);
            return Ok(plan);
        }
    };
    if capsule.language != "c" {
        plan.execution_status = "execution_unsupported_in_slice".to_string();
        plan.message = Some(format!(
            "boundary planned; execution wired for C in this slice (capsule is {})",
            capsule.language
        ));
        return Ok(plan);
    }

    let toolchain = match detect_toolchain() {
        Some(toolchain) => toolchain,
        None => {
            plan.execution_status = "compiler_unavailable".to_string();
            plan.message =
                Some("no host C compiler found (looked for $CC, cc, clang, gcc)".to_string());
            return Ok(plan);
        }
    };
    let compiler_dir = Path::new(&toolchain.name).parent();

    if let Err(message) = std::fs::create_dir_all(out_dir) {
        return Err(format!("failed to create {}: {message}", out_dir.display()));
    }
    let capsule_c = out_dir.join("capsule.c");
    let boundary_h = out_dir.join("boundary.h");
    let runner_c = out_dir.join("runner.c");
    let capsule_o = out_dir.join("capsule.o");
    let runner_o = out_dir.join("runner.o");
    let exe = out_dir.join(if cfg!(windows) {
        "runner.exe"
    } else {
        "runner"
    });

    let body = capsule.code.join("\n");
    let kind = out_kind(&capsule.outputs[0].ty)
        .ok_or_else(|| "output type is not a runnable scalar".to_string())?;
    let harness = generate_harness(capsule, &signature, kind);
    write_file(&capsule_c, &body)?;
    write_file(&boundary_h, &format!("{signature};\n"))?;
    write_file(&runner_c, &harness)?;

    // Compile the capsule body and the harness, then link them.
    let steps = [
        vec![
            tc(&toolchain),
            "-std=c11".into(),
            "-O2".into(),
            "-c".into(),
            disp(&capsule_c),
            "-o".into(),
            disp(&capsule_o),
        ],
        vec![
            tc(&toolchain),
            "-std=c11".into(),
            "-O2".into(),
            "-c".into(),
            "-I".into(),
            disp(out_dir),
            disp(&runner_c),
            "-o".into(),
            disp(&runner_o),
        ],
        vec![
            tc(&toolchain),
            disp(&capsule_o),
            disp(&runner_o),
            "-o".into(),
            disp(&exe),
        ],
    ];
    for argv in &steps {
        match run_with_path(argv, compiler_dir) {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                plan.execution_status = "compile_failed".to_string();
                plan.toolchain = Some(toolchain);
                plan.message = Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
                return Ok(plan);
            }
            Err(err) => {
                plan.execution_status = "compile_failed".to_string();
                plan.toolchain = Some(toolchain);
                plan.message = Some(format!("failed to launch compiler: {err}"));
                return Ok(plan);
            }
        }
    }
    plan.link_units = vec![disp(&capsule_o), disp(&runner_o)];

    // Run the linked program; it prints one line per fixture.
    let run_argv = vec![disp(&exe)];
    let output = match run_with_path(&run_argv, compiler_dir) {
        Ok(output) => output,
        Err(err) => {
            plan.execution_status = "run_failed".to_string();
            plan.toolchain = Some(toolchain);
            plan.message = Some(format!("failed to launch program: {err}"));
            return Ok(plan);
        }
    };
    plan.toolchain = Some(toolchain);
    if !output.status.success() {
        plan.execution_status = "run_failed".to_string();
        plan.message = Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
        return Ok(plan);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    for (index, fixture) in capsule.fixtures.iter().enumerate() {
        let inputs: Vec<String> = fixture
            .inputs
            .iter()
            .map(CapsuleValue::to_literal)
            .collect();
        let expected = fixture
            .outputs
            .first()
            .map(CapsuleValue::to_literal)
            .unwrap_or_default();
        let actual = lines.get(index).copied().unwrap_or("").trim().to_string();
        let pass = fixture
            .outputs
            .first()
            .map(|exp| outputs_match(&actual, exp))
            .unwrap_or(false);
        if pass {
            plan.passed += 1;
        }
        plan.transcript.push(FixtureRun {
            index,
            inputs,
            output: actual,
            expected,
            pass,
        });
    }
    plan.execution_status = if capsule.fixtures.is_empty() {
        "compiled".to_string()
    } else if plan.passed == plan.total {
        "executed".to_string()
    } else {
        "mismatch".to_string()
    };
    Ok(plan)
}

fn generate_harness(capsule: &Capsule, _signature: &str, kind: OutKind) -> String {
    let mut out = String::new();
    out.push_str("#include <stdio.h>\n#include <stdint.h>\n#include <stddef.h>\n");
    out.push_str("#include \"boundary.h\"\n\nint main(void) {\n");
    for fixture in &capsule.fixtures {
        let args = fixture
            .inputs
            .iter()
            .zip(capsule.inputs.iter())
            .map(|(value, binding)| c_input_literal(value, &binding.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let call = format!("{}({args})", capsule.name);
        let line = match kind {
            OutKind::Int => format!("    printf(\"%lld\\n\", (long long)({call}));\n"),
            OutKind::Float => format!("    printf(\"%.9g\\n\", (double)({call}));\n"),
            OutKind::Bool => format!("    printf(\"%d\\n\", ({call}) ? 1 : 0);\n"),
        };
        out.push_str(&line);
    }
    out.push_str("    return 0;\n}\n");
    out
}

fn tc(toolchain: &Toolchain) -> String {
    toolchain.name.clone()
}

fn disp(path: &Path) -> String {
    path.display().to_string()
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn select_capsule<'a>(program: &'a Program, name: Option<&str>) -> Result<&'a Capsule, String> {
    match name {
        Some(name) => program
            .capsules()
            .find(|capsule| capsule.name == name)
            .ok_or_else(|| format!("no capsule named '{name}'")),
        None => {
            let mut capsules = program.capsules();
            let first = capsules
                .next()
                .ok_or_else(|| "no capsule in file".to_string())?;
            if capsules.next().is_some() {
                return Err(
                    "file has multiple capsules; pass --capsule <name> to choose one".to_string(),
                );
            }
            Ok(first)
        }
    }
}

impl FfiPlan {
    pub fn is_success(&self) -> bool {
        // A missing host compiler or a not-yet-wired language is not a failure
        // of Forgentex itself; a real mismatch or denial is.
        matches!(
            self.execution_status.as_str(),
            "executed"
                | "compiled"
                | "compiler_unavailable"
                | "execution_unsupported_in_slice"
                | "planned"
        )
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex C-ABI FFI bridge\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} lang={} class={}\n",
            self.schema, self.file, self.capsule, self.language, self.risk_class
        ));
        out.push_str(&format!("  extern \"C\": {}\n", self.extern_c_signature));
        for m in &self.marshaling {
            out.push_str(&format!(
                "    {:<4} {:<12} -> {:<10} {}\n",
                m.direction, m.fgx_type, m.c_type, m.name
            ));
        }
        out.push_str(&format!(
            "  gate: {} effects=[{}] grants=[{}] deps_pinned={}{}\n",
            self.gate_decision,
            self.effects.join(", "),
            self.grants.join(", "),
            self.deps_pinned,
            if self.gate_reasons.is_empty() {
                String::new()
            } else {
                format!(" reasons=[{}]", self.gate_reasons.join("; "))
            }
        ));
        if let Some(toolchain) = &self.toolchain {
            out.push_str(&format!(
                "  toolchain: {} ({})\n",
                toolchain.name, toolchain.version
            ));
        }
        if !self.link_units.is_empty() {
            out.push_str(&format!("  link: {}\n", self.link_units.join(" + ")));
        }
        out.push_str(&format!(
            "  execution: {} ({}/{} fixtures)\n",
            self.execution_status, self.passed, self.total
        ));
        for run in &self.transcript {
            out.push_str(&format!(
                "    [{}] ({}) -> {} expected {} {}\n",
                run.index,
                run.inputs.join(", "),
                run.output,
                run.expected,
                if run.pass { "ok" } else { "MISMATCH" }
            ));
        }
        if let Some(message) = &self.message {
            out.push_str(&format!("  note: {message}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let marshaling = self
            .marshaling
            .iter()
            .map(|m| {
                format!(
                    "{{ \"name\": {}, \"fgx_type\": {}, \"c_type\": {}, \"direction\": {} }}",
                    json::string(&m.name),
                    json::string(&m.fgx_type),
                    json::string(&m.c_type),
                    json::string(&m.direction)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        let transcript = self
            .transcript
            .iter()
            .map(|r| {
                format!(
                    "{{ \"index\": {}, \"inputs\": {}, \"output\": {}, \"expected\": {}, \"pass\": {} }}",
                    r.index,
                    json::string_array(&r.inputs),
                    json::string(&r.output),
                    json::string(&r.expected),
                    r.pass
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        let toolchain = match &self.toolchain {
            Some(t) => format!(
                "{{ \"name\": {}, \"version\": {} }}",
                json::string(&t.name),
                json::string(&t.version)
            ),
            None => "null".to_string(),
        };
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsule\": {},\n  \"language\": {},\n  \"risk_class\": {},\n  \"extern_c_signature\": {},\n  \"marshaling\": [\n    {}\n  ],\n  \"link_units\": {},\n  \"effects\": {},\n  \"grants\": {},\n  \"deps_pinned\": {},\n  \"gate_decision\": {},\n  \"gate_reasons\": {},\n  \"execution_status\": {},\n  \"toolchain\": {},\n  \"passed\": {},\n  \"total\": {},\n  \"transcript\": [\n    {}\n  ],\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.capsule),
            json::string(&self.language),
            json::string(&self.risk_class),
            json::string(&self.extern_c_signature),
            marshaling,
            json::string_array(&self.link_units),
            json::string_array(&self.effects),
            json::string_array(&self.grants),
            self.deps_pinned,
            json::string(&self.gate_decision),
            json::string_array(&self.gate_reasons),
            json::string(&self.execution_status),
            toolchain,
            self.passed,
            self.total,
            transcript,
            json::opt_string(self.message.as_deref()),
        )
    }
}
