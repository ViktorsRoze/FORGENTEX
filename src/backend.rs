use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::ast::{FunctionDecl, Item, Program, Stmt, Type};
use crate::c_backend::build_c_project;
use crate::diag::diagnostics_to_json;
use crate::effects::{analyze as analyze_effects, effects_to_json};
use crate::format::format_expr;
use crate::project::{check_project, load_project, Project, PublicSymbol};
use crate::targets::target_metadata_for_function;
use crate::typecheck::check;
use crate::zig_backend::build_zig_project;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendBuildArtifacts {
    pub backend: String,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendInfo {
    pub name: &'static str,
    pub status: &'static str,
    pub role: &'static str,
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
    pub structs: Vec<IrStruct>,
    pub functions: Vec<IrFunction>,
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

const BACKENDS: &[BackendInfo] = &[
    BackendInfo {
        name: "c",
        status: "available",
        role: "reference transpiler",
        target_families: &["x86_64", "arm", "aarch64", "riscv32", "riscv64"],
        notes: "C11 output is the reference backend for safe project code.",
    },
    BackendInfo {
        name: "zig",
        status: "available",
        role: "source transpiler and cross-target experiment",
        target_families: &["x86_64", "arm", "aarch64", "riscv32", "riscv64"],
        notes: "Zig source output for safe project code; compiler invocation is deferred.",
    },
    BackendInfo {
        name: "cranelift",
        status: "planned",
        role: "JIT/native experiment",
        target_families: &["x86_64", "aarch64"],
        notes: "Planned after the backend IR is exercised by more examples.",
    },
    BackendInfo {
        name: "llvm",
        status: "planned",
        role: "optimizing native backend",
        target_families: &["x86_64", "arm", "aarch64", "riscv32", "riscv64"],
        notes: "Reserved for broad native target support; not linked in v0.",
    },
    BackendInfo {
        name: "mlir",
        status: "planned",
        role: "dialect and media/GPU experiments",
        target_families: &["x86_64", "aarch64", "riscv64"],
        notes: "Longer-term path for domain-specific lowering.",
    },
];

pub fn build_project_with_backend(
    input: &Path,
    backend: &str,
    out_dir: &Path,
) -> Result<BackendBuildArtifacts, String> {
    match backend_info(backend) {
        Some(info) if info.status == "available" && info.name == "c" => {
            let artifacts = build_c_project(input, out_dir)?;
            Ok(BackendBuildArtifacts {
                backend: info.name.to_string(),
                files: vec![artifacts.header, artifacts.source],
            })
        }
        Some(info) if info.status == "available" && info.name == "zig" => {
            let artifacts = build_zig_project(input, out_dir)?;
            Ok(BackendBuildArtifacts {
                backend: info.name.to_string(),
                files: vec![artifacts.source],
            })
        }
        Some(info) => Err(format!(
            "backend '{}' is {} for Stage 15; use --backend c or --backend zig for available source backends",
            info.name, info.status
        )),
        None => Err(format!("unknown backend '{backend}'")),
    }
}

pub fn backend_catalog_text() -> String {
    let mut out = String::new();
    out.push_str("Forgentex backend catalog\n");
    out.push_str("Backend IR schema: forgentex.backend_ir.v1\n");
    for backend in BACKENDS {
        out.push_str(&format!(
            "  - {} status={} role={} targets=[{}] notes={}\n",
            backend.name,
            backend.status,
            backend.role,
            backend.target_families.join(","),
            backend.notes
        ));
    }
    out
}

pub fn backend_catalog_json() -> String {
    let mut out = String::new();
    out.push_str("{\n  \"schema\": \"forgentex.backends.v1\",\n  \"backends\": [");
    for (idx, backend) in BACKENDS.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"name\": {},\n      \"status\": {},\n      \"role\": {},",
            quoted(backend.name),
            quoted(backend.status),
            quoted(backend.role)
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
        out.push_str("\n      \"structs\": [");
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
    let project = load_project(input)?;
    let program = combined_program(&project);
    let type_diagnostics = check(&program);
    let effects = analyze_effects(&program);
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.frontend_contract.v1\",\n");
    out.push_str(&format!(
        "  \"package\": {},\n  \"version\": {},\n",
        quoted(&project.manifest.name),
        quoted(&project.manifest.version)
    ));
    out.push_str(&format!(
        "  \"canonical_ast_hash\": {},\n",
        quoted(&stable_hash(&program.to_canonical_json()))
    ));
    let diagnostics_json = diagnostics_to_json(&type_diagnostics);
    let diagnostics = diagnostics_json
        .trim()
        .strip_prefix("{\n  \"diagnostics\": ")
        .and_then(|value| value.strip_suffix("\n}"))
        .unwrap_or("[]");
    out.push_str(&format!("  \"type_diagnostics\": {diagnostics},\n"));
    let effects_json = effects_to_json(&effects).replace('\n', "\n  ");
    out.push_str(&format!("  \"effects\": {effects_json}\n"));
    out.push('}');
    Ok(out)
}

fn lower_project_to_ir(project: &Project) -> BackendIr {
    let mut modules = project.modules.iter().collect::<Vec<_>>();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
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
                let mut structs = Vec::new();
                let mut functions = Vec::new();
                for item in &module.program.items {
                    match item {
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
                        Item::Function(function) => {
                            functions.push(lower_function(function, &public))
                        }
                        Item::Test(_) => {}
                    }
                }
                IrModule {
                    name: module.name.clone(),
                    path: module
                        .path
                        .strip_prefix(&project.package_root)
                        .unwrap_or(&module.path)
                        .display()
                        .to_string()
                        .replace('\\', "/"),
                    imports: module.imports.clone(),
                    source_hash: module.source_hash.clone(),
                    structs,
                    functions,
                }
            })
            .collect(),
    }
}

fn lower_function(function: &FunctionDecl, public: &BTreeSet<String>) -> IrFunction {
    let metadata = target_metadata_for_function(function);
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
        blocks: vec![IrBlock {
            label: "entry".to_string(),
            instructions: lower_stmts(&function.body),
        }],
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
            Stmt::Require(expr) => instructions.push(IrInstruction {
                op: "require".to_string(),
                args: vec![format_expr(expr)],
                ty: Some("bool".to_string()),
            }),
            Stmt::If { cond, then_body } => {
                instructions.push(IrInstruction {
                    op: "if".to_string(),
                    args: vec![format_expr(cond), format!("then_len={}", then_body.len())],
                    ty: Some("bool".to_string()),
                });
                instructions.extend(lower_stmts(then_body));
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
    out.push_str("]}");
}

fn combined_program(project: &Project) -> Program {
    let mut items = Vec::new();
    for module in &project.modules {
        items.extend(module.program.items.clone());
    }
    Program { items }
}

fn backend_info(name: &str) -> Option<&'static BackendInfo> {
    BACKENDS.iter().find(|backend| backend.name == name)
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
