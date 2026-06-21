use std::collections::{BTreeSet, HashMap};

use crate::ast::{body_has_asm, Expr, FunctionDecl, Item, Program, Stmt, StructDecl, Type};
use crate::diag::Diagnostic;
use crate::effects::{analyze as analyze_effects, effect_requires_unsafe};
use crate::lifetimes;
use crate::simd::{check_intrinsic_call, is_simd_intrinsic};
use crate::stdlib::standard_builtin_call_type;
use crate::targets::is_cpu_feature_probe;

pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut checker = Checker::new(program);
    checker.check_program();
    let mut diagnostics = checker.diagnostics;
    // The CFG dataflow borrow checker runs behind the staged rules as a
    // second net: it adds a diagnostic only when the staged checker found
    // nothing of the same code in the same function, so it can close
    // control-flow shapes the staged rules miss without changing any
    // existing diagnostic output.
    let staged: BTreeSet<(String, String)> = diagnostics
        .iter()
        .map(|diagnostic| (function_prefix(&diagnostic.node), diagnostic.code.clone()))
        .collect();
    for diagnostic in crate::borrowck::check_program(program) {
        let key = (function_prefix(&diagnostic.node), diagnostic.code.clone());
        if !staged.contains(&key) {
            diagnostics.push(diagnostic);
        }
    }
    // Foreign capsule contract validation (v2.0): unpinned dependencies,
    // missing sandbox policy, missing typed boundary, and empty bodies are
    // rejected at check time.
    diagnostics.extend(crate::capsule::validate(program));
    // Shared typed namespace validation (v2.2): unknown bindings, multiple
    // writers, and default type mismatches.
    diagnostics.extend(crate::share::validate(program));
    // Sandbox policy reconciliation (v2.3): declared effects must be
    // covered by a matching sandbox grant; resource limits must be valid.
    diagnostics.extend(crate::sandbox::validate(program));
    // Managed runtime reconciliation (v2.5): a capsule's dependencies must
    // come from its language's package ecosystem (or a generic source).
    diagnostics.extend(crate::runtime::validate(program));
    // Native/GPU capsule review (v2.6): a CUDA capsule must declare a GPU
    // architecture target.
    diagnostics.extend(crate::native_capsule::validate(program));
    // Shell capsule hardening (v2.7): shell capsules must declare an
    // explicit, non-wildcard command allowlist.
    diagnostics.extend(crate::shell::validate(program));
    // Exact-arithmetic opt-in: a function marked @exact must not use float.
    diagnostics.extend(crate::numeric::validate(program));
    // Prelude aliases are reserved: a user function may not take one.
    diagnostics.extend(crate::prelude::validate(program));
    // Smart-contract capsule review (v2.8): require an ABI hash and deny
    // secret-key/wallet access.
    diagnostics.extend(crate::contract::validate(program));
    diagnostics
}

/// The `@function.name` prefix of a node ID, used to scope deduplication
/// between the staged borrow rules and the CFG dataflow checker.
fn function_prefix(node: &str) -> String {
    node.split('.').take(2).collect::<Vec<_>>().join(".")
}

struct TypeEnvironment<'a> {
    _modules: Vec<String>,
    _imports: Vec<String>,
    functions: HashMap<String, &'a FunctionDecl>,
    structs: HashMap<String, &'a StructDecl>,
}

impl<'a> TypeEnvironment<'a> {
    fn from_program(program: &'a Program) -> Self {
        let mut functions = HashMap::new();
        let mut structs = HashMap::new();
        for item in &program.items {
            match item {
                Item::Function(function) => {
                    functions.insert(function.name.clone(), function);
                }
                Item::Struct(decl) => {
                    structs.insert(decl.name.clone(), decl);
                }
                Item::Test(_) | Item::Capsule(_) | Item::Share(_) => {}
            }
        }
        Self {
            _modules: vec!["root".to_string()],
            _imports: Vec::new(),
            functions,
            structs,
        }
    }
}

#[derive(Clone, Default)]
struct LocalTypeEnv {
    bindings: HashMap<String, Type>,
}

impl LocalTypeEnv {
    fn for_function(function: &FunctionDecl) -> Self {
        let mut bindings = HashMap::new();
        for param in &function.params {
            bindings.insert(param.name.clone(), param.ty.clone());
        }
        Self { bindings }
    }

    fn get(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    fn insert(&mut self, name: String, ty: Type) {
        self.bindings.insert(name, ty);
    }
}

struct TypeDiagnostic<'a> {
    code: &'a str,
    message: String,
    suggestions: &'a [&'a str],
}

