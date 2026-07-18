use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    TraitImpl(TraitImplDecl),
    Function(FunctionDecl),
    Test(TestDecl),
    Capsule(Capsule),
    Share(ShareDecl),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitDecl {
    pub id: String,
    pub name: String,
    pub attrs: Vec<Attribute>,
    pub methods: Vec<TraitMethodDecl>,
    pub constants: Vec<TraitConstDecl>,
    pub associated_types: Vec<TraitTypeDecl>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitTypeDecl {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitConstDecl {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitMethodDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Type,
    pub default_function: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitImplDecl {
    pub id: String,
    pub trait_name: String,
    pub for_type: Type,
    pub type_params: Vec<String>,
    pub type_bounds: Vec<TypeParamBound>,
    pub attrs: Vec<Attribute>,
    pub methods: Vec<TraitMethodImpl>,
    pub constants: Vec<TraitConstImpl>,
    pub associated_types: Vec<TraitTypeImpl>,
}

impl TraitImplDecl {
    pub fn bounds_for(&self, type_param: &str) -> &[String] {
        self.type_bounds
            .iter()
            .find(|bound| bound.type_param == type_param)
            .map_or(&[], |bound| bound.traits.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitTypeImpl {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitConstImpl {
    pub name: String,
    pub value: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitMethodImpl {
    pub method: String,
    pub function: String,
}

/// A sum type. Each variant is data-less or carries a single optional payload
/// (`Circle(i32)`); matched with exhaustive `match` and binding patterns.
#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    pub id: String,
    pub name: String,
    pub attrs: Vec<Attribute>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    /// A single optional payload field (`Circle(i32)`); `None` is data-less.
    /// Multi-field variants are expressed by carrying a struct.
    pub payload: Option<Type>,
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
    /// Generic type parameters (lang.4), e.g. `fn first[T](v: Vec[T]) -> T`
    /// declares `["T"]`. Empty for monomorphic functions.
    pub type_params: Vec<String>,
    /// Static interface constraints for generic parameters (lang.4 slice 5),
    /// e.g. `T: Eq`. Kept separate so existing monomorphization continues to
    /// use plain parameter names.
    pub type_bounds: Vec<TypeParamBound>,
    pub unsafe_fn: bool,
    pub params: Vec<Param>,
    pub ret: Type,
    pub effects: Vec<String>,
    pub attrs: Vec<Attribute>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeParamBound {
    pub type_param: String,
    pub traits: Vec<String>,
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
        else_body: Vec<Stmt>,
    },
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Match {
        scrutinee: Expr,
        arms: Vec<MatchArm>,
    },
    Assign {
        target: Expr,
        expr: Expr,
    },
    Break,
    Continue,
    Expr(Expr),
    Asm(AsmBlock),
}

/// One arm of a `match`: a pattern and the body it guards.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
}

/// A `match` pattern: the wildcard `_`, or an enum variant path (`Color::Red`)
/// optionally binding its payload (`Shape::Circle(r)` binds `r`).
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Wildcard,
    Variant {
        path: String,
        binding: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Var(String),
    TraitConst {
        trait_name: String,
        name: String,
        for_type: Type,
    },
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
    /// An anonymous function (lang.4 closures): typed parameters and a single
    /// expression body. Free variables are captured by value at creation.
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
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

/// A foreign-language code capsule (v2.0). The capsule contract — language,
/// version, runtime environment, typed boundary, declared effects, pinned
/// dependencies, and sandbox policy — maps to the AST and is reviewable.
/// The `code` body is opaque foreign source carried verbatim as a payload
/// in both `.fgx` and `.fgai` views.
#[derive(Clone, Debug, PartialEq)]
pub struct Capsule {
    pub id: String,
    pub name: String,
    pub language: String,
    pub version: String,
    pub env: Option<String>,
    pub effects: Vec<String>,
    pub inputs: Vec<CapsuleBinding>,
    pub outputs: Vec<CapsuleBinding>,
    pub deps: Vec<CapsuleDep>,
    pub sandbox: Vec<String>,
    pub bindings: Vec<CapsuleShareBinding>,
    pub fixtures: Vec<CapsuleFixture>,
    pub code: Vec<String>,
    pub attrs: Vec<Attribute>,
}

/// A Forgentex-owned shared typed value (v2.2). Capsules bind it by name
/// and see it as a native value under snapshot-in / write-back-out
/// semantics; Forgentex remains the single source of truth.
#[derive(Clone, Debug, PartialEq)]
pub struct ShareDecl {
    pub id: String,
    pub name: String,
    pub ty: Type,
    pub default: Option<CapsuleValue>,
    pub attrs: Vec<Attribute>,
}

impl ShareDecl {
    pub fn new(name: String, ty: Type, default: Option<CapsuleValue>) -> Self {
        let id = format!("@share.{name}");
        Self {
            id,
            name,
            ty,
            default,
            attrs: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareMode {
    Read,
    Rw,
}

impl ShareMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ShareMode::Read => "read",
            ShareMode::Rw => "rw",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(ShareMode::Read),
            "rw" => Some(ShareMode::Rw),
            _ => None,
        }
    }

    pub fn is_writer(&self) -> bool {
        matches!(self, ShareMode::Rw)
    }
}

/// A capsule's binding of a shared name into its runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleShareBinding {
    pub name: String,
    pub mode: ShareMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleBinding {
    pub name: String,
    pub ty: Type,
}

/// A deterministic mock-execution fixture (v2.1): a tuple of input literal
/// values mapped to expected output literal values at the typed boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleFixture {
    pub inputs: Vec<CapsuleValue>,
    pub outputs: Vec<CapsuleValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CapsuleValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

impl CapsuleValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            CapsuleValue::Int(_) => "int",
            CapsuleValue::Float(_) => "float",
            CapsuleValue::Bool(_) => "bool",
            CapsuleValue::Str(_) => "str",
        }
    }

    /// A fixture literal is compatible with a boundary type when it is the
    /// same kind, or an integer literal filling a float-typed binding.
    pub fn matches_type(&self, ty: &Type) -> bool {
        match (self, ty) {
            (CapsuleValue::Bool(_), Type::Bool) => true,
            // An integer literal may fill an integer or float binding.
            (CapsuleValue::Int(_), Type::Primitive(name)) => {
                is_integer_primitive(name) || is_float_primitive(name)
            }
            (CapsuleValue::Float(_), Type::Primitive(name)) => is_float_primitive(name),
            (CapsuleValue::Str(_), Type::Primitive(name)) => name == "str",
            (CapsuleValue::Str(_), Type::Named(name)) => name == "str",
            _ => false,
        }
    }

    /// The canonical source literal form, chosen so it re-parses to the
    /// same value (floats always carry a decimal point).
    pub fn to_literal(&self) -> String {
        match self {
            CapsuleValue::Int(value) => value.to_string(),
            CapsuleValue::Float(value) => {
                let text = format!("{value}");
                if text.contains('.') || text.contains('e') || text.contains("inf") {
                    text
                } else {
                    format!("{text}.0")
                }
            }
            CapsuleValue::Bool(value) => value.to_string(),
            CapsuleValue::Str(value) => {
                format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
            }
        }
    }
}

pub fn is_integer_primitive(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize"
    )
}

pub fn is_float_primitive(name: &str) -> bool {
    matches!(name, "f32" | "f64")
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleDep {
    /// The raw dependency reference exactly as authored, for example
    /// `pypi:numpy==2.2.0#sha256:...` or `github:org/repo@<commit>`.
    pub source: String,
}

impl CapsuleDep {
    /// A dependency is considered pinned when it carries an exact version,
    /// a commit/revision, or a checksum/artifact hash. Floating references
    /// (bare names, `latest`, `*`, branch names) are not pinned.
    pub fn is_pinned(&self) -> bool {
        let source = &self.source;
        let has_hash = source.contains("#sha256:")
            || source.contains("#sha512:")
            || source.contains("#blake3:");
        let has_exact_version = source.contains("==");
        let has_commit = source
            .rsplit_once('@')
            .map(|(_, rev)| is_commit_like(rev))
            .unwrap_or(false);
        let floating = source.ends_with("latest")
            || source.ends_with('*')
            || source.contains("@latest")
            || source.contains("@main")
            || source.contains("@master");
        (has_hash || has_exact_version || has_commit) && !floating
    }
}

fn is_commit_like(rev: &str) -> bool {
    rev.len() >= 7 && rev.chars().all(|ch| ch.is_ascii_hexdigit())
}

impl Capsule {
    pub fn new(name: String, language: String, version: String) -> Self {
        let id = format!("@capsule.{name}");
        Self {
            id,
            name,
            language,
            version,
            env: None,
            effects: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            deps: Vec::new(),
            sandbox: Vec::new(),
            bindings: Vec::new(),
            fixtures: Vec::new(),
            code: Vec::new(),
            attrs: Vec::new(),
        }
    }

    /// High-risk capsule classes carry stricter review requirements
    /// (shell, native, GPU, and smart-contract code per the v2 roadmap).
    pub fn risk_class(&self) -> &'static str {
        match self.language.as_str() {
            "bash" | "powershell" | "sh" => "shell",
            "c" | "cpp" | "c++" | "rust" | "zig" => "native",
            "cuda" => "gpu",
            "solidity" | "vyper" => "smart_contract",
            _ => "managed",
        }
    }
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

    pub fn enums(&self) -> impl Iterator<Item = &EnumDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::Enum(decl) => Some(decl),
            _ => None,
        })
    }

    pub fn traits(&self) -> impl Iterator<Item = &TraitDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::Trait(decl) => Some(decl),
            _ => None,
        })
    }

    pub fn trait_impls(&self) -> impl Iterator<Item = &TraitImplDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::TraitImpl(decl) => Some(decl),
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

    pub fn capsules(&self) -> impl Iterator<Item = &Capsule> {
        self.items.iter().filter_map(|item| match item {
            Item::Capsule(capsule) => Some(capsule),
            _ => None,
        })
    }

    pub fn shares(&self) -> impl Iterator<Item = &ShareDecl> {
        self.items.iter().filter_map(|item| match item {
            Item::Share(share) => Some(share),
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
            type_params: Vec::new(),
            type_bounds: Vec::new(),
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

    pub fn bounds_for(&self, type_param: &str) -> &[String] {
        self.type_bounds
            .iter()
            .find(|bound| bound.type_param == type_param)
            .map_or(&[], |bound| bound.traits.as_slice())
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
            // A growable `Vec<T>` indexes to `T`.
            Type::Generic { name, args } if name == "Vec" => args.first().cloned(),
            _ => None,
        }
    }

    /// Decode a first-class function type. The pure form is `Fn[P..., R]`;
    /// `forall[T: Bound]` may appear first and `effects[E...]` may appear last.
    /// Both markers reuse generic type nodes and are excluded from the returned
    /// parameter/return slices.
    pub fn function_parts(&self) -> Option<(&[Type], &Type, &[Type])> {
        let Type::Generic { name, args } = self else {
            return None;
        };
        if name != "Fn" || args.is_empty() {
            return None;
        }
        let (signature, effects) = match args.last() {
            Some(Type::Generic {
                name: marker,
                args: effects,
            }) if marker == "effects" => (&args[..args.len() - 1], effects.as_slice()),
            _ => (args.as_slice(), &[][..]),
        };
        let signature = match signature.first() {
            Some(Type::Generic { name, .. }) if name == "forall" => &signature[1..],
            _ => signature,
        };
        let (ret, params) = signature.split_last()?;
        Some((params, ret, effects))
    }

    /// Return the higher-rank binders declared by a leading `forall[...]`
    /// marker. Each entry is `(parameter, bounds)`; malformed entries return
    /// `None` so the checker can diagnose the contract.
    pub fn function_binders(&self) -> Option<Vec<(String, Vec<String>)>> {
        let Type::Generic { name, args } = self else {
            return None;
        };
        if name != "Fn" {
            return None;
        }
        let Some(Type::Generic {
            name: marker,
            args: binders,
        }) = args.first()
        else {
            return Some(Vec::new());
        };
        if marker != "forall" {
            return Some(Vec::new());
        }
        binders
            .iter()
            .map(|binder| {
                let Type::Named(spec) = binder else {
                    return None;
                };
                let (name, bounds) = spec.split_once(':').map_or_else(
                    || (spec.as_str(), Vec::new()),
                    |(name, bounds)| {
                        (
                            name,
                            bounds
                                .split('+')
                                .filter(|bound| !bound.is_empty())
                                .map(str::to_string)
                                .collect(),
                        )
                    },
                );
                (!name.is_empty()).then(|| (name.to_string(), bounds))
            })
            .collect()
    }

    pub fn function_effect_names(&self) -> Option<Vec<String>> {
        let (_, _, effects) = self.function_parts()?;
        Some(effects.iter().map(Type::to_agent).collect())
    }

    pub fn function_type(params: Vec<Type>, ret: Type, effects: &[String]) -> Type {
        Self::generic_function_type(&[], &[], params, ret, effects)
    }

    pub fn generic_function_type(
        type_params: &[String],
        type_bounds: &[TypeParamBound],
        params: Vec<Type>,
        ret: Type,
        effects: &[String],
    ) -> Type {
        let mut signature = Vec::new();
        if !type_params.is_empty() {
            let binders = type_params
                .iter()
                .map(|param| {
                    let bounds = type_bounds
                        .iter()
                        .find(|bound| bound.type_param == *param)
                        .map(|bound| bound.traits.join("+"))
                        .unwrap_or_default();
                    Type::Named(if bounds.is_empty() {
                        param.clone()
                    } else {
                        format!("{param}:{bounds}")
                    })
                })
                .collect();
            signature.push(Type::Generic {
                name: "forall".to_string(),
                args: binders,
            });
        }
        signature.extend(params);
        signature.push(ret);
        if !effects.is_empty() {
            signature.push(Type::Generic {
                name: "effects".to_string(),
                args: effects.iter().map(|effect| Type::parse(effect)).collect(),
            });
        }
        Type::Generic {
            name: "Fn".to_string(),
            args: signature,
        }
    }

    pub fn is_mutable_slice(&self) -> bool {
        matches!(self, Type::Slice { mutable: true, .. })
    }

    /// Decode a standalone associated-type projection such as
    /// `Iterator::Item[Vec[i32]]`. Keeping this in the existing generic type
    /// node preserves both source syntaxes and the canonical AST schema.
    pub fn associated_projection_parts(&self) -> Option<(&str, &str, &Type)> {
        let Type::Generic { name, args } = self else {
            return None;
        };
        let (trait_name, item_name) = name.split_once("::")?;
        if trait_name.is_empty() || item_name.is_empty() || args.len() != 1 {
            return None;
        }
        Some((trait_name, item_name, &args[0]))
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
        Stmt::If {
            then_body,
            else_body,
            ..
        } => body_has_asm(then_body) || body_has_asm(else_body),
        Stmt::For { body, .. } | Stmt::While { body, .. } => body_has_asm(body),
        Stmt::Match { arms, .. } => arms.iter().any(|arm| body_has_asm(&arm.body)),
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
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_asm_target_metadata(then_body, labels);
                collect_asm_target_metadata(else_body, labels);
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                collect_asm_target_metadata(body, labels)
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    collect_asm_target_metadata(&arm.body, labels);
                }
            }
            Stmt::Let { .. }
            | Stmt::Return(_)
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Break
            | Stmt::Continue => {}
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
        Item::Enum(decl) => {
            let mut out = format!(
                "{pad}{{\n{pad}  \"kind\": \"enum\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\",\n{pad}  \"variants\": [",
                escape(&decl.id),
                escape(&decl.name)
            );
            for (idx, variant) in decl.variants.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("\"{}\"", escape(&variant_repr(variant))));
            }
            out.push_str(&format!("]\n{pad}}}"));
            out
        }
        Item::Trait(decl) => format!(
            "{pad}{{\n{pad}  \"kind\": \"trait\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\"\n{pad}}}",
            escape(&decl.id),
            escape(&decl.name)
        ),
        Item::TraitImpl(decl) => format!(
            "{pad}{{\n{pad}  \"kind\": \"trait_impl\",\n{pad}  \"id\": \"{}\",\n{pad}  \"trait\": \"{}\",\n{pad}  \"for_type\": \"{}\"\n{pad}}}",
            escape(&decl.id),
            escape(&decl.trait_name),
            escape(&decl.for_type.to_agent())
        ),
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
        Item::Capsule(capsule) => {
            let bindings = |bindings: &[CapsuleBinding]| {
                bindings
                    .iter()
                    .map(|binding| {
                        format!(
                            "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                            escape(&binding.name),
                            escape(&binding.ty.to_agent())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "{pad}{{\n{pad}  \"kind\": \"capsule\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\",\n{pad}  \"language\": \"{}\",\n{pad}  \"version\": \"{}\",\n{pad}  \"env\": {},\n{pad}  \"risk_class\": \"{}\",\n{pad}  \"effects\": [{}],\n{pad}  \"inputs\": [{}],\n{pad}  \"outputs\": [{}],\n{pad}  \"deps\": [{}],\n{pad}  \"sandbox\": [{}],\n{pad}  \"bindings\": [{}],\n{pad}  \"fixtures\": {},\n{pad}  \"code_lines\": {}\n{pad}}}",
                escape(&capsule.id),
                escape(&capsule.name),
                escape(&capsule.language),
                escape(&capsule.version),
                capsule
                    .env
                    .as_ref()
                    .map(|env| format!("\"{}\"", escape(env)))
                    .unwrap_or_else(|| "null".to_string()),
                capsule.risk_class(),
                capsule
                    .effects
                    .iter()
                    .map(|effect| format!("\"{}\"", escape(effect)))
                    .collect::<Vec<_>>()
                    .join(", "),
                bindings(&capsule.inputs),
                bindings(&capsule.outputs),
                capsule
                    .deps
                    .iter()
                    .map(|dep| format!(
                        "{{\"source\":\"{}\",\"pinned\":{}}}",
                        escape(&dep.source),
                        dep.is_pinned()
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                capsule
                    .sandbox
                    .iter()
                    .map(|rule| format!("\"{}\"", escape(rule)))
                    .collect::<Vec<_>>()
                    .join(", "),
                capsule
                    .bindings
                    .iter()
                    .map(|binding| format!(
                        "{{\"name\":\"{}\",\"mode\":\"{}\"}}",
                        escape(&binding.name),
                        binding.mode.as_str()
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                capsule.fixtures.len(),
                capsule.code.len()
            )
        }
        Item::Share(share) => {
            format!(
                "{pad}{{\n{pad}  \"kind\": \"share\",\n{pad}  \"id\": \"{}\",\n{pad}  \"name\": \"{}\",\n{pad}  \"type\": \"{}\",\n{pad}  \"default\": {}\n{pad}}}",
                escape(&share.id),
                escape(&share.name),
                escape(&share.ty.to_agent()),
                share
                    .default
                    .as_ref()
                    .map(|value| value.to_literal())
                    .unwrap_or_else(|| "null".to_string())
            )
        }
    }
}

fn item_to_canonical_json(out: &mut String, item: &Item, indent: usize) {
    match item {
        Item::Struct(decl) => struct_to_canonical_json(out, decl, indent),
        Item::Enum(decl) => enum_to_canonical_json(out, decl, indent),
        Item::Trait(decl) => trait_to_canonical_json(out, decl, indent),
        Item::TraitImpl(decl) => trait_impl_to_canonical_json(out, decl, indent),
        Item::Function(function) => function_to_canonical_json(out, function, indent),
        Item::Test(test) => test_to_canonical_json(out, test, indent),
        Item::Capsule(capsule) => capsule_to_canonical_json(out, capsule, indent),
        Item::Share(share) => share_to_canonical_json(out, share, indent),
    }
}

fn trait_to_canonical_json(out: &mut String, decl: &TraitDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("trait"), true);
    push_prop(out, indent + 1, "id", &quoted(&decl.id), true);
    push_prop(out, indent + 1, "name", &quoted(&decl.name), true);
    let methods = decl
        .methods
        .iter()
        .map(|method| {
            let params = method
                .params
                .iter()
                .map(|param| param.ty.to_agent())
                .collect::<Vec<_>>()
                .join(",");
            let mut signature = format!("{}({})->{}", method.name, params, method.ret.to_agent());
            if let Some(default) = &method.default_function {
                signature.push_str(&format!("={default}"));
            }
            signature
        })
        .map(|method| quoted(&method))
        .collect::<Vec<_>>()
        .join(", ");
    push_prop(out, indent + 1, "methods", &format!("[{methods}]"), true);
    let constants = decl
        .constants
        .iter()
        .map(|constant| quoted(&format!("{}:{}", constant.name, constant.ty.to_agent())))
        .collect::<Vec<_>>()
        .join(", ");
    push_prop(
        out,
        indent + 1,
        "constants",
        &format!("[{constants}]"),
        true,
    );
    let associated_types = decl
        .associated_types
        .iter()
        .map(|associated_type| quoted(&associated_type.name))
        .collect::<Vec<_>>()
        .join(", ");
    push_prop(
        out,
        indent + 1,
        "associated_types",
        &format!("[{associated_types}]"),
        true,
    );
    push_attributes(out, indent + 1, "annotations", &decl.attrs, false);
    push_line(out, indent, "}");
}

fn trait_impl_to_canonical_json(out: &mut String, decl: &TraitImplDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("trait_impl"), true);
    push_prop(out, indent + 1, "id", &quoted(&decl.id), true);
    push_prop(out, indent + 1, "trait", &quoted(&decl.trait_name), true);
    if !decl.type_params.is_empty() {
        let list = decl
            .type_params
            .iter()
            .map(|param| quoted(param))
            .collect::<Vec<_>>()
            .join(", ");
        push_prop(out, indent + 1, "type_params", &format!("[{list}]"), true);
    }
    if !decl.type_bounds.is_empty() {
        let list = decl
            .type_bounds
            .iter()
            .map(|bound| quoted(&format!("{}:{}", bound.type_param, bound.traits.join("+"))))
            .collect::<Vec<_>>()
            .join(", ");
        push_prop(out, indent + 1, "type_bounds", &format!("[{list}]"), true);
    }
    push_prop(
        out,
        indent + 1,
        "for_type",
        &quoted(&decl.for_type.to_agent()),
        true,
    );
    let methods = decl
        .methods
        .iter()
        .map(|method| quoted(&format!("{}={}", method.method, method.function)))
        .collect::<Vec<_>>()
        .join(", ");
    push_prop(out, indent + 1, "methods", &format!("[{methods}]"), true);
    let constants = decl
        .constants
        .iter()
        .map(|constant| {
            quoted(&format!(
                "{}={}",
                constant.name,
                expr_to_inline_text(&constant.value)
            ))
        })
        .collect::<Vec<_>>()
        .join(", ");
    push_prop(
        out,
        indent + 1,
        "constants",
        &format!("[{constants}]"),
        true,
    );
    let associated_types = decl
        .associated_types
        .iter()
        .map(|associated_type| {
            quoted(&format!(
                "{}={}",
                associated_type.name,
                associated_type.ty.to_agent()
            ))
        })
        .collect::<Vec<_>>()
        .join(", ");
    push_prop(
        out,
        indent + 1,
        "associated_types",
        &format!("[{associated_types}]"),
        true,
    );
    push_attributes(out, indent + 1, "annotations", &decl.attrs, false);
    push_line(out, indent, "}");
}

fn enum_to_canonical_json(out: &mut String, decl: &EnumDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("enum"), true);
    push_prop(out, indent + 1, "id", &quoted(&decl.id), true);
    push_prop(out, indent + 1, "name", &quoted(&decl.name), true);
    push_array_block_start(out, indent + 1, "variants");
    for (idx, variant) in decl.variants.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_indent(out, indent + 2);
        out.push_str(&quoted(&variant_repr(variant)));
    }
    out.push('\n');
    push_line(out, indent + 1, "]");
    push_line(out, indent, "}");
}

fn share_to_canonical_json(out: &mut String, share: &ShareDecl, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("share"), true);
    push_prop(out, indent + 1, "id", &quoted(&share.id), true);
    push_prop(out, indent + 1, "name", &quoted(&share.name), true);
    push_prop(out, indent + 1, "type", &quoted(&share.ty.to_agent()), true);
    let default = share
        .default
        .as_ref()
        .map(|value| value.to_literal())
        .unwrap_or_else(|| "null".to_string());
    push_prop(out, indent + 1, "default", &default, false);
    push_line(out, indent, "}");
}

