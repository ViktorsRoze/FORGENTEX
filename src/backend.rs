use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Expr, FunctionDecl, Item, Program, Stmt, Type};
use crate::c_backend::build_c_project;
use crate::diag::{diagnostics_array_to_json, diagnostics_to_json};
use crate::effects::{analyze as analyze_effects, effects_to_json};
use crate::format::format_expr;
use crate::parser::parse_file;
use crate::project::{check_project, load_project, Project, PublicSymbol};
use crate::targets::target_metadata_for_function;
use crate::typecheck::check;
use crate::zig_backend::build_zig_project;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendBuildArtifacts {
    pub backend: String,
    pub files: Vec<PathBuf>,
    pub metadata_files: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendRunArtifacts {
    pub backend: String,
    pub fir_schema: String,
    pub contract_status: String,
    pub json: String,
}

pub trait Backend {
    fn info(&self) -> &'static BackendInfo;

    fn build_project(
        &self,
        _input: &Path,
        _out_dir: &Path,
    ) -> Result<BackendBuildArtifacts, String> {
        Err(format!(
            "backend '{}' does not emit build artifacts",
            self.info().name
        ))
    }

    fn run_entry_json(
        &self,
        _program: &Program,
        _entry: &str,
        _input: &str,
    ) -> Result<BackendRunArtifacts, String> {
        Err(format!(
            "backend '{}' does not run entries in-process yet; use backend 'interp'",
            self.info().name
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Interpreter,
    C,
    Zig,
    Planned(&'static BackendInfo),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendInfo {
    pub name: &'static str,
    pub status: &'static str,
    pub interface: &'static str,
    pub role: &'static str,
    pub feature: Option<&'static str>,
    pub default_enabled: bool,
    pub dependency_status: &'static str,
    pub target_families: &'static [&'static str],
    pub notes: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendIr {
    pub package: String,
    pub version: String,
    pub modules: Vec<IrModule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrModule {
    pub name: String,
    pub path: String,
    pub imports: Vec<String>,
    pub source_hash: String,
    pub enums: Vec<IrEnum>,
    pub structs: Vec<IrStruct>,
    pub functions: Vec<IrFunction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrEnum {
    pub id: String,
    pub name: String,
    pub public: bool,
    pub variants: Vec<IrVariant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrVariant {
    pub name: String,
    pub payload: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrStruct {
    pub id: String,
    pub name: String,
    pub public: bool,
    pub fields: Vec<IrField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrField {
    pub name: String,
    pub ty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrFunction {
    pub id: String,
    pub name: String,
    pub public: bool,
    pub unsafe_fn: bool,
    pub ret: String,
    pub params: Vec<IrParam>,
    pub effects: Vec<String>,
    pub target_labels: Vec<String>,
    pub blocks: Vec<IrBlock>,
    pub backend_contracts: Vec<IrBackendContract>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrParam {
    pub name: String,
    pub ty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrBlock {
    pub label: String,
    pub instructions: Vec<IrInstruction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrInstruction {
    pub op: String,
    pub args: Vec<String>,
    pub ty: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrBackendContract {
    pub backend: String,
    pub status: String,
    pub blockers: Vec<String>,
}

const BACKENDS: &[BackendInfo] = &[
    BackendInfo {
        name: "interp",
        status: "available",
        interface: "run",
        role: "deterministic interpreter",
        feature: None,
        default_enabled: true,
        dependency_status: "zero-dep core",
        target_families: &["host"],
        notes: "Default execution backend for checked Forgentex programs.",
    },
    BackendInfo {
        name: "c",
        status: "available",
        interface: "aot-source",
        role: "reference transpiler",
        feature: None,
        default_enabled: true,
        dependency_status: "zero-dep source emission; host C compiler is external",
        target_families: &["x86_64", "arm", "aarch64", "riscv32", "riscv64"],
        notes: "C11 output is the reference backend for safe project code.",
    },
    BackendInfo {
        name: "zig",
        status: "available",
        interface: "aot-source",
        role: "source transpiler and cross-target experiment",
        feature: None,
        default_enabled: true,
        dependency_status: "zero-dep source emission; Zig compiler invocation is deferred",
        target_families: &["x86_64", "arm", "aarch64", "riscv32", "riscv64"],
        notes: "Zig source output for safe project code; compiler invocation is deferred.",
    },
    BackendInfo {
        name: "cranelift",
        status: "planned",
        interface: "jit",
        role: "JIT/native experiment",
        feature: Some("jit-cranelift"),
        default_enabled: false,
        dependency_status: "feature scaffold only; no Rust dependency linked",
        target_families: &["x86_64", "aarch64"],
        notes: "Planned after the backend IR is exercised by more examples.",
    },
    BackendInfo {
        name: "llvm",
        status: "planned",
        interface: "aot-native",
        role: "optimizing native backend",
        feature: Some("backend-llvm"),
        default_enabled: false,
        dependency_status: "feature scaffold only; no Rust dependency linked",
        target_families: &["x86_64", "arm", "aarch64", "riscv32", "riscv64"],
        notes: "Reserved for broad native target support; not linked in v0.",
    },
    BackendInfo {
        name: "mlir",
        status: "planned",
        interface: "dialect",
        role: "dialect and media/GPU experiments",
        feature: Some("backend-mlir"),
        default_enabled: false,
        dependency_status: "feature scaffold only; no Rust dependency linked",
        target_families: &["x86_64", "aarch64", "riscv64"],
        notes: "Longer-term path for domain-specific lowering.",
    },
];

impl Backend for BackendKind {
    fn info(&self) -> &'static BackendInfo {
        match self {
            BackendKind::Interpreter => {
                backend_info("interp").expect("interp backend is cataloged")
            }
            BackendKind::C => backend_info("c").expect("c backend is cataloged"),
            BackendKind::Zig => backend_info("zig").expect("zig backend is cataloged"),
            BackendKind::Planned(info) => info,
        }
    }

    fn build_project(&self, input: &Path, out_dir: &Path) -> Result<BackendBuildArtifacts, String> {
        match self {
            BackendKind::C => {
                let ir = generate_ready_backend_ir(input, self.info().name)?;
                let artifacts = build_c_project(input, out_dir)?;
                let files = vec![artifacts.header, artifacts.source];
                let metadata_files =
                    write_backend_metadata(out_dir, self.info().name, &ir, &files)?;
                Ok(BackendBuildArtifacts {
                    backend: self.info().name.to_string(),
                    files,
                    metadata_files,
                })
            }
            BackendKind::Zig => {
                let ir = generate_ready_backend_ir(input, self.info().name)?;
                let artifacts = build_zig_project(input, out_dir)?;
                let files = vec![artifacts.source];
                let metadata_files =
                    write_backend_metadata(out_dir, self.info().name, &ir, &files)?;
                Ok(BackendBuildArtifacts {
                    backend: self.info().name.to_string(),
                    files,
                    metadata_files,
                })
            }
            BackendKind::Planned(info) => Err(planned_backend_error(info)),
            BackendKind::Interpreter => Err(format!(
                "backend '{}' is a run backend and does not emit build artifacts; use --backend c or --backend zig",
                self.info().name
            )),
        }
    }

    fn run_entry_json(
        &self,
        program: &Program,
        entry: &str,
        input: &str,
    ) -> Result<BackendRunArtifacts, String> {
        match self {
            BackendKind::Interpreter => {
                let ir = lower_program_to_run_ir(program);
                ensure_backend_entry_ready(&ir, self.info().name, entry)?;
                Ok(BackendRunArtifacts {
                    backend: self.info().name.to_string(),
                    fir_schema: "forgentex.backend_ir.v1".to_string(),
                    contract_status: "ready".to_string(),
                    json: crate::interp::run_entry_json_with_ir(program, &ir, entry, input)?,
                })
            }
            BackendKind::Planned(info) => Err(planned_backend_error(info)),
            BackendKind::C | BackendKind::Zig => Err(format!(
                "backend '{}' is an AOT source backend and does not run entries in-process yet; use backend 'interp'",
                self.info().name
            )),
        }
    }
}

pub fn build_project_with_backend(
    input: &Path,
    backend: &str,
    out_dir: &Path,
) -> Result<BackendBuildArtifacts, String> {
    backend_by_name(backend)
        .ok_or_else(|| format!("unknown backend '{backend}'"))?
        .build_project(input, out_dir)
}

pub fn run_entry_with_backend(
    program: &Program,
    backend: &str,
    entry: &str,
    input: &str,
) -> Result<BackendRunArtifacts, String> {
    backend_by_name(backend)
        .ok_or_else(|| format!("unknown backend '{backend}'"))?
        .run_entry_json(program, entry, input)
}

/// Generate FGIR and fail if the named backend's contracts block any
/// declaration or function — the single honesty gate every source-emitting
/// entry point must pass through (used by the Backend trait AND the raw
/// build_c_project / build_zig_project APIs, so no path can emit invalid code).
pub(crate) fn generate_ready_backend_ir(input: &Path, backend: &str) -> Result<BackendIr, String> {
    // Source backends lower eligible private generic functions through a
    // deterministic AST monomorphization pass before FIR contract preflight.
    // Keep the general/public FGIR path generic so tooling can still inspect
    // the source-level declarations.
    if matches!(backend, "c" | "zig") && !is_single_source_file(input) {
        let diagnostics = check_project(input)?;
        if !diagnostics.is_empty() {
            return Err(format!(
                "project has diagnostics and IR was not generated:\n{}",
                diagnostics_to_json(&diagnostics)
            ));
        }
        let mut project = load_project(input)?;
        crate::monomorphize::monomorphize_project(&mut project)?;
        crate::closure_lower::lower_non_capturing_closures(&mut project);
        return generate_ready_backend_ir_from_project(&project, backend);
    }
    let ir = generate_backend_ir(input)?;
    let blockers = backend_contract_blockers(&ir, backend);
    if blockers.is_empty() {
        return Ok(ir);
    }
    Err(format!(
        "FIR contract blocks backend '{}': {}",
        backend,
        blockers.join("; ")
    ))
}

/// Project-based variant of the readiness gate: lower an already-loaded
/// (possibly optimized) project to FGIR and fail on any contract blocker.
/// Used by the `--opt` build path so FIR derives from the optimized ASTs.
pub(crate) fn generate_ready_backend_ir_from_project(
    project: &Project,
    backend: &str,
) -> Result<BackendIr, String> {
    let ir = lower_project_to_ir(project);
    let blockers = backend_contract_blockers(&ir, backend);
    if blockers.is_empty() {
        return Ok(ir);
    }
    Err(format!(
        "FIR contract blocks backend '{}': {}",
        backend,
        blockers.join("; ")
    ))
}

/// `fgx build --opt`: build through the v3.2 optimizer. Returns the artifacts
/// plus the optimize report; FIR receipts are written from the optimized IR.
pub fn build_project_with_backend_opt(
    input: &Path,
    backend: &str,
    out_dir: &Path,
) -> Result<(BackendBuildArtifacts, crate::optimize::OptReport), String> {
    match backend {
        "c" => {
            let (artifacts, ir, report) = crate::c_backend::build_c_project_opt(input, out_dir)?;
            let files = vec![artifacts.header, artifacts.source];
            let metadata_files = write_backend_metadata(out_dir, "c", &ir, &files)?;
            Ok((
                BackendBuildArtifacts {
                    backend: "c".to_string(),
                    files,
                    metadata_files,
                },
                report,
            ))
        }
        "zig" => {
            let (artifacts, ir, report) =
                crate::zig_backend::build_zig_project_opt(input, out_dir)?;
            let files = vec![artifacts.source];
            let metadata_files = write_backend_metadata(out_dir, "zig", &ir, &files)?;
            Ok((
                BackendBuildArtifacts {
                    backend: "zig".to_string(),
                    files,
                    metadata_files,
                },
                report,
            ))
        }
        other => Err(format!(
            "--opt builds support backends c and zig (got '{other}')"
        )),
    }
}

fn ensure_backend_entry_ready(ir: &BackendIr, backend: &str, entry: &str) -> Result<(), String> {
    let blockers = backend_entry_contract_blockers(ir, backend, entry);
    if blockers.is_empty() {
        return Ok(());
    }
    Err(format!(
        "FIR contract blocks backend '{}': {}",
        backend,
        blockers.join("; ")
    ))
}

fn write_backend_metadata(
    out_dir: &Path,
    backend: &str,
    ir: &BackendIr,
    files: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(out_dir)
        .map_err(|err| format!("failed to create {}: {err}", out_dir.display()))?;
    let fir_json = backend_ir_json(ir);
    let fir_hash = stable_hash(&fir_json);
    let fir_path = out_dir.join("forgentex.fgir.json");
    let receipt_path = out_dir.join("forgentex.backend_build.json");
    fs::write(&fir_path, &fir_json)
        .map_err(|err| format!("failed to write {}: {err}", fir_path.display()))?;
    let receipt = backend_build_receipt_json(backend, ir, &fir_path, &fir_hash, files);
    fs::write(&receipt_path, receipt)
        .map_err(|err| format!("failed to write {}: {err}", receipt_path.display()))?;
    Ok(vec![fir_path, receipt_path])
}

fn backend_build_receipt_json(
    backend: &str,
    ir: &BackendIr,
    fir_path: &Path,
    fir_hash: &str,
    files: &[PathBuf],
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.backend_build.v1\",\n");
    out.push_str(&format!("  \"backend\": {},\n", quoted(backend)));
    out.push_str(&format!("  \"package\": {},\n", quoted(&ir.package)));
    out.push_str(&format!("  \"version\": {},\n", quoted(&ir.version)));
    out.push_str("  \"fir_schema\": \"forgentex.backend_ir.v1\",\n");
    out.push_str(&format!(
        "  \"fir_path\": {},\n",
        quoted(&display_path(fir_path))
    ));
    out.push_str(&format!("  \"fir_hash\": {},\n", quoted(fir_hash)));
    out.push_str("  \"contract_status\": \"ready\",\n");
    let source_files = files
        .iter()
        .map(|path| display_path(path))
        .collect::<Vec<_>>();
    push_string_array(&mut out, 2, "source_files", &source_files, false);
    out.push('\n');
    out.push('}');
    out
}

fn backend_contract_blockers(ir: &BackendIr, backend: &str) -> Vec<String> {
    let mut blockers = Vec::new();
    for module in &ir.modules {
        blockers.extend(backend_declaration_blockers(module, backend));
        for function in &module.functions {
            if let Some(blocker) =
                backend_contract_blocker_for_function(&module.name, function, backend)
            {
                blockers.push(blocker);
            }
        }
    }
    blockers
}

fn backend_declaration_blockers(module: &IrModule, backend: &str) -> Vec<String> {
    if !matches!(backend, "c" | "zig") {
        return Vec::new();
    }
    let mut blockers = Vec::new();
    for decl in &module.structs {
        if let Some(blocker) = backend_struct_blocker(&module.name, decl) {
            blockers.push(blocker);
        }
    }
    for decl in &module.enums {
        if let Some(blocker) = backend_enum_blocker(&module.name, decl) {
            blockers.push(blocker);
        }
    }
    blockers
}

fn backend_struct_blocker(module: &str, decl: &IrStruct) -> Option<String> {
    let mut blockers = BTreeSet::new();
    for field in &decl.fields {
        collect_generic_type_blockers(&Type::parse(&field.ty), &mut blockers);
    }
    format_declaration_blocker(module, "struct", &decl.name, blockers)
}

fn backend_enum_blocker(module: &str, decl: &IrEnum) -> Option<String> {
    let mut blockers = BTreeSet::new();
    for variant in &decl.variants {
        if let Some(payload) = &variant.payload {
            collect_generic_type_blockers(&Type::parse(payload), &mut blockers);
        }
    }
    format_declaration_blocker(module, "enum", &decl.name, blockers)
}

fn format_declaration_blocker(
    module: &str,
    kind: &str,
    name: &str,
    blockers: BTreeSet<String>,
) -> Option<String> {
    if blockers.is_empty() {
        return None;
    }
    Some(format!(
        "{}::{} {} blockers=[{}]",
        module,
        kind,
        name,
        blockers.into_iter().collect::<Vec<_>>().join(",")
    ))
}

fn backend_entry_contract_blockers(ir: &BackendIr, backend: &str, entry: &str) -> Vec<String> {
    let mut blockers = Vec::new();
    for module in &ir.modules {
        for function in &module.functions {
            if function.name != entry {
                continue;
            }
            if let Some(blocker) =
                backend_contract_blocker_for_function(&module.name, function, backend)
            {
                blockers.push(blocker);
            }
        }
    }
    blockers
}

fn backend_contract_blocker_for_function(
    module: &str,
    function: &IrFunction,
    backend: &str,
) -> Option<String> {
    let contract = function
        .backend_contracts
        .iter()
        .find(|contract| contract.backend == backend)?;
    if contract.status == "ready" {
        return None;
    }
    let reasons = if contract.blockers.is_empty() {
        "unknown".to_string()
    } else {
        contract.blockers.join(",")
    };
    Some(format!(
        "{}::{} blockers=[{}]",
        module, function.name, reasons
    ))
}

pub fn backend_catalog_text() -> String {
    let mut out = String::new();
    out.push_str("Forgentex backend catalog\n");
    out.push_str("Backend info schema: forgentex.backend_info.v1\n");
    out.push_str("Backend IR schema: forgentex.backend_ir.v1\n");
    out.push_str("Default cargo features: [] (zero Rust dependencies)\n");
    for backend in BACKENDS {
        let feature = backend.feature.unwrap_or("-");
        out.push_str(&format!(
            "  - {} status={} interface={} feature={} default={} deps={} role={} targets=[{}] notes={}\n",
            backend.name,
            backend.status,
            backend.interface,
            feature,
            bool_text(backend.default_enabled),
            backend.dependency_status,
            backend.role,
            backend.target_families.join(","),
            backend.notes
        ));
    }
    out
}

pub fn backend_catalog_json() -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.backend_info.v1\",\n");
    out.push_str("  \"default_features\": [],\n");
    out.push_str("  \"default_zero_dependency\": true,\n");
    out.push_str("  \"policy\": \"optional backend Rust dependencies must be behind off-by-default cargo features\",\n");
    out.push_str("  \"backends\": [");
    for (idx, backend) in BACKENDS.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"name\": {},\n      \"status\": {},\n      \"interface\": {},\n      \"role\": {},\n      \"feature\": {},\n      \"default_enabled\": {},\n      \"dependency_status\": {},",
            quoted(backend.name),
            quoted(backend.status),
            quoted(backend.interface),
            quoted(backend.role),
            optional_quoted(backend.feature),
            bool_json(backend.default_enabled),
            quoted(backend.dependency_status)
        ));
        push_static_array(
            &mut out,
            6,
            "target_families",
            backend.target_families,
            true,
        );
        out.push_str(&format!(
            "\n      \"notes\": {}\n    }}",
            quoted(backend.notes)
        ));
    }
    if !BACKENDS.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("]\n}");
    out
}

pub fn generate_backend_ir(input: &Path) -> Result<BackendIr, String> {
    if is_single_source_file(input) {
        return generate_file_backend_ir(input);
    }
    let diagnostics = check_project(input)?;
    if !diagnostics.is_empty() {
        return Err(format!(
            "project has diagnostics and IR was not generated:\n{}",
            diagnostics_to_json(&diagnostics)
        ));
    }
    let project = load_project(input)?;
    Ok(lower_project_to_ir(&project))
}

fn generate_file_backend_ir(input: &Path) -> Result<BackendIr, String> {
    let source = fs::read_to_string(input)
        .map_err(|err| format!("failed to read {}: {err}", input.display()))?;
    let program = parse_file(input, &source)
        .map_err(|err| format!("failed to parse {}: {err}", input.display()))?;
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "file has diagnostics and IR was not generated:\n{}",
            diagnostics_to_json(&diagnostics)
        ));
    }
    let public = single_file_public_symbols(&source);
    let module_name = single_file_module_name(input, &source);
    let module_path = input.display().to_string().replace('\\', "/");
    let native_vec_element_names = program
        .enums()
        .map(|decl| decl.name.clone())
        .chain(program.structs().map(|decl| decl.name.clone()))
        .collect::<BTreeSet<_>>();
    Ok(BackendIr {
        package: "single-file".to_string(),
        version: "0.0.0".to_string(),
        modules: vec![lower_program_module_to_ir(
            module_name,
            module_path,
            single_file_imports(&source),
            stable_hash(&source),
            &program,
            &public,
            &native_vec_element_names,
        )],
    })
}

fn is_single_source_file(input: &Path) -> bool {
    input.is_file() && input.file_name().and_then(|name| name.to_str()) != Some("Forgentex.toml")
}

fn single_file_module_name(path: &Path, source: &str) -> String {
    for line in source_semantic_lines(source) {
        if let Some(name) = line.strip_prefix("module ") {
            return name.trim().to_string();
        }
        if let Some(name) = line.strip_prefix("M ") {
            return name.trim().to_string();
        }
    }
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("main")
        .to_string()
}

fn single_file_imports(source: &str) -> Vec<String> {
    let mut imports = BTreeSet::new();
    for line in source_semantic_lines(source) {
        let import = line
            .strip_prefix("import ")
            .or_else(|| line.strip_prefix("I "))
            .or_else(|| line.strip_prefix("IMPORT "));
        if let Some(import) = import {
            if let Some(name) = import.split_whitespace().next() {
                imports.insert(name.to_string());
            }
        }
    }
    imports.into_iter().collect()
}

fn single_file_public_symbols(source: &str) -> BTreeSet<String> {
    let mut public = BTreeSet::new();
    for line in source_semantic_lines(source) {
        let exported = line
            .strip_prefix("export ")
            .or_else(|| line.strip_prefix("pub "))
            .or_else(|| line.strip_prefix("EXPORT "))
            .or_else(|| line.strip_prefix("PUB "));
        let Some(exported) = exported else {
            continue;
        };
        if let Some(rest) = exported.strip_prefix("enum ") {
            if let Some(name) = rest.split([':', ' ']).find(|part| !part.is_empty()) {
                public.insert(format!("enum:{name}"));
            }
        } else if let Some(rest) = exported.strip_prefix("struct ") {
            if let Some(name) = rest.split([':', ' ']).find(|part| !part.is_empty()) {
                public.insert(format!("struct:{name}"));
            }
        } else if let Some(rest) = exported.strip_prefix("fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                public.insert(format!("fn:{}", name.trim()));
            }
        } else if let Some(rest) = exported.strip_prefix("unsafe fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                public.insert(format!("fn:{}", name.trim()));
            }
        } else if let Some(rest) = exported.strip_prefix("EN ") {
            if let Some((name, _)) = rest.split_once('{') {
                public.insert(format!("enum:{}", name.trim()));
            }
        } else if let Some(rest) = exported.strip_prefix("S ") {
            if let Some((name, _)) = rest.split_once('{') {
                public.insert(format!("struct:{}", name.trim()));
            }
        } else if let Some(rest) = exported
            .strip_prefix("F ")
            .or_else(|| exported.strip_prefix("UF "))
            .or_else(|| exported.strip_prefix("unsafe F "))
        {
            if let Some((name, _)) = rest.split_once('(') {
                public.insert(format!("fn:{}", name.trim()));
            }
        }
    }
    public
}

fn source_semantic_lines(source: &str) -> impl Iterator<Item = &str> {
    source.lines().filter_map(|raw_line| {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    })
}

pub fn backend_ir_text(ir: &BackendIr) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "FGIR schema=forgentex.backend_ir.v1 package={} version={}\n",
        ir.package, ir.version
    ));
    out.push_str("BACKENDS available=c,zig planned=cranelift,llvm,mlir\n");
    for module in &ir.modules {
        let imports = if module.imports.is_empty() {
            "-".to_string()
        } else {
            module.imports.join(",")
        };
        out.push_str(&format!(
            "M {} path={} imports={} hash={}\n",
            module.name, module.path, imports, module.source_hash
        ));
        for decl in &module.enums {
            out.push_str(&format!(
                "  E {} {} visibility={} variants={}\n",
                decl.name,
                decl.id,
                visibility(decl.public),
                decl.variants.len()
            ));
            for variant in &decl.variants {
                let payload = variant.payload.as_deref().unwrap_or("-");
                out.push_str(&format!(
                    "    VARIANT {} payload={}\n",
                    variant.name, payload
                ));
            }
        }
        for decl in &module.structs {
            out.push_str(&format!(
                "  S {} {} visibility={} fields={}\n",
                decl.name,
                decl.id,
                visibility(decl.public),
                decl.fields.len()
            ));
            for field in &decl.fields {
                out.push_str(&format!("    FIELD {} {}\n", field.name, field.ty));
            }
        }
        for function in &module.functions {
            out.push_str(&format!(
                "  F {} {} visibility={} safety={} ret={} effects=[{}] targets=[{}]\n",
                function.name,
                function.id,
                visibility(function.public),
                if function.unsafe_fn { "unsafe" } else { "safe" },
                function.ret,
                function.effects.join(","),
                function.target_labels.join(",")
            ));
            let params = function
                .params
                .iter()
                .map(|param| format!("{}:{}", param.name, param.ty))
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&format!("    PARAMS [{}]\n", params));
            for block in &function.blocks {
                out.push_str(&format!("    BLOCK {}\n", block.label));
                for instruction in &block.instructions {
                    let ty = instruction
                        .ty
                        .as_ref()
                        .map(|ty| format!(" : {ty}"))
                        .unwrap_or_default();
                    let args = if instruction.args.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", instruction.args.join(" "))
                    };
                    out.push_str(&format!("      {}{}{}\n", instruction.op, args, ty));
                }
            }
            for contract in &function.backend_contracts {
                let blockers = if contract.blockers.is_empty() {
                    "-".to_string()
                } else {
                    contract.blockers.join(",")
                };
                out.push_str(&format!(
                    "    BACKEND {} status={} blockers=[{}]\n",
                    contract.backend, contract.status, blockers
                ));
            }
        }
    }
    out
}