impl<'a> TypeDiagnostic<'a> {
    fn new(code: &'a str, message: impl Into<String>, suggestions: &'a [&'a str]) -> Self {
        Self {
            code,
            message: message.into(),
            suggestions,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ReferenceOrigin {
    Parameter,
    Local,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PlaceProjection {
    Field(String),
    IndexConst(i128),
    IndexUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BorrowPlace {
    root: String,
    projections: Vec<PlaceProjection>,
}

impl BorrowPlace {
    fn root(root: String) -> Self {
        Self {
            root,
            projections: Vec::new(),
        }
    }

    fn with_projection(&self, projection: PlaceProjection) -> Self {
        let mut place = self.clone();
        place.projections.push(projection);
        place
    }

    fn label(&self) -> String {
        let mut label = self.root.clone();
        for projection in &self.projections {
            match projection {
                PlaceProjection::Field(field) => {
                    label.push('.');
                    label.push_str(field);
                }
                PlaceProjection::IndexConst(index) => {
                    label.push('[');
                    label.push_str(&index.to_string());
                    label.push(']');
                }
                PlaceProjection::IndexUnknown => label.push_str("[*]"),
            }
        }
        label
    }
}

#[derive(Clone, Debug)]
struct OwnershipBinding {
    ty: Type,
    moved: bool,
    moved_paths: BTreeSet<String>,
    origin: ReferenceOrigin,
    borrow_root: Option<String>,
    borrow_place: Option<BorrowPlace>,
    borrow_alternates: Vec<BorrowPlace>,
    borrow_parent: Option<String>,
}

#[derive(Clone, Default)]
struct OwnershipEnv {
    bindings: HashMap<String, OwnershipBinding>,
}

impl OwnershipEnv {
    fn for_function(function: &FunctionDecl) -> Self {
        let mut bindings = HashMap::new();
        for param in &function.params {
            bindings.insert(
                param.name.clone(),
                OwnershipBinding {
                    ty: param.ty.clone(),
                    moved: false,
                    moved_paths: BTreeSet::new(),
                    origin: ReferenceOrigin::Parameter,
                    borrow_root: if is_borrow_type(&param.ty) {
                        Some(param.name.clone())
                    } else {
                        None
                    },
                    borrow_place: if is_borrow_type(&param.ty) {
                        Some(BorrowPlace::root(param.name.clone()))
                    } else {
                        None
                    },
                    borrow_alternates: Vec::new(),
                    borrow_parent: None,
                },
            );
        }
        Self { bindings }
    }

    fn get(&self, name: &str) -> Option<&OwnershipBinding> {
        self.bindings.get(name)
    }

    fn insert(
        &mut self,
        name: String,
        ty: Type,
        origin: ReferenceOrigin,
        borrow_root: Option<String>,
        borrow_place: Option<BorrowPlace>,
        borrow_parent: Option<String>,
    ) {
        self.bindings.insert(
            name,
            OwnershipBinding {
                ty,
                moved: false,
                moved_paths: BTreeSet::new(),
                origin,
                borrow_root,
                borrow_place,
                borrow_alternates: Vec::new(),
                borrow_parent,
            },
        );
    }

    fn mark_moved(&mut self, name: &str) {
        if let Some(binding) = self.bindings.get_mut(name) {
            binding.moved = true;
            binding.moved_paths.clear();
        }
    }

    fn mark_path_moved(&mut self, name: &str, path: &[String]) {
        if path.is_empty() {
            self.mark_moved(name);
            return;
        }
        if let Some(binding) = self.bindings.get_mut(name) {
            let moved_path = encode_move_path(path);
            binding
                .moved_paths
                .retain(|existing| !encoded_path_is_descendant(existing, &moved_path));
            binding.moved_paths.insert(moved_path);
        }
    }

    fn mark_initialized(
        &mut self,
        name: &str,
        origin: ReferenceOrigin,
        borrow_root: Option<String>,
        borrow_place: Option<BorrowPlace>,
        borrow_parent: Option<String>,
    ) {
        if let Some(binding) = self.bindings.get_mut(name) {
            binding.moved = false;
            binding.moved_paths.clear();
            binding.origin = origin;
            binding.borrow_root = borrow_root;
            binding.borrow_place = borrow_place;
            binding.borrow_alternates.clear();
            binding.borrow_parent = borrow_parent;
        }
    }

    fn mark_path_initialized(&mut self, name: &str, path: &[String]) {
        if path.is_empty() {
            return;
        }
        if let Some(binding) = self.bindings.get_mut(name) {
            let initialized_path = encode_move_path(path);
            binding.moved_paths.retain(|existing| {
                existing != &initialized_path
                    && !encoded_path_is_descendant(existing, &initialized_path)
            });
        }
    }

    fn merge_moved_from(&mut self, other: &OwnershipEnv) {
        for (name, binding) in &other.bindings {
            if binding.moved {
                self.mark_moved(name);
            }
            for moved_path in &binding.moved_paths {
                let path = split_move_path(moved_path);
                self.mark_path_moved(name, &path);
            }
        }
    }

    fn merge_branch_borrows_from(&mut self, other: &OwnershipEnv) {
        for (name, other_binding) in &other.bindings {
            let Some(binding) = self.bindings.get_mut(name) else {
                continue;
            };
            if !is_borrow_type(&binding.ty) && !is_borrow_type(&other_binding.ty) {
                continue;
            }

            let base_origin = binding.origin.clone();
            let other_origin = other_binding.origin.clone();
            let mut places = binding_borrow_places(name, binding);
            for place in binding_borrow_places(name, other_binding) {
                push_unique_place(&mut places, place);
            }

            binding.origin = if base_origin == other_origin {
                base_origin
            } else {
                ReferenceOrigin::Unknown
            };
            binding.borrow_place = places.first().cloned();
            binding.borrow_root = binding
                .borrow_place
                .as_ref()
                .map(|place| place.root.clone());
            binding.borrow_alternates = places.into_iter().skip(1).collect();
            if binding.borrow_parent != other_binding.borrow_parent {
                binding.borrow_parent = None;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ValueUse {
    Read,
    Move,
}

#[derive(Clone, Copy)]
struct BorrowLiveness<'a> {
    current: &'a BTreeSet<String>,
    future: &'a BTreeSet<String>,
}

struct Checker<'a> {
    program: &'a Program,
    type_env: TypeEnvironment<'a>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            program,
            type_env: TypeEnvironment::from_program(program),
            diagnostics: Vec::new(),
        }
    }

    fn check_program(&mut self) {
        for function in self.program.functions() {
            self.check_function(function);
        }
        for function in self.program.functions() {
            self.check_safety_function(function);
        }
        self.check_effects();
    }

    fn check_function(&mut self, function: &FunctionDecl) {
        if !function.unsafe_fn && body_has_asm(&function.body) {
            self.diagnostics.push(
                Diagnostic::new(
                    "unsafe_required",
                    &function.id,
                    "assembly blocks are only allowed inside unsafe functions",
                )
                .suggest("mark_function_unsafe"),
            );
        }
        if !function.unsafe_fn {
            for param in &function.params {
                if is_raw_pointer_type(&param.ty) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "raw_pointer_requires_unsafe",
                            &function.id,
                            format!(
                                "raw pointer parameter '{}' requires unsafe code",
                                param.name
                            ),
                        )
                        .with_types(&param.ty, &param.ty)
                        .suggest("mark_function_unsafe")
                        .suggest("replace_pointer_with_slice"),
                    );
                }
            }
        }

        let mut locals = LocalTypeEnv::for_function(function);
        self.check_body(&function.body, &function.ret, &mut locals, function);
    }

    fn check_body(
        &mut self,
        body: &[Stmt],
        ret: &Type,
        locals: &mut LocalTypeEnv,
        function: &FunctionDecl,
    ) {
        for stmt in body {
            self.check_stmt(stmt, ret, locals, function);
        }
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        ret: &Type,
        locals: &mut LocalTypeEnv,
        function: &FunctionDecl,
    ) {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                let found = self.infer_expr(expr, locals, &function.id);
                if let Some(expected) = ty {
                    self.check_expr_assignable(
                        expected,
                        expr,
                        &found,
                        &function.id,
                        TypeDiagnostic::new(
                            "type_mismatch",
                            format!("let binding '{name}' has incompatible type"),
                            &["insert_explicit_cast", "replace_local_type"],
                        ),
                    );
                    locals.insert(name.clone(), expected.clone());
                } else {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "local_type_required",
                            &function.id,
                            format!("let binding '{name}' requires an explicit type"),
                        )
                        .with_types(&Type::Unknown, &found)
                        .suggest("add_local_type_annotation"),
                    );
                    locals.insert(name.clone(), found);
                }
            }
            Stmt::Return(Some(expr)) => {
                let found = self.infer_expr(expr, locals, &function.id);
                self.check_expr_assignable(
                    ret,
                    expr,
                    &found,
                    &function.id,
                    TypeDiagnostic::new(
                        "return_type_mismatch",
                        "return expression does not match function return type",
                        &[
                            "change return type or insert cast",
                            "replace_return_type",
                            "insert_explicit_cast",
                        ],
                    ),
                );
            }
            Stmt::Return(None) => {
                if *ret != Type::Void {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "return_type_mismatch",
                            &function.id,
                            "bare return is only valid for void functions",
                        )
                        .with_types(ret, &Type::Void)
                        .suggest("replace_return_expression"),
                    );
                }
            }
            Stmt::Require(expr) => {
                let found = self.infer_expr(expr, locals, &function.id);
                if found != Type::Bool && found != Type::Unknown {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "require_not_bool",
                            &function.id,
                            "require condition must be bool",
                        )
                        .with_types(&Type::Bool, &found)
                        .suggest("replace_require_condition"),
                    );
                }
            }
            Stmt::If { cond, then_body } => {
                let found = self.infer_expr(cond, locals, &function.id);
                if found != Type::Bool && found != Type::Unknown {
                    self.diagnostics.push(
                        Diagnostic::new("if_not_bool", &function.id, "if condition must be bool")
                            .with_types(&Type::Bool, &found)
                            .suggest("replace_if_condition"),
                    );
                }
                let mut branch_locals = locals.clone();
                self.check_body(then_body, ret, &mut branch_locals, function);
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                let start_ty = self.infer_expr(start, locals, &function.id);
                let end_ty = self.infer_expr(end, locals, &function.id);
                let usize_ty = Type::Primitive("usize".to_string());
                self.check_expr_assignable(
                    &usize_ty,
                    start,
                    &start_ty,
                    &function.id,
                    TypeDiagnostic::new(
                        "for_start_type",
                        "for start must be usize",
                        &["insert_explicit_cast", "replace_for_start"],
                    ),
                );
                self.check_expr_assignable(
                    &usize_ty,
                    end,
                    &end_ty,
                    &function.id,
                    TypeDiagnostic::new(
                        "for_end_type",
                        "for end must be usize",
                        &["insert_explicit_cast", "replace_for_end"],
                    ),
                );
                let mut loop_locals = locals.clone();
                loop_locals.insert(var.clone(), usize_ty);
                self.check_body(body, ret, &mut loop_locals, function);
            }
            Stmt::Assign { target, expr } => {
                let expected = self.infer_lvalue(target, locals, &function.id);
                let found = self.infer_expr(expr, locals, &function.id);
                self.check_expr_assignable(
                    &expected,
                    expr,
                    &found,
                    &function.id,
                    TypeDiagnostic::new(
                        "assignment_type_mismatch",
                        "assignment value does not match target type",
                        &["insert_explicit_cast", "change_target_type"],
                    ),
                );
                if matches!(target, Expr::Index { .. })
                    && !self.is_mutable_index_target(target, locals)
                {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "immutable_write",
                            &function.id,
                            "indexed writes require a mutable slice or pointer target",
                        )
                        .suggest("change_parameter_to_mutable_slice"),
                    );
                }
            }
            Stmt::Expr(expr) => {
                self.infer_expr(expr, locals, &function.id);
            }
            Stmt::Asm(asm) => {
                for output in &asm.outputs {
                    locals.insert(output.value.clone(), Type::Unknown);
                }
                if !function.unsafe_fn {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "unsafe_required",
                            &function.id,
                            "assembly block appears in a safe function",
                        )
                        .suggest("mark_function_unsafe"),
                    );
                }
            }
        }
    }

    fn infer_lvalue(&mut self, expr: &Expr, locals: &LocalTypeEnv, node: &str) -> Type {
        match expr {
            Expr::Var(name) => locals.get(name).cloned().unwrap_or_else(|| {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_symbol",
                        node,
                        format!("unknown assignment target '{name}'"),
                    )
                    .suggest("add_local_or_parameter"),
                );
                Type::Unknown
            }),
            Expr::Index { base, index } => {
                let index_ty = self.infer_expr(index, locals, node);
                let usize_ty = Type::Primitive("usize".to_string());
                self.check_expr_assignable(
                    &usize_ty,
                    index,
                    &index_ty,
                    node,
                    TypeDiagnostic::new(
                        "index_not_integer",
                        "slice index must be integer",
                        &["insert_explicit_cast", "replace_index_expression"],
                    ),
                );
                self.infer_expr(base, locals, node)
                    .element_type()
                    .unwrap_or(Type::Unknown)
            }
            Expr::Field { .. } => self.infer_expr(expr, locals, node),
            _ => {
                self.diagnostics.push(
                    Diagnostic::new("invalid_lvalue", node, "assignment target is not writable")
                        .suggest("replace_assignment_target"),
                );
                Type::Unknown
            }
        }
    }

    fn infer_expr(&mut self, expr: &Expr, locals: &LocalTypeEnv, node: &str) -> Type {
        match expr {
            Expr::Int(_) => Type::Primitive("$int".to_string()),
            Expr::Float(_) => Type::Primitive("$float".to_string()),
            Expr::Bool(_) => Type::Bool,
            Expr::String(_) => Type::Primitive("str".to_string()),
            Expr::Var(name) => locals.get(name).cloned().unwrap_or_else(|| {
                self.diagnostics.push(
                    Diagnostic::new("unknown_symbol", node, format!("unknown symbol '{name}'"))
                        .suggest("add_local_or_parameter"),
                );
                Type::Unknown
            }),
            Expr::Unary { op, expr } => {
                let ty = self.infer_expr(expr, locals, node);
                if op == "!" {
                    if ty != Type::Bool && ty != Type::Unknown {
                        self.diagnostics.push(
                            Diagnostic::new("not_bool", node, "logical not requires bool")
                                .with_types(&Type::Bool, &ty)
                                .suggest("replace_unary_expression"),
                        );
                    }
                    Type::Bool
                } else if op == "-" {
                    if !ty.is_numeric() && ty != Type::Unknown {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "negation_type_mismatch",
                                node,
                                "negation requires numeric input",
                            )
                            .with_types(&Type::Named("numeric".to_string()), &ty)
                            .suggest("replace_unary_expression"),
                        );
                        Type::Unknown
                    } else {
                        ty
                    }
                } else {
                    ty
                }
            }
            Expr::Binary { op, left, right } => {
                let left_ty = self.infer_expr(left, locals, node);
                let right_ty = self.infer_expr(right, locals, node);
                match op.as_str() {
                    "==" | "!=" | "<" | ">" | "<=" | ">=" => {
                        if !are_comparable(&left_ty, &right_ty) {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "comparison_type_mismatch",
                                    node,
                                    "comparison operands are incompatible",
                                )
                                .with_types(&left_ty, &right_ty)
                                .suggest("insert_explicit_cast"),
                            );
                        }
                        Type::Bool
                    }
                    "&&" | "||" => {
                        if left_ty != Type::Bool && left_ty != Type::Unknown {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "logical_type_mismatch",
                                    node,
                                    "logical operands must be bool",
                                )
                                .with_types(&Type::Bool, &left_ty)
                                .suggest("replace_logical_operand"),
                            );
                        }
                        if right_ty != Type::Bool && right_ty != Type::Unknown {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "logical_type_mismatch",
                                    node,
                                    "logical operands must be bool",
                                )
                                .with_types(&Type::Bool, &right_ty)
                                .suggest("replace_logical_operand"),
                            );
                        }
                        Type::Bool
                    }
                    "+" | "-" | "*" | "/" | "%" | "<<" | ">>" | "&" | "|" => {
                        if !left_ty.is_numeric() || !right_ty.is_numeric() {
                            if !left_ty.is_numeric() && left_ty != Type::Unknown {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "arithmetic_type_mismatch",
                                        node,
                                        "arithmetic operands must be numeric",
                                    )
                                    .with_types(&Type::Named("numeric".to_string()), &left_ty)
                                    .suggest("replace_arithmetic_operand"),
                                );
                            }
                            if !right_ty.is_numeric() && right_ty != Type::Unknown {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "arithmetic_type_mismatch",
                                        node,
                                        "arithmetic operands must be numeric",
                                    )
                                    .with_types(&Type::Named("numeric".to_string()), &right_ty)
                                    .suggest("replace_arithmetic_operand"),
                                );
                            }
                            Type::Unknown
                        } else if !numeric_operands_compatible(&left_ty, &right_ty) {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "arithmetic_type_mismatch",
                                    node,
                                    "arithmetic operands must have compatible numeric types",
                                )
                                .with_types(&left_ty, &right_ty)
                                .suggest("insert_explicit_cast"),
                            );
                            Type::Unknown
                        } else {
                            arithmetic_result(&left_ty, &right_ty)
                        }
                    }
                    _ => Type::Unknown,
                }
            }
            Expr::Call { name, args } => {
                if is_simd_intrinsic(name) {
                    let arg_types = args
                        .iter()
                        .map(|arg| self.infer_expr(arg, locals, node))
                        .collect::<Vec<_>>();
                    let (ty, diagnostics) = check_intrinsic_call(name, &arg_types, node);
                    self.diagnostics.extend(diagnostics);
                    return ty;
                }
                if let Some(ty) = builtin_call_type(name) {
                    for arg in args {
                        self.infer_expr(arg, locals, node);
                    }
                    return ty;
                }
                if let Some(struct_decl) = self.type_env.structs.get(name) {
                    let fields = struct_decl.fields.clone();
                    if fields.len() != args.len() {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "constructor_arity",
                                node,
                                format!("constructor '{name}' expects {} arguments", fields.len()),
                            )
                            .suggest("fix_constructor_arguments"),
                        );
                    }
                    for (field, arg) in fields.iter().zip(args.iter()) {
                        let found = self.infer_expr(arg, locals, node);
                        self.check_expr_assignable(
                            &field.ty,
                            arg,
                            &found,
                            node,
                            TypeDiagnostic::new(
                                "constructor_type_mismatch",
                                format!("constructor field '{}' has wrong type", field.name),
                                &["insert_explicit_cast", "replace_constructor_argument"],
                            ),
                        );
                    }
                    return Type::Named(name.clone());
                }
                let Some(function) = self.type_env.functions.get(name) else {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "unknown_function",
                            node,
                            format!("unknown function '{name}'"),
                        )
                        .suggest("add_function_declaration"),
                    );
                    for arg in args {
                        self.infer_expr(arg, locals, node);
                    }
                    return Type::Unknown;
                };
                let params = function.params.clone();
                let ret = function.ret.clone();
                if params.len() != args.len() {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "call_arity",
                            node,
                            format!("function '{name}' expects {} arguments", params.len()),
                        )
                        .suggest("fix_call_arguments"),
                    );
                }
                for (param, arg) in params.iter().zip(args.iter()) {
                    let found = self.infer_expr(arg, locals, node);
                    self.check_expr_assignable(
                        &param.ty,
                        arg,
                        &found,
                        node,
                        TypeDiagnostic::new(
                            "argument_type_mismatch",
                            format!("argument '{}' has wrong type", param.name),
                            &["insert_explicit_cast", "replace_call_argument"],
                        ),
                    );
                }
                ret
            }
            Expr::Cast { ty, expr } => {
                let found = self.infer_expr(expr, locals, node);
                if !is_castable(&found, ty) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "invalid_cast",
                            node,
                            "cast source and destination are incompatible",
                        )
                        .with_types(ty, &found)
                        .suggest("replace_cast_target"),
                    );
                }
                self.check_integer_literal_range(ty, expr, &found, node);
                ty.clone()
            }
            Expr::Field { base, field } => {
                let base_ty = self.infer_expr(base, locals, node);
                if field == "len" && matches!(base_ty, Type::Slice { .. } | Type::Array { .. }) {
                    return Type::Primitive("usize".to_string());
                }
                match base_ty {
                    Type::Named(name) => {
                        let Some(struct_decl) = self.type_env.structs.get(&name) else {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "unknown_struct",
                                    node,
                                    format!("unknown struct '{name}'"),
                                )
                                .suggest("add_struct_declaration"),
                            );
                            return Type::Unknown;
                        };
                        struct_decl
                            .fields
                            .iter()
                            .find(|candidate| candidate.name == *field)
                            .map(|candidate| candidate.ty.clone())
                            .unwrap_or_else(|| {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "unknown_field",
                                        node,
                                        format!("struct '{name}' has no field '{field}'"),
                                    )
                                    .suggest("add_struct_field"),
                                );
                                Type::Unknown
                            })
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "field_access_type",
                                node,
                                format!("type '{}' has no field '{field}'", base_ty.to_agent()),
                            )
                            .suggest("replace_field_access"),
                        );
                        Type::Unknown
                    }
                }
            }
            Expr::Index { base, index } => {
                let index_ty = self.infer_expr(index, locals, node);
                let usize_ty = Type::Primitive("usize".to_string());
                self.check_expr_assignable(
                    &usize_ty,
                    index,
                    &index_ty,
                    node,
                    TypeDiagnostic::new(
                        "index_not_integer",
                        "slice index must be integer",
                        &["insert_explicit_cast", "replace_index_expression"],
                    ),
                );
                self.infer_expr(base, locals, node)
                    .element_type()
                    .unwrap_or_else(|| {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "index_type",
                                node,
                                "indexed expression must be array, slice, or pointer",
                            )
                            .suggest("replace_index_base"),
                        );
                        Type::Unknown
                    })
            }
        }
    }

    fn check_expr_assignable(
        &mut self,
        expected: &Type,
        expr: &Expr,
        found: &Type,
        node: &str,
        diagnostic: TypeDiagnostic<'_>,
    ) -> bool {
        if !is_assignable(expected, found) {
            self.push_type_diagnostic(
                diagnostic.code,
                node,
                diagnostic.message,
                expected,
                found,
                diagnostic.suggestions,
            );
            return false;
        }
        self.check_integer_literal_range(expected, expr, found, node)
    }

    fn check_integer_literal_range(
        &mut self,
        expected: &Type,
        expr: &Expr,
        found: &Type,
        node: &str,
    ) -> bool {
        let Some(value) = integer_literal_value(expr) else {
            return true;
        };
        let Some((min, max)) = integer_bounds(expected) else {
            return true;
        };
        if value < min || value > max {
            self.diagnostics.push(
                Diagnostic::new(
                    "integer_literal_out_of_range",
                    node,
                    format!(
                        "integer literal {value} is outside the range {min}..{max} for {}",
                        expected.to_agent()
                    ),
                )
                .with_types(expected, found)
                .suggest("replace_literal")
                .suggest("change_destination_type"),
            );
            false
        } else {
            true
        }
    }

    fn push_type_diagnostic(
        &mut self,
        code: &str,
        node: &str,
        message: impl Into<String>,
        expected: &Type,
        found: &Type,
        suggestions: &[&str],
    ) {
        let mut diagnostic = Diagnostic::new(code, node, message).with_types(expected, found);
        for suggestion in suggestions {
            diagnostic = diagnostic.suggest(suggestion);
        }
        self.diagnostics.push(diagnostic);
    }

    fn check_safety_function(&mut self, function: &FunctionDecl) {
        let mut env = OwnershipEnv::for_function(function);
        self.check_safety_body(
            &function.body,
            &function.ret,
            &mut env,
            function,
            &BTreeSet::new(),
        );
    }

    fn check_effects(&mut self) {
        for report in analyze_effects(self.program) {
            for effect in &report.invalid {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_effect",
                        &report.id,
                        format!("effect '{effect}' is not part of the v1 effect vocabulary"),
                    )
                    .with_types(
                        &Type::Named("known_effect".to_string()),
                        &Type::Named(effect.clone()),
                    )
                    .suggest("replace_effect_name"),
                );
            }
            for effect in &report.missing {
                self.diagnostics.push(
                    Diagnostic::new(
                        "undeclared_effect",
                        &report.id,
                        format!("effect '{effect}' is inferred but not declared"),
                    )
                    .with_types(
                        &Type::Named(effect.clone()),
                        &Type::Named("inferred_effect".to_string()),
                    )
                    .suggest("add_effect_declaration"),
                );
            }
            for effect in report.declared.iter().chain(report.inferred.iter()) {
                if !report.unsafe_fn && effect_requires_unsafe(effect) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "unsafe_effect_requires_unsafe",
                            &report.id,
                            format!("effect '{effect}' requires unsafe function boundary"),
                        )
                        .with_types(
                            &Type::Named("unsafe_fn".to_string()),
                            &Type::Named("safe_fn".to_string()),
                        )
                        .suggest("mark_function_unsafe")
                        .suggest("replace_raw_memory_effect"),
                    );
                }
            }
        }
    }

    fn check_safety_body(
        &mut self,
        body: &[Stmt],
        ret: &Type,
        env: &mut OwnershipEnv,
        function: &FunctionDecl,
        outer_future_uses: &BTreeSet<String>,
    ) {
        for (idx, stmt) in body.iter().enumerate() {
            let mut future_uses = collect_stmt_var_reads(&body[idx + 1..]);
            future_uses.extend(outer_future_uses.iter().cloned());
            self.check_safety_stmt(stmt, ret, env, function, &future_uses);
        }
    }

    fn check_safety_stmt(
        &mut self,
        stmt: &Stmt,
        ret: &Type,
        env: &mut OwnershipEnv,
        function: &FunctionDecl,
        future_uses: &BTreeSet<String>,
    ) {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                let expr_ty = self.safety_expr_type(expr, env);
                let binding_ty = ty.clone().unwrap_or(expr_ty);
                let value_use = if moves_into_type(&binding_ty) {
                    ValueUse::Move
                } else {
                    ValueUse::Read
                };
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(expr, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(expr, env, function, &function.id, value_use, liveness);
                let origin = if is_borrow_type(&binding_ty) {
                    self.reference_origin(expr, env)
                } else {
                    ReferenceOrigin::Local
                };
                let borrow_root = if is_borrow_type(&binding_ty) {
                    self.borrow_root(expr, env)
                } else {
                    None
                };
                let borrow_place = if is_borrow_type(&binding_ty) {
                    self.borrow_place(expr, env)
                } else {
                    None
                };
                let borrow_parent = if is_borrow_type(&binding_ty) {
                    self.borrow_parent(expr, env)
                } else {
                    None
                };
                env.insert(
                    name.clone(),
                    binding_ty,
                    origin,
                    borrow_root,
                    borrow_place,
                    borrow_parent,
                );
            }
            Stmt::Return(Some(expr)) => {
                self.check_return_reference_safety(ret, expr, env, function);
                let value_use = if moves_into_type(ret) {
                    ValueUse::Move
                } else {
                    ValueUse::Read
                };
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(expr, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(expr, env, function, &function.id, value_use, liveness);
            }
            Stmt::Return(None) => {}
            Stmt::Require(expr) => {
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(expr, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(
                    expr,
                    env,
                    function,
                    &function.id,
                    ValueUse::Read,
                    liveness,
                );
            }
            Stmt::If { cond, then_body } => {
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(cond, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(
                    cond,
                    env,
                    function,
                    &function.id,
                    ValueUse::Read,
                    liveness,
                );
                let mut branch_env = env.clone();
                self.check_safety_body(then_body, ret, &mut branch_env, function, future_uses);
                env.merge_moved_from(&branch_env);
                env.merge_branch_borrows_from(&branch_env);
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(start, &mut current_uses);
                collect_expr_var_reads(end, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(
                    start,
                    env,
                    function,
                    &function.id,
                    ValueUse::Read,
                    liveness,
                );
                self.check_expr_ownership(
                    end,
                    env,
                    function,
                    &function.id,
                    ValueUse::Read,
                    liveness,
                );
                let mut loop_env = env.clone();
                loop_env.insert(
                    var.clone(),
                    Type::Primitive("usize".to_string()),
                    ReferenceOrigin::Local,
                    None,
                    None,
                    None,
                );
                let outer_bindings = env.bindings.keys().cloned().collect::<BTreeSet<_>>();
                let mut loop_future_uses = future_uses.clone();
                let mut loop_carried_uses = collect_stmt_var_reads(body);
                loop_carried_uses.retain(|name| outer_bindings.contains(name));
                loop_future_uses.extend(loop_carried_uses);
                self.check_safety_body(body, ret, &mut loop_env, function, &loop_future_uses);
                env.merge_moved_from(&loop_env);
            }
            Stmt::Assign { target, expr } => {
                let expected = self.safety_expr_type(target, env);
                let value_use = if moves_into_type(&expected) {
                    ValueUse::Move
                } else {
                    ValueUse::Read
                };
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(expr, &mut current_uses);
                collect_assignment_target_reads(target, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(expr, env, function, &function.id, value_use, liveness);
                self.check_lvalue_safety(target, env, function, &function.id, liveness);
                if let Expr::Var(name) = target {
                    let origin = if is_borrow_type(&expected) {
                        self.reference_origin(expr, env)
                    } else {
                        ReferenceOrigin::Local
                    };
                    let borrow_root = if is_borrow_type(&expected) {
                        self.borrow_root(expr, env)
                    } else {
                        None
                    };
                    let borrow_place = if is_borrow_type(&expected) {
                        self.borrow_place(expr, env)
                    } else {
                        None
                    };
                    let borrow_parent = if is_borrow_type(&expected) {
                        self.borrow_parent(expr, env)
                    } else {
                        None
                    };
                    env.mark_initialized(name, origin, borrow_root, borrow_place, borrow_parent);
                } else if let Some((root, path)) = move_path(target) {
                    env.mark_path_initialized(&root, &path);
                }
            }
            Stmt::Expr(expr) => {
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(expr, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(
                    expr,
                    env,
                    function,
                    &function.id,
                    ValueUse::Read,
                    liveness,
                );
            }
            Stmt::Asm(asm) => {
                for output in &asm.outputs {
                    env.insert(
                        output.value.clone(),
                        Type::Unknown,
                        ReferenceOrigin::Unknown,
                        None,
                        None,
                        None,
                    );
                }
            }
        }
    }

    fn check_expr_ownership(
        &mut self,
        expr: &Expr,
        env: &mut OwnershipEnv,
        function: &FunctionDecl,
        node: &str,
        value_use: ValueUse,
        liveness: BorrowLiveness<'_>,
    ) {
        match expr {
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => {}
            Expr::Var(name) => {
                self.check_move_path_available(name, &[], env, node);
                if value_use == ValueUse::Move {
                    if let Some(binding) = env.get(name) {
                        if moves_into_type(&binding.ty) {
                            env.mark_moved(name);
                        }
                    }
                }
            }
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
                self.check_expr_ownership(expr, env, function, node, ValueUse::Read, liveness);
            }
            Expr::Binary { left, right, .. } => {
                self.check_expr_ownership(left, env, function, node, ValueUse::Read, liveness);
                self.check_expr_ownership(right, env, function, node, ValueUse::Read, liveness);
            }
            Expr::Call { name, args } => {
                self.check_call_ownership(name, args, env, function, node, liveness);
            }
            Expr::Field { base, .. } => {
                if let Some((root, path)) = move_path(expr) {
                    self.check_move_path_available(&root, &path, env, node);
                    if value_use == ValueUse::Move {
                        let field_ty = self.safety_expr_type(expr, env);
                        if moves_into_type(&field_ty) {
                            env.mark_path_moved(&root, &path);
                        }
                    }
                } else {
                    self.check_expr_ownership(base, env, function, node, ValueUse::Read, liveness);
                }
            }
            Expr::Index { base, index } => {
                self.check_raw_pointer_index(base, env, function, node);
                self.check_expr_ownership(base, env, function, node, ValueUse::Read, liveness);
                self.check_expr_ownership(index, env, function, node, ValueUse::Read, liveness);
            }
        }
    }

    fn check_call_ownership(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut OwnershipEnv,
        function: &FunctionDecl,
        node: &str,
        liveness: BorrowLiveness<'_>,
    ) {
        let arg_types = args
            .iter()
            .map(|arg| self.safety_expr_type(arg, env))
            .collect::<Vec<_>>();
        let callee_effects = self
            .type_env
            .functions
            .get(name)
            .map(|callee| callee.effects.clone())
            .unwrap_or_default();
        if let Some(reason) =
            secret_call_violation(name, &arg_types, &callee_effects, function.unsafe_fn)
        {
            self.diagnostics.push(
                Diagnostic::new(
                    "secret_leak",
                    node,
                    format!("secret value passed to '{name}' would {reason}"),
                )
                .with_types(
                    &Type::Named("secret-safe API".to_string()),
                    &Type::Named("secret sink".to_string()),
                )
                .suggest("use_approved_crypto_api")
                .suggest("mark_function_unsafe_for_secret_export"),
            );
        }

        if is_cpu_feature_probe(name) || name == "aligned" {
            for arg in args {
                self.check_expr_ownership(arg, env, function, node, ValueUse::Read, liveness);
            }
            return;
        }

        if let Some(struct_decl) = self.type_env.structs.get(name) {
            let fields = struct_decl.fields.clone();
            for (field, arg) in fields.iter().zip(args.iter()) {
                let value_use = if moves_into_type(&field.ty) {
                    ValueUse::Move
                } else {
                    ValueUse::Read
                };
                self.check_expr_ownership(arg, env, function, node, value_use, liveness);
            }
            return;
        }

        let Some(callee) = self.type_env.functions.get(name) else {
            for arg in args {
                self.check_expr_ownership(arg, env, function, node, ValueUse::Read, liveness);
            }
            return;
        };
        let params = callee.params.clone();
        self.check_call_borrow_aliases(name, &params, args, env, node);
        self.check_call_borrow_activations(name, &params, args, env, node, liveness);
        for (param, arg) in params.iter().zip(args.iter()) {
            if is_raw_pointer_type(&param.ty) && !function.unsafe_fn {
                self.diagnostics.push(
                    Diagnostic::new(
                        "raw_pointer_requires_unsafe",
                        node,
                        format!("raw pointer argument '{}' requires unsafe code", param.name),
                    )
                    .with_types(&param.ty, &param.ty)
                    .suggest("mark_function_unsafe")
                    .suggest("replace_pointer_with_slice"),
                );
            }
            let value_use = if moves_into_type(&param.ty) {
                ValueUse::Move
            } else {
                ValueUse::Read
            };
            self.check_expr_ownership(arg, env, function, node, value_use, liveness);
        }
    }

    fn check_call_borrow_aliases(
        &mut self,
        name: &str,
        params: &[crate::ast::Param],
        args: &[Expr],
        env: &OwnershipEnv,
        node: &str,
    ) {
        #[derive(Clone, Copy, Debug, PartialEq)]
        enum BorrowKind {
            Shared,
            Mutable,
        }

        let mut active: Vec<(Vec<BorrowPlace>, BorrowKind)> = Vec::new();
        for (param, arg) in params.iter().zip(args.iter()) {
            if !is_borrow_type(&param.ty) {
                continue;
            }
            let places = self.borrow_places(arg, env);
            if places.is_empty() {
                continue;
            };
            let requested = if is_mutable_borrow_type(&param.ty) {
                BorrowKind::Mutable
            } else {
                BorrowKind::Shared
            };
            let conflict = active.iter().any(|(active_places, active_kind)| {
                place_sets_overlap(active_places, &places)
                    && matches!(
                        (*active_kind, requested),
                        (BorrowKind::Mutable, _) | (BorrowKind::Shared, BorrowKind::Mutable)
                    )
            });
            if conflict {
                let place_label = place_set_label(&places);
                self.diagnostics.push(
                    Diagnostic::new(
                        "mutable_borrow_alias",
                        node,
                        format!("call to '{name}' aliases mutable borrow of '{place_label}'"),
                    )
                    .with_types(
                        &Type::Named("unique_mutable_borrow".to_string()),
                        &Type::Named("aliased_borrow".to_string()),
                    )
                    .suggest("split_slice_or_copy_input")
                    .suggest("avoid_aliasing_mutable_argument"),
                );
            }
            active.push((places, requested));
        }
    }

    fn check_call_borrow_activations(
        &mut self,
        name: &str,
        params: &[crate::ast::Param],
        args: &[Expr],
        env: &OwnershipEnv,
        node: &str,
        liveness: BorrowLiveness<'_>,
    ) {
        let Some(mutated_params) = self.mutated_borrow_param_indices_by_name(name) else {
            return;
        };
        for (idx, (param, arg)) in params.iter().zip(args.iter()).enumerate() {
            if !is_mutable_borrow_type(&param.ty) {
                continue;
            }
            if !mutated_params.contains(&idx) {
                continue;
            }
            self.check_live_borrow_alias_activation(
                arg,
                env,
                node,
                liveness,
                &format!("mutable call argument for '{name}'"),
            );
        }
    }

    fn check_lvalue_safety(
        &mut self,
        target: &Expr,
        env: &mut OwnershipEnv,
        function: &FunctionDecl,
        node: &str,
        liveness: BorrowLiveness<'_>,
    ) {
        match target {
            Expr::Var(_) => {}
            Expr::Field { base, .. } => {
                if let Some((root, path)) = move_path(target) {
                    self.check_move_path_assignable(&root, &path, env, node);
                    self.check_live_borrow_alias_write(target, env, node, liveness);
                } else {
                    self.check_expr_ownership(base, env, function, node, ValueUse::Read, liveness);
                }
            }
            Expr::Index { base, index } => {
                self.check_raw_pointer_index(base, env, function, node);
                self.check_live_borrow_alias_write(target, env, node, liveness);
                self.check_expr_ownership(base, env, function, node, ValueUse::Read, liveness);
                self.check_expr_ownership(index, env, function, node, ValueUse::Read, liveness);
            }
            _ => {
                self.check_expr_ownership(target, env, function, node, ValueUse::Read, liveness);
            }
        }
    }

    fn check_return_reference_safety(
        &mut self,
        ret: &Type,
        expr: &Expr,
        env: &OwnershipEnv,
        function: &FunctionDecl,
    ) {
        if is_borrow_type(ret) {
            let origin = self.reference_origin(expr, env);
            if origin != ReferenceOrigin::Parameter {
                let found = self.safety_expr_type(expr, env);
                self.diagnostics.push(
                    Diagnostic::new(
                        "escaping_reference",
                        &function.id,
                        "returned reference is not proven to outlive the function",
                    )
                    .with_types(ret, &found)
                    .suggest("return_owned_value")
                    .suggest("add_lifetime_parameter_later"),
                );
            }
        }
        if is_raw_pointer_type(ret) && !function.unsafe_fn {
            self.diagnostics.push(
                Diagnostic::new(
                    "raw_pointer_requires_unsafe",
                    &function.id,
                    "returning a raw pointer requires unsafe code",
                )
                .with_types(ret, ret)
                .suggest("mark_function_unsafe")
                .suggest("replace_pointer_with_slice"),
            );
        }
    }

    fn check_raw_pointer_index(
        &mut self,
        base: &Expr,
        env: &OwnershipEnv,
        function: &FunctionDecl,
        node: &str,
    ) {
        let base_ty = self.safety_expr_type(base, env);
        if is_raw_pointer_type(&base_ty) && !function.unsafe_fn {
            self.diagnostics.push(
                Diagnostic::new(
                    "raw_pointer_dereference_requires_unsafe",
                    node,
                    "raw pointer indexing is a dereference and requires unsafe code",
                )
                .with_types(&Type::Named("safe_reference".to_string()), &base_ty)
                .suggest("mark_function_unsafe")
                .suggest("replace_pointer_with_slice"),
            );
        }
    }

    fn check_live_borrow_alias_write(
        &mut self,
        base: &Expr,
        env: &OwnershipEnv,
        node: &str,
        liveness: BorrowLiveness<'_>,
    ) {
        self.check_live_borrow_alias_activation(base, env, node, liveness, "write through");
    }

    fn check_live_borrow_alias_activation(
        &mut self,
        base: &Expr,
        env: &OwnershipEnv,
        node: &str,
        liveness: BorrowLiveness<'_>,
        action: &str,
    ) {
        let activation_places = self.activation_places(base, env);
        if activation_places.is_empty() {
            return;
        };
        let root = activation_places
            .first()
            .map(|place| place.root.clone())
            .unwrap_or_default();
        let writer = root_var(base);
        for (name, binding) in &env.bindings {
            if Some(name.as_str()) == writer {
                continue;
            }
            if !self.borrow_alias_is_active_for_writer(writer, name, env, liveness) {
                continue;
            }
            let alias_places = binding_borrow_places(name, binding);
            if is_borrow_type(&binding.ty) && place_sets_overlap(&activation_places, &alias_places)
            {
                let alias_label = place_set_label(&alias_places);
                let writer_label = writer.unwrap_or(root.as_str());
                let message = if action == "write through" {
                    format!(
                        "write through '{writer_label}' conflicts with live borrow alias '{name}' of '{alias_label}'"
                    )
                } else {
                    format!(
                        "{action} '{writer_label}' conflicts with live borrow alias '{name}' of '{alias_label}'"
                    )
                };
                self.diagnostics.push(
                    Diagnostic::new("mutable_borrow_alias", node, message)
                        .with_types(
                            &Type::Named("unique_mutable_borrow".to_string()),
                            &Type::Named("live_borrow_alias".to_string()),
                        )
                        .suggest("avoid_aliasing_mutable_argument")
                        .suggest("reassign_borrow_alias_before_write"),
                );
            }
        }
    }

    fn borrow_alias_is_active_for_writer(
        &self,
        writer: Option<&str>,
        alias: &str,
        env: &OwnershipEnv,
        liveness: BorrowLiveness<'_>,
    ) -> bool {
        let current = liveness.current.contains(alias);
        let future = liveness.future.contains(alias);
        if !current && !future {
            return false;
        }
        let Some(writer) = writer else {
            return current || future;
        };
        if alias == writer {
            return false;
        }
        if is_reborrow_descendant(writer, alias, env) {
            return current;
        }
        if env
            .get(writer)
            .filter(|binding| is_borrow_type(&binding.ty))
            .map(|binding| {
                binding_borrow_places(writer, binding)
                    .iter()
                    .any(|place| place.root == alias)
            })
            .unwrap_or(false)
        {
            return current;
        }
        current || future
    }

    fn check_move_path_available(
        &mut self,
        name: &str,
        path: &[String],
        env: &OwnershipEnv,
        node: &str,
    ) {
        if let Some(binding) = env.get(name) {
            if binding.moved {
                self.push_use_after_move(
                    node,
                    move_path_label(name, path),
                    binding.ty.clone(),
                    "moved_value",
                );
                return;
            }
            if let Some(moved_path) = move_path_conflict(path, &binding.moved_paths) {
                let moved_segments = split_move_path(moved_path);
                let expected_path = if path_is_prefix(&moved_segments, path) {
                    moved_segments
                } else {
                    path.to_vec()
                };
                let label = move_path_label(name, path);
                let message_label = if path.is_empty() {
                    format!("value '{name}' is used after field '{moved_path}' was moved")
                } else {
                    format!("value '{label}' is used after it was moved")
                };
                self.push_use_after_move_with_message(
                    node,
                    message_label,
                    self.move_path_type(&binding.ty, &expected_path),
                    if path.is_empty() {
                        "partially_moved_value"
                    } else {
                        "moved_value"
                    },
                );
            }
        }
    }

    fn check_move_path_assignable(
        &mut self,
        name: &str,
        path: &[String],
        env: &OwnershipEnv,
        node: &str,
    ) {
        if let Some(binding) = env.get(name) {
            if binding.moved {
                self.push_use_after_move(
                    node,
                    move_path_label(name, path),
                    binding.ty.clone(),
                    "moved_value",
                );
                return;
            }
            if let Some(moved_path) = move_path_assignment_conflict(path, &binding.moved_paths) {
                self.push_use_after_move_with_message(
                    node,
                    format!(
                        "field '{}' cannot be assigned because parent field '{moved_path}' was moved",
                        move_path_label(name, path)
                    ),
                    self.move_path_type(&binding.ty, path),
                    "moved_value",
                );
            }
        }
    }

    fn push_use_after_move(&mut self, node: &str, label: String, expected: Type, found: &str) {
        self.push_use_after_move_with_message(
            node,
            format!("value '{label}' is used after it was moved"),
            expected,
            found,
        );
    }

    fn push_use_after_move_with_message(
        &mut self,
        node: &str,
        message: String,
        expected: Type,
        found: &str,
    ) {
        self.diagnostics.push(
            Diagnostic::new("use_after_move", node, message)
                .with_types(&expected, &Type::Named(found.to_string()))
                .suggest("reorder_use_before_move")
                .suggest("pass_by_reference")
                .suggest("reinitialize_moved_field"),
        );
    }

    fn move_path_type(&self, root_ty: &Type, path: &[String]) -> Type {
        let mut ty = root_ty.clone();
        for field in path {
            if field == "len" && matches!(ty, Type::Slice { .. } | Type::Array { .. }) {
                ty = Type::Primitive("usize".to_string());
                continue;
            }
            ty = match ty {
                Type::Named(name) => self
                    .type_env
                    .structs
                    .get(&name)
                    .and_then(|decl| {
                        decl.fields
                            .iter()
                            .find(|candidate| candidate.name == *field)
                    })
                    .map(|field| field.ty.clone())
                    .unwrap_or(Type::Unknown),
                _ => Type::Unknown,
            };
        }
        ty
    }

    fn reference_origin(&self, expr: &Expr, env: &OwnershipEnv) -> ReferenceOrigin {
        match expr {
            Expr::Var(name) => env
                .get(name)
                .map(|binding| binding.origin.clone())
                .unwrap_or(ReferenceOrigin::Unknown),
            Expr::Call { name, args } => self
                .returned_borrow_param_indices_by_name(name)
                .and_then(|indices| {
                    if indices.iter().all(|idx| {
                        args.get(*idx)
                            .map(|arg| {
                                self.reference_origin(arg, env) == ReferenceOrigin::Parameter
                            })
                            .unwrap_or(false)
                    }) {
                        Some(ReferenceOrigin::Parameter)
                    } else {
                        None
                    }
                })
                .unwrap_or(ReferenceOrigin::Unknown),
            Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => self.reference_origin(expr, env),
            Expr::Field { .. } | Expr::Index { .. } => ReferenceOrigin::Unknown,
            _ => ReferenceOrigin::Local,
        }
    }

    fn borrow_root(&self, expr: &Expr, env: &OwnershipEnv) -> Option<String> {
        self.borrow_place(expr, env).map(|place| place.root)
    }

    fn borrow_place(&self, expr: &Expr, env: &OwnershipEnv) -> Option<BorrowPlace> {
        self.borrow_places(expr, env).into_iter().next()
    }

    fn borrow_places(&self, expr: &Expr, env: &OwnershipEnv) -> Vec<BorrowPlace> {
        match expr {
            Expr::Var(name) => env
                .get(name)
                .and_then(|binding| {
                    let places = binding_borrow_places(name, binding);
                    if places.is_empty() {
                        None
                    } else {
                        Some(places)
                    }
                })
                .unwrap_or_default(),
            Expr::Field { .. } | Expr::Index { .. } => self.places_for_expr(expr, env),
            Expr::Call { name, args } => self
                .returned_borrow_param_indices_by_name(name)
                .and_then(|indices| single_borrow_param_index(&indices))
                .and_then(|idx| args.get(idx))
                .map(|arg| self.borrow_places(arg, env))
                .unwrap_or_default(),
            Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => self.borrow_places(expr, env),
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => Vec::new(),
            Expr::Binary { .. } => Vec::new(),
        }
    }

    fn activation_places(&self, expr: &Expr, env: &OwnershipEnv) -> Vec<BorrowPlace> {
        let places = self.places_for_expr(expr, env);
        if places.is_empty() {
            self.borrow_places(expr, env)
        } else {
            places
        }
    }

    fn places_for_expr(&self, expr: &Expr, env: &OwnershipEnv) -> Vec<BorrowPlace> {
        match expr {
            Expr::Var(name) => env
                .get(name)
                .map(|binding| {
                    let places = binding_borrow_places(name, binding);
                    if places.is_empty() {
                        vec![BorrowPlace::root(name.clone())]
                    } else {
                        places
                    }
                })
                .unwrap_or_default(),
            Expr::Field { base, field } => self
                .places_for_expr(base, env)
                .into_iter()
                .map(|place| place.with_projection(PlaceProjection::Field(field.clone())))
                .collect(),
            Expr::Index { base, index } => self
                .places_for_expr(base, env)
                .into_iter()
                .map(|place| {
                    let projection = integer_literal_value(index)
                        .map(PlaceProjection::IndexConst)
                        .unwrap_or(PlaceProjection::IndexUnknown);
                    place.with_projection(projection)
                })
                .collect(),
            Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => self.places_for_expr(expr, env),
            Expr::Call { .. }
            | Expr::Binary { .. }
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_) => Vec::new(),
        }
    }

    fn borrow_parent(&self, expr: &Expr, env: &OwnershipEnv) -> Option<String> {
        match expr {
            Expr::Var(name) => env
                .get(name)
                .filter(|binding| is_borrow_type(&binding.ty))
                .map(|_| name.clone()),
            Expr::Field { base, .. } | Expr::Index { base, .. } => {
                root_var(base).and_then(|name| {
                    env.get(name)
                        .filter(|binding| is_borrow_type(&binding.ty))
                        .map(|_| name.to_string())
                })
            }
            Expr::Call { name, args } => self
                .returned_borrow_param_indices_by_name(name)
                .and_then(|indices| single_borrow_param_index(&indices))
                .and_then(|idx| args.get(idx))
                .and_then(|arg| {
                    self.borrow_parent(arg, env)
                        .or_else(|| root_var(arg).map(str::to_string))
                }),
            Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => self.borrow_parent(expr, env),
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => None,
            Expr::Binary { .. } => None,
        }
    }

    fn returned_borrow_param_indices_by_name(&self, name: &str) -> Option<BTreeSet<usize>> {
        let function = self.type_env.functions.get(name)?;
        lifetimes::returned_borrow_param_indices(function, &self.type_env.functions)
    }

    fn mutated_borrow_param_indices_by_name(&self, name: &str) -> Option<BTreeSet<usize>> {
        let function = self.type_env.functions.get(name)?;
        Some(lifetimes::mutated_borrow_param_indices(
            function,
            &self.type_env.functions,
        ))
    }

    fn safety_expr_type(&self, expr: &Expr, env: &OwnershipEnv) -> Type {
        match expr {
            Expr::Int(_) => Type::Primitive("$int".to_string()),
            Expr::Float(_) => Type::Primitive("$float".to_string()),
            Expr::Bool(_) => Type::Bool,
            Expr::String(_) => Type::Primitive("str".to_string()),
            Expr::Var(name) => env
                .get(name)
                .map(|binding| binding.ty.clone())
                .unwrap_or(Type::Unknown),
            Expr::Unary { op, expr } => {
                if op == "!" {
                    Type::Bool
                } else {
                    self.safety_expr_type(expr, env)
                }
            }
            Expr::Binary { op, left, right } => match op.as_str() {
                "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" => Type::Bool,
                _ => {
                    let left_ty = self.safety_expr_type(left, env);
                    let right_ty = self.safety_expr_type(right, env);
                    if left_ty.is_numeric() && right_ty.is_numeric() {
                        arithmetic_result(&left_ty, &right_ty)
                    } else {
                        Type::Unknown
                    }
                }
            },
            Expr::Call { name, .. } => {
                if let Some(ty) = builtin_call_type(name) {
                    ty
                } else if self.type_env.structs.contains_key(name) {
                    Type::Named(name.clone())
                } else {
                    self.type_env
                        .functions
                        .get(name)
                        .map(|function| function.ret.clone())
                        .unwrap_or(Type::Unknown)
                }
            }
            Expr::Cast { ty, .. } => ty.clone(),
            Expr::Field { base, field } => {
                let base_ty = self.safety_expr_type(base, env);
                if field == "len" && matches!(base_ty, Type::Slice { .. } | Type::Array { .. }) {
                    return Type::Primitive("usize".to_string());
                }
                match base_ty {
                    Type::Named(name) => self
                        .type_env
                        .structs
                        .get(&name)
                        .and_then(|decl| {
                            decl.fields
                                .iter()
                                .find(|candidate| candidate.name == *field)
                        })
                        .map(|field| field.ty.clone())
                        .unwrap_or(Type::Unknown),
                    _ => Type::Unknown,
                }
            }
            Expr::Index { base, .. } => self
                .safety_expr_type(base, env)
                .element_type()
                .unwrap_or(Type::Unknown),
        }
    }

    fn is_mutable_index_target(&mut self, expr: &Expr, locals: &LocalTypeEnv) -> bool {
        match expr {
            Expr::Index { base, .. } => match base.as_ref() {
                Expr::Var(name) => locals
                    .get(name)
                    .map(is_mutable_index_target_type)
                    .unwrap_or(false),
                _ => false,
            },
            _ => false,
        }
    }
}

fn is_assignable(expected: &Type, found: &Type) -> bool {
    if expected == found || *expected == Type::Unknown || *found == Type::Unknown {
        return true;
    }
    if matches!(found, Type::Primitive(name) if name == "$int")
        && (expected.is_integer() || expected.is_float())
    {
        return true;
    }
    if matches!(found, Type::Primitive(name) if name == "$float") && expected.is_float() {
        return true;
    }
    if let (
        Type::Slice {
            mutable: false,
            elem: expected_elem,
        },
        Type::Slice {
            mutable: true,
            elem: found_elem,
        },
    ) = (expected, found)
    {
        return expected_elem == found_elem;
    }
    false
}

fn is_borrow_type(ty: &Type) -> bool {
    matches!(ty, Type::Slice { .. })
}

fn is_mutable_borrow_type(ty: &Type) -> bool {
    matches!(ty, Type::Slice { mutable: true, .. })
}

fn is_raw_pointer_type(ty: &Type) -> bool {
    matches!(ty, Type::Pointer { .. })
}

fn is_copy_type(ty: &Type) -> bool {
    match ty {
        Type::Bool | Type::Primitive(_) | Type::Slice { .. } | Type::Pointer { .. } => true,
        Type::Void | Type::Unknown => true,
        Type::Named(_) | Type::Generic { .. } | Type::Array { .. } => false,
    }
}

fn is_secret_type(ty: &Type) -> bool {
    matches!(ty, Type::Generic { name, .. } if name == "SecretKey")
}

fn secret_call_violation(
    name: &str,
    arg_types: &[Type],
    callee_effects: &[String],
    unsafe_fn: bool,
) -> Option<&'static str> {
    if unsafe_fn || !arg_types.iter().any(is_secret_type) || is_approved_secret_api(name) {
        return None;
    }
    if name.contains("log") || name.contains("print") || name.contains("debug") {
        return Some("log or print secret material");
    }
    if name.contains("serialize") || name.contains("export") {
        return Some("serialize secret material");
    }
    if name.contains("clone") || name.contains("copy") {
        return Some("copy secret material");
    }
    if callee_effects
        .iter()
        .any(|effect| matches!(effect.as_str(), "serialize" | "stream" | "io.write"))
    {
        return Some("send secret material to a serialization or output boundary");
    }
    None
}

fn is_approved_secret_api(name: &str) -> bool {
    matches!(
        name,
        "core_crypto_sign_ed25519"
            | "core_runtime_crypto_sign_ed25519"
            | "core_crypto_public_key_ed25519"
            | "core_runtime_crypto_public_key_ed25519"
    )
}

fn moves_into_type(ty: &Type) -> bool {
    !is_copy_type(ty) && !is_borrow_type(ty)
}

fn is_mutable_index_target_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Slice { mutable: true, .. } | Type::Pointer { mutable: true, .. }
    )
}

