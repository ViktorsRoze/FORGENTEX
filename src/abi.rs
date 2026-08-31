use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ast::{Attribute, FunctionDecl, Program, StructDecl, Type};
use crate::diag::{diagnostics_array_to_json, Diagnostic};
use crate::targets::{target_by_triple, TargetSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiModel {
    pub triple: String,
    pub family: String,
    pub pointer_size: usize,
    pub pointer_align: usize,
    pub endian: String,
    pub calling_convention: String,
    pub integer_arg_registers: Vec<String>,
    pub return_registers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLayout {
    pub id: String,
    pub name: String,
    pub ty: String,
    pub offset: usize,
    pub size: usize,
    pub align: usize,
    pub padding_before: usize,
    pub explicit_padding: bool,
    pub endian: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructLayout {
    pub id: String,
    pub name: String,
    pub repr: String,
    pub packed: bool,
    pub align: usize,
    pub size: usize,
    pub tail_padding: usize,
    pub fields: Vec<FieldLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionAbi {
    pub id: String,
    pub name: String,
    pub extern_c: bool,
    pub calling_convention: String,
    pub params: Vec<String>,
    pub ret: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AbiReport {
    pub model: AbiModel,
    pub diagnostics: Vec<Diagnostic>,
    pub structs: Vec<StructLayout>,
    pub functions: Vec<FunctionAbi>,
}

pub fn analyze_abi(program: &Program, triple: &str) -> AbiReport {
    let Some(target) = target_by_triple(triple) else {
        return AbiReport {
            model: unknown_model(triple),
            diagnostics: vec![Diagnostic::new(
                "unknown_target",
                "@program",
                format!("unknown target triple '{triple}'"),
            )
            .suggest("run_fgx_targets")],
            structs: Vec::new(),
            functions: Vec::new(),
        };
    };
    let model = model_for_target(target);
    let structs = program
        .structs()
        .map(|decl| (decl.name.clone(), decl))
        .collect::<HashMap<_, _>>();
    let mut diagnostics = Vec::new();
    let mut layouts = Vec::new();
    for decl in program.structs() {
        layouts.push(layout_struct(
            decl,
            &structs,
            &model,
            &mut diagnostics,
            &mut Vec::new(),
        ));
    }
    let functions = program
        .functions()
        .map(|function| function_abi(function, &model))
        .collect();
    AbiReport {
        model,
        diagnostics,
        structs: layouts,
        functions,
    }
}

pub fn abi_text(report: &AbiReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "ABI report for {}\n",
        if report.model.triple.is_empty() {
            "unknown"
        } else {
            &report.model.triple
        }
    ));
    out.push_str(&format!(
        "  target family={} pointer={} align={} endian={} cc={} int_args=[{}] returns=[{}]\n",
        report.model.family,
        report.model.pointer_size,
        report.model.pointer_align,
        report.model.endian,
        report.model.calling_convention,
        report.model.integer_arg_registers.join(","),
        report.model.return_registers.join(",")
    ));
    for layout in &report.structs {
        out.push_str(&format!(
            "  struct {} id={} repr={} packed={} size={} align={} tail_padding={}\n",
            layout.name,
            layout.id,
            layout.repr,
            yes_no(layout.packed),
            layout.size,
            layout.align,
            layout.tail_padding
        ));
        for field in &layout.fields {
            let endian = field
                .endian
                .as_ref()
                .map(|value| format!(" endian={value}"))
                .unwrap_or_default();
            let explicit = if field.explicit_padding {
                " explicit_padding=yes"
            } else {
                ""
            };
            out.push_str(&format!(
                "    field {} type={} offset={} size={} align={} padding_before={}{}{}\n",
                field.name,
                field.ty,
                field.offset,
                field.size,
                field.align,
                field.padding_before,
                endian,
                explicit
            ));
        }
    }
    for function in &report.functions {
        out.push_str(&format!(
            "  fn {} id={} extern_c={} cc={} params=[{}] return={}\n",
            function.name,
            function.id,
            yes_no(function.extern_c),
            function.calling_convention,
            function.params.join(","),
            function.ret
        ));
    }
    if report.diagnostics.is_empty() {
        out.push_str("abi: ok\n");
    } else {
        for diagnostic in &report.diagnostics {
            out.push_str(&format!(
                "{} {}: {}\n",
                diagnostic.code, diagnostic.node, diagnostic.message
            ));
        }
    }
    out
}

pub fn abi_json(report: &AbiReport) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.abi.v1\",\n");
    out.push_str(&format!(
        "  \"target\": {},\n",
        quoted(&report.model.triple)
    ));
    out.push_str("  \"model\": {\n");
    out.push_str(&format!(
        "    \"family\": {},\n    \"pointer_size\": {},\n    \"pointer_align\": {},\n    \"endian\": {},\n    \"calling_convention\": {},",
        quoted(&report.model.family),
        report.model.pointer_size,
        report.model.pointer_align,
        quoted(&report.model.endian),
        quoted(&report.model.calling_convention)
    ));
    push_string_array(
        &mut out,
        4,
        "integer_arg_registers",
        &report.model.integer_arg_registers,
        true,
    );
    push_string_array(
        &mut out,
        4,
        "return_registers",
        &report.model.return_registers,
        false,
    );
    out.push_str("\n  },\n");
    out.push_str("  \"structs\": [");
    for (idx, layout) in report.structs.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"id\": {},\n      \"name\": {},\n      \"repr\": {},\n      \"packed\": {},\n      \"size\": {},\n      \"align\": {},\n      \"tail_padding\": {},\n      \"fields\": [",
            quoted(&layout.id),
            quoted(&layout.name),
            quoted(&layout.repr),
            bool_json(layout.packed),
            layout.size,
            layout.align,
            layout.tail_padding
        ));
        for (field_idx, field) in layout.fields.iter().enumerate() {
            if field_idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n        {{\"id\": {}, \"name\": {}, \"type\": {}, \"offset\": {}, \"size\": {}, \"align\": {}, \"padding_before\": {}, \"explicit_padding\": {}, \"endian\": {}}}",
                quoted(&field.id),
                quoted(&field.name),
                quoted(&field.ty),
                field.offset,
                field.size,
                field.align,
                field.padding_before,
                bool_json(field.explicit_padding),
                optional_quoted(field.endian.as_deref())
            ));
        }
        if !layout.fields.is_empty() {
            out.push('\n');
            out.push_str("      ");
        }
        out.push_str("]\n    }");
    }
    if !report.structs.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("],\n");
    out.push_str("  \"functions\": [");
    for (idx, function) in report.functions.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "\n    {{\"id\": {}, \"name\": {}, \"extern_c\": {}, \"calling_convention\": {}, \"params\": [",
            quoted(&function.id),
            quoted(&function.name),
            bool_json(function.extern_c),
            quoted(&function.calling_convention)
        ));
        for (param_idx, param) in function.params.iter().enumerate() {
            if param_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&quoted(param));
        }
        out.push_str(&format!("], \"return\": {}}}", quoted(&function.ret)));
    }
    if !report.functions.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("],\n");
    let diagnostics = diagnostics_array_to_json(&report.diagnostics);
    out.push_str(&format!("  \"diagnostics\": {diagnostics}\n"));
    out.push('}');
    out
}

