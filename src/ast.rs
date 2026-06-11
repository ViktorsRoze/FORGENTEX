use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Struct(StructDecl),
    Function(FunctionDecl),
    Test(TestDecl),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructDecl {
    pub id: String,
    pub name: String,
    pub attrs: Vec<Attribute>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub attrs: Vec<Attribute>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionDecl {
    pub id: String,
    pub name: String,
    pub unsafe_fn: bool,
    pub params: Vec<Param>,
    pub ret: Type,
    pub effects: Vec<String>,
    pub attrs: Vec<Attribute>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Bool,
    Primitive(String),
    Named(String),
    Generic { name: String, args: Vec<Type> },
    Array { elem: Box<Type>, len: usize },
    Slice { mutable: bool, elem: Box<Type> },
    Pointer { mutable: bool, elem: Box<Type> },
    Void,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<Type>,
        expr: Expr,
    },
    Return(Option<Expr>),
    Require(Expr),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
    },
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
    Assign {
        target: Expr,
        expr: Expr,
    },
    Expr(Expr),
    Asm(AsmBlock),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Var(String),
    Unary {
        op: String,
        expr: Box<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Cast {
        ty: Type,
        expr: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        field: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AsmBlock {
    pub target: String,
    pub inputs: Vec<AsmBinding>,
    pub outputs: Vec<AsmBinding>,
    pub clobbers: Vec<String>,
    pub ops: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AsmBinding {
    pub register: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestDecl {
    pub id: String,
    pub name: String,
    pub xfail: bool,
    pub expectations: Vec<TestExpectation>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TestExpectation {
    Expr { expr: Expr },
    Diagnostic { code: String },
}

impl Program {
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    pub fn functions(&self) -> impl Iterator<Item = &FunctionDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::Function(function) => Some(function),
            _ => None,
        })
    }

    pub fn structs(&self) -> impl Iterator<Item = &StructDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::Struct(decl) => Some(decl),
            _ => None,
        })
    }

    pub fn find_function(&self, name: &str) -> Option<&FunctionDecl> {
        self.functions().find(|function| function.name == name)
    }

    pub fn find_function_mut(&mut self, name: &str) -> Option<&mut FunctionDecl> {
        self.items.iter_mut().find_map(|item| match item {
            Item::Function(function) if function.name == name => Some(function),
            _ => None,
        })
    }

    pub fn find_struct_mut(&mut self, name: &str) -> Option<&mut StructDecl> {
        self.items.iter_mut().find_map(|item| match item {
            Item::Struct(decl) if decl.name == name => Some(decl),
            _ => None,
        })
    }

    pub fn tests(&self) -> impl Iterator<Item = &TestDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::Test(test) => Some(test),
            _ => None,
        })
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n  \"items\": [\n");
        for (idx, item) in self.items.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            out.push_str(&item_to_json(item, 4));
        }
        out.push_str("\n  ]\n}");
        out
    }

    pub fn to_canonical_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", "\"forgentex.ast.v1\"", true);
        out.push_str("  \"items\": [");
        if !self.items.is_empty() {
            out.push('\n');
        }
        for (idx, item) in self.items.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            item_to_canonical_json(&mut out, item, 2);
        }
        if !self.items.is_empty() {
            out.push('\n');
        }
        out.push_str("  ]\n}");
        out
    }

    pub fn canonical_eq(&self, other: &Program) -> bool {
        self.to_canonical_json() == other.to_canonical_json()
    }
}

impl StructDecl {
    pub fn new(name: String, fields: Vec<Field>) -> Self {
        let id = format!("@struct.{name}");
        Self {
            id,
            name,
            attrs: Vec::new(),
            fields,
        }
    }

    pub fn with_attrs(name: String, attrs: Vec<Attribute>, fields: Vec<Field>) -> Self {
        let id = format!("@struct.{name}");
        Self {
            id,
            name,
            attrs,
            fields,
        }
    }
}

impl FunctionDecl {
    pub fn new(
        name: String,
        unsafe_fn: bool,
        params: Vec<Param>,
        ret: Type,
        body: Vec<Stmt>,
    ) -> Self {
        let id = format!("@function.{name}");
        Self {
            id,
            name,
            unsafe_fn,
            params,
            ret,
            effects: Vec::new(),
            attrs: Vec::new(),
            body,
        }
    }

    pub fn has_asm(&self) -> bool {
        body_has_asm(&self.body)
    }
}