fn collect_stmt_var_reads(body: &[Stmt]) -> BTreeSet<String> {
    let mut reads = BTreeSet::new();
    for stmt in body {
        collect_stmt_var_reads_into(stmt, &mut reads);
    }
    reads
}

fn collect_stmt_var_reads_into(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match stmt {
        Stmt::Let { expr, .. } => collect_expr_var_reads(expr, out),
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            collect_expr_var_reads(expr, out)
        }
        Stmt::Return(None) | Stmt::Asm(_) => {}
        Stmt::If { cond, then_body } => {
            collect_expr_var_reads(cond, out);
            for stmt in then_body {
                collect_stmt_var_reads_into(stmt, out);
            }
        }
        Stmt::For {
            start, end, body, ..
        } => {
            collect_expr_var_reads(start, out);
            collect_expr_var_reads(end, out);
            for stmt in body {
                collect_stmt_var_reads_into(stmt, out);
            }
        }
        Stmt::Assign { target, expr } => {
            collect_assignment_target_reads(target, out);
            collect_expr_var_reads(expr, out);
        }
    }
}

fn collect_assignment_target_reads(target: &Expr, out: &mut BTreeSet<String>) {
    match target {
        Expr::Var(_) => {}
        Expr::Index { base, index } => {
            collect_expr_var_reads(base, out);
            collect_expr_var_reads(index, out);
        }
        Expr::Field { base, .. } => collect_assignment_target_reads(base, out),
        Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => {
            collect_assignment_target_reads(expr, out)
        }
        _ => collect_expr_var_reads(target, out),
    }
}

