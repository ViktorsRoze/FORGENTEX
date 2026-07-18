use crate::ast::{
    AsmBlock, Attribute, Capsule, CapsuleBinding, CapsuleFixture, EnumDecl, Expr, FunctionDecl,
    Item, Program, ShareDecl, Stmt, StructDecl, TestDecl, TestExpectation, TraitDecl,
    TraitImplDecl,
};

pub fn format_human(program: &Program) -> String {
    let mut out = String::new();
    for (idx, item) in program.items.iter().enumerate() {
        if idx > 0 {
            out.push_str("\n\n");
        }
        match item {
            Item::Struct(decl) => format_struct_human(&mut out, decl),
            Item::Enum(decl) => format_enum_human(&mut out, decl),
            Item::Trait(decl) => format_trait_human(&mut out, decl),
            Item::TraitImpl(decl) => format_trait_impl_human(&mut out, decl),
            Item::Function(function) => format_function_human(&mut out, function),
            Item::Test(test) => format_test_human(&mut out, test),
            Item::Capsule(capsule) => format_capsule_human(&mut out, capsule),
            Item::Share(share) => format_share_human(&mut out, share),
        }
    }
    finish_document(out)
}

pub fn format_agent(program: &Program) -> String {
    let mut out = String::new();
    for (idx, item) in program.items.iter().enumerate() {
        if idx > 0 {
            out.push_str("\n\n");
        }
        match item {
            Item::Struct(decl) => format_struct_agent(&mut out, decl),
            Item::Enum(decl) => format_enum_agent(&mut out, decl),
            Item::Trait(decl) => format_trait_agent(&mut out, decl),
            Item::TraitImpl(decl) => format_trait_impl_agent(&mut out, decl),
            Item::Function(function) => format_function_agent(&mut out, function),
            Item::Test(test) => format_test_agent(&mut out, test),
            Item::Capsule(capsule) => format_capsule_agent(&mut out, capsule),
            Item::Share(share) => format_share_agent(&mut out, share),
        }
    }
    // Render high-frequency builtins with their short two-letter aliases.
    finish_document(crate::prelude::apply_aliases(&out))
}

fn finish_document(out: String) -> String {
    format!("{}\n", out.trim_end_matches('\n'))
}

fn format_struct_human(out: &mut String, decl: &StructDecl) {
    format_attrs_human(out, &decl.attrs, 0);
    out.push_str(&format!("struct {}:\n", decl.name));
    for field in &decl.fields {
        format_attrs_human(out, &field.attrs, 1);
        out.push_str(&format!("    {}: {}\n", field.name, field.ty.to_human()));
    }
}

fn format_enum_human(out: &mut String, decl: &EnumDecl) {
    format_attrs_human(out, &decl.attrs, 0);
    out.push_str(&format!("enum {}:\n", decl.name));
    for variant in &decl.variants {
        match &variant.payload {
            Some(ty) => out.push_str(&format!("    {}({})\n", variant.name, ty.to_human())),
            None => out.push_str(&format!("    {}\n", variant.name)),
        }
    }
}

fn format_enum_agent(out: &mut String, decl: &EnumDecl) {
    // Dense agent keyword: EN = enum; variants on one line, ';'-separated.
    // `variant_repr` renders the optional payload as `Name(Type)`.
    let variants = decl
        .variants
        .iter()
        .map(crate::ast::variant_repr)
        .collect::<Vec<_>>()
        .join("; ");
    out.push_str(&format!("EN {} {{ {} }}\n", decl.name, variants));
}

fn format_trait_human(out: &mut String, decl: &TraitDecl) {
    format_attrs_human(out, &decl.attrs, 0);
    out.push_str(&format!("trait {}:\n", decl.name));
    for associated_type in &decl.associated_types {
        out.push_str(&format!("    requires type {}\n", associated_type.name));
    }
    for method in &decl.methods {
        let params = method
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.to_human()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "    requires {}({}) -> {}{}\n",
            method.name,
            params,
            method.ret.to_human(),
            method
                .default_function
                .as_ref()
                .map_or(String::new(), |function| format!(" default {function}"))
        ));
    }
    for constant in &decl.constants {
        out.push_str(&format!(
            "    requires const {}: {}\n",
            constant.name,
            constant.ty.to_human()
        ));
    }
}

