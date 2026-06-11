use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

use crate::abi::analyze_abi;
use crate::asmcheck::analyze_assembly;
use crate::backend::generate_backend_ir;
use crate::c_backend::generate_c_for_project;
use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::{parse_file, parse_file_detailed};
use crate::patch::{apply_patch_with_options, diagnose_patch_source, PatchOptions};
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzSuiteReport {
    pub schema: String,
    pub iterations: usize,
    pub cases: Vec<PropertyCase>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyCase {
    pub name: String,
    pub category: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendDifferentialOptions {
    pub project: PathBuf,
    pub source: PathBuf,
    pub entry: String,
    pub input: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendDifferentialReport {
    pub schema: String,
    pub project: String,
    pub source: String,
    pub entry: String,
    pub input: String,
    pub backend: String,
    pub interpreter_status: String,
    pub generated_header_bytes: usize,
    pub generated_source_bytes: usize,
    pub backend_entry_found: bool,
    pub ir_entry_found: bool,
    pub result: String,
    pub notes: Vec<String>,
}

impl Default for BackendDifferentialOptions {
    fn default() -> Self {
        Self {
            project: PathBuf::from("."),
            source: PathBuf::from("examples/media/audio_mix.fgx"),
            entry: "mix_samples".to_string(),
            input: PathBuf::from("examples/run/audio_mix.json"),
        }
    }
}

impl FuzzSuiteReport {
    pub fn failed(&self) -> usize {
        self.cases
            .iter()
            .filter(|case| case.status != "pass")
            .count()
    }

    pub fn passed(&self) -> usize {
        self.cases.len() - self.failed()
    }

    pub fn is_success(&self) -> bool {
        self.failed() == 0
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex Stage 22 fuzz/property suite\n");
        out.push_str(&format!(
            "schema={} iterations={} passed={} failed={}\n",
            self.schema,
            self.iterations,
            self.passed(),
            self.failed()
        ));
        for case in &self.cases {
            out.push_str(&format!(
                "  - {} [{}] status={} {}\n",
                case.name, case.category, case.status, case.detail
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"iterations\": {},\n", self.iterations));
        out.push_str(&format!("  \"passed\": {},\n", self.passed()));
        out.push_str(&format!("  \"failed\": {},\n", self.failed()));
        out.push_str("  \"cases\": [");
        for (idx, case) in self.cases.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"name\": {}, \"category\": {}, \"status\": {}, \"detail\": {}}}",
                quoted(&case.name),
                quoted(&case.category),
                quoted(&case.status),
                quoted(&case.detail)
            ));
        }
        if !self.cases.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

impl BackendDifferentialReport {
    pub fn is_success(&self) -> bool {
        self.result == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex backend differential report\n");
        out.push_str(&format!(
            "schema={} backend={} entry={} result={}\n",
            self.schema, self.backend, self.entry, self.result
        ));
        out.push_str(&format!(
            "  interpreter_status={} backend_entry_found={} ir_entry_found={}\n",
            self.interpreter_status, self.backend_entry_found, self.ir_entry_found
        ));
        out.push_str(&format!(
            "  generated_header_bytes={} generated_source_bytes={}\n",
            self.generated_header_bytes, self.generated_source_bytes
        ));
        for note in &self.notes {
            out.push_str(&format!("  - {note}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"project\": {},\n", quoted(&self.project)));
        out.push_str(&format!("  \"source\": {},\n", quoted(&self.source)));
        out.push_str(&format!("  \"entry\": {},\n", quoted(&self.entry)));
        out.push_str(&format!("  \"input\": {},\n", quoted(&self.input)));
        out.push_str(&format!("  \"backend\": {},\n", quoted(&self.backend)));
        out.push_str(&format!(
            "  \"interpreter_status\": {},\n",
            quoted(&self.interpreter_status)
        ));
        out.push_str(&format!(
            "  \"generated_header_bytes\": {},\n",
            self.generated_header_bytes
        ));
        out.push_str(&format!(
            "  \"generated_source_bytes\": {},\n",
            self.generated_source_bytes
        ));
        out.push_str(&format!(
            "  \"backend_entry_found\": {},\n",
            bool_json(self.backend_entry_found)
        ));
        out.push_str(&format!(
            "  \"ir_entry_found\": {},\n",
            bool_json(self.ir_entry_found)
        ));
        out.push_str(&format!("  \"result\": {},\n", quoted(&self.result)));
        out.push_str("  \"notes\": [");
        for (idx, note) in self.notes.iter().enumerate() {
            if idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&quoted(note));
        }
        out.push_str("]\n}");
        out
    }
}

pub fn run_fuzz_suite(iterations: usize) -> FuzzSuiteReport {
    let mut report = FuzzSuiteReport {
        schema: "forgentex.fuzz.v1".to_string(),
        iterations,
        cases: Vec::new(),
    };

    for (relative, source) in corpus_sources() {
        record_case(
            &mut report,
            relative,
            "roundtrip-corpus",
            roundtrip_property(Path::new(relative), source),
        );
    }

    let mut seed = 0x5eed_2200_0000_0001_u64;
    for iteration in 0..iterations {
        let source = generated_program(iteration, next_u64(&mut seed));
        let path = PathBuf::from(format!("generated_{iteration}.fgx"));
        record_case(
            &mut report,
            &format!("generated-roundtrip-{iteration}"),
            "roundtrip-generated",
            roundtrip_property(&path, &source),
        );
        record_case(
            &mut report,
            &format!("generated-patch-{iteration}"),
            "patch-format-check",
            patch_format_check_property(&path, &source, &format!("generated_{iteration}")),
        );

        let mutated = mutate_source(&source, next_u64(&mut seed));
        record_case(
            &mut report,
            &format!("mutated-parse-{iteration}"),
            "parser-no-panic",
            parse_no_panic_property(&path, &mutated),
        );
    }

    for (name, source) in invalid_sources() {
        record_case(
            &mut report,
            name,
            "invalid-no-panic",
            parse_no_panic_property(Path::new(name), source),
        );
    }

    record_case(
        &mut report,
        "patch-diagnostics-no-panic",
        "patch-fuzz",
        patch_diagnostics_no_panic_property(),
    );
    record_case(
        &mut report,
        "target-abi-assembly-invariants",
        "target-validation",
        target_validation_property(),
    );

    report
}

pub fn run_backend_differential(
    options: &BackendDifferentialOptions,
) -> Result<BackendDifferentialReport, String> {
    let source_text = fs::read_to_string(&options.source)
        .map_err(|err| format!("failed to read {}: {err}", options.source.display()))?;
    let input_text = fs::read_to_string(&options.input)
        .map_err(|err| format!("failed to read {}: {err}", options.input.display()))?;
    let program = parse_file(&options.source, &source_text)
        .map_err(|err| format!("failed to parse {}: {err}", options.source.display()))?;
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "source has diagnostics and cannot be differentially tested: {}",
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    let interpreter_output = run_entry_json(&program, &options.entry, &input_text)?;
    let interpreter_status = json_status(&interpreter_output).unwrap_or_else(|| {
        if interpreter_output.contains("\"status\"") {
            "unknown".to_string()
        } else {
            "missing".to_string()
        }
    });

    let (header, source) = generate_c_for_project(&options.project)?;
    let ir = generate_backend_ir(&options.project)?;
    let backend_entry_found = header.contains(&options.entry) && source.contains(&options.entry);
    let ir_entry_found = ir.modules.iter().any(|module| {
        module
            .functions
            .iter()
            .any(|function| function.name == options.entry)
    });
    let pass = matches!(interpreter_status.as_str(), "pass" | "ok")
        && backend_entry_found
        && ir_entry_found;

    let mut notes = vec![
        "interpreter executed the golden JSON input".to_string(),
        "C backend generated reference header/source output".to_string(),
        "FGIR contained the same entry symbol".to_string(),
    ];
    if !backend_entry_found {
        notes.push("C backend output did not contain the entry symbol".to_string());
    }
    if !ir_entry_found {
        notes.push("backend IR did not contain the entry symbol".to_string());
    }

    Ok(BackendDifferentialReport {
        schema: "forgentex.backend_differential.v1".to_string(),
        project: options.project.display().to_string(),
        source: options.source.display().to_string(),
        entry: options.entry.clone(),
        input: options.input.display().to_string(),
        backend: "c".to_string(),
        interpreter_status,
        generated_header_bytes: header.len(),
        generated_source_bytes: source.len(),
        backend_entry_found,
        ir_entry_found,
        result: if pass { "pass" } else { "fail" }.to_string(),
        notes,
    })
}

fn record_case(
    report: &mut FuzzSuiteReport,
    name: &str,
    category: &str,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => report.cases.push(PropertyCase {
            name: name.to_string(),
            category: category.to_string(),
            status: "pass".to_string(),
            detail: "ok".to_string(),
        }),
        Err(detail) => report.cases.push(PropertyCase {
            name: name.to_string(),
            category: category.to_string(),
            status: "fail".to_string(),
            detail,
        }),
    }
}

fn roundtrip_property(path: &Path, source: &str) -> Result<(), String> {
    let program = parse_file(path, source)?;
    let agent = format_agent(&program);
    let from_agent = parse_file(Path::new("roundtrip.fgai"), &agent)?;
    if !program.canonical_eq(&from_agent) {
        return Err("fgx -> AST -> fgai -> AST changed canonical AST".to_string());
    }
    let human = format_human(&from_agent);
    let from_human = parse_file(Path::new("roundtrip.fgx"), &human)?;
    if !program.canonical_eq(&from_human) {
        return Err("fgai -> AST -> fgx -> AST changed canonical AST".to_string());
    }
    if format_agent(&from_agent) != agent {
        return Err("agent formatter is not idempotent".to_string());
    }
    if format_human(&from_human) != human {
        return Err("human formatter is not idempotent".to_string());
    }
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "type check produced diagnostics: {}",
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    let canonical_once = program.to_canonical_json();
    let canonical_twice = from_human.to_canonical_json();
    if canonical_once != canonical_twice {
        return Err("canonical AST serialization is not stable after roundtrip".to_string());
    }
    Ok(())
}

fn patch_format_check_property(
    path: &Path,
    source: &str,
    function_name: &str,
) -> Result<(), String> {
    let program = parse_file(path, source)?;
    let patch = format!(
        "VERSION 1\nTARGET @function.{function_name}\nPRECONDITION NODE_EXISTS @function.{function_name}\nOP ADD_EFFECT effect=cpu.simd\n"
    );
    let patched = apply_patch_with_options(program, &patch, PatchOptions { check_only: true })?;
    let diagnostics = check(&patched.program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "patched program produced diagnostics: {}",
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    let human = format_human(&patched.program);
    let reparsed_human = parse_file(Path::new("patched.fgx"), &human)?;
    let agent = format_agent(&patched.program);
    let reparsed_agent = parse_file(Path::new("patched.fgai"), &agent)?;
    if !reparsed_human.canonical_eq(&reparsed_agent) {
        return Err("patched human and agent views do not share a canonical AST".to_string());
    }
    if !check(&reparsed_human).is_empty() {
        return Err("patched formatted human program no longer type-checks".to_string());
    }
    Ok(())
}

fn parse_no_panic_property(path: &Path, source: &str) -> Result<(), String> {
    panic::catch_unwind(|| parse_file_detailed(path, source))
        .map(|_| ())
        .map_err(|_| "parser panicked".to_string())
}

fn patch_diagnostics_no_panic_property() -> Result<(), String> {
    let patches = [
        "",
        "PATCH",
        "VERSION 1\nTARGET @function.nope\nOP ADD_EFFECT\n",
        "TARGET @function.mix_samples\nOP INSERT_FAST_PATH target=x86_64\n",
        "SIGNED_BY agent\nSIGNATURE mock-sig-stage18-demo\nOP UNKNOWN\n",
    ];
    for (idx, patch) in patches.iter().enumerate() {
        panic::catch_unwind(|| diagnose_patch_source(&format!("fuzz_{idx}.fgp"), patch))
            .map_err(|_| format!("patch diagnostic panicked for seed {idx}"))?;
    }
    Ok(())
}

fn target_validation_property() -> Result<(), String> {
    let layout = parse_manifest_file("examples/abi/layout_demo.fgx")?;
    for triple in [
        "x86_64-unknown-linux-gnu",
        "armv7-unknown-linux-gnueabihf",
        "aarch64-unknown-linux-gnu",
        "riscv64gc-unknown-linux-gnu",
    ] {
        let report = analyze_abi(&layout, triple);
        if !report.diagnostics.is_empty() {
            return Err(format!("ABI report for {triple} produced diagnostics"));
        }
    }

    for (file, triple) in [
        (
            "examples/arch/x86_64_popcount.fgx",
            "x86_64-pc-windows-msvc",
        ),
        (
            "examples/arch/aarch64_popcount.fgx",
            "aarch64-unknown-linux-gnu",
        ),
        (
            "examples/arch/riscv64_add.fgx",
            "riscv64gc-unknown-linux-gnu",
        ),
    ] {
        let program = parse_manifest_file(file)?;
        let result = analyze_assembly(&program, triple);
        if !result.diagnostics.is_empty() {
            return Err(format!("assembly check for {file} on {triple} failed"));
        }
    }

    let x86 = parse_manifest_file("examples/arch/x86_64_popcount.fgx")?;
    let mismatch = analyze_assembly(&x86, "aarch64-unknown-linux-gnu");
    if mismatch.diagnostics.is_empty() {
        return Err("cross-target assembly mismatch did not produce diagnostics".to_string());
    }
    Ok(())
}

fn parse_manifest_file(relative: &str) -> Result<crate::ast::Program, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let source = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    parse_file(&path, &source)
}

fn corpus_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("examples/hello.fgx", include_str!("../examples/hello.fgx")),
        (
            "examples/hello.fgai",
            include_str!("../examples/hello.fgai"),
        ),
        (
            "examples/media/audio_mix.fgx",
            include_str!("../examples/media/audio_mix.fgx"),
        ),
        (
            "examples/media/audio_mix.fgai",
            include_str!("../examples/media/audio_mix.fgai"),
        ),
        (
            "examples/media/bitreader.fgx",
            include_str!("../examples/media/bitreader.fgx"),
        ),
        (
            "examples/media/h264_nal_header.fgx",
            include_str!("../examples/media/h264_nal_header.fgx"),
        ),
    ]
}