fn collect_expr_var_reads(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Var(name) => {
            out.insert(name.clone());
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => collect_expr_var_reads(expr, out),
        Expr::Binary { left, right, .. } => {
            collect_expr_var_reads(left, out);
            collect_expr_var_reads(right, out);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_var_reads(arg, out);
            }
        }
        Expr::Field { base, .. } => collect_expr_var_reads(base, out),
        Expr::Index { base, index } => {
            collect_expr_var_reads(base, out);
            collect_expr_var_reads(index, out);
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => {}
    }
}

fn root_var(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var(name) => Some(name),
        Expr::Field { base, .. } | Expr::Index { base, .. } => root_var(base),
        _ => None,
    }
}

fn binding_borrow_places(name: &str, binding: &OwnershipBinding) -> Vec<BorrowPlace> {
    let mut places = Vec::new();
    if let Some(place) = binding
        .borrow_place
        .clone()
        .or_else(|| binding.borrow_root.clone().map(BorrowPlace::root))
        .or_else(|| {
            if is_borrow_type(&binding.ty) {
                Some(BorrowPlace::root(name.to_string()))
            } else {
                None
            }
        })
    {
        push_unique_place(&mut places, place);
    }
    for place in &binding.borrow_alternates {
        push_unique_place(&mut places, place.clone());
    }
    places
}