fn format_trait_impl_human(out: &mut String, decl: &TraitImplDecl) {
    format_attrs_human(out, &decl.attrs, 0);
    out.push_str(&format!(
        "impl{} {} for {}:\n",
        format_decl_type_params(&decl.type_params, &decl.type_bounds),
        decl.trait_name,
        decl.for_type.to_human()
    ));
    for associated_type in &decl.associated_types {
        out.push_str(&format!(
            "    type {} = {}\n",
            associated_type.name,
            associated_type.ty.to_human()
        ));
    }
    for method in &decl.methods {
        out.push_str(&format!("    {} = {}\n", method.method, method.function));
    }
    for constant in &decl.constants {
        out.push_str(&format!(
            "    const {} = {}\n",
            constant.name,
            format_expr(&constant.value)
        ));
    }
}

fn format_trait_agent(out: &mut String, decl: &TraitDecl) {
    format_attrs_agent(out, &decl.attrs);
    out.push_str(&format!("TR {} {{\n", decl.name));
    for associated_type in &decl.associated_types {
        out.push_str(&format!("  TYPE {}\n", associated_type.name));
    }
    for method in &decl.methods {
        let params = method
            .params
            .iter()
            .map(|p| format!("{} {}", p.name, p.ty.to_agent()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "  REQ {}({}) -> {}{}\n",
            method.name,
            params,
            method.ret.to_agent(),
            method
                .default_function
                .as_ref()
                .map_or(String::new(), |function| format!(" DEFAULT {function}"))
        ));
    }
    for constant in &decl.constants {
        out.push_str(&format!(
            "  CONST {} {}\n",
            constant.name,
            constant.ty.to_agent()
        ));
    }
    out.push_str("}\n");
}

fn format_trait_impl_agent(out: &mut String, decl: &TraitImplDecl) {
    format_attrs_agent(out, &decl.attrs);
    out.push_str(&format!(
        "IM{} {} {} {{\n",
        format_decl_type_params(&decl.type_params, &decl.type_bounds),
        decl.trait_name,
        decl.for_type.to_agent()
    ));
    for associated_type in &decl.associated_types {
        out.push_str(&format!(
            "  TYPE {} = {}\n",
            associated_type.name,
            associated_type.ty.to_agent()
        ));
    }
    for method in &decl.methods {
        out.push_str(&format!("  BIND {} {}\n", method.method, method.function));
    }
    for constant in &decl.constants {
        out.push_str(&format!(
            "  CONST {} = {}\n",
            constant.name,
            format_expr(&constant.value)
        ));
    }
    out.push_str("}\n");
}

/// `[T, U]` when a function is generic (lang.4); empty otherwise. Identical on
/// both syntax surfaces (brackets are already the dense form).
fn format_decl_type_params(
    type_params: &[String],
    type_bounds: &[crate::ast::TypeParamBound],
) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        let params = type_params
            .iter()
            .map(|param| {
                let bounds = type_bounds
                    .iter()
                    .find(|bound| bound.type_param == *param)
                    .map_or(&[][..], |bound| bound.traits.as_slice());
                if bounds.is_empty() {
                    param.clone()
                } else {
                    format!("{param}: {}", bounds.join(" + "))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{params}]")
    }
}

fn format_type_params(function: &FunctionDecl) -> String {
    format_decl_type_params(&function.type_params, &function.type_bounds)
}

fn format_function_human(out: &mut String, function: &FunctionDecl) {
    format_attrs_human(out, &function.attrs, 0);
    let params = function
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.ty.to_human()))
        .collect::<Vec<_>>()
        .join(", ");
    if function.unsafe_fn {
        out.push_str("unsafe ");
    }
    out.push_str(&format!(
        "fn {}{}({}) -> {}{}:\n",
        function.name,
        format_type_params(function),
        params,
        function.ret.to_human(),
        format_effects_human(&function.effects)
    ));
    format_stmts_human(out, &function.body, 1);
}

fn format_test_human(out: &mut String, test: &TestDecl) {
    if test.xfail {
        out.push_str("xfail ");
    }
    out.push_str(&format!("test {}:\n", quoted_string(&test.name)));
    for expectation in &test.expectations {
        match expectation {
            TestExpectation::Expr { expr } => {
                out.push_str(&format!("    expect {}\n", format_expr(expr)));
            }
            TestExpectation::Diagnostic { code } => {
                out.push_str(&format!("    expect_diagnostic {code}\n"));
            }
        }
    }
}