pub fn backend_ir_json(ir: &BackendIr) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.backend_ir.v1\",\n");
    out.push_str(&format!("  \"package\": {},\n", quoted(&ir.package)));
    out.push_str(&format!("  \"version\": {},\n", quoted(&ir.version)));
    out.push_str("  \"modules\": [");
    for (module_idx, module) in ir.modules.iter().enumerate() {
        if module_idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"name\": {},\n      \"path\": {},\n      \"source_hash\": {},",
            quoted(&module.name),
            quoted(&module.path),
            quoted(&module.source_hash)
        ));
        push_string_array(&mut out, 6, "imports", &module.imports, true);
        out.push_str("\n      \"enums\": [");
        for (enum_idx, decl) in module.enums.iter().enumerate() {
            if enum_idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n        {{\"id\": {}, \"name\": {}, \"public\": {}, \"variants\": [",
                quoted(&decl.id),
                quoted(&decl.name),
                bool_json(decl.public)
            ));
            for (variant_idx, variant) in decl.variants.iter().enumerate() {
                if variant_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"name\": {}, \"payload\": {}}}",
                    quoted(&variant.name),
                    optional_quoted(variant.payload.as_deref())
                ));
            }
            out.push_str("]}");
        }
        if !module.enums.is_empty() {
            out.push('\n');
            out.push_str("      ");
        }
        out.push_str("],\n      \"structs\": [");
        for (struct_idx, decl) in module.structs.iter().enumerate() {
            if struct_idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n        {{\"id\": {}, \"name\": {}, \"public\": {}, \"fields\": [",
                quoted(&decl.id),
                quoted(&decl.name),
                bool_json(decl.public)
            ));
            for (field_idx, field) in decl.fields.iter().enumerate() {
                if field_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"name\": {}, \"type\": {}}}",
                    quoted(&field.name),
                    quoted(&field.ty)
                ));
            }
            out.push_str("]}");
        }
        if !module.structs.is_empty() {
            out.push('\n');
            out.push_str("      ");
        }
        out.push_str("],\n      \"functions\": [");
        for (function_idx, function) in module.functions.iter().enumerate() {
            if function_idx > 0 {
                out.push(',');
            }
            function_to_json(&mut out, function);
        }
        if !module.functions.is_empty() {
            out.push('\n');
            out.push_str("      ");
        }
        out.push_str("]\n    }");
    }
    if !ir.modules.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("]\n}");
    out
}