fn push_unique_place(places: &mut Vec<BorrowPlace>, place: BorrowPlace) {
    if !places.contains(&place) {
        places.push(place);
    }
}

fn place_sets_overlap(left: &[BorrowPlace], right: &[BorrowPlace]) -> bool {
    left.iter().any(|left_place| {
        right
            .iter()
            .any(|right_place| places_overlap(left_place, right_place))
    })
}

fn place_set_label(places: &[BorrowPlace]) -> String {
    match places {
        [] => "unknown".to_string(),
        [place] => place.label(),
        _ => places
            .iter()
            .map(BorrowPlace::label)
            .collect::<Vec<_>>()
            .join("|"),
    }
}

fn is_reborrow_descendant(child: &str, ancestor: &str, env: &OwnershipEnv) -> bool {
    let mut current = Some(child);
    let mut seen = BTreeSet::new();
    while let Some(name) = current {
        if name == ancestor {
            return true;
        }
        if !seen.insert(name.to_string()) {
            return false;
        }
        current = env
            .get(name)
            .and_then(|binding| binding.borrow_parent.as_deref());
    }
    false
}

fn places_overlap(left: &BorrowPlace, right: &BorrowPlace) -> bool {
    if left.root != right.root {
        return false;
    }
    for (left_projection, right_projection) in left.projections.iter().zip(right.projections.iter())
    {
        match (left_projection, right_projection) {
            (PlaceProjection::Field(left_field), PlaceProjection::Field(right_field)) => {
                if left_field != right_field {
                    return false;
                }
            }
            (PlaceProjection::IndexConst(left_index), PlaceProjection::IndexConst(right_index)) => {
                if left_index != right_index {
                    return false;
                }
            }
            (PlaceProjection::IndexUnknown, _)
            | (_, PlaceProjection::IndexUnknown)
            | (PlaceProjection::Field(_), PlaceProjection::IndexConst(_))
            | (PlaceProjection::IndexConst(_), PlaceProjection::Field(_)) => return true,
        }
    }
    true
}

