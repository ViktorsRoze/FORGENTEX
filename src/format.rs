use crate::ast::{
    AsmBlock, Attribute, Expr, FunctionDecl, Item, Program, Stmt, StructDecl, TestDecl,
    TestExpectation,
};

pub fn format_human(program: &Program) -> String {
    let mut out = String::new();
    for (idx, item) in program.items.iter().enumerate() {
        if idx > 0 {
            out.push_str("\n\n");
        }
        match item {
            Item::Struct(decl) => format_struct_human(&mut out, decl),
            Item::Function(function) => format_function_human(&mut out, function),
            Item::Test(test) => format_test_human(&mut out, test),
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
            Item::Function(function) => format_function_agent(&mut out, function),
            Item::Test(test) => format_test_agent(&mut out, test),
        }
    }
    finish_document(out)
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
        "fn {}({}) -> {}{}:\n",
        function.name,
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
            Stmt::Require(expr) => {
                out.push_str(&format!("{pad}require {}\n", format_expr(expr)));
            }
            Stmt::If { cond, then_body } => {
                out.push_str(&format!("{pad}if {}:\n", format_expr(cond)));
                format_stmts_human(out, then_body, level + 1);
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
        "{}({}) -> {}{} {{\n",
        function.name,
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
            Stmt::Require(expr) => out.push_str(&format!("{pad}REQ {}\n", format_expr(expr))),
            Stmt::If { cond, then_body } => {
                out.push_str(&format!("{pad}IF {} {{\n", format_expr(cond)));
                format_stmts_agent(out, then_body, level + 1);
                out.push_str(&format!("{pad}}}\n"));
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