fn capsule_to_canonical_json(out: &mut String, capsule: &Capsule, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted("capsule"), true);
    push_prop(out, indent + 1, "id", &quoted(&capsule.id), true);
    push_prop(out, indent + 1, "name", &quoted(&capsule.name), true);
    push_prop(
        out,
        indent + 1,
        "language",
        &quoted(&capsule.language),
        true,
    );
    push_prop(out, indent + 1, "version", &quoted(&capsule.version), true);
    push_prop(
        out,
        indent + 1,
        "env",
        &capsule
            .env
            .as_ref()
            .map(|env| quoted(env))
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    push_prop(
        out,
        indent + 1,
        "risk_class",
        &quoted(capsule.risk_class()),
        true,
    );
    push_string_list(out, indent + 1, "effects", &capsule.effects, true);
    push_capsule_bindings(out, indent + 1, "inputs", &capsule.inputs, true);
    push_capsule_bindings(out, indent + 1, "outputs", &capsule.outputs, true);
    push_array_block_start(out, indent + 1, "deps");
    for (idx, dep) in capsule.deps.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_line(out, indent + 2, "{");
        push_prop(out, indent + 3, "source", &quoted(&dep.source), true);
        push_prop(
            out,
            indent + 3,
            "pinned",
            if dep.is_pinned() { "true" } else { "false" },
            false,
        );
        push_indent(out, indent + 2);
        out.push('}');
    }
    if !capsule.deps.is_empty() {
        out.push('\n');
    }
    push_line(out, indent + 1, "],");
    push_string_list(out, indent + 1, "sandbox", &capsule.sandbox, true);
    push_indent(out, indent + 1);
    out.push_str("\"bindings\": [");
    for (idx, binding) in capsule.bindings.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"name\": {}, \"mode\": {}}}",
            quoted(&binding.name),
            quoted(binding.mode.as_str())
        ));
    }
    out.push_str("],\n");
    push_array_block_start(out, indent + 1, "fixtures");
    for (idx, fixture) in capsule.fixtures.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_indent(out, indent + 2);
        out.push_str(&format!(
            "{{\"inputs\": [{}], \"outputs\": [{}]}}",
            fixture
                .inputs
                .iter()
                .map(|value| value.to_literal())
                .collect::<Vec<_>>()
                .join(", "),
            fixture
                .outputs
                .iter()
                .map(|value| value.to_literal())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !capsule.fixtures.is_empty() {
        out.push('\n');
    }
    push_line(out, indent + 1, "],");
    push_string_list(out, indent + 1, "code", &capsule.code, false);
    push_line(out, indent, "}");
}

fn push_string_list(out: &mut String, indent: usize, key: &str, values: &[String], trailing: bool) {
    push_indent(out, indent);
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
    out.push('\n');
}

fn push_capsule_bindings(
    out: &mut String,
    indent: usize,
    key: &str,
    bindings: &[CapsuleBinding],
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": ["));
    for (idx, binding) in bindings.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"name\": {}, \"type\": {}}}",
            quoted(&binding.name),
            quoted(&binding.ty.to_agent())
        ));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
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
    // Emitted only for generic functions (lang.4) so monomorphic programs keep
    // their existing canonical form byte-for-byte.
    if !function.type_params.is_empty() {
        let list = function
            .type_params
            .iter()
            .map(|p| quoted(p))
            .collect::<Vec<_>>()
            .join(", ");
        push_prop(out, indent + 1, "type_params", &format!("[{list}]"), true);
    }
    if !function.type_bounds.is_empty() {
        let list = function
            .type_bounds
            .iter()
            .map(|bound| quoted(&format!("{}:{}", bound.type_param, bound.traits.join("+"))))
            .collect::<Vec<_>>()
            .join(", ");
        push_prop(out, indent + 1, "type_bounds", &format!("[{list}]"), true);
    }
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