fn move_path(expr: &Expr) -> Option<(String, Vec<String>)> {
    let mut path = Vec::new();
    let root = collect_move_path(expr, &mut path)?;
    Some((root, path))
}

fn collect_move_path(expr: &Expr, path: &mut Vec<String>) -> Option<String> {
    match expr {
        Expr::Var(name) => Some(name.clone()),
        Expr::Field { base, field } => {
            let root = collect_move_path(base, path)?;
            path.push(field.clone());
            Some(root)
        }
        _ => None,
    }
}

fn encode_move_path(path: &[String]) -> String {
    path.join(".")
}

fn split_move_path(path: &str) -> Vec<String> {
    path.split('.').map(|segment| segment.to_string()).collect()
}

fn move_path_label(root: &str, path: &[String]) -> String {
    if path.is_empty() {
        root.to_string()
    } else {
        format!("{root}.{}", encode_move_path(path))
    }
}

fn move_path_conflict<'a>(
    requested: &[String],
    moved_paths: &'a BTreeSet<String>,
) -> Option<&'a str> {
    for moved_path in moved_paths {
        let moved = split_move_path(moved_path);
        if requested.is_empty()
            || path_is_prefix(&moved, requested)
            || path_is_prefix(requested, &moved)
        {
            return Some(moved_path.as_str());
        }
    }
    None
}