pub fn generate_header(program: &Program, triple: &str) -> Result<String, String> {
    if target_by_triple(triple).is_none() {
        return Err(format!("unknown target triple '{triple}'"));
    }
    let report = analyze_abi(program, triple);
    let mut registry = TypeRegistry::default();
    registry.collect_program(program);
    let guard = format!("FORGENTEX_ABI_{}", ident(triple).to_ascii_uppercase());
    let mut out = String::new();
    out.push_str("/* Generated by fgx header. */\n");
    out.push_str(&format!("#ifndef {guard}\n#define {guard}\n\n"));
    out.push_str("#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\n");
    out.push_str(&format!(
        "#define FGX_ABI_TARGET \"{}\"\n",
        report.model.triple
    ));
    out.push_str(&format!(
        "#define FGX_ABI_POINTER_SIZE {}\n",
        report.model.pointer_size
    ));
    out.push_str("#ifndef FGX_EXTERN\n#define FGX_EXTERN extern\n#endif\n");
    out.push_str("#if defined(_MSC_VER)\n#define FGX_ALIGNAS(n) __declspec(align(n))\n#else\n#define FGX_ALIGNAS(n) __attribute__((aligned(n)))\n#endif\n\n");

    for typedef in registry.typedefs() {
        out.push_str(&typedef);
        out.push('\n');
    }
    if !registry.is_empty() {
        out.push('\n');
    }

    for decl in program.structs() {
        out.push_str(&emit_header_struct(decl)?);
        out.push('\n');
    }
    for function in program.functions() {
        out.push_str(&format!("FGX_EXTERN {};\n", function_signature(function)?));
    }
    out.push_str(&format!("\n#endif /* {guard} */\n"));
    Ok(out)
}