/// Canonical text for a match pattern: `_` for the wildcard, otherwise the
/// variant path verbatim (`Color::Red`).
/// Canonical text for an enum variant: `Name` when data-less, `Name(Type)`
/// when it carries a payload (the agent type rendering).
pub fn variant_repr(variant: &EnumVariant) -> String {
    match &variant.payload {
        Some(ty) => format!("{}({})", variant.name, ty.to_agent()),
        None => variant.name.clone(),
    }
}

pub fn pattern_to_string(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard => "_".to_string(),
        Pattern::Variant {
            path,
            binding: Some(name),
        } => format!("{path}({name})"),
        Pattern::Variant {
            path,
            binding: None,
        } => path.clone(),
    }
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
        Stmt::Break => {
            push_prop(out, indent + 1, "kind", &quoted("break"), false);
        }
        Stmt::Continue => {
            push_prop(out, indent + 1, "kind", &quoted("continue"), false);
        }
        Stmt::Require(expr) => {
            push_prop(out, indent + 1, "kind", &quoted("require"), true);
            push_expr_prop(out, indent + 1, "expr", expr, &format!("{id}.expr"), false);
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
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
            push_line(out, indent + 1, "],");
            push_array_block_start(out, indent + 1, "else_body");
            for (idx, stmt) in else_body.iter().enumerate() {
                if idx > 0 {
                    out.push_str(",\n");
                }
                stmt_to_canonical_json(out, stmt, &format!("{id}.else.{idx}"), indent + 2);
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
        Stmt::While { cond, body } => {
            push_prop(out, indent + 1, "kind", &quoted("while"), true);
            push_prop(
                out,
                indent + 1,
                "loop_id",
                &quoted(&format!("{id}.loop")),
                true,
            );
            push_expr_prop(out, indent + 1, "cond", cond, &format!("{id}.cond"), true);
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
        Stmt::Match { scrutinee, arms } => {
            push_prop(out, indent + 1, "kind", &quoted("match"), true);
            push_expr_prop(
                out,
                indent + 1,
                "scrutinee",
                scrutinee,
                &format!("{id}.scrutinee"),
                true,
            );
            push_array_block_start(out, indent + 1, "arms");
            for (idx, arm) in arms.iter().enumerate() {
                if idx > 0 {
                    out.push_str(",\n");
                }
                push_line(out, indent + 2, "{");
                push_prop(
                    out,
                    indent + 3,
                    "pattern",
                    &quoted(&pattern_to_string(&arm.pattern)),
                    true,
                );
                push_array_block_start(out, indent + 3, "body");
                for (sidx, stmt) in arm.body.iter().enumerate() {
                    if sidx > 0 {
                        out.push_str(",\n");
                    }
                    stmt_to_canonical_json(
                        out,
                        stmt,
                        &format!("{id}.arm.{idx}.{sidx}"),
                        indent + 4,
                    );
                }
                out.push('\n');
                push_line(out, indent + 3, "]");
                push_line(out, indent + 2, "}");
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
        Expr::TraitConst {
            trait_name,
            name,
            for_type,
        } => {
            push_prop(out, indent + 1, "kind", &quoted("trait_const"), true);
            push_prop(out, indent + 1, "trait", &quoted(trait_name), true);
            push_prop(out, indent + 1, "name", &quoted(name), true);
            push_prop(
                out,
                indent + 1,
                "for_type",
                &quoted(&for_type.to_agent()),
                false,
            );
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
        Expr::Lambda { params, body } => {
            push_prop(out, indent + 1, "kind", &quoted("lambda"), true);
            let params_json = params
                .iter()
                .map(|param| {
                    format!(
                        "{{\"name\": {}, \"type\": {}}}",
                        quoted(&param.name),
                        quoted(&param.ty.to_agent())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            push_prop(out, indent + 1, "params", &format!("[{params_json}]"), true);
            push_expr_prop(out, indent + 1, "body", body, &format!("{id}.body"), false);
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
        Expr::TraitConst {
            trait_name,
            name,
            for_type,
        } => format!("{trait_name}::{name}[{}]", for_type.to_agent()),
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
        Expr::Lambda { params, body } => format!(
            "|{}| {}",
            params
                .iter()
                .map(|param| format!("{}: {}", param.name, param.ty.to_agent()))
                .collect::<Vec<_>>()
                .join(", "),
            expr_to_inline_text(body)
        ),
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