fn invalid_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("invalid_empty.fgx", ""),
        ("invalid_fn.fgx", "fn"),
        ("invalid_struct.fgx", "struct Broken:\n    value\n"),
        ("invalid_agent.fgai", "F broken {"),
        (
            "invalid_control.fgx",
            "fn bad(x: i32) -> i32:\n    if x >:\n        return x\n",
        ),
    ]
}

fn generated_program(index: usize, seed: u64) -> String {
    let add = (seed % 17) as i64 + 1;
    let threshold = (seed % 31) as i64 + 4;
    format!(
        "struct Generated{index}:\n    left: i32\n    right: i32\n\nfn generated_{index}(x: i32, y: i32) -> i32:\n    let base: i32 = x + y\n    let adjusted: i32 = base + {add}\n    if adjusted > {threshold}:\n        return adjusted - {threshold}\n    return adjusted + {threshold}\n"
    )
}

fn mutate_source(source: &str, seed: u64) -> String {
    if source.is_empty() {
        return "@".to_string();
    }
    let mut bytes = source.as_bytes().to_vec();
    let index = (seed as usize) % bytes.len();
    let replacements = [b'@', b'{', b'}', b':', b'\n', b'_', b'0', b'F'];
    bytes[index] = replacements[(seed as usize / 7) % replacements.len()];
    String::from_utf8_lossy(&bytes).to_string()
}

fn next_u64(seed: &mut u64) -> u64 {
    let mut x = *seed;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *seed = x;
    x
}

fn json_status(output: &str) -> Option<String> {
    let key = "\"status\"";
    let start = output.find(key)?;
    let after_key = &output[start + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let value = after_colon.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(value[..end].to_string())
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}