fn format_attrs_human(out: &mut String, attrs: &[Attribute], level: usize) {
    let pad = "    ".repeat(level);
    for attr in attrs {
        out.push_str(&pad);
        out.push('@');
        out.push_str(&format_attr_body(attr, true));
        out.push('\n');
    }
}

fn format_effects_human(effects: &[String]) -> String {
    if effects.is_empty() {
        String::new()
    } else {
        format!(" effects [{}]", effects.join(", "))
    }
}

fn format_stmts_human(out: &mut String, body: &[Stmt], level: usize) {
    for stmt in body {
        let pad = "    ".repeat(level);
        match stmt {
            Stmt::Let { name, ty, expr } => {
                if let Some(ty) = ty {
                    out.push_str(&format!(
                        "{pad}let {name}: {} = {}\n",
                        ty.to_human(),
                        format_expr(expr)
                    ));
                } else {
                    out.push_str(&format!("{pad}let {name} = {}\n", format_expr(expr)));
                }
            }
            Stmt::Return(Some(expr)) => {
                out.push_str(&format!("{pad}return {}\n", format_expr(expr)));
            }
            Stmt::Return(None) => {
                out.push_str(&format!("{pad}return\n"));
            }
            Stmt::Break => out.push_str(&format!("{pad}break\n")),
            Stmt::Continue => out.push_str(&format!("{pad}continue\n")),
            Stmt::Require(expr) => {
                out.push_str(&format!("{pad}require {}\n", format_expr(expr)));
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                out.push_str(&format!("{pad}if {}:\n", format_expr(cond)));
                format_stmts_human(out, then_body, level + 1);
                // Flatten an `else { if … }` chain into `else if …:`.
                let mut chain: &[Stmt] = else_body;
                loop {
                    if chain.is_empty() {
                        break;
                    }
                    if let [Stmt::If {
                        cond,
                        then_body,
                        else_body,
                    }] = chain
                    {
                        out.push_str(&format!("{pad}else if {}:\n", format_expr(cond)));
                        format_stmts_human(out, then_body, level + 1);
                        chain = else_body;
                        continue;
                    }
                    out.push_str(&format!("{pad}else:\n"));
                    format_stmts_human(out, chain, level + 1);
                    break;
                }
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                out.push_str(&format!(
                    "{pad}for {var} in {} .. {}:\n",
                    format_expr(start),
                    format_expr(end)
                ));
                format_stmts_human(out, body, level + 1);
            }
            Stmt::While { cond, body } => {
                out.push_str(&format!("{pad}while {}:\n", format_expr(cond)));
                format_stmts_human(out, body, level + 1);
            }
            Stmt::Match { scrutinee, arms } => {
                out.push_str(&format!("{pad}match {}:\n", format_expr(scrutinee)));
                let arm_pad = "    ".repeat(level + 1);
                for arm in arms {
                    out.push_str(&format!(
                        "{arm_pad}case {}:\n",
                        crate::ast::pattern_to_string(&arm.pattern)
                    ));
                    format_stmts_human(out, &arm.body, level + 2);
                }
            }
            Stmt::Assign { target, expr } => {
                out.push_str(&format!(
                    "{pad}{} = {}\n",
                    format_expr(target),
                    format_expr(expr)
                ));
            }
            Stmt::Expr(expr) => {
                out.push_str(&format!("{pad}{}\n", format_expr(expr)));
            }
            Stmt::Asm(asm) => {
                format_asm_human(out, asm, level);
            }
        }
    }
}

fn format_asm_human(out: &mut String, asm: &AsmBlock, level: usize) {
    let pad = "    ".repeat(level);
    let inner = "    ".repeat(level + 1);
    out.push_str(&format!("{pad}asm {}:\n", asm.target));
    for input in &asm.inputs {
        out.push_str(&format!("{inner}in {}: {}\n", input.register, input.value));
    }
    for output in &asm.outputs {
        out.push_str(&format!(
            "{inner}out {}: {}\n",
            output.register, output.value
        ));
    }
    for clobber in &asm.clobbers {
        out.push_str(&format!("{inner}clobber {clobber}\n"));
    }
    for op in &asm.ops {
        out.push_str(&format!("{inner}op {}\n", op.join(" ")));
    }
}