pub fn frontend_contract_json(input: &Path) -> Result<String, String> {
    if is_single_source_file(input) {
        let source = fs::read_to_string(input)
            .map_err(|err| format!("failed to read {}: {err}", input.display()))?;
        let program = parse_file(input, &source)
            .map_err(|err| format!("failed to parse {}: {err}", input.display()))?;
        return Ok(frontend_contract_json_for_program(
            "single-file",
            "0.0.0",
            &program,
        ));
    }
    let project = load_project(input)?;
    let program = combined_program(&project);
    Ok(frontend_contract_json_for_program(
        &project.manifest.name,
        &project.manifest.version,
        &program,
    ))
}

fn frontend_contract_json_for_program(package: &str, version: &str, program: &Program) -> String {
    let type_diagnostics = check(program);
    let effects = analyze_effects(program);
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.frontend_contract.v1\",\n");
    out.push_str(&format!(
        "  \"package\": {},\n  \"version\": {},\n",
        quoted(package),
        quoted(version)
    ));
    out.push_str(&format!(
        "  \"canonical_ast_hash\": {},\n",
        quoted(&stable_hash(&program.to_canonical_json()))
    ));
    let diagnostics = diagnostics_array_to_json(&type_diagnostics);
    out.push_str(&format!("  \"type_diagnostics\": {diagnostics},\n"));
    let effects_json = effects_to_json(&effects).replace('\n', "\n  ");
    out.push_str(&format!("  \"effects\": {effects_json}\n"));
    out.push('}');
    out
}