/// Emit just the selected named struct declarations for a foreign C-ABI
/// boundary. Unlike [`generate_header`], this does not emit Forgentex function
/// declarations, so unrelated functions cannot make a capsule's private type
/// header unavailable. Callers still own the policy that decides which layout
/// attributes and field types are safe to expose.
pub fn generate_struct_header(
    program: &Program,
    triple: &str,
    names: &[String],
) -> Result<String, String> {
    if target_by_triple(triple).is_none() {
        return Err(format!("unknown target triple '{triple}'"));
    }
    let selected = names.iter().cloned().collect::<BTreeSet<_>>();
    let guard = format!(
        "FORGENTEX_NATIVE_TYPES_{}",
        ident(triple).to_ascii_uppercase()
    );
    let mut out = String::new();
    out.push_str("/* Generated by Forgentex for a native capsule boundary. */\n");
    out.push_str(&format!("#ifndef {guard}\n#define {guard}\n\n"));
    out.push_str("#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\n");
    out.push_str("#if defined(_MSC_VER)\n#define FGX_ALIGNAS(n) __declspec(align(n))\n#else\n#define FGX_ALIGNAS(n) __attribute__((aligned(n)))\n#endif\n\n");
    let mut emitted = BTreeSet::new();
    let mut stack = Vec::new();
    for name in &selected {
        emit_selected_struct(program, name, &selected, &mut emitted, &mut stack, &mut out)?;
    }
    out.push_str(&format!("#endif /* {guard} */\n"));
    Ok(out)
}

fn emit_selected_struct(
    program: &Program,
    name: &str,
    selected: &BTreeSet<String>,
    emitted: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    out: &mut String,
) -> Result<(), String> {
    if emitted.contains(name) {
        return Ok(());
    }
    if stack.iter().any(|entry| entry == name) {
        return Err(format!(
            "native boundary struct '{name}' is recursively embedded"
        ));
    }
    let decl = program
        .structs()
        .find(|decl| decl.name == name)
        .ok_or_else(|| format!("native boundary refers to unknown struct '{name}'"))?;
    stack.push(name.to_string());
    let mut dependencies = BTreeSet::new();
    for field in &decl.fields {
        collect_named_types(&field.ty, &mut dependencies);
    }
    for dependency in dependencies {
        if selected.contains(&dependency) {
            emit_selected_struct(program, &dependency, selected, emitted, stack, out)?;
        }
    }
    stack.pop();
    out.push_str(&emit_header_struct(decl)?);
    out.push('\n');
    emitted.insert(name.to_string());
    Ok(())
}

fn collect_named_types(ty: &Type, names: &mut BTreeSet<String>) {
    match ty {
        Type::Named(name) => {
            names.insert(name.clone());
        }
        Type::Pointer { elem, .. } | Type::Slice { elem, .. } | Type::Array { elem, .. } => {
            collect_named_types(elem, names);
        }
        Type::Generic { args, .. } => {
            for arg in args {
                collect_named_types(arg, names);
            }
        }
        _ => {}
    }
}