fn format_struct_agent(out: &mut String, decl: &StructDecl) {
    format_attrs_agent(out, &decl.attrs);
    let fields = decl
        .fields
        .iter()
        .map(|field| {
            let attrs = if field.attrs.is_empty() {
                String::new()
            } else {
                format!(
                    " {}",
                    field
                        .attrs
                        .iter()
                        .map(|attr| format!("@{}", format_attr_body(attr, false)))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            format!("{} {}{}", field.name, field.ty.to_agent(), attrs)
        })
        .collect::<Vec<_>>()
        .join("; ");
    out.push_str(&format!("S {} {{ {} }}\n", decl.name, fields));
}

fn format_function_agent(out: &mut String, function: &FunctionDecl) {
    format_attrs_agent(out, &function.attrs);
    let params = function
        .params
        .iter()
        .map(|param| format!("{} {}", param.name, param.ty.to_agent()))
        .collect::<Vec<_>>()
        .join(", ");
    if function.unsafe_fn {
        out.push_str("UF ");
    } else {
        out.push_str("F ");
    }
    out.push_str(&format!(
        "{}{}({}) -> {}{} {{\n",
        function.name,
        format_type_params(function),
        params,
        function.ret.to_agent(),
        format_effects_agent(&function.effects)
    ));
    format_stmts_agent(out, &function.body, 1);
    out.push_str("}\n");
}

fn format_test_agent(out: &mut String, test: &TestDecl) {
    if test.xfail {
        out.push_str("XTEST ");
    } else {
        out.push_str("TEST ");
    }
    out.push_str(&format!("{} {{\n", quoted_string(&test.name)));
    for expectation in &test.expectations {
        match expectation {
            TestExpectation::Expr { expr } => {
                out.push_str(&format!("  EXPECT {}\n", format_expr(expr)));
            }
            TestExpectation::Diagnostic { code } => {
                out.push_str(&format!("  EXPECT_DIAGNOSTIC {code}\n"));
            }
        }
    }
    out.push_str("}\n");
}

fn format_share_human(out: &mut String, share: &ShareDecl) {
    format_attrs_human(out, &share.attrs, 0);
    out.push_str(&format!("share {}: {}", share.name, share.ty.to_human()));
    if let Some(default) = &share.default {
        out.push_str(&format!(" = {}", default.to_literal()));
    }
    out.push('\n');
}

fn format_share_agent(out: &mut String, share: &ShareDecl) {
    format_attrs_agent(out, &share.attrs);
    out.push_str(&format!("SHARE {} {}", share.name, share.ty.to_agent()));
    if let Some(default) = &share.default {
        out.push_str(&format!(" = {}", default.to_literal()));
    }
    out.push('\n');
}

fn format_capsule_fixture(fixture: &CapsuleFixture) -> String {
    let render = |values: &[crate::ast::CapsuleValue]| {
        values
            .iter()
            .map(|value| value.to_literal())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{} -> {}",
        render(&fixture.inputs),
        render(&fixture.outputs)
    )
}

fn capsule_header_suffix(capsule: &Capsule) -> String {
    let mut suffix = String::new();
    if let Some(env) = &capsule.env {
        suffix.push_str(&format!(" env {}", quoted_string(env)));
    }
    if !capsule.effects.is_empty() {
        suffix.push_str(&format!(" effects [{}]", capsule.effects.join(", ")));
    }
    suffix
}

fn format_capsule_human(out: &mut String, capsule: &Capsule) {
    format_attrs_human(out, &capsule.attrs, 0);
    out.push_str(&format!(
        "foreign {} {} version {}{}:\n",
        capsule.name,
        capsule.language,
        quoted_string(&capsule.version),
        capsule_header_suffix(capsule)
    ));
    for input in &capsule.inputs {
        out.push_str(&format!(
            "    input {}: {}\n",
            input.name,
            input.ty.to_human()
        ));
    }
    for output in &capsule.outputs {
        out.push_str(&format!(
            "    output {}: {}\n",
            output.name,
            output.ty.to_human()
        ));
    }
    for dep in &capsule.deps {
        out.push_str(&format!("    dep {}\n", quoted_string(&dep.source)));
    }
    for rule in &capsule.sandbox {
        out.push_str(&format!("    sandbox {}\n", quoted_string(rule)));
    }
    for binding in &capsule.bindings {
        out.push_str(&format!(
            "    bind {} {}\n",
            binding.name,
            binding.mode.as_str()
        ));
    }
    for fixture in &capsule.fixtures {
        out.push_str(&format!("    mock {}\n", format_capsule_fixture(fixture)));
    }
    out.push_str("    code:\n");
    for line in &capsule.code {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&format!("        {line}\n"));
        }
    }
}