fn lower_project_to_ir(project: &Project) -> BackendIr {
    let mut modules = project.modules.iter().collect::<Vec<_>>();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    let native_vec_element_names = modules
        .iter()
        .flat_map(|module| {
            module
                .program
                .enums()
                .map(|decl| decl.name.clone())
                .chain(module.program.structs().map(|decl| decl.name.clone()))
        })
        .collect::<BTreeSet<_>>();
    BackendIr {
        package: project.manifest.name.clone(),
        version: project.manifest.version.clone(),
        modules: modules
            .into_iter()
            .map(|module| {
                let public = module
                    .public_symbols
                    .iter()
                    .filter(|_| !module.standard && !module.dependency)
                    .map(public_symbol_key)
                    .collect::<BTreeSet<_>>();
                let path = module
                    .path
                    .strip_prefix(&project.package_root)
                    .unwrap_or(&module.path)
                    .display()
                    .to_string()
                    .replace('\\', "/");
                lower_program_module_to_ir(
                    module.name.clone(),
                    path,
                    module.imports.clone(),
                    module.source_hash.clone(),
                    &module.program,
                    &public,
                    &native_vec_element_names,
                )
            })
            .collect(),
    }
}

pub fn lower_program_to_run_ir(program: &Program) -> BackendIr {
    let public = BTreeSet::new();
    let canonical = program.to_canonical_json();
    let native_vec_element_names = program
        .enums()
        .map(|decl| decl.name.clone())
        .chain(program.structs().map(|decl| decl.name.clone()))
        .collect::<BTreeSet<_>>();
    BackendIr {
        package: "runtime".to_string(),
        version: "0.0.0".to_string(),
        modules: vec![lower_program_module_to_ir(
            "runtime".to_string(),
            "<memory>".to_string(),
            Vec::new(),
            stable_hash(&canonical),
            program,
            &public,
            &native_vec_element_names,
        )],
    }
}

fn lower_program_module_to_ir(
    name: String,
    path: String,
    imports: Vec<String>,
    source_hash: String,
    program: &Program,
    public: &BTreeSet<String>,
    native_vec_element_names: &BTreeSet<String>,
) -> IrModule {
    let capsule_names = program
        .capsules()
        .map(|capsule| capsule.name.clone())
        .collect::<BTreeSet<_>>();
    let trait_names = program
        .traits()
        .map(|decl| decl.name.clone())
        .collect::<BTreeSet<_>>();
    let mut enums = Vec::new();
    let mut structs = Vec::new();
    let mut functions = Vec::new();
    for item in &program.items {
        match item {
            // Data, blob, and note sections carry content, not behaviour.
            Item::Data(_) | Item::Blob(_) | Item::Note(_) => {}
            Item::Enum(decl) => enums.push(IrEnum {
                id: decl.id.clone(),
                name: decl.name.clone(),
                public: public.contains(&format!("enum:{}", decl.name)),
                variants: decl
                    .variants
                    .iter()
                    .map(|variant| IrVariant {
                        name: variant.name.clone(),
                        payload: variant.payload.as_ref().map(Type::to_agent),
                    })
                    .collect(),
            }),
            Item::Struct(decl) => structs.push(IrStruct {
                id: decl.id.clone(),
                name: decl.name.clone(),
                public: public.contains(&format!("struct:{}", decl.name)),
                fields: decl
                    .fields
                    .iter()
                    .map(|field| IrField {
                        name: field.name.clone(),
                        ty: field.ty.to_agent(),
                    })
                    .collect(),
            }),
            Item::Function(function) => functions.push(lower_function(
                function,
                public,
                &capsule_names,
                &trait_names,
                native_vec_element_names,
            )),
            // Capsules are review-only foreign code; tests and shares are
            // metadata. None are lowered to backend IR.
            Item::Trait(_)
            | Item::TraitImpl(_)
            | Item::Test(_)
            | Item::Capsule(_)
            | Item::Share(_)
            | Item::Pipeline(_) => {}
        }
    }
    IrModule {
        name,
        path,
        imports,
        source_hash,
        enums,
        structs,
        functions,
    }
}