fn layout_struct(
    decl: &StructDecl,
    structs: &HashMap<String, &StructDecl>,
    model: &AbiModel,
    diagnostics: &mut Vec<Diagnostic>,
    stack: &mut Vec<String>,
) -> StructLayout {
    let attrs = layout_attrs(&decl.attrs, &decl.id, diagnostics);
    if attrs.repr != "C" {
        diagnostics.push(
            Diagnostic::new(
                "abi_repr_required",
                &decl.id,
                format!(
                    "struct '{}' should declare @repr(C) for stable C ABI layout",
                    decl.name
                ),
            )
            .suggest("add_repr_c"),
        );
    }
    let mut offset = 0usize;
    let mut max_align = 1usize;
    let mut fields = Vec::new();
    for field in &decl.fields {
        let field_id = format!("{}.field.{}", decl.id, field.name);
        let field_attrs = field_attrs(&field.attrs, &field_id, diagnostics);
        let type_layout = type_layout(&field.ty, structs, model, diagnostics, stack);
        if field_attrs.endian.is_some() && !is_endian_compatible(&field.ty) {
            diagnostics.push(
                Diagnostic::new(
                    "abi_endian_non_integer",
                    &field_id,
                    format!(
                        "field '{}' has endian annotation on a non-integer type",
                        field.name
                    ),
                )
                .suggest("remove_endian_annotation"),
            );
        }
        let field_align = if attrs.packed { 1 } else { type_layout.align };
        let aligned_offset = align_to(offset, field_align);
        let padding_before = aligned_offset - offset;
        offset = aligned_offset + type_layout.size;
        max_align = max_align.max(field_align);
        fields.push(FieldLayout {
            id: field_id,
            name: field.name.clone(),
            ty: field.ty.to_agent(),
            offset: aligned_offset,
            size: type_layout.size,
            align: field_align,
            padding_before,
            explicit_padding: type_layout.explicit_padding,
            endian: field_attrs.endian,
        });
    }
    let align = attrs.aligned.unwrap_or(max_align).max(max_align);
    let size = align_to(offset, align);
    StructLayout {
        id: decl.id.clone(),
        name: decl.name.clone(),
        repr: attrs.repr,
        packed: attrs.packed,
        align,
        size,
        tail_padding: size - offset,
        fields,
    }
}