fn format_capsule_agent(out: &mut String, capsule: &Capsule) {
    format_attrs_agent(out, &capsule.attrs);
    out.push_str(&format!(
        "CAP {} {} {}{}\n",
        capsule.name,
        capsule.language,
        quoted_string(&capsule.version),
        capsule_header_suffix(capsule)
    ));
    let binding = |out: &mut String, label: &str, bindings: &[CapsuleBinding]| {
        for b in bindings {
            out.push_str(&format!("  {label} {} {}\n", b.name, b.ty.to_agent()));
        }
    };
    binding(out, "IN", &capsule.inputs);
    binding(out, "OUT", &capsule.outputs);
    for dep in &capsule.deps {
        out.push_str(&format!("  DEP {}\n", quoted_string(&dep.source)));
    }
    for rule in &capsule.sandbox {
        out.push_str(&format!("  SBX {}\n", quoted_string(rule)));
    }
    for binding in &capsule.bindings {
        out.push_str(&format!(
            "  BIND {} {}\n",
            binding.name,
            binding.mode.as_str()
        ));
    }
    for fixture in &capsule.fixtures {
        out.push_str(&format!("  MOCK {}\n", format_capsule_fixture(fixture)));
    }
    out.push_str(&format!("  CODE {}\n", capsule.code.len()));
    for line in &capsule.code {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("ENDCAP\n");
}

fn format_attrs_agent(out: &mut String, attrs: &[Attribute]) {
    for attr in attrs {
        out.push_str("A ");
        out.push_str(&format_attr_body(attr, false));
        out.push('\n');
    }
}

fn format_attr_body(attr: &Attribute, human_spacing: bool) -> String {
    if attr.args.is_empty() {
        attr.name.clone()
    } else {
        let separator = if human_spacing { ", " } else { "," };
        format!("{}({})", attr.name, attr.args.join(separator))
    }
}

fn format_effects_agent(effects: &[String]) -> String {
    if effects.is_empty() {
        String::new()
    } else {
        format!(" effects[{}]", effects.join(","))
    }
}

fn format_stmts_agent(out: &mut String, body: &[Stmt], level: usize) {
    let pad = "  ".repeat(level);
    for stmt in body {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                if let Some(ty) = ty {
                    out.push_str(&format!(
                        "{pad}LET {name}: {} = {}\n",
                        ty.to_agent(),
                        format_expr(expr)
                    ));
                } else {
                    out.push_str(&format!("{pad}LET {name} = {}\n", format_expr(expr)));
                }
            }
            Stmt::Return(Some(expr)) => out.push_str(&format!("{pad}R {}\n", format_expr(expr))),
            Stmt::Return(None) => out.push_str(&format!("{pad}R\n")),
            Stmt::Break => out.push_str(&format!("{pad}BR\n")),
            Stmt::Continue => out.push_str(&format!("{pad}CN\n")),
            Stmt::Require(expr) => out.push_str(&format!("{pad}REQ {}\n", format_expr(expr))),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                out.push_str(&format!("{pad}IF {} {{\n", format_expr(cond)));
                format_stmts_agent(out, then_body, level + 1);
                out.push_str(&format!("{pad}}}\n"));
                // Dense agent keywords: EL = else, EI = else if.
                let mut chain: &[Stmt] = else_body;
                loop {
                    if chain.is_empty() {
                        break;
                    }
                    if let [Stmt::If {
                        cond,
                        then_body,
                        else_body,
                    }] = chain
                    {
                        out.push_str(&format!("{pad}EI {} {{\n", format_expr(cond)));
                        format_stmts_agent(out, then_body, level + 1);
                        out.push_str(&format!("{pad}}}\n"));
                        chain = else_body;
                        continue;
                    }
                    out.push_str(&format!("{pad}EL {{\n"));
                    format_stmts_agent(out, chain, level + 1);
                    out.push_str(&format!("{pad}}}\n"));
                    break;
                }
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                out.push_str(&format!(
                    "{pad}FOR {var} {} {} {{\n",
                    format_expr(start),
                    format_expr(end)
                ));
                format_stmts_agent(out, body, level + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::While { cond, body } => {
                // Dense agent keyword: WH = while.
                out.push_str(&format!("{pad}WH {} {{\n", format_expr(cond)));
                format_stmts_agent(out, body, level + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::Match { scrutinee, arms } => {
                // Dense agent keyword: MT = match; each arm is `<pat> => { … }`.
                out.push_str(&format!("{pad}MT {} {{\n", format_expr(scrutinee)));
                let arm_pad = "  ".repeat(level + 1);
                for arm in arms {
                    out.push_str(&format!(
                        "{arm_pad}{} => {{\n",
                        crate::ast::pattern_to_string(&arm.pattern)
                    ));
                    format_stmts_agent(out, &arm.body, level + 2);
                    out.push_str(&format!("{arm_pad}}}\n"));
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::Assign { target, expr } => {
                out.push_str(&format!(
                    "{pad}SET {} = {}\n",
                    format_expr(target),
                    format_expr(expr)
                ));
            }
            Stmt::Expr(expr) => out.push_str(&format!("{pad}{}\n", format_expr(expr))),
            Stmt::Asm(asm) => format_asm_agent(out, asm, level),
        }
    }
}

fn format_asm_agent(out: &mut String, asm: &AsmBlock, level: usize) {
    let pad = "  ".repeat(level);
    let inner = "  ".repeat(level + 1);
    out.push_str(&format!("{pad}ASM {} {{\n", asm.target));
    for input in &asm.inputs {
        out.push_str(&format!("{inner}IN {} {}\n", input.register, input.value));
    }
    for output in &asm.outputs {
        out.push_str(&format!(
            "{inner}OUT {} {}\n",
            output.register, output.value
        ));
    }
    for clobber in &asm.clobbers {
        out.push_str(&format!("{inner}CLOBBER {clobber}\n"));
    }
    for op in &asm.ops {
        out.push_str(&format!("{inner}OP {}\n", op.join(" ")));
    }
    out.push_str(&format!("{pad}}}\n"));
}

pub fn format_expr(expr: &Expr) -> String {
    format_expr_prec(expr, 0)
}

fn format_expr_prec(expr: &Expr, parent_prec: u8) -> String {
    match expr {
        Expr::Int(value) => value.to_string(),
        Expr::Float(value) => {
            let mut text = value.to_string();
            if !text.contains('.') {
                text.push_str(".0");
            }
            text
        }
        Expr::Bool(value) => value.to_string(),
        Expr::String(value) => quoted_string(value),
        Expr::Var(name) => name.clone(),
        Expr::TraitConst {
            trait_name,
            name,
            for_type,
        } => format!("{trait_name}::{name}[{}]", for_type.to_agent()),
        // A closure (lang.4). One rendering on both syntax surfaces: brackets
        // and the pipe form are already dense, and parse_expr is shared.
        Expr::Lambda { params, body } => format!(
            "|{}| {}",
            params
                .iter()
                .map(|param| format!("{}: {}", param.name, param.ty.to_human()))
                .collect::<Vec<_>>()
                .join(", "),
            format_expr_prec(body, 0)
        ),
        Expr::Unary { op, expr } => format!("{}{}", op, format_expr_prec(expr, 10)),
        Expr::Binary { op, left, right } => {
            let prec = expr_precedence(expr);
            let text = format!(
                "{} {} {}",
                format_expr_prec(left, prec),
                op,
                format_expr_prec(right, prec + 1)
            );
            if prec < parent_prec {
                format!("({text})")
            } else {
                text
            }
        }
        Expr::Call { name, args } => {
            let args = args.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        Expr::Cast { ty, expr } => format!("{}({})", ty.to_agent(), format_expr(expr)),
        Expr::Field { base, field } => format!("{}.{}", format_expr_prec(base, 11), field),
        Expr::Index { base, index } => {
            format!("{}[{}]", format_expr_prec(base, 11), format_expr(index))
        }
    }
}

fn expr_precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::Binary { op, .. } => match op.as_str() {
            "||" => 1,
            "&&" => 2,
            "==" | "!=" => 3,
            "<" | ">" | "<=" | ">=" => 4,
            "|" => 5,
            "&" => 6,
            "<<" | ">>" => 7,
            "+" | "-" => 8,
            "*" | "/" | "%" => 9,
            _ => 0,
        },
        Expr::Unary { .. } => 10,
        Expr::Call { .. } | Expr::Cast { .. } | Expr::Field { .. } | Expr::Index { .. } => 11,
        _ => 12,
    }
}

fn quoted_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