fn lower_function(
    function: &FunctionDecl,
    public: &BTreeSet<String>,
    capsule_names: &BTreeSet<String>,
    trait_names: &BTreeSet<String>,
    native_vec_element_names: &BTreeSet<String>,
) -> IrFunction {
    let metadata = target_metadata_for_function(function);
    let blocks = vec![IrBlock {
        label: "entry".to_string(),
        instructions: lower_stmts(&function.body),
    }];
    let backend_contracts = backend_contracts_for_function(
        function,
        &blocks,
        capsule_names,
        trait_names,
        native_vec_element_names,
    );
    IrFunction {
        id: function.id.clone(),
        name: function.name.clone(),
        public: public.contains(&format!("fn:{}", function.name)),
        unsafe_fn: function.unsafe_fn,
        ret: function.ret.to_agent(),
        params: function
            .params
            .iter()
            .map(|param| IrParam {
                name: param.name.clone(),
                ty: param.ty.to_agent(),
            })
            .collect(),
        effects: function.effects.clone(),
        target_labels: metadata.labels,
        blocks,
        backend_contracts,
    }
}

fn lower_stmts(body: &[Stmt]) -> Vec<IrInstruction> {
    let mut instructions = Vec::new();
    for stmt in body {
        match stmt {
            Stmt::Let { name, ty, expr } => instructions.push(IrInstruction {
                op: "let".to_string(),
                args: vec![name.clone(), format_expr(expr)],
                ty: ty.as_ref().map(Type::to_agent),
            }),
            Stmt::Return(Some(expr)) => instructions.push(IrInstruction {
                op: "return".to_string(),
                args: vec![format_expr(expr)],
                ty: None,
            }),
            Stmt::Return(None) => instructions.push(IrInstruction {
                op: "return".to_string(),
                args: Vec::new(),
                ty: None,
            }),
            Stmt::Break => instructions.push(IrInstruction {
                op: "break".to_string(),
                args: Vec::new(),
                ty: None,
            }),
            Stmt::Continue => instructions.push(IrInstruction {
                op: "continue".to_string(),
                args: Vec::new(),
                ty: None,
            }),
            Stmt::Require(expr) => instructions.push(IrInstruction {
                op: "require".to_string(),
                args: vec![format_expr(expr)],
                ty: Some("bool".to_string()),
            }),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                instructions.push(IrInstruction {
                    op: "if".to_string(),
                    args: vec![format_expr(cond), format!("then_len={}", then_body.len())],
                    ty: Some("bool".to_string()),
                });
                instructions.extend(lower_stmts(then_body));
                if !else_body.is_empty() {
                    instructions.push(IrInstruction {
                        op: "else".to_string(),
                        args: vec![format!("else_len={}", else_body.len())],
                        ty: None,
                    });
                    instructions.extend(lower_stmts(else_body));
                }
                instructions.push(IrInstruction {
                    op: "endif".to_string(),
                    args: Vec::new(),
                    ty: None,
                });
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                instructions.push(IrInstruction {
                    op: "for".to_string(),
                    args: vec![
                        var.clone(),
                        format_expr(start),
                        format_expr(end),
                        format!("body_len={}", body.len()),
                    ],
                    ty: Some("usize".to_string()),
                });
                instructions.extend(lower_stmts(body));
                instructions.push(IrInstruction {
                    op: "endfor".to_string(),
                    args: Vec::new(),
                    ty: None,
                });
            }
            Stmt::While { cond, body } => {
                instructions.push(IrInstruction {
                    op: "while".to_string(),
                    args: vec![format_expr(cond), format!("body_len={}", body.len())],
                    ty: Some("bool".to_string()),
                });
                instructions.extend(lower_stmts(body));
                instructions.push(IrInstruction {
                    op: "endwhile".to_string(),
                    args: Vec::new(),
                    ty: None,
                });
            }
            Stmt::Match { scrutinee, arms } => {
                instructions.push(IrInstruction {
                    op: "match".to_string(),
                    args: vec![format_expr(scrutinee), format!("arms={}", arms.len())],
                    ty: None,
                });
                for arm in arms {
                    instructions.push(IrInstruction {
                        op: "case".to_string(),
                        args: vec![crate::ast::pattern_to_string(&arm.pattern)],
                        ty: None,
                    });
                    instructions.extend(lower_stmts(&arm.body));
                }
                instructions.push(IrInstruction {
                    op: "endmatch".to_string(),
                    args: Vec::new(),
                    ty: None,
                });
            }
            Stmt::Assign { target, expr } => instructions.push(IrInstruction {
                op: "assign".to_string(),
                args: vec![format_expr(target), format_expr(expr)],
                ty: None,
            }),
            Stmt::Expr(expr) => instructions.push(IrInstruction {
                op: "expr".to_string(),
                args: vec![format_expr(expr)],
                ty: None,
            }),
            Stmt::Asm(asm) => instructions.push(IrInstruction {
                op: "asm.review_only".to_string(),
                args: vec![asm.target.clone(), format!("ops={}", asm.ops.len())],
                ty: None,
            }),
        }
    }
    instructions
}

fn backend_contracts_for_function(
    function: &FunctionDecl,
    blocks: &[IrBlock],
    capsule_names: &BTreeSet<String>,
    trait_names: &BTreeSet<String>,
    native_vec_element_names: &BTreeSet<String>,
) -> Vec<IrBackendContract> {
    ["interp", "c", "zig"]
        .into_iter()
        .map(|backend| {
            let blockers = backend_blockers(
                function,
                blocks,
                backend,
                capsule_names,
                trait_names,
                native_vec_element_names,
            );
            IrBackendContract {
                backend: backend.to_string(),
                status: if blockers.is_empty() {
                    "ready".to_string()
                } else {
                    "blocked".to_string()
                },
                blockers,
            }
        })
        .collect()
}

fn backend_blockers(
    function: &FunctionDecl,
    blocks: &[IrBlock],
    backend: &str,
    capsule_names: &BTreeSet<String>,
    trait_names: &BTreeSet<String>,
    native_vec_element_names: &BTreeSet<String>,
) -> Vec<String> {
    let mut blockers = BTreeSet::new();
    if function.unsafe_fn {
        blockers.insert("unsafe_function".to_string());
    }
    if has_ir_op(blocks, "asm.review_only") {
        blockers.insert("op:asm.review_only".to_string());
    }

    match backend {
        "interp" => {}
        "c" | "zig" => {
            // Generic (type-parameterized) functions need monomorphization,
            // which the source backends do not do yet (lang.4): block honestly.
            if !function.type_params.is_empty() {
                blockers.insert(format!(
                    "generic_function:[{}]",
                    function.type_params.join(",")
                ));
            }
            let has_native_vec = function_has_native_vec(function, native_vec_element_names);
            let returns_native_option =
                function_returns_native_option(function, native_vec_element_names);
            for effect in &function.effects {
                if is_runtime_only_effect(effect) && !(effect == "alloc" && has_native_vec) {
                    blockers.insert(format!("effect:{effect}"));
                }
            }
            if !returns_native_option {
                collect_generic_type_blockers(&function.ret, &mut blockers);
            }
            for param in &function.params {
                if !is_native_value_option(&param.ty, native_vec_element_names) {
                    collect_generic_type_blockers(&param.ty, &mut blockers);
                }
            }
            collect_stmt_type_blockers(
                &function.body,
                true,
                native_vec_element_names,
                &mut blockers,
            );
            collect_source_expr_blockers(
                &function.body,
                capsule_names,
                trait_names,
                native_vec_element_names,
                returns_native_option,
                &mut blockers,
            );
            collect_native_vec_blockers(
                &function.body,
                &BTreeSet::new(),
                native_vec_element_names,
                returns_native_option,
                &mut blockers,
            );
        }
        _ => {}
    }

    blockers.into_iter().collect()
}

fn collect_stmt_type_blockers(
    body: &[Stmt],
    allow_native_vec_local: bool,
    native_vec_element_names: &BTreeSet<String>,
    blockers: &mut BTreeSet<String>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let {
                ty: Some(ty), expr, ..
            } if allow_native_vec_local
                && is_native_vec(ty, native_vec_element_names)
                && is_vec_new(expr) => {}
            Stmt::Let {
                ty: Some(ty), expr, ..
            } if is_native_value_option(ty, native_vec_element_names)
                && (is_pop_call(expr) || is_native_option_constructor(expr)) => {}
            Stmt::Let { ty: Some(ty), .. } => collect_generic_type_blockers(ty, blockers),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_stmt_type_blockers(
                    then_body,
                    allow_native_vec_local,
                    native_vec_element_names,
                    blockers,
                );
                collect_stmt_type_blockers(
                    else_body,
                    allow_native_vec_local,
                    native_vec_element_names,
                    blockers,
                );
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                collect_stmt_type_blockers(
                    body,
                    allow_native_vec_local,
                    native_vec_element_names,
                    blockers,
                );
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    collect_stmt_type_blockers(
                        &arm.body,
                        allow_native_vec_local,
                        native_vec_element_names,
                        blockers,
                    );
                }
            }
            Stmt::Let { ty: None, .. }
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Asm(_) => {}
        }
    }
}