impl TestDecl {
    pub fn new(name: String, xfail: bool, expectations: Vec<TestExpectation>) -> Self {
        let id = format!("@test.{}", node_safe_name(&name));
        Self {
            id,
            name,
            xfail,
            expectations,
        }
    }
}

impl Type {
    pub fn parse(raw: &str) -> Self {
        let compact = raw.trim().replace(' ', "");
        if compact.is_empty() {
            return Type::Unknown;
        }
        if compact == "void" {
            return Type::Void;
        }
        if compact == "bool" {
            return Type::Bool;
        }
        if let Some(array) = parse_array_type(&compact) {
            return array;
        }
        if let Some(inner) = compact.strip_prefix("&mut[") {
            if let Some(inner) = inner.strip_suffix(']') {
                return Type::Slice {
                    mutable: true,
                    elem: Box::new(Type::parse(inner)),
                };
            }
        }
        if let Some(inner) = compact.strip_prefix("&[") {
            if let Some(inner) = inner.strip_suffix(']') {
                return Type::Slice {
                    mutable: false,
                    elem: Box::new(Type::parse(inner)),
                };
            }
        }
        if let Some(inner) = compact.strip_prefix("*mut") {
            return Type::Pointer {
                mutable: true,
                elem: Box::new(Type::parse(inner)),
            };
        }
        if let Some(inner) = compact.strip_prefix("*const") {
            return Type::Pointer {
                mutable: false,
                elem: Box::new(Type::parse(inner)),
            };
        }
        if let Some(generic) = parse_generic_type(&compact) {
            return generic;
        }
        if is_primitive_name(&compact) {
            Type::Primitive(compact)
        } else {
            Type::Named(compact)
        }
    }

    pub fn to_human(&self) -> String {
        match self {
            Type::Bool => "bool".to_string(),
            Type::Primitive(name) | Type::Named(name) => name.clone(),
            Type::Generic { name, args } => {
                let args = args
                    .iter()
                    .map(Type::to_human)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}[{args}]")
            }
            Type::Array { elem, len } => format!("[{}; {len}]", elem.to_human()),
            Type::Slice { mutable, elem } => {
                if *mutable {
                    format!("&mut [{}]", elem.to_human())
                } else {
                    format!("&[{}]", elem.to_human())
                }
            }
            Type::Pointer { mutable, elem } => {
                if *mutable {
                    format!("*mut {}", elem.to_human())
                } else {
                    format!("*const {}", elem.to_human())
                }
            }
            Type::Void => "void".to_string(),
            Type::Unknown => "unknown".to_string(),
        }
    }

    pub fn to_agent(&self) -> String {
        match self {
            Type::Bool => "bool".to_string(),
            Type::Primitive(name) | Type::Named(name) => name.clone(),
            Type::Generic { name, args } => {
                let args = args
                    .iter()
                    .map(Type::to_agent)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{name}[{args}]")
            }
            Type::Array { elem, len } => format!("[{};{len}]", elem.to_agent()),
            Type::Slice { mutable, elem } => {
                if *mutable {
                    format!("&mut[{}]", elem.to_agent())
                } else {
                    format!("&[{}]", elem.to_agent())
                }
            }
            Type::Pointer { mutable, elem } => {
                if *mutable {
                    format!("*mut{}", elem.to_agent())
                } else {
                    format!("*const{}", elem.to_agent())
                }
            }
            Type::Void => "void".to_string(),
            Type::Unknown => "unknown".to_string(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Primitive(name) if is_numeric_name(name))
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Primitive(name) if is_integer_name(name))
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::Primitive(name) if name == "f32" || name == "f64")
    }

    pub fn is_literal(&self) -> bool {
        matches!(self, Type::Primitive(name) if name == "$int" || name == "$float")
    }

    pub fn element_type(&self) -> Option<Type> {
        match self {
            Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
                Some((**elem).clone())
            }
            _ => None,
        }
    }

    pub fn is_mutable_slice(&self) -> bool {
        matches!(self, Type::Slice { mutable: true, .. })
    }
}

fn parse_array_type(compact: &str) -> Option<Type> {
    let inner = compact.strip_prefix('[')?.strip_suffix(']')?;
    let semicolon = find_top_level_char(inner, ';')?;
    let elem = inner[..semicolon].trim();
    let len = inner[semicolon + 1..].trim().parse::<usize>().ok()?;
    Some(Type::Array {
        elem: Box::new(Type::parse(elem)),
        len,
    })
}