fn type_layout(
    ty: &Type,
    structs: &HashMap<String, &StructDecl>,
    model: &AbiModel,
    diagnostics: &mut Vec<Diagnostic>,
    stack: &mut Vec<String>,
) -> TypeLayout {
    match ty {
        Type::Bool => TypeLayout::new(1, 1),
        Type::Primitive(name) => primitive_layout(name, model),
        Type::Pointer { .. } => TypeLayout::new(model.pointer_size, model.pointer_align),
        Type::Slice { .. } => TypeLayout::new(model.pointer_size * 2, model.pointer_align),
        Type::Array { elem, len } => {
            let elem = type_layout(elem, structs, model, diagnostics, stack);
            TypeLayout::new(align_to(elem.size, elem.align) * len, elem.align)
        }
        Type::Named(name) => {
            if stack.iter().any(|entry| entry == name) {
                diagnostics.push(
                    Diagnostic::new(
                        "abi_recursive_type",
                        format!("@struct.{name}"),
                        format!("recursive struct '{name}' cannot have a finite ABI layout"),
                    )
                    .suggest("replace_recursive_field_with_pointer"),
                );
                return TypeLayout::new(0, 1);
            }
            let Some(decl) = structs.get(name) else {
                diagnostics.push(
                    Diagnostic::new(
                        "abi_unknown_type",
                        "@program",
                        format!("unknown named type '{name}' in ABI layout"),
                    )
                    .suggest("add_struct_declaration"),
                );
                return TypeLayout::new(0, 1);
            };
            stack.push(name.clone());
            let layout = layout_struct(decl, structs, model, diagnostics, stack);
            stack.pop();
            TypeLayout::new(layout.size, layout.align)
        }
        Type::Generic { name, args } if name == "padding" && args.len() == 1 => {
            let size = lane_count_type(&args[0]).unwrap_or_else(|| {
                diagnostics.push(
                    Diagnostic::new(
                        "abi_invalid_padding",
                        "@program",
                        "padding[N] requires an integer byte count",
                    )
                    .suggest("replace_padding_count"),
                );
                0
            });
            TypeLayout {
                size,
                align: 1,
                explicit_padding: true,
            }
        }
        Type::Generic { name, args } if name == "vec" && args.len() == 2 => {
            let elem = type_layout(&args[0], structs, model, diagnostics, stack);
            let lanes = lane_count_type(&args[1]).unwrap_or(1);
            TypeLayout::new(elem.size * lanes, elem.align.min(16).max(elem.align))
        }
        Type::Generic { name, .. } => {
            diagnostics.push(
                Diagnostic::new(
                    "abi_unsupported_type",
                    "@program",
                    format!("generic type '{name}' has no stable ABI layout in v1"),
                )
                .suggest("replace_with_repr_c_struct"),
            );
            TypeLayout::new(0, 1)
        }
        Type::Void | Type::Unknown => TypeLayout::new(0, 1),
    }
}

fn primitive_layout(name: &str, model: &AbiModel) -> TypeLayout {
    match name {
        "u8" | "i8" => TypeLayout::new(1, 1),
        "u16" | "i16" => TypeLayout::new(2, 2),
        "u32" | "i32" | "f32" => TypeLayout::new(4, 4),
        "u64" | "i64" | "f64" => {
            let align = if model.family == "arm" { 4 } else { 8 };
            TypeLayout::new(8, align)
        }
        "usize" | "isize" => TypeLayout::new(model.pointer_size, model.pointer_align),
        "$int" => TypeLayout::new(8, 8),
        "$float" => TypeLayout::new(8, 8),
        _ => TypeLayout::new(0, 1),
    }
}

fn function_abi(function: &FunctionDecl, model: &AbiModel) -> FunctionAbi {
    FunctionAbi {
        id: function.id.clone(),
        name: function.name.clone(),
        extern_c: has_attr_arg(&function.attrs, "extern", "C")
            || has_attr_arg(&function.attrs, "abi", "C"),
        calling_convention: model.calling_convention.clone(),
        params: function
            .params
            .iter()
            .map(|param| format!("{}:{}", param.name, param.ty.to_agent()))
            .collect(),
        ret: function.ret.to_agent(),
    }
}

fn model_for_target(target: &TargetSpec) -> AbiModel {
    let (pointer_size, pointer_align) = match target.family {
        "arm" | "riscv32" => (4, 4),
        _ => (8, 8),
    };
    let (calling_convention, integer_arg_registers, return_registers) = match target.family {
        "x86_64" if target.triple.contains("windows") => (
            "win64".to_string(),
            strings(&["rcx", "rdx", "r8", "r9"]),
            strings(&["rax"]),
        ),
        "x86_64" => (
            "sysv64".to_string(),
            strings(&["rdi", "rsi", "rdx", "rcx", "r8", "r9"]),
            strings(&["rax", "rdx"]),
        ),
        "arm" => (
            "aapcs32".to_string(),
            strings(&["r0", "r1", "r2", "r3"]),
            strings(&["r0", "r1"]),
        ),
        "aarch64" if target.triple.contains("windows") => (
            "aapcs64-windows".to_string(),
            strings(&["x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7"]),
            strings(&["x0", "x1"]),
        ),
        "aarch64" => (
            "aapcs64".to_string(),
            strings(&["x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7"]),
            strings(&["x0", "x1"]),
        ),
        "riscv32" | "riscv64" => (
            "riscv-elf-psabi".to_string(),
            strings(&["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7"]),
            strings(&["a0", "a1"]),
        ),
        _ => ("unknown".to_string(), Vec::new(), Vec::new()),
    };
    AbiModel {
        triple: target.triple.to_string(),
        family: target.family.to_string(),
        pointer_size,
        pointer_align,
        endian: "little".to_string(),
        calling_convention,
        integer_arg_registers,
        return_registers,
    }
}