fn is_native_vec(ty: &Type, native_vec_element_names: &BTreeSet<String>) -> bool {
    let Type::Generic { name, args } = ty else {
        return false;
    };
    if name != "Vec" {
        return false;
    }
    match args.as_slice() {
        [Type::Bool] => true,
        [Type::Primitive(element)] => matches!(
            element.as_str(),
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
                | "str"
        ),
        [Type::Slice {
            mutable: false,
            elem,
        }] => is_native_vec_slice_element(elem),
        [element @ Type::Array { .. }] => is_native_vec_array_element(element),
        [Type::Named(element)] => native_vec_element_names.contains(element),
        _ => false,
    }
}

fn is_native_value_option(ty: &Type, native_vec_element_names: &BTreeSet<String>) -> bool {
    let Type::Generic { name, args } = ty else {
        return false;
    };
    if name != "Option" || args.len() != 1 {
        return false;
    }
    is_native_vec(
        &Type::Generic {
            name: "Vec".to_string(),
            args: args.clone(),
        },
        native_vec_element_names,
    )
}

fn is_pop_call(expr: &Expr) -> bool {
    matches!(expr, Expr::Call { name, args } if name == "pop" && args.len() == 1)
}

fn is_native_option_constructor(expr: &Expr) -> bool {
    matches!(expr, Expr::Call { name, args } if (name == "Some" && args.len() == 1) || (name == "None" && args.is_empty()))
}

fn function_returns_native_option(
    function: &FunctionDecl,
    native_vec_element_names: &BTreeSet<String>,
) -> bool {
    if !is_native_value_option(&function.ret, native_vec_element_names) {
        return false;
    }
    let mut saw_return = false;
    let option_params = function
        .params
        .iter()
        .filter(|param| param.ty == function.ret)
        .map(|param| param.name.clone())
        .collect::<BTreeSet<_>>();
    body_returns_only_native_options(
        &function.body,
        &option_params,
        &function.ret,
        &mut saw_return,
    ) && saw_return
}

fn body_returns_only_native_options(
    body: &[Stmt],
    inherited: &BTreeSet<String>,
    expected: &Type,
    saw_return: &mut bool,
) -> bool {
    let mut options = inherited.clone();
    for stmt in body {
        match stmt {
            Stmt::Let {
                name,
                ty: Some(ty),
                expr,
            } => {
                options.remove(name);
                if ty == expected && (is_pop_call(expr) || is_native_option_constructor(expr)) {
                    options.insert(name.clone());
                }
            }
            Stmt::Return(Some(expr)) => {
                *saw_return = true;
                if !is_pop_call(expr)
                    && !is_native_option_constructor(expr)
                    && !matches!(expr, Expr::Var(name) if options.contains(name))
                {
                    return false;
                }
            }
            Stmt::Return(None) => return false,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                if !body_returns_only_native_options(then_body, &options, expected, saw_return)
                    || !body_returns_only_native_options(else_body, &options, expected, saw_return)
                {
                    return false;
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                if !body_returns_only_native_options(body, &options, expected, saw_return) {
                    return false;
                }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    if !body_returns_only_native_options(&arm.body, &options, expected, saw_return)
                    {
                        return false;
                    }
                }
            }
            Stmt::Assign {
                target: Expr::Var(name),
                expr,
            } if options.contains(name) => {
                if !is_native_option_constructor(expr) {
                    return false;
                }
            }
            Stmt::Let { name, .. } => {
                options.remove(name);
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Asm(_) => {}
        }
    }
    true
}

fn is_native_vec_slice_element(ty: &Type) -> bool {
    match ty {
        Type::Bool => true,
        Type::Primitive(element) => matches!(
            element.as_str(),
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
        ),
        _ => false,
    }
}

pub(crate) fn is_native_vec_array_element(ty: &Type) -> bool {
    fn scalar_leaves(ty: &Type, depth: usize, leaves: usize) -> Option<usize> {
        match ty {
            Type::Array { elem, len } if depth < 16 && *len != 0 => {
                let leaves = leaves.checked_mul(*len)?;
                if leaves > 1024 {
                    return None;
                }
                scalar_leaves(elem, depth + 1, leaves)
            }
            ty if depth != 0 && is_native_vec_array_leaf(ty) => Some(leaves),
            _ => None,
        }
    }

    scalar_leaves(ty, 0, 1).is_some()
}

fn is_native_vec_array_leaf(ty: &Type) -> bool {
    is_native_vec_slice_element(ty)
        || matches!(ty, Type::Named(_))
        || matches!(ty, Type::Primitive(name) if name == "str")
        || matches!(
            ty,
            Type::Slice {
                mutable: false,
                elem,
            } if is_native_vec_slice_element(elem)
        )
}

fn function_has_native_vec(
    function: &FunctionDecl,
    native_vec_element_names: &BTreeSet<String>,
) -> bool {
    stmts_have_native_vec(&function.body, native_vec_element_names)
}

fn stmts_have_native_vec(body: &[Stmt], native_vec_element_names: &BTreeSet<String>) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Let {
            ty: Some(ty), expr, ..
        } => is_native_vec(ty, native_vec_element_names) && is_vec_new(expr),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            stmts_have_native_vec(then_body, native_vec_element_names)
                || stmts_have_native_vec(else_body, native_vec_element_names)
        }
        Stmt::For { body, .. } | Stmt::While { body, .. } => {
            stmts_have_native_vec(body, native_vec_element_names)
        }
        Stmt::Match { arms, .. } => arms
            .iter()
            .any(|arm| stmts_have_native_vec(&arm.body, native_vec_element_names)),
        _ => false,
    })
}

fn is_vec_new(expr: &Expr) -> bool {
    matches!(expr, Expr::Call { name, args } if name == "vec_new" && args.is_empty())
}

/// Validate the deliberately small owned native-Vec subset. The source
/// backends can prove exactly-once cleanup only for fresh, annotated local
/// `Vec[T]` values, where `T` is a scalar primitive, borrowed string/scalar-slice
/// view, bounded recursively fixed-size scalar array, or already-lowered named enum/struct
/// value. Any alias, whole-vector assignment, return, or call argument remains
/// an explicit FIR blocker instead of becoming an unsafe shallow copy in
/// generated C/Zig.
fn collect_native_vec_blockers(
    body: &[Stmt],
    inherited: &BTreeSet<String>,
    native_vec_element_names: &BTreeSet<String>,
    allow_returned_pop: bool,
    blockers: &mut BTreeSet<String>,
) {
    let mut vecs = inherited.clone();
    for stmt in body {
        match stmt {
            Stmt::Let {
                name,
                ty: Some(ty),
                expr,
            } if is_native_vec(ty, native_vec_element_names) && is_vec_new(expr) => {
                vecs.insert(name.clone());
            }
            Stmt::Let {
                ty: Some(ty), expr, ..
            } if is_native_value_option(ty, native_vec_element_names) && is_pop_call(expr) => {
                collect_native_vec_expr_blockers(expr, &vecs, true, blockers);
            }
            Stmt::Let { expr, .. } | Stmt::Require(expr) => {
                collect_native_vec_expr_blockers(expr, &vecs, false, blockers);
            }
            Stmt::Expr(expr) => {
                collect_native_vec_expr_blockers(expr, &vecs, true, blockers);
            }
            Stmt::Return(Some(expr)) => {
                collect_native_vec_expr_blockers(
                    expr,
                    &vecs,
                    allow_returned_pop && is_pop_call(expr),
                    blockers,
                );
            }
            Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                collect_native_vec_expr_blockers(cond, &vecs, false, blockers);
                collect_native_vec_blockers(
                    then_body,
                    &vecs,
                    native_vec_element_names,
                    allow_returned_pop,
                    blockers,
                );
                collect_native_vec_blockers(
                    else_body,
                    &vecs,
                    native_vec_element_names,
                    allow_returned_pop,
                    blockers,
                );
            }
            Stmt::For {
                start, end, body, ..
            } => {
                collect_native_vec_expr_blockers(start, &vecs, false, blockers);
                collect_native_vec_expr_blockers(end, &vecs, false, blockers);
                collect_native_vec_blockers(
                    body,
                    &vecs,
                    native_vec_element_names,
                    allow_returned_pop,
                    blockers,
                );
            }
            Stmt::While { cond, body } => {
                collect_native_vec_expr_blockers(cond, &vecs, false, blockers);
                collect_native_vec_blockers(
                    body,
                    &vecs,
                    native_vec_element_names,
                    allow_returned_pop,
                    blockers,
                );
            }
            Stmt::Match { scrutinee, arms } => {
                collect_native_vec_expr_blockers(scrutinee, &vecs, false, blockers);
                for arm in arms {
                    collect_native_vec_blockers(
                        &arm.body,
                        &vecs,
                        native_vec_element_names,
                        allow_returned_pop,
                        blockers,
                    );
                }
            }
            Stmt::Assign { target, expr } => {
                collect_native_vec_assignment_target_blockers(target, &vecs, blockers);
                collect_native_vec_expr_blockers(expr, &vecs, false, blockers);
            }
        }
    }
}