fn parse_generic_type(compact: &str) -> Option<Type> {
    let close = compact.strip_suffix(']')?;
    let open = close.find('[')?;
    let name = close[..open].trim();
    if name.is_empty() {
        return None;
    }
    let args = split_type_args(&close[open + 1..])
        .into_iter()
        .filter(|arg| !arg.trim().is_empty())
        .map(Type::parse)
        .collect::<Vec<_>>();
    if args.is_empty() {
        return None;
    }
    Some(Type::Generic {
        name: name.to_string(),
        args,
    })
}

fn find_top_level_char(value: &str, needle: char) -> Option<usize> {
    let mut bracket = 0i32;
    let mut paren = 0i32;
    for (index, ch) in value.char_indices() {
        match ch {
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '(' => paren += 1,
            ')' => paren -= 1,
            _ if ch == needle && bracket == 0 && paren == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn split_type_args(value: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0;
    let mut bracket = 0i32;
    let mut paren = 0i32;
    for (index, ch) in value.char_indices() {
        match ch {
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '(' => paren += 1,
            ')' => paren -= 1,
            ',' if bracket == 0 && paren == 0 => {
                args.push(&value[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    args.push(&value[start..]);
    args
}

pub fn is_primitive_name(name: &str) -> bool {
    matches!(
        name,
        "u8" | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
            | "str"
    )
}

pub fn is_numeric_name(name: &str) -> bool {
    is_integer_name(name) || matches!(name, "f32" | "f64" | "$int" | "$float")
}

pub fn is_integer_name(name: &str) -> bool {
    matches!(
        name,
        "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" | "$int"
    )
}

pub fn body_has_asm(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Asm(_) => true,
        Stmt::If { then_body, .. }
        | Stmt::For {
            body: then_body, ..
        } => body_has_asm(then_body),
        _ => false,
    })
}

fn function_target_metadata(function: &FunctionDecl) -> Vec<String> {
    let mut labels = BTreeSet::new();
    for effect in &function.effects {
        if effect.starts_with("cpu.") || effect.starts_with("target.") {
            labels.insert(effect.clone());
        }
    }
    collect_asm_target_metadata(&function.body, &mut labels);
    labels.into_iter().collect()
}

fn collect_asm_target_metadata(body: &[Stmt], labels: &mut BTreeSet<String>) {
    for stmt in body {
        match stmt {
            Stmt::Asm(asm) => {
                labels.insert(format!("target.{}", asm.target));
            }
            Stmt::If { then_body, .. }
            | Stmt::For {
                body: then_body, ..
            } => collect_asm_target_metadata(then_body, labels),
            Stmt::Let { .. }
            | Stmt::Return(_)
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_) => {}
        }
    }
}

fn item_to_json(item: &Item, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match item {
        Item::Struct(decl) => {
            let mut out = format!(
                "{pad}{{\n{pad}  \"kind\": \"struct\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\",\n{pad}  \"fields\": [",
                escape(&decl.id),
                escape(&decl.name)
            );
            for (idx, field) in decl.fields.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\",\"annotations\":{}}}",
                    escape(&field.name),
                    escape(&field.ty.to_agent()),
                    attributes_to_json_inline(&field.attrs)
                ));
            }
            out.push_str(&format!("]\n{pad}}}"));
            out
        }
        Item::Function(function) => {
            let mut out = format!(
                "{pad}{{\n{pad}  \"kind\": \"function\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\",\n{pad}  \"unsafe\": {},\n{pad}  \"return\": \"{}\",\n{pad}  \"params\": [",
                escape(&function.id),
                escape(&function.name),
                function.unsafe_fn,
                escape(&function.ret.to_agent())
            );
            for (idx, param) in function.params.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                    escape(&param.name),
                    escape(&param.ty.to_agent())
                ));
            }
            out.push_str(&format!(
                "],\n{pad}  \"effects\": [{}],\n{pad}  \"target_metadata\": [{}],\n{pad}  \"body_len\": {},\n{pad}  \"has_asm\": {}\n{pad}}}",
                function
                    .effects
                    .iter()
                    .map(|effect| quoted(effect))
                    .collect::<Vec<_>>()
                    .join(", "),
                function_target_metadata(function)
                    .iter()
                    .map(|label| quoted(label))
                    .collect::<Vec<_>>()
                    .join(", "),
                function.body.len(),
                function.has_asm()
            ));
            out
        }
        Item::Test(test) => {
            let mut out = format!(
                "{pad}{{\n{pad}  \"kind\": \"test\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\",\n{pad}  \"xfail\": {},\n{pad}  \"expectations\": [",
                escape(&test.id),
                escape(&test.name),
                test.xfail
            );
            for (idx, expectation) in test.expectations.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                match expectation {
                    TestExpectation::Expr { expr } => out.push_str(&format!(
                        "{{\"kind\":\"expr\",\"expr\":\"{}\"}}",
                        escape(&expr_to_inline_text(expr))
                    )),
                    TestExpectation::Diagnostic { code } => out.push_str(&format!(
                        "{{\"kind\":\"diagnostic\",\"code\":\"{}\"}}",
                        escape(code)
                    )),
                }
            }
            out.push_str(&format!("]\n{pad}}}"));
            out
        }
    }
}