fn unknown_model(triple: &str) -> AbiModel {
    AbiModel {
        triple: triple.to_string(),
        family: "unknown".to_string(),
        pointer_size: 0,
        pointer_align: 0,
        endian: "unknown".to_string(),
        calling_convention: "unknown".to_string(),
        integer_arg_registers: Vec::new(),
        return_registers: Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LayoutAttrs {
    repr: String,
    packed: bool,
    aligned: Option<usize>,
}

fn layout_attrs(attrs: &[Attribute], node: &str, diagnostics: &mut Vec<Diagnostic>) -> LayoutAttrs {
    let repr_args = attr_args(attrs, "repr").unwrap_or_default();
    let repr = repr_args
        .first()
        .cloned()
        .unwrap_or_else(|| "forgentex".to_string());
    let packed = has_attr(attrs, "packed") || repr_args.iter().any(|arg| arg == "packed");
    let aligned = attr_args(attrs, "aligned").and_then(|args| {
        let value = args.first()?.parse::<usize>().ok();
        match value {
            Some(value) if value.is_power_of_two() => Some(value),
            _ => {
                diagnostics.push(
                    Diagnostic::new(
                        "abi_invalid_alignment",
                        node,
                        "aligned(N) requires a power-of-two byte alignment",
                    )
                    .suggest("replace_alignment"),
                );
                None
            }
        }
    });
    LayoutAttrs {
        repr,
        packed,
        aligned,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FieldAttrs {
    endian: Option<String>,
}

fn field_attrs(attrs: &[Attribute], node: &str, diagnostics: &mut Vec<Diagnostic>) -> FieldAttrs {
    let endian = attr_args(attrs, "endian").and_then(|args| args.first().cloned());
    if let Some(endian) = endian.as_deref() {
        if endian != "little" && endian != "big" && endian != "native" {
            diagnostics.push(
                Diagnostic::new(
                    "abi_invalid_endian",
                    node,
                    format!("unknown endian annotation '{endian}'"),
                )
                .suggest("choose_little_big_or_native"),
            );
        }
    }
    FieldAttrs { endian }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TypeLayout {
    size: usize,
    align: usize,
    explicit_padding: bool,
}

impl TypeLayout {
    fn new(size: usize, align: usize) -> Self {
        Self {
            size,
            align,
            explicit_padding: false,
        }
    }
}

fn emit_header_struct(decl: &StructDecl) -> Result<String, String> {
    let attrs = header_layout_attrs(&decl.attrs);
    let mut out = String::new();
    if attrs.packed {
        out.push_str("#pragma pack(push, 1)\n");
    }
    let align = attrs
        .aligned
        .map(|value| format!(" FGX_ALIGNAS({value})"))
        .unwrap_or_default();
    out.push_str(&format!("typedef struct{align} {} {{\n", ident(&decl.name)));
    for field in &decl.fields {
        out.push_str(&format!("    {}\n", c_field_decl(&field.name, &field.ty)?));
    }
    out.push_str(&format!("}} {};\n", ident(&decl.name)));
    if attrs.packed {
        out.push_str("#pragma pack(pop)\n");
    }
    Ok(out)
}

fn function_signature(function: &FunctionDecl) -> Result<String, String> {
    let params = if function.params.is_empty() {
        "void".to_string()
    } else {
        function
            .params
            .iter()
            .map(|param| Ok(format!("{} {}", c_type(&param.ty)?, ident(&param.name))))
            .collect::<Result<Vec<_>, String>>()?
            .join(", ")
    };
    Ok(format!(
        "{} {}({params})",
        c_type(&function.ret)?,
        ident(&function.name)
    ))
}

fn c_field_decl(name: &str, ty: &Type) -> Result<String, String> {
    if let Some(bytes) = padding_bytes(ty) {
        return Ok(format!("uint8_t {}[{bytes}];", ident(name)));
    }
    match ty {
        Type::Array { .. } => {
            let mut dimensions = String::new();
            let mut element = ty;
            while let Type::Array { elem, len } = element {
                dimensions.push_str(&format!("[{len}]"));
                element = elem;
            }
            Ok(format!("{} {}{dimensions};", c_type(element)?, ident(name)))
        }
        _ => Ok(format!("{} {};", c_type(ty)?, ident(name))),
    }
}

fn c_type(ty: &Type) -> Result<String, String> {
    match ty {
        Type::Bool => Ok("bool".to_string()),
        Type::Primitive(name) => match name.as_str() {
            "i8" => Ok("int8_t".to_string()),
            "i16" => Ok("int16_t".to_string()),
            "i32" => Ok("int32_t".to_string()),
            "i64" => Ok("int64_t".to_string()),
            "isize" => Ok("ptrdiff_t".to_string()),
            "u8" => Ok("uint8_t".to_string()),
            "u16" => Ok("uint16_t".to_string()),
            "u32" => Ok("uint32_t".to_string()),
            "u64" => Ok("uint64_t".to_string()),
            "usize" => Ok("size_t".to_string()),
            "f32" => Ok("float".to_string()),
            "f64" => Ok("double".to_string()),
            "$int" => Ok("int64_t".to_string()),
            "$float" => Ok("double".to_string()),
            other => Err(format!("ABI header does not know primitive type '{other}'")),
        },
        Type::Named(name) => Ok(ident(name)),
        Type::Pointer { mutable, elem } => {
            if *mutable {
                Ok(format!("{} *", c_type(elem)?))
            } else {
                Ok(format!("const {} *", c_type(elem)?))
            }
        }
        Type::Slice { mutable, elem } => Ok(slice_type_name(elem, *mutable)?),
        Type::Array { elem, len } => Ok(array_type_name(elem, *len)?),
        Type::Generic { name, args } if name == "padding" && args.len() == 1 => {
            Ok("uint8_t".to_string())
        }
        Type::Generic { name, .. } => Err(format!("ABI header does not lower generic '{name}'")),
        Type::Void => Ok("void".to_string()),
        Type::Unknown => Ok("void *".to_string()),
    }
}

#[derive(Default)]
struct TypeRegistry {
    slices: BTreeMap<(bool, String), Type>,
    arrays: BTreeMap<(String, usize), Type>,
}

impl TypeRegistry {
    fn collect_program(&mut self, program: &Program) {
        for decl in program.structs() {
            for field in &decl.fields {
                self.collect_type(&field.ty);
            }
        }
        for function in program.functions() {
            self.collect_type(&function.ret);
            for param in &function.params {
                self.collect_type(&param.ty);
            }
        }
    }

    fn collect_type(&mut self, ty: &Type) {
        match ty {
            Type::Slice { mutable, elem } => {
                self.collect_type(elem);
                self.slices
                    .insert((*mutable, elem.to_agent()), elem.as_ref().clone());
            }
            Type::Array { elem, len } => {
                self.collect_type(elem);
                self.arrays
                    .insert((elem.to_agent(), *len), elem.as_ref().clone());
            }
            Type::Pointer { elem, .. } => self.collect_type(elem),
            Type::Generic { args, .. } => {
                for arg in args {
                    self.collect_type(arg);
                }
            }
            _ => {}
        }
    }

    fn typedefs(&self) -> Vec<String> {
        let mut out = Vec::new();
        for ((_, len), elem) in &self.arrays {
            if let (Ok(name), Ok(c_elem)) = (array_type_name(elem, *len), c_type(elem)) {
                out.push(format!(
                    "typedef struct {name} {{ {c_elem} data[{len}]; }} {name};"
                ));
            }
        }
        for ((mutable, _), elem) in &self.slices {
            if let (Ok(name), Ok(c_elem)) = (slice_type_name(elem, *mutable), c_type(elem)) {
                let qualifier = if *mutable { "" } else { "const " };
                out.push(format!(
                    "typedef struct {name} {{ {qualifier}{c_elem} *data; size_t len; }} {name};"
                ));
            }
        }
        out
    }

    fn is_empty(&self) -> bool {
        self.slices.is_empty() && self.arrays.is_empty()
    }
}

fn header_layout_attrs(attrs: &[Attribute]) -> LayoutAttrs {
    let repr_args = attr_args(attrs, "repr").unwrap_or_default();
    LayoutAttrs {
        repr: repr_args
            .first()
            .cloned()
            .unwrap_or_else(|| "forgentex".to_string()),
        packed: has_attr(attrs, "packed") || repr_args.iter().any(|arg| arg == "packed"),
        aligned: attr_args(attrs, "aligned")
            .and_then(|args| args.first().and_then(|value| value.parse::<usize>().ok())),
    }
}

fn is_endian_compatible(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(name) if matches!(name.as_str(), "u16" | "u32" | "u64" | "i16" | "i32" | "i64" | "usize" | "isize"))
}

fn padding_bytes(ty: &Type) -> Option<usize> {
    match ty {
        Type::Generic { name, args } if name == "padding" && args.len() == 1 => {
            lane_count_type(&args[0])
        }
        _ => None,
    }
}

fn lane_count_type(ty: &Type) -> Option<usize> {
    match ty {
        Type::Named(value) | Type::Primitive(value) => value.parse::<usize>().ok(),
        _ => None,
    }
}

fn attr_args(attrs: &[Attribute], name: &str) -> Option<Vec<String>> {
    attrs
        .iter()
        .find(|attr| attr.name == name)
        .map(|attr| attr.args.clone())
}

fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| attr.name == name)
}

fn has_attr_arg(attrs: &[Attribute], name: &str, arg: &str) -> bool {
    attrs
        .iter()
        .any(|attr| attr.name == name && attr.args.iter().any(|value| value == arg))
}

fn align_to(value: usize, align: usize) -> usize {
    if align == 0 {
        value
    } else {
        value.div_ceil(align) * align
    }
}

fn slice_type_name(elem: &Type, mutable: bool) -> Result<String, String> {
    let prefix = if mutable {
        "fgx_slice"
    } else {
        "fgx_const_slice"
    };
    Ok(format!("{prefix}_{}", type_suffix(elem)?))
}

fn array_type_name(elem: &Type, len: usize) -> Result<String, String> {
    Ok(format!("fgx_array_{}_{}", type_suffix(elem)?, len))
}

fn type_suffix(ty: &Type) -> Result<String, String> {
    match ty {
        Type::Bool => Ok("bool".to_string()),
        Type::Primitive(name) | Type::Named(name) => Ok(ident(name)),
        Type::Pointer { mutable, elem } => {
            let mutability = if *mutable { "mut" } else { "const" };
            Ok(format!("ptr_{mutability}_{}", type_suffix(elem)?))
        }
        Type::Slice { mutable, elem } => {
            let mutability = if *mutable { "mut" } else { "const" };
            Ok(format!("slice_{mutability}_{}", type_suffix(elem)?))
        }
        Type::Array { elem, len } => Ok(format!("array_{}_{}", type_suffix(elem)?, len)),
        Type::Generic { name, args } => {
            let args = args
                .iter()
                .map(type_suffix)
                .collect::<Result<Vec<_>, _>>()?
                .join("_");
            Ok(format!("{}_{}", ident(name), args))
        }
        Type::Void => Ok("void".to_string()),
        Type::Unknown => Ok("unknown".to_string()),
    }
}

fn ident(name: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in name.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            if idx == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
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

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