fn collect_native_vec_assignment_target_blockers(
    target: &Expr,
    vecs: &BTreeSet<String>,
    blockers: &mut BTreeSet<String>,
) {
    match target {
        Expr::Index { base, index } if is_local_vec_var(base, vecs) => {
            collect_native_vec_expr_blockers(index, vecs, false, blockers);
        }
        _ => collect_native_vec_expr_blockers(target, vecs, false, blockers),
    }
}

fn collect_native_vec_expr_blockers(
    expr: &Expr,
    vecs: &BTreeSet<String>,
    discarded: bool,
    blockers: &mut BTreeSet<String>,
) {
    match expr {
        Expr::Var(name) if vecs.contains(name) => {
            blockers.insert("native_vec:ownership_escape".to_string());
        }
        Expr::Call { name, args } if name == "vec_new" => {
            blockers.insert("native_vec:vec_new_requires_annotated_local".to_string());
            for arg in args {
                collect_native_vec_expr_blockers(arg, vecs, false, blockers);
            }
        }
        Expr::Call { name, args } if name == "push" => {
            if args.len() != 2 || !is_local_vec_var(&args[0], vecs) {
                blockers.insert("native_vec:push_requires_local_native_vec".to_string());
            }
            for arg in args.iter().skip(1) {
                collect_native_vec_expr_blockers(arg, vecs, false, blockers);
            }
        }
        Expr::Call { name, args } if name == "pop" => {
            if !discarded || args.len() != 1 || !is_local_vec_var(&args[0], vecs) {
                blockers.insert("native_vec:pop_requires_discarded_local_native_vec".to_string());
            }
            for arg in args.iter().skip(1) {
                collect_native_vec_expr_blockers(arg, vecs, false, blockers);
            }
        }
        Expr::Call { name, args } if is_immediate_pop_consumer(name, args) => {
            collect_native_vec_expr_blockers(&args[0], vecs, true, blockers);
            for arg in args.iter().skip(1) {
                collect_native_vec_expr_blockers(arg, vecs, false, blockers);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_native_vec_expr_blockers(arg, vecs, false, blockers);
            }
        }
        Expr::Lambda { body, .. } => collect_native_vec_expr_blockers(body, vecs, false, blockers),
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
            collect_native_vec_expr_blockers(expr, vecs, false, blockers)
        }
        Expr::Binary { left, right, .. } => {
            collect_native_vec_expr_blockers(left, vecs, false, blockers);
            collect_native_vec_expr_blockers(right, vecs, false, blockers);
        }
        Expr::Field { base, field } if field == "len" && is_local_vec_var(base, vecs) => {}
        Expr::Field { base, .. } => collect_native_vec_expr_blockers(base, vecs, false, blockers),
        Expr::Index { base, index } if is_local_vec_var(base, vecs) => {
            collect_native_vec_expr_blockers(index, vecs, false, blockers);
        }
        Expr::Index { base, index } => {
            collect_native_vec_expr_blockers(base, vecs, false, blockers);
            collect_native_vec_expr_blockers(index, vecs, false, blockers);
        }
        Expr::SliceRange { base, start, end } => {
            collect_native_vec_expr_blockers(base, vecs, false, blockers);
            collect_native_vec_expr_blockers(start, vecs, false, blockers);
            collect_native_vec_expr_blockers(end, vecs, false, blockers);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
    }
}

fn is_local_vec_var(expr: &Expr, vecs: &BTreeSet<String>) -> bool {
    matches!(expr, Expr::Var(name) if vecs.contains(name))
}

fn collect_source_expr_blockers(
    body: &[Stmt],
    capsule_names: &BTreeSet<String>,
    trait_names: &BTreeSet<String>,
    native_vec_element_names: &BTreeSet<String>,
    allow_returned_native_option: bool,
    blockers: &mut BTreeSet<String>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let {
                ty: Some(ty), expr, ..
            } if is_native_value_option(ty, native_vec_element_names)
                && is_native_option_constructor(expr) =>
            {
                collect_native_option_constructor_args(expr, capsule_names, trait_names, blockers);
            }
            Stmt::Let { expr, .. } | Stmt::Require(expr) | Stmt::Expr(expr) => {
                collect_expr_source_blockers(expr, capsule_names, trait_names, blockers);
            }
            Stmt::Return(Some(expr))
                if allow_returned_native_option && is_native_option_constructor(expr) =>
            {
                collect_native_option_constructor_args(expr, capsule_names, trait_names, blockers);
            }
            Stmt::Return(Some(expr)) => {
                collect_expr_source_blockers(expr, capsule_names, trait_names, blockers)
            }
            Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                collect_expr_source_blockers(cond, capsule_names, trait_names, blockers);
                collect_source_expr_blockers(
                    then_body,
                    capsule_names,
                    trait_names,
                    native_vec_element_names,
                    allow_returned_native_option,
                    blockers,
                );
                collect_source_expr_blockers(
                    else_body,
                    capsule_names,
                    trait_names,
                    native_vec_element_names,
                    allow_returned_native_option,
                    blockers,
                );
            }
            Stmt::For {
                start, end, body, ..
            } => {
                collect_expr_source_blockers(start, capsule_names, trait_names, blockers);
                collect_expr_source_blockers(end, capsule_names, trait_names, blockers);
                collect_source_expr_blockers(
                    body,
                    capsule_names,
                    trait_names,
                    native_vec_element_names,
                    allow_returned_native_option,
                    blockers,
                );
            }
            Stmt::While { cond, body } => {
                collect_expr_source_blockers(cond, capsule_names, trait_names, blockers);
                collect_source_expr_blockers(
                    body,
                    capsule_names,
                    trait_names,
                    native_vec_element_names,
                    allow_returned_native_option,
                    blockers,
                );
            }
            Stmt::Match { scrutinee, arms } => {
                collect_expr_source_blockers(scrutinee, capsule_names, trait_names, blockers);
                for arm in arms {
                    collect_source_expr_blockers(
                        &arm.body,
                        capsule_names,
                        trait_names,
                        native_vec_element_names,
                        allow_returned_native_option,
                        blockers,
                    );
                }
            }
            Stmt::Assign { target, expr } if is_native_option_constructor(expr) => {
                collect_expr_source_blockers(target, capsule_names, trait_names, blockers);
                collect_native_option_constructor_args(expr, capsule_names, trait_names, blockers);
            }
            Stmt::Assign { target, expr } => {
                collect_expr_source_blockers(target, capsule_names, trait_names, blockers);
                collect_expr_source_blockers(expr, capsule_names, trait_names, blockers);
            }
        }
    }
}

fn collect_native_option_constructor_args(
    expr: &Expr,
    capsule_names: &BTreeSet<String>,
    trait_names: &BTreeSet<String>,
    blockers: &mut BTreeSet<String>,
) {
    if let Expr::Call { args, .. } = expr {
        for arg in args {
            collect_expr_source_blockers(arg, capsule_names, trait_names, blockers);
        }
    }
}