fn move_path_assignment_conflict<'a>(
    assigned: &[String],
    moved_paths: &'a BTreeSet<String>,
) -> Option<&'a str> {
    for moved_path in moved_paths {
        let moved = split_move_path(moved_path);
        if moved.len() < assigned.len() && path_is_prefix(&moved, assigned) {
            return Some(moved_path.as_str());
        }
    }
    None
}

fn encoded_path_is_descendant(candidate: &str, parent: &str) -> bool {
    candidate
        .strip_prefix(parent)
        .is_some_and(|remaining| remaining.starts_with('.'))
}

fn path_is_prefix(prefix: &[String], path: &[String]) -> bool {
    prefix.len() <= path.len()
        && prefix
            .iter()
            .zip(path.iter())
            .all(|(left, right)| left == right)
}

fn single_borrow_param_index(indices: &BTreeSet<usize>) -> Option<usize> {
    if indices.len() == 1 {
        indices.iter().next().copied()
    } else {
        None
    }
}

fn builtin_call_type(name: &str) -> Option<Type> {
    if let Some(ty) = standard_builtin_call_type(name) {
        return Some(ty);
    }
    if is_cpu_feature_probe(name) {
        return Some(Type::Bool);
    }
    match name {
        "Some" | "None" | "Ok" | "Err" | "unwrap_or" => Some(Type::Unknown),
        "is_some" | "is_none" | "is_ok" | "is_err" => Some(Type::Bool),
        "aligned" | "crypto_verify" => Some(Type::Bool),
        "core_fs_scope_read"
        | "core_runtime_fs_scope_read"
        | "core_fs_scope_write"
        | "core_runtime_fs_scope_write" => Some(Type::Bool),
        "core_fs_read_text"
        | "core_runtime_fs_read_text"
        | "core_net_http_get"
        | "core_runtime_net_http_get"
        | "core_net_http_post"
        | "core_runtime_net_http_post" => Some(Type::Generic {
            name: "Result".to_string(),
            args: vec![
                Type::Primitive("str".to_string()),
                Type::Primitive("str".to_string()),
            ],
        }),
        "core_fs_write_text"
        | "core_runtime_fs_write_text"
        | "core_fs_copy_text"
        | "core_runtime_fs_copy_text"
        | "core_net_tcp_send"
        | "core_runtime_net_tcp_send"
        | "core_net_udp_send"
        | "core_runtime_net_udp_send"
        | "core_net_websocket_send"
        | "core_runtime_net_websocket_send" => Some(Type::Generic {
            name: "Result".to_string(),
            args: vec![
                Type::Primitive("usize".to_string()),
                Type::Primitive("str".to_string()),
            ],
        }),
        "core_serialize_text_len"
        | "core_runtime_serialize_text_len"
        | "core_stream_count_bytes"
        | "core_runtime_stream_count_bytes" => Some(Type::Primitive("usize".to_string())),
        "read_file" | "file_read" | "io_read" | "http_get" | "net_recv" | "network_read"
        | "tcp_read" | "crypto_sign" | "crypto_hash" => Some(Type::Unknown),
        "write_file" | "file_write" | "io_write" | "http_post" | "net_send" | "network_write"
        | "tcp_write" | "free" => Some(Type::Void),
        "alloc" | "alloc_buffer" | "realloc" => Some(Type::Pointer {
            mutable: true,
            elem: Box::new(Type::Primitive("u8".to_string())),
        }),
        _ if name.starts_with("alloc_")
            || name.starts_with("network_")
            || name.starts_with("net_")
            || name.starts_with("crypto_")
            || name.starts_with("simd_") =>
        {
            Some(Type::Unknown)
        }
        _ => None,
    }
}