fn item_to_canonical_json(out: &mut String, item: &Item, indent: usize) {
    match item {
        Item::Struct(decl) => struct_to_canonical_json(out, decl, indent),
        Item::Function(function) => function_to_canonical_json(out, function, indent),
        Item::Test(test) => test_to_canonical_json(out, test, indent),
    }
}

fn struct_to_canonical_json(out: &mut String, decl: &StructDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("struct"), true);
    push_prop(out, indent + 1, "id", &quoted(&decl.id), true);
    push_prop(out, indent + 1, "name", &quoted(&decl.name), true);
    push_array_block_start(out, indent + 1, "fields");
    for (idx, field) in decl.fields.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        field_to_canonical_json(out, decl, field, indent + 2);
    }
    out.push('\n');
    push_line(out, indent + 1, "],");
    push_attributes(out, indent + 1, "annotations", &decl.attrs, false);
    push_line(out, indent, "}");
}

fn field_to_canonical_json(out: &mut String, decl: &StructDecl, field: &Field, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("field"), true);
    push_prop(
        out,
        indent + 1,
        "id",
        &quoted(&format!("{}.field.{}", decl.id, field.name)),
        true,
    );
    push_prop(out, indent + 1, "name", &quoted(&field.name), true);
    push_prop(out, indent + 1, "type", &quoted(&field.ty.to_agent()), true);
    push_attributes(out, indent + 1, "annotations", &field.attrs, false);
    push_line(out, indent, "}");
}

fn function_to_canonical_json(out: &mut String, function: &FunctionDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("function"), true);
    push_prop(out, indent + 1, "id", &quoted(&function.id), true);
    push_prop(out, indent + 1, "name", &quoted(&function.name), true);
    push_prop(
        out,
        indent + 1,
        "unsafe",
        if function.unsafe_fn { "true" } else { "false" },
        true,
    );
    if function.unsafe_fn {
        push_prop(
            out,
            indent + 1,
            "unsafe_region_id",
            &quoted(&format!("{}.unsafe", function.id)),
            true,
        );
    }
    push_prop(
        out,
        indent + 1,
        "return",
        &quoted(&function.ret.to_agent()),
        true,
    );
    push_array_start(out, indent + 1, "effects");
    for (idx, effect) in function.effects.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(effect));
    }
    out.push_str("],\n");
    push_array_start(out, indent + 1, "target_metadata");
    for (idx, label) in function_target_metadata(function).iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(label));
    }
    out.push_str("],\n");
    push_array_block_start(out, indent + 1, "params");
    for (idx, param) in function.params.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        param_to_canonical_json(out, function, param, indent + 2);
    }
    out.push('\n');
    push_line(out, indent + 1, "],");
    push_array_block_start(out, indent + 1, "body");
    for (idx, stmt) in function.body.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        stmt_to_canonical_json(
            out,
            stmt,
            &format!("{}.stmt.{idx}", function.id),
            indent + 2,
        );
    }
    out.push('\n');
    push_line(out, indent + 1, "],");
    push_attributes(out, indent + 1, "annotations", &function.attrs, false);
    push_line(out, indent, "}");
}