fn collect_expr_source_blockers(
    expr: &Expr,
    capsule_names: &BTreeSet<String>,
    trait_names: &BTreeSet<String>,
    blockers: &mut BTreeSet<String>,
) {
    match expr {
        Expr::Lambda { body, .. } => {
            // Closures are interpreter-only until lowered (lang.4).
            blockers.insert("op:lambda".to_string());
            collect_expr_source_blockers(body, capsule_names, trait_names, blockers);
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => {}
        Expr::TraitConst { .. } => {
            blockers.insert("op:trait_const".to_string());
        }
        Expr::Unary { expr, .. } => {
            collect_expr_source_blockers(expr, capsule_names, trait_names, blockers)
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_source_blockers(left, capsule_names, trait_names, blockers);
            collect_expr_source_blockers(right, capsule_names, trait_names, blockers);
        }
        Expr::Call { name, args } => {
            if is_interpreter_only_intrinsic(name)
                && !is_immediate_pop_consumer(name, args)
                && !is_stored_option_consumer(name, args)
            {
                blockers.insert(format!("intrinsic:{name}"));
            }
            if capsule_names.contains(name) {
                blockers.insert(format!("capsule:{name}"));
            }
            if name
                .split_once("::")
                .is_some_and(|(qualifier, _)| trait_names.contains(qualifier))
            {
                blockers.insert("op:trait_dispatch".to_string());
            }
            for arg in args {
                collect_expr_source_blockers(arg, capsule_names, trait_names, blockers);
            }
        }
        Expr::Cast { ty, expr } => {
            collect_generic_type_blockers(ty, blockers);
            collect_expr_source_blockers(expr, capsule_names, trait_names, blockers);
        }
        Expr::Field { base, .. } => {
            collect_expr_source_blockers(base, capsule_names, trait_names, blockers)
        }
        Expr::Index { base, index } => {
            collect_expr_source_blockers(base, capsule_names, trait_names, blockers);
            collect_expr_source_blockers(index, capsule_names, trait_names, blockers);
        }
        Expr::SliceRange { base, start, end } => {
            blockers.insert("op:slice_range".to_string());
            collect_expr_source_blockers(base, capsule_names, trait_names, blockers);
            collect_expr_source_blockers(start, capsule_names, trait_names, blockers);
            collect_expr_source_blockers(end, capsule_names, trait_names, blockers);
        }
    }
}

fn is_stored_option_consumer(name: &str, args: &[Expr]) -> bool {
    let expected = match name {
        "is_some" | "is_none" => 1,
        "unwrap_or" => 2,
        _ => return false,
    };
    args.len() == expected
        && matches!(args.first(), Some(Expr::Var(_)))
        && (name != "unwrap_or" || is_native_pop_fallback(&args[1]))
}

fn is_immediate_pop_consumer(name: &str, args: &[Expr]) -> bool {
    let expected = match name {
        "is_some" | "is_none" => 1,
        "unwrap_or" => 2,
        _ => return false,
    };
    args.len() == expected
        && matches!(
            args.first(),
            Some(Expr::Call { name, args }) if name == "pop" && args.len() == 1
        )
        && (name != "unwrap_or" || is_native_pop_fallback(&args[1]))
}

fn is_native_pop_fallback(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_)
    )
}

fn is_interpreter_only_intrinsic(name: &str) -> bool {
    matches!(
        name,
        "Some"
            | "None"
            | "Ok"
            | "Err"
            | "is_some"
            | "is_none"
            | "is_ok"
            | "is_err"
            | "unwrap_or"
            // These String operations allocate, return Option, or return Vec.
            // Source backends only lower the bounded borrowed subset for now.
            | "str_concat"
            | "int_to_str"
            | "str_slice"
            | "str_find"
            | "str_split"
            | "alloc"
            // A carried payload lives in the source file. Lowering it would
            // mean emitting it as a static constant, which is a sound idea and
            // not yet built; until then this is an explicit blocker rather
            // than a silent omission.
            | "section_text"
            | "section_bytes"
            | "alloc_buffer"
            | "realloc"
            | "free"
    )
}

fn collect_generic_type_blockers(ty: &Type, blockers: &mut BTreeSet<String>) {
    match ty {
        Type::Generic { name, .. } if name == "Fn" && is_native_function_type(ty) => {}
        Type::Generic { name, args } => {
            blockers.insert(format!("generic_type:{name}"));
            for arg in args {
                collect_generic_type_blockers(arg, blockers);
            }
        }
        Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
            collect_generic_type_blockers(elem, blockers);
        }
        Type::Bool | Type::Primitive(_) | Type::Named(_) | Type::Void | Type::Unknown => {}
    }
}

/// The source-backend function-value ABI is intentionally narrow: named,
/// monomorphic functions whose signature is made only from the scalar types
/// already shared by the C and Zig emitters. Effect annotations are static
/// capability metadata and do not alter that ABI; runtime-only effects remain
/// guarded by the function-level backend contract. Closures still have an
/// `op:lambda` blocker, while `forall` binders, nested function values,
/// aggregates, and borrowed/container signatures keep the established
/// `generic_type:Fn` blocker until their ABI is designed explicitly.
pub(crate) fn is_native_function_type(ty: &Type) -> bool {
    if !matches!(ty, Type::Generic { name, .. } if name == "Fn") {
        return false;
    }
    let Some((params, ret, _effects)) = ty.function_parts() else {
        return false;
    };
    ty.function_binders()
        .is_some_and(|binders| binders.is_empty())
        && params.iter().all(is_native_function_param_type)
        && (is_native_function_param_type(ret) || *ret == Type::Void)
}

fn is_native_function_param_type(ty: &Type) -> bool {
    matches!(ty, Type::Bool | Type::Primitive(_))
}

fn has_ir_op(blocks: &[IrBlock], op: &str) -> bool {
    blocks
        .iter()
        .any(|block| block.instructions.iter().any(|inst| inst.op == op))
}

fn is_runtime_only_effect(effect: &str) -> bool {
    effect == "alloc"
        || effect.starts_with("alloc.")
        || effect.starts_with("io.")
        || effect.starts_with("fs.")
        || effect.starts_with("network.")
        || effect.starts_with("crypto.")
        || effect.starts_with("ledger.")
        || effect.starts_with("agent.")
        || effect.starts_with("concurrency.")
        || effect == "serialize"
        || effect == "stream"
}

fn function_to_json(out: &mut String, function: &IrFunction) {
    out.push_str(&format!(
        "\n        {{\"id\": {}, \"name\": {}, \"public\": {}, \"unsafe\": {}, \"return\": {},",
        quoted(&function.id),
        quoted(&function.name),
        bool_json(function.public),
        bool_json(function.unsafe_fn),
        quoted(&function.ret)
    ));
    out.push_str(" \"params\": [");
    for (idx, param) in function.params.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"name\": {}, \"type\": {}}}",
            quoted(&param.name),
            quoted(&param.ty)
        ));
    }
    out.push_str("],");
    push_string_array_inline(out, "effects", &function.effects);
    out.push(',');
    push_string_array_inline(out, "target_labels", &function.target_labels);
    out.push_str(", \"blocks\": [");
    for (block_idx, block) in function.blocks.iter().enumerate() {
        if block_idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"label\": {}, \"instructions\": [",
            quoted(&block.label)
        ));
        for (inst_idx, inst) in block.instructions.iter().enumerate() {
            if inst_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{{\"op\": {}, \"args\": [", quoted(&inst.op)));
            for (arg_idx, arg) in inst.args.iter().enumerate() {
                if arg_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&quoted(arg));
            }
            out.push_str(&format!(
                "], \"type\": {}}}",
                optional_quoted(inst.ty.as_deref())
            ));
        }
        out.push_str("]}");
    }
    out.push_str("], \"backend_contracts\": [");
    for (contract_idx, contract) in function.backend_contracts.iter().enumerate() {
        if contract_idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"backend\": {}, \"status\": {}, \"blockers\": [",
            quoted(&contract.backend),
            quoted(&contract.status)
        ));
        for (blocker_idx, blocker) in contract.blockers.iter().enumerate() {
            if blocker_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&quoted(blocker));
        }
        out.push_str("]}");
    }
    out.push_str("]}");
}

fn combined_program(project: &Project) -> Program {
    let mut items = Vec::new();
    for module in &project.modules {
        items.extend(module.program.items.clone());
    }
    Program { items }
}

pub fn backend_by_name(name: &str) -> Option<BackendKind> {
    let canonical = if name == "interpreter" {
        "interp"
    } else {
        name
    };
    let info = backend_info(canonical)?;
    match info.name {
        "interp" => Some(BackendKind::Interpreter),
        "c" => Some(BackendKind::C),
        "zig" => Some(BackendKind::Zig),
        _ => Some(BackendKind::Planned(info)),
    }
}

fn backend_info(name: &str) -> Option<&'static BackendInfo> {
    BACKENDS.iter().find(|backend| backend.name == name)
}

fn planned_backend_error(info: &BackendInfo) -> String {
    let feature = info.feature.unwrap_or("-");
    format!(
        "backend '{}' is {} for v3.0 (feature={}, deps={}); use --backend c or --backend zig for available source backends",
        info.name, info.status, feature, info.dependency_status
    )
}

fn public_symbol_key(symbol: &PublicSymbol) -> String {
    format!("{}:{}", symbol.kind, symbol.name)
}

fn visibility(value: bool) -> &'static str {
    if value {
        "public"
    } else {
        "private"
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn stable_hash(source: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn push_static_array(out: &mut String, indent: usize, key: &str, values: &[&str], trailing: bool) {
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
}

fn push_string_array(
    out: &mut String,
    indent: usize,
    key: &str,
    values: &[String],
    trailing: bool,
) {
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
}

fn push_string_array_inline(out: &mut String, key: &str, values: &[String]) {
    out.push_str(&format!(" \"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
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

fn optional_quoted(value: Option<&str>) -> String {
    value.map(quoted).unwrap_or_else(|| "null".to_string())
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}