fn is_castable(found: &Type, target: &Type) -> bool {
    if *found == Type::Unknown || *target == Type::Unknown || found == target {
        return true;
    }
    found.is_numeric() && target.is_numeric()
}

fn are_comparable(left: &Type, right: &Type) -> bool {
    is_assignable(left, right) || is_assignable(right, left)
}

fn numeric_operands_compatible(left: &Type, right: &Type) -> bool {
    left == right || is_assignable(left, right) || is_assignable(right, left)
}

fn arithmetic_result(left: &Type, right: &Type) -> Type {
    if matches!(left, Type::Primitive(name) if name == "$float")
        && matches!(right, Type::Primitive(name) if name == "$float")
    {
        return Type::Primitive("$float".to_string());
    }
    if matches!(left, Type::Primitive(name) if name == "$float") {
        return if right.is_float() {
            right.clone()
        } else {
            Type::Primitive("f32".to_string())
        };
    }
    if matches!(right, Type::Primitive(name) if name == "$float") {
        return if left.is_float() {
            left.clone()
        } else {
            Type::Primitive("f32".to_string())
        };
    }
    if !left.is_literal() {
        left.clone()
    } else if !right.is_literal() {
        right.clone()
    } else {
        Type::Primitive("$int".to_string())
    }
}

fn integer_literal_value(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Int(value) => Some(i128::from(*value)),
        Expr::Unary { op, expr } if op == "-" => match expr.as_ref() {
            Expr::Int(value) => Some(-i128::from(*value)),
            _ => None,
        },
        _ => None,
    }
}

fn integer_bounds(ty: &Type) -> Option<(i128, i128)> {
    let Type::Primitive(name) = ty else {
        return None;
    };
    match name.as_str() {
        "u8" => Some((0, i128::from(u8::MAX))),
        "u16" => Some((0, i128::from(u16::MAX))),
        "u32" => Some((0, i128::from(u32::MAX))),
        "u64" => Some((0, i128::from(u64::MAX))),
        "usize" => Some((0, i128::from(u64::MAX))),
        "i8" => Some((i128::from(i8::MIN), i128::from(i8::MAX))),
        "i16" => Some((i128::from(i16::MIN), i128::from(i16::MAX))),
        "i32" => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
        "i64" | "isize" => Some((i128::from(i64::MIN), i128::from(i64::MAX))),
        _ => None,
    }
}