fn test_to_canonical_json(out: &mut String, test: &TestDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("test"), true);
    push_prop(out, indent + 1, "id", &quoted(&test.id), true);
    push_prop(out, indent + 1, "name", &quoted(&test.name), true);
    push_prop(
        out,
        indent + 1,
        "xfail",
        if test.xfail { "true" } else { "false" },
        true,
    );
    push_array_block_start(out, indent + 1, "expectations");
    for (idx, expectation) in test.expectations.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        test_expectation_to_canonical_json(
            out,
            expectation,
            &format!("{}.expect.{idx}", test.id),
            indent + 2,
        );
    }
    out.push('\n');
    push_line(out, indent + 1, "]");
    push_line(out, indent, "}");
}

fn test_expectation_to_canonical_json(
    out: &mut String,
    expectation: &TestExpectation,
    id: &str,
    indent: usize,
) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "id", &quoted(id), true);
    match expectation {
        TestExpectation::Expr { expr } => {
            push_prop(out, indent + 1, "kind", &quoted("expect"), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        TestExpectation::Diagnostic { code } => {
            push_prop(out, indent + 1, "kind", &quoted("expect_diagnostic"), true);
            push_prop(out, indent + 1, "code", &quoted(code), false);
        }
    }
    push_line(out, indent, "}");
}

fn param_to_canonical_json(
    out: &mut String,
    function: &FunctionDecl,
    param: &Param,
    indent: usize,
) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("param"), true);
    push_prop(
        out,
        indent + 1,
        "id",
        &quoted(&format!("{}.param.{}", function.id, param.name)),
        true,
    );
    push_prop(out, indent + 1, "name", &quoted(&param.name), true);
    push_prop(
        out,
        indent + 1,
        "type",
        &quoted(&param.ty.to_agent()),
        false,
    );
    push_line(out, indent, "}");
}

fn stmt_to_canonical_json(out: &mut String, stmt: &Stmt, id: &str, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "id", &quoted(id), true);
    match stmt {
        Stmt::Let { name, ty, expr } => {
            push_prop(out, indent + 1, "kind", &quoted("let"), true);
            push_prop(out, indent + 1, "name", &quoted(name), true);
            let ty = ty
                .as_ref()
                .map(Type::to_agent)
                .unwrap_or_else(|| "inferred".to_string());
            push_prop(out, indent + 1, "type", &quoted(&ty), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Stmt::Return(Some(expr)) => {
            push_prop(out, indent + 1, "kind", &quoted("return"), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Stmt::Return(None) => {
            push_prop(out, indent + 1, "kind", &quoted("return"), true);
            push_prop(out, indent + 1, "expr", "null", false);
        }
        Stmt::Require(expr) => {
            push_prop(out, indent + 1, "kind", &quoted("require"), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Stmt::If { cond, then_body } => {
            push_prop(out, indent + 1, "kind", &quoted("if"), true);
            push_expr_prop(out, indent + 1, "cond", cond, &format!("{id}.cond"), true);
            push_array_block_start(out, indent + 1, "then_body");
            for (idx, stmt) in then_body.iter().enumerate() {
                if idx > 0 {
                    out.push_str(",\n");
                }
                stmt_to_canonical_json(out, stmt, &format!("{id}.then.{idx}"), indent + 2);
            }
            out.push('\n');
            push_line(out, indent + 1, "]");
        }
        Stmt::For {
            var,
            start,
            end,
            body,
        } => {
            push_prop(out, indent + 1, "kind", &quoted("for"), true);
            push_prop(
                out,
                indent + 1,
                "loop_id",
                &quoted(&format!("{id}.loop")),
                true,
            );
            push_prop(out, indent + 1, "var", &quoted(var), true);
            push_expr_prop(
                out,
                indent + 1,
                "start",
                start,
                &format!("{id}.start"),
                true,
            );
            push_expr_prop(out, indent + 1, "end", end, &format!("{id}.end"), true);
            push_array_block_start(out, indent + 1, "body");
            for (idx, stmt) in body.iter().enumerate() {
                if idx > 0 {
                    out.push_str(",\n");
                }
                stmt_to_canonical_json(out, stmt, &format!("{id}.body.{idx}"), indent + 2);
            }
            out.push('\n');
            push_line(out, indent + 1, "]");
        }
        Stmt::Assign { target, expr } => {
            push_prop(out, indent + 1, "kind", &quoted("assign"), true);
            push_expr_prop(
                out,
                indent + 1,
                "target",
                target,
                &format!("{id}.target"),
                true,
            );
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Stmt::Expr(expr) => {
            push_prop(out, indent + 1, "kind", &quoted("expr"), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Stmt::Asm(asm) => {
            push_prop(out, indent + 1, "kind", &quoted("asm"), true);
            push_prop(
                out,
                indent + 1,
                "asm_id",
                &quoted(&format!("{id}.asm")),
                true,
            );
            asm_to_canonical_json(out, asm, indent + 1, false);
        }
    }
    push_line(out, indent, "}");
}

fn asm_to_canonical_json(out: &mut String, asm: &AsmBlock, indent: usize, trailing: bool) {
    push_line(out, indent, "\"asm\": {");
    push_prop(out, indent + 1, "target", &quoted(&asm.target), true);
    push_bindings(out, indent + 1, "inputs", &asm.inputs, true);
    push_bindings(out, indent + 1, "outputs", &asm.outputs, true);
    push_string_array(out, indent + 1, "clobbers", &asm.clobbers, true);
    push_ops(out, indent + 1, &asm.ops, false);
    if trailing {
        push_line(out, indent, "},");
    } else {
        push_line(out, indent, "}");
    }
}

fn expr_to_canonical_json(out: &mut String, expr: &Expr, id: &str, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "id", &quoted(id), true);
    match expr {
        Expr::Int(value) => {
            push_prop(out, indent + 1, "kind", &quoted("int"), true);
            push_prop(out, indent + 1, "value", &value.to_string(), false);
        }
        Expr::Float(value) => {
            push_prop(out, indent + 1, "kind", &quoted("float"), true);
            push_prop(out, indent + 1, "value", &canonical_float(*value), false);
        }
        Expr::Bool(value) => {
            push_prop(out, indent + 1, "kind", &quoted("bool"), true);
            push_prop(
                out,
                indent + 1,
                "value",
                if *value { "true" } else { "false" },
                false,
            );
        }
        Expr::String(value) => {
            push_prop(out, indent + 1, "kind", &quoted("string"), true);
            push_prop(out, indent + 1, "value", &quoted(value), false);
        }
        Expr::Var(name) => {
            push_prop(out, indent + 1, "kind", &quoted("var"), true);
            push_prop(out, indent + 1, "name", &quoted(name), false);
        }
        Expr::Unary { op, expr } => {
            push_prop(out, indent + 1, "kind", &quoted("unary"), true);
            push_prop(out, indent + 1, "op", &quoted(op), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Expr::Binary { op, left, right } => {
            push_prop(out, indent + 1, "kind", &quoted("binary"), true);
            push_prop(out, indent + 1, "op", &quoted(op), true);
            push_expr_prop(out, indent + 1, "left", left, &format!("{id}.left"), true);
            push_expr_prop(
                out,
                indent + 1,
                "right",
                right,
                &format!("{id}.right"),
                false,
            );
        }
        Expr::Call { name, args } => {
            push_prop(out, indent + 1, "kind", &quoted("call"), true);
            push_prop(out, indent + 1, "name", &quoted(name), true);
            push_array_block_start(out, indent + 1, "args");
            for (idx, arg) in args.iter().enumerate() {
                if idx > 0 {
                    out.push_str(",\n");
                }
                expr_to_canonical_json(out, arg, &format!("{id}.arg.{idx}"), indent + 2);
            }
            out.push('\n');
            push_line(out, indent + 1, "]");
        }
        Expr::Cast { ty, expr } => {
            push_prop(out, indent + 1, "kind", &quoted("cast"), true);
            push_prop(out, indent + 1, "type", &quoted(&ty.to_agent()), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Expr::Field { base, field } => {
            push_prop(out, indent + 1, "kind", &quoted("field"), true);
            push_prop(out, indent + 1, "field", &quoted(field), true);
            push_expr_prop(out, indent + 1, "base", base, &format!("{id}.base"), false);
        }
        Expr::Index { base, index } => {
            push_prop(out, indent + 1, "kind", &quoted("index"), true);
            push_expr_prop(out, indent + 1, "base", base, &format!("{id}.base"), true);
            push_expr_prop(
                out,
                indent + 1,
                "index",
                index,
                &format!("{id}.index"),
                false,
            );
        }
    }
    push_line(out, indent, "}");
}

fn push_expr_prop(
    out: &mut String,
    indent: usize,
    key: &str,
    expr: &Expr,
    id: &str,
    trailing: bool,
) {
    push_line(out, indent, &format!("\"{key}\": "));
    let suffix_start = out.len().saturating_sub(1);
    out.truncate(suffix_start);
    expr_to_canonical_json(out, expr, id, indent);
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_bindings(
    out: &mut String,
    indent: usize,
    key: &str,
    bindings: &[AsmBinding],
    trailing: bool,
) {
    push_array_start(out, indent, key);
    for (idx, binding) in bindings.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"register\":\"{}\",\"value\":\"{}\"}}",
            escape(&binding.register),
            escape(&binding.value)
        ));
    }
    if trailing {
        out.push_str("],\n");
    } else {
        out.push(']');
    }
}

fn push_string_array(
    out: &mut String,
    indent: usize,
    key: &str,
    values: &[String],
    trailing: bool,
) {
    push_array_start(out, indent, key);
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    if trailing {
        out.push_str("],\n");
    } else {
        out.push(']');
    }
}

fn push_ops(out: &mut String, indent: usize, ops: &[Vec<String>], trailing: bool) {
    push_array_start(out, indent, "ops");
    for (idx, op) in ops.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push('[');
        for (part_idx, part) in op.iter().enumerate() {
            if part_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&quoted(part));
        }
        out.push(']');
    }
    if trailing {
        out.push_str("],\n");
    } else {
        out.push_str("]\n");
    }
}

fn push_empty_array(out: &mut String, indent: usize, key: &str, trailing: bool) {
    push_prop(out, indent, key, "[]", trailing);
}

fn push_attributes(
    out: &mut String,
    indent: usize,
    key: &str,
    attrs: &[Attribute],
    trailing: bool,
) {
    if attrs.is_empty() {
        push_empty_array(out, indent, key, trailing);
        return;
    }
    push_array_block_start(out, indent, key);
    for (idx, attr) in attrs.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_line(out, indent + 1, "{");
        push_prop(out, indent + 2, "kind", &quoted("attribute"), true);
        push_prop(out, indent + 2, "name", &quoted(&attr.name), true);
        push_array_start(out, indent + 2, "args");
        for (arg_idx, arg) in attr.args.iter().enumerate() {
            if arg_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&quoted(arg));
        }
        out.push_str("]\n");
        push_line(out, indent + 1, "}");
    }
    out.push('\n');
    push_indent(out, indent);
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn attributes_to_json_inline(attrs: &[Attribute]) -> String {
    if attrs.is_empty() {
        return "[]".to_string();
    }
    let values = attrs
        .iter()
        .map(|attr| {
            let args = attr.args.iter().map(|arg| quoted(arg)).collect::<Vec<_>>();
            format!(
                "{{\"kind\":\"attribute\",\"name\":{},\"args\":[{}]}}",
                quoted(&attr.name),
                args.join(",")
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn push_array_start(out: &mut String, indent: usize, key: &str) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": ["));
}

fn push_array_block_start(out: &mut String, indent: usize, key: &str) {
    push_array_start(out, indent, key);
    out.push('\n');
}

fn push_prop(out: &mut String, indent: usize, key: &str, value: &str, trailing: bool) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": {value}"));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_line(out: &mut String, indent: usize, line: &str) {
    push_indent(out, indent);
    out.push_str(line);
    out.push('\n');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape(value))
}

fn canonical_float(value: f64) -> String {
    let mut text = value.to_string();
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

fn expr_to_inline_text(expr: &Expr) -> String {
    match expr {
        Expr::Int(value) => value.to_string(),
        Expr::Float(value) => canonical_float(*value),
        Expr::Bool(value) => value.to_string(),
        Expr::String(value) => quoted(value),
        Expr::Var(name) => name.clone(),
        Expr::Unary { op, expr } => format!("{op}{}", expr_to_inline_text(expr)),
        Expr::Binary { op, left, right } => {
            format!(
                "{} {op} {}",
                expr_to_inline_text(left),
                expr_to_inline_text(right)
            )
        }
        Expr::Call { name, args } => {
            let args = args
                .iter()
                .map(expr_to_inline_text)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}({args})")
        }
        Expr::Cast { ty, expr } => format!("{}({})", ty.to_agent(), expr_to_inline_text(expr)),
        Expr::Field { base, field } => format!("{}.{}", expr_to_inline_text(base), field),
        Expr::Index { base, index } => {
            format!(
                "{}[{}]",
                expr_to_inline_text(base),
                expr_to_inline_text(index)
            )
        }
    }
}

fn node_safe_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

use crate::json::escape;
