use std::collections::{BTreeSet, HashMap};

use crate::ast::{
    body_has_asm, Expr, FunctionDecl, Item, Pattern, Program, Stmt, StructDecl, Type,
};
use crate::diag::Diagnostic;
use crate::effects::{
    analyze as analyze_effects, effect_requires_unsafe, infer_expression_effects, is_known_effect,
};
use crate::lifetimes;
use crate::simd::{check_intrinsic_call, is_simd_intrinsic};
use crate::stdlib::standard_builtin_call_type;
use crate::targets::is_cpu_feature_probe;

/// `break`/`continue` are only valid inside a loop.
fn validate_loop_control(program: &Program) -> Vec<Diagnostic> {
    fn walk(body: &[Stmt], in_loop: bool, id: &str, out: &mut Vec<Diagnostic>) {
        for stmt in body {
            match stmt {
                Stmt::Break | Stmt::Continue if !in_loop => {
                    let (code, word) = if matches!(stmt, Stmt::Break) {
                        ("break_outside_loop", "break")
                    } else {
                        ("continue_outside_loop", "continue")
                    };
                    out.push(
                        Diagnostic::new(code, id, format!("'{word}' is only valid inside a loop"))
                            .suggest("remove the statement or wrap it in a loop"),
                    );
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, in_loop, id, out);
                    walk(else_body, in_loop, id, out);
                }
                Stmt::For { body, .. } | Stmt::While { body, .. } => walk(body, true, id, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for function in program.functions() {
        walk(&function.body, false, &function.id, &mut out);
    }
    out
}

pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut checker = Checker::new(program);
    checker.check_program();
    let mut diagnostics = checker.diagnostics;
    diagnostics.extend(validate_loop_control(program));
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
                // Enums are looked up directly via `program.enums()`; named
                // enum types are accepted leniently like other named types.
                Item::Enum(_)
                | Item::Trait(_)
                | Item::TraitImpl(_)
                | Item::Test(_)
                | Item::Capsule(_)
                | Item::Share(_) => {}
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
    type_params: BTreeSet<String>,
    trait_bounds: HashMap<String, BTreeSet<String>>,
}

impl LocalTypeEnv {
    fn for_function(function: &FunctionDecl) -> Self {
        let mut bindings = HashMap::new();
        for param in &function.params {
            bindings.insert(param.name.clone(), param.ty.clone());
        }
        let type_params = function.type_params.iter().cloned().collect();
        let trait_bounds = function
            .type_bounds
            .iter()
            .map(|bound| {
                (
                    bound.type_param.clone(),
                    bound.traits.iter().cloned().collect(),
                )
            })
            .collect();
        Self {
            bindings,
            type_params,
            trait_bounds,
        }
    }

    fn get(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    fn insert(&mut self, name: String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    fn is_type_param(&self, name: &str) -> bool {
        self.type_params.contains(name)
    }

    fn has_trait_bound(&self, type_param: &str, trait_name: &str) -> bool {
        self.trait_bounds
            .get(type_param)
            .is_some_and(|bounds| bounds.contains(trait_name))
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
        self.validate_traits();
        for function in self.program.functions() {
            self.check_function(function);
        }
        for function in self.program.functions() {
            self.check_safety_function(function);
        }
        self.check_effects();
    }

    fn validate_traits(&mut self) {
        let mut traits = BTreeSet::new();
        for decl in self.program.traits() {
            if decl.name == "Eq" {
                self.diagnostics.push(
                    Diagnostic::new(
                        "reserved_trait_name",
                        &decl.id,
                        "Eq is a built-in trait and cannot be redeclared",
                    )
                    .suggest("rename_trait"),
                );
            } else if !traits.insert(decl.name.clone()) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "duplicate_trait",
                        &decl.id,
                        format!("trait '{}' is declared more than once", decl.name),
                    )
                    .suggest("remove_duplicate_trait"),
                );
            }
            let mut methods = BTreeSet::new();
            for method in &decl.methods {
                if !methods.insert(method.name.clone()) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "duplicate_trait_method",
                            &decl.id,
                            format!("trait method '{}' is duplicated", method.name),
                        )
                        .suggest("remove_duplicate_trait_method"),
                    );
                }
                if let Some(default) = &method.default_function {
                    match self.program.find_function(default) {
                        None => self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_trait_default_function",
                                &decl.id,
                                format!(
                                    "default for trait method '{}::{}' references unknown function '{default}'",
                                    decl.name, method.name
                                ),
                            )
                            .suggest("declare_default_function"),
                        ),
                        Some(function)
                            if function.unsafe_fn || !function.effects.is_empty() =>
                        {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "trait_default_not_pure",
                                    &decl.id,
                                    format!(
                                        "default function '{default}' for '{}::{}' must be safe and pure",
                                        decl.name, method.name
                                    ),
                                )
                                .suggest("use_pure_default_function"),
                            );
                        }
                        Some(_) => {}
                    }
                }
            }
            for constant in &decl.constants {
                if !methods.insert(constant.name.clone()) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "duplicate_trait_item",
                            &decl.id,
                            format!("trait item '{}' is duplicated", constant.name),
                        )
                        .suggest("remove_duplicate_trait_item"),
                    );
                }
            }
            for associated_type in &decl.associated_types {
                if !methods.insert(associated_type.name.clone()) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "duplicate_trait_item",
                            &decl.id,
                            format!("trait item '{}' is duplicated", associated_type.name),
                        )
                        .suggest("remove_duplicate_trait_item"),
                    );
                }
            }
        }
        let mut impls: Vec<&crate::ast::TraitImplDecl> = Vec::new();
        for decl in self.program.trait_impls() {
            self.validate_impl_type_params(decl);
            if decl.trait_name == "Eq" {
                self.diagnostics.push(
                    Diagnostic::new(
                        "builtin_trait_impl",
                        &decl.id,
                        "Eq conformance is structural and cannot be implemented explicitly",
                    )
                    .suggest("remove_builtin_trait_impl"),
                );
            } else if !traits.contains(&decl.trait_name) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_impl_trait",
                        &decl.id,
                        format!(
                            "implementation references unknown trait '{}'",
                            decl.trait_name
                        ),
                    )
                    .suggest("declare_trait"),
                );
            }
            if let Some(previous) = impls.iter().copied().find(|previous| {
                previous.trait_name == decl.trait_name
                    && impl_patterns_overlap(
                        &previous.for_type,
                        &previous.type_params,
                        &decl.for_type,
                        &decl.type_params,
                    )
            }) {
                let (code, message, suggestion) = if previous.type_params.is_empty()
                    && decl.type_params.is_empty()
                    && previous.for_type == decl.for_type
                {
                    (
                        "duplicate_trait_impl",
                        format!(
                            "trait '{}' is implemented more than once for '{}'",
                            decl.trait_name,
                            decl.for_type.to_human()
                        ),
                        "remove_duplicate_trait_impl",
                    )
                } else {
                    (
                        "overlapping_trait_impl",
                        format!(
                            "implementation of trait '{}' for '{}' overlaps an existing implementation",
                            decl.trait_name,
                            decl.for_type.to_human()
                        ),
                        "make_trait_impl_disjoint",
                    )
                };
                self.diagnostics
                    .push(Diagnostic::new(code, &decl.id, message).suggest(suggestion));
            }
            impls.push(decl);
            if let Some(trait_decl) = self
                .program
                .traits()
                .find(|item| item.name == decl.trait_name)
            {
                let mut type_params = vec!["Self".to_string()];
                let mut type_bindings =
                    HashMap::from([("Self".to_string(), decl.for_type.clone())]);
                for type_param in &decl.type_params {
                    type_params.push(type_param.clone());
                    type_bindings.insert(type_param.clone(), Type::Named(type_param.clone()));
                }
                let mut associated_types = BTreeSet::new();
                for binding in &decl.associated_types {
                    if !associated_types.insert(binding.name.clone()) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "duplicate_trait_type_impl",
                                &decl.id,
                                format!(
                                    "trait associated type '{}' is defined more than once",
                                    binding.name
                                ),
                            )
                            .suggest("remove_duplicate_trait_type"),
                        );
                        continue;
                    }
                    if !trait_decl
                        .associated_types
                        .iter()
                        .any(|item| item.name == binding.name)
                    {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_trait_type",
                                &decl.id,
                                format!(
                                    "trait '{}' has no associated type '{}'",
                                    decl.trait_name, binding.name
                                ),
                            )
                            .suggest("replace_trait_type_name"),
                        );
                        continue;
                    }
                    let projection = format!("Self::{}", binding.name);
                    let resolved =
                        substitute_type_params(&binding.ty, &type_params, &type_bindings);
                    type_params.push(projection.clone());
                    type_bindings.insert(projection, resolved);
                }
                for associated_type in &trait_decl.associated_types {
                    if !associated_types.contains(&associated_type.name) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "missing_trait_type_impl",
                                &decl.id,
                                format!(
                                    "implementation is missing associated type '{}::{}'",
                                    decl.trait_name, associated_type.name
                                ),
                            )
                            .suggest("add_trait_type_binding"),
                        );
                    }
                }

                let mut mapped = BTreeSet::new();
                for mapping in &decl.methods {
                    if !mapped.insert(mapping.method.clone()) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "duplicate_trait_method_impl",
                                &decl.id,
                                format!(
                                    "trait method '{}' is mapped more than once",
                                    mapping.method
                                ),
                            )
                            .suggest("remove_duplicate_method_mapping"),
                        );
                        continue;
                    }
                    let Some(contract) = trait_decl
                        .methods
                        .iter()
                        .find(|method| method.name == mapping.method)
                    else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_trait_method",
                                &decl.id,
                                format!(
                                    "trait '{}' has no method '{}'",
                                    decl.trait_name, mapping.method
                                ),
                            )
                            .suggest("replace_trait_method_name"),
                        );
                        continue;
                    };
                    let Some(function) = self.program.find_function(&mapping.function) else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_trait_method_function",
                                &decl.id,
                                format!(
                                    "method '{}' maps to unknown function '{}'",
                                    mapping.method, mapping.function
                                ),
                            )
                            .suggest("declare_method_function"),
                        );
                        continue;
                    };
                    let expected_params = contract
                        .params
                        .iter()
                        .map(|param| {
                            substitute_type_params(&param.ty, &type_params, &type_bindings)
                        })
                        .collect::<Vec<_>>();
                    let expected_ret =
                        substitute_type_params(&contract.ret, &type_params, &type_bindings);
                    let signature_matches = self.trait_default_signature_matches(
                        function,
                        &expected_params,
                        &expected_ret,
                    );
                    if !signature_matches {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "trait_method_signature_mismatch",
                                &decl.id,
                                format!(
                                    "function '{}' does not match method '{}::{}'",
                                    mapping.function, decl.trait_name, mapping.method
                                ),
                            )
                            .suggest("fix_trait_method_function"),
                        );
                    }
                }
                for method in &trait_decl.methods {
                    if !mapped.contains(&method.name) {
                        if let Some(default) = &method.default_function {
                            if let Some(function) = self.program.find_function(default) {
                                let expected_params = method
                                    .params
                                    .iter()
                                    .map(|param| {
                                        substitute_type_params(
                                            &param.ty,
                                            &type_params,
                                            &type_bindings,
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let expected_ret = substitute_type_params(
                                    &method.ret,
                                    &type_params,
                                    &type_bindings,
                                );
                                if !self.trait_default_signature_matches(
                                    function,
                                    &expected_params,
                                    &expected_ret,
                                ) {
                                    self.diagnostics.push(
                                        Diagnostic::new(
                                            "trait_default_signature_mismatch",
                                            &decl.id,
                                            format!(
                                                "default function '{default}' does not match method '{}::{}' for '{}'",
                                                decl.trait_name,
                                                method.name,
                                                decl.for_type.to_human()
                                            ),
                                        )
                                        .suggest("fix_trait_default_function"),
                                    );
                                }
                            }
                        } else {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "missing_trait_method_impl",
                                    &decl.id,
                                    format!(
                                        "implementation is missing method '{}::{}'",
                                        decl.trait_name, method.name
                                    ),
                                )
                                .suggest("add_trait_method_mapping"),
                            );
                        }
                    }
                }
                let contracts = trait_decl.constants.clone();
                let mut constants = BTreeSet::new();
                for binding in &decl.constants {
                    if !constants.insert(binding.name.clone()) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "duplicate_trait_const_impl",
                                &decl.id,
                                format!(
                                    "trait constant '{}' is defined more than once",
                                    binding.name
                                ),
                            )
                            .suggest("remove_duplicate_trait_constant"),
                        );
                        continue;
                    }
                    let Some(contract) = contracts.iter().find(|item| item.name == binding.name)
                    else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_trait_const",
                                &decl.id,
                                format!(
                                    "trait '{}' has no constant '{}'",
                                    decl.trait_name, binding.name
                                ),
                            )
                            .suggest("replace_trait_constant_name"),
                        );
                        continue;
                    };
                    if !is_const_expr(&binding.value) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "trait_const_not_constant",
                                &decl.id,
                                format!(
                                    "trait constant '{}' must use a literal constant expression",
                                    binding.name
                                ),
                            )
                            .suggest("replace_with_constant_expression"),
                        );
                        continue;
                    }
                    let expected =
                        substitute_type_params(&contract.ty, &type_params, &type_bindings);
                    let found = self.infer_expr(&binding.value, &LocalTypeEnv::default(), &decl.id);
                    self.check_expr_assignable(
                        &expected,
                        &binding.value,
                        &found,
                        &decl.id,
                        TypeDiagnostic::new(
                            "trait_const_type_mismatch",
                            format!("trait constant '{}' has the wrong type", binding.name),
                            &["replace_trait_constant_value"],
                        ),
                    );
                }
                for constant in &contracts {
                    if !constants.contains(&constant.name) {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "missing_trait_const_impl",
                                &decl.id,
                                format!(
                                    "implementation is missing constant '{}::{}'",
                                    decl.trait_name, constant.name
                                ),
                            )
                            .suggest("add_trait_constant"),
                        );
                    }
                }
            }
        }
    }

    fn validate_impl_type_params(&mut self, implementation: &crate::ast::TraitImplDecl) {
        let mut params = BTreeSet::new();
        for param in &implementation.type_params {
            if !params.insert(param.clone()) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "duplicate_impl_type_param",
                        &implementation.id,
                        format!("implementation type parameter '{param}' is duplicated"),
                    )
                    .suggest("remove_duplicate_type_param"),
                );
            }
            if !type_mentions_param(&implementation.for_type, param) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unconstrained_impl_type_param",
                        &implementation.id,
                        format!(
                            "implementation type parameter '{param}' does not appear in its target type"
                        ),
                    )
                    .suggest("use_type_param_in_impl_target"),
                );
            }
        }
        for bound in &implementation.type_bounds {
            if !params.contains(&bound.type_param) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_impl_bound_param",
                        &implementation.id,
                        format!(
                            "implementation bound references unknown type parameter '{}'",
                            bound.type_param
                        ),
                    )
                    .suggest("replace_bound_type_param"),
                );
            }
            let mut traits = BTreeSet::new();
            for trait_name in &bound.traits {
                if !is_known_trait(self.program, trait_name) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "unknown_trait",
                            &implementation.id,
                            format!("unknown trait '{trait_name}'"),
                        )
                        .suggest("replace_trait_bound"),
                    );
                } else if !traits.insert(trait_name) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "duplicate_impl_trait_bound",
                            &implementation.id,
                            format!("implementation trait bound '{trait_name}' is duplicated"),
                        )
                        .suggest("remove_duplicate_trait_bound"),
                    );
                }
            }
        }
    }

    fn trait_default_signature_matches(
        &self,
        function: &FunctionDecl,
        expected_params: &[Type],
        expected_ret: &Type,
    ) -> bool {
        if function.unsafe_fn
            || !function.effects.is_empty()
            || !function.type_bounds.is_empty()
            || function.params.len() != expected_params.len()
        {
            return false;
        }
        if function.type_params.is_empty() {
            return function
                .params
                .iter()
                .map(|param| &param.ty)
                .eq(expected_params.iter())
                && function.ret == *expected_ret;
        }
        if expected_params.iter().any(Type::is_mutable_slice) {
            return false;
        }
        let mut bindings = HashMap::new();
        for (param, expected) in function.params.iter().zip(expected_params) {
            unify_type_params(&param.ty, expected, &function.type_params, &mut bindings);
        }
        unify_type_params(
            &function.ret,
            expected_ret,
            &function.type_params,
            &mut bindings,
        );
        function
            .type_params
            .iter()
            .all(|param| bindings.contains_key(param))
            && function
                .params
                .iter()
                .zip(expected_params)
                .all(|(param, expected)| {
                    substitute_type_params(&param.ty, &function.type_params, &bindings) == *expected
                })
            && substitute_type_params(&function.ret, &function.type_params, &bindings)
                == *expected_ret
    }

    fn check_function(&mut self, function: &FunctionDecl) {
        self.validate_type_param_bounds(function);
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
        let projection_locals = LocalTypeEnv::for_function(function);
        for param in &function.params {
            self.validate_fn_effect_type(&param.ty, &function.id);
            self.validate_associated_type_projections(&param.ty, &projection_locals, &function.id);
        }
        self.validate_fn_effect_type(&function.ret, &function.id);
        self.validate_associated_type_projections(&function.ret, &projection_locals, &function.id);
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

    fn validate_type_param_bounds(&mut self, function: &FunctionDecl) {
        let mut params = BTreeSet::new();
        for param in &function.type_params {
            if !params.insert(param.clone()) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "duplicate_type_param",
                        &function.id,
                        format!("generic type parameter '{param}' is duplicated"),
                    )
                    .suggest("remove_duplicate_type_param"),
                );
            }
        }
        for bound in &function.type_bounds {
            if !params.contains(&bound.type_param) {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_bound_param",
                        &function.id,
                        format!(
                            "trait bound references unknown type parameter '{}'",
                            bound.type_param
                        ),
                    )
                    .suggest("replace_bound_type_param"),
                );
            }
            let mut traits = BTreeSet::new();
            for trait_name in &bound.traits {
                if trait_name != "Eq" && !self.program.traits().any(|decl| decl.name == *trait_name)
                {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "unknown_trait",
                            &function.id,
                            format!("unknown trait '{trait_name}'"),
                        )
                        .suggest("replace_trait_bound"),
                    );
                } else if !traits.insert(trait_name) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "duplicate_trait_bound",
                            &function.id,
                            format!("trait bound '{trait_name}' is duplicated"),
                        )
                        .suggest("remove_duplicate_trait_bound"),
                    );
                }
            }
        }
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
                    self.validate_fn_effect_type(expected, &function.id);
                    self.validate_associated_type_projections(expected, locals, &function.id);
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
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let found = self.infer_expr(cond, locals, &function.id);
                if found != Type::Bool && found != Type::Unknown {
                    self.diagnostics.push(
                        Diagnostic::new("if_not_bool", &function.id, "if condition must be bool")
                            .with_types(&Type::Bool, &found)
                            .suggest("replace_if_condition"),
                    );
                }
                let mut then_locals = locals.clone();
                self.check_body(then_body, ret, &mut then_locals, function);
                let mut else_locals = locals.clone();
                self.check_body(else_body, ret, &mut else_locals, function);
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
            Stmt::While { cond, body } => {
                let found = self.infer_expr(cond, locals, &function.id);
                if found != Type::Bool && found != Type::Unknown {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "while_not_bool",
                            &function.id,
                            "while condition must be bool",
                        )
                        .with_types(&Type::Bool, &found)
                        .suggest("replace_while_condition"),
                    );
                }
                let mut loop_locals = locals.clone();
                self.check_body(body, ret, &mut loop_locals, function);
            }
            Stmt::Match { scrutinee, arms } => {
                let scrut_ty = self.infer_expr(scrutinee, locals, &function.id);
                // Resolve the scrutinee's enum, if any, for variant and
                // exhaustiveness checks. Unknown types stay lenient.
                let enum_decl = match &scrut_ty {
                    Type::Named(name) => self.program.enums().find(|e| &e.name == name),
                    _ => None,
                };
                if enum_decl.is_none() && scrut_ty != Type::Unknown {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "match_not_enum",
                            &function.id,
                            "match scrutinee must be an enum value",
                        )
                        .with_types(&Type::Named("enum".to_string()), &scrut_ty)
                        .suggest("match_on_an_enum_value"),
                    );
                }
                let mut covered: BTreeSet<String> = BTreeSet::new();
                let mut has_wildcard = false;
                for arm in arms {
                    let mut arm_locals = locals.clone();
                    match &arm.pattern {
                        Pattern::Wildcard => has_wildcard = true,
                        Pattern::Variant { path, binding } => {
                            if let Some(decl) = enum_decl {
                                let (en, variant_name) =
                                    path.split_once("::").unwrap_or(("", path.as_str()));
                                if en != decl.name {
                                    self.diagnostics.push(
                                        Diagnostic::new(
                                            "pattern_type_mismatch",
                                            &function.id,
                                            format!(
                                                "pattern '{path}' is not a variant of enum '{}'",
                                                decl.name
                                            ),
                                        )
                                        .suggest("use_a_variant_of_the_scrutinee_enum"),
                                    );
                                } else if let Some(variant) =
                                    decl.variants.iter().find(|v| v.name == variant_name)
                                {
                                    covered.insert(variant_name.to_string());
                                    // A binding introduces a local of the
                                    // payload type into the arm body.
                                    if let Some(bind_name) = binding {
                                        match &variant.payload {
                                            Some(ty) => {
                                                arm_locals.insert(bind_name.clone(), ty.clone());
                                            }
                                            None => self.diagnostics.push(
                                                Diagnostic::new(
                                                    "binding_on_data_less_variant",
                                                    &function.id,
                                                    format!(
                                                        "variant '{variant_name}' has no payload to bind"
                                                    ),
                                                )
                                                .suggest("remove_the_binding"),
                                            ),
                                        }
                                    }
                                } else {
                                    self.diagnostics.push(
                                        Diagnostic::new(
                                            "unknown_enum_variant",
                                            &function.id,
                                            format!(
                                                "enum '{}' has no variant '{variant_name}'",
                                                decl.name
                                            ),
                                        )
                                        .suggest("choose_a_declared_variant"),
                                    );
                                }
                            }
                        }
                    }
                    self.check_body(&arm.body, ret, &mut arm_locals, function);
                }
                if let Some(decl) = enum_decl {
                    if !has_wildcard {
                        let missing: Vec<String> = decl
                            .variants
                            .iter()
                            .map(|v| v.name.clone())
                            .filter(|v| !covered.contains(v))
                            .collect();
                        if !missing.is_empty() {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "non_exhaustive_match",
                                    &function.id,
                                    format!("match does not cover: {}", missing.join(", ")),
                                )
                                .suggest("add_missing_arms_or_a_wildcard"),
                            );
                        }
                    }
                }
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
                            "indexed writes require a Vec, mutable slice, or mutable pointer target",
                        )
                        .suggest("change_parameter_to_mutable_slice"),
                    );
                }
            }
            Stmt::Break | Stmt::Continue => {}
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

    fn validate_fn_effect_type(&mut self, ty: &Type, node: &str) {
        match ty {
            Type::Generic { name, args } if name == "Fn" => {
                let marker_positions = args
                    .iter()
                    .enumerate()
                    .filter_map(|(index, arg)| {
                        matches!(arg, Type::Generic { name, .. } if name == "effects")
                            .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if ty.function_parts().is_none() {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "invalid_fn_type",
                            node,
                            "Fn requires a return type before its optional effects marker",
                        )
                        .suggest("add_fn_return_type"),
                    );
                    return;
                }
                if marker_positions
                    .iter()
                    .any(|position| *position + 1 != args.len())
                    || marker_positions.len() > 1
                {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "fn_effect_position",
                            node,
                            "effects[...] must be the single final argument of Fn[...]",
                        )
                        .suggest("move_fn_effects_to_final_position"),
                    );
                }
                let binder_positions = args
                    .iter()
                    .enumerate()
                    .filter_map(|(index, arg)| {
                        matches!(arg, Type::Generic { name, .. } if name == "forall")
                            .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if binder_positions.iter().any(|position| *position != 0)
                    || binder_positions.len() > 1
                {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "fn_binder_position",
                            node,
                            "forall[...] must be the single first argument of Fn[...]",
                        )
                        .suggest("move_fn_binder_to_first_position"),
                    );
                }
                match ty.function_binders() {
                    None => self.diagnostics.push(
                        Diagnostic::new(
                            "invalid_fn_binder",
                            node,
                            "forall[...] entries must be named type parameters with optional bounds",
                        )
                        .suggest("replace_fn_binder"),
                    ),
                    Some(binders) => {
                        let mut seen_params = BTreeSet::new();
                        for (param, bounds) in &binders {
                            if !seen_params.insert(param.clone()) {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "duplicate_fn_binder",
                                        node,
                                        format!("function-value binder '{param}' is duplicated"),
                                    )
                                    .suggest("remove_duplicate_fn_binder"),
                                );
                            }
                            let mut seen_bounds = BTreeSet::new();
                            for bound in bounds {
                                if !is_known_trait(self.program, bound) {
                                    self.diagnostics.push(
                                        Diagnostic::new(
                                            "unknown_trait",
                                            node,
                                            format!("unknown trait '{bound}' in function-value binder"),
                                        )
                                        .suggest("replace_trait_bound"),
                                    );
                                } else if !seen_bounds.insert(bound.clone()) {
                                    self.diagnostics.push(
                                        Diagnostic::new(
                                            "duplicate_fn_binder_bound",
                                            node,
                                            format!(
                                                "function-value binder bound '{bound}' is duplicated"
                                            ),
                                        )
                                        .suggest("remove_duplicate_fn_binder_bound"),
                                    );
                                }
                            }
                        }
                        if let Some((params, ret, _)) = ty.function_parts() {
                            for (param, _) in binders {
                                if !params.iter().any(|ty| type_mentions_param(ty, &param))
                                    && !type_mentions_param(ret, &param)
                                {
                                    self.diagnostics.push(
                                        Diagnostic::new(
                                            "unused_fn_binder",
                                            node,
                                            format!(
                                                "function-value binder '{param}' cannot be inferred from its signature"
                                            ),
                                        )
                                        .suggest("remove_unused_fn_binder"),
                                    );
                                }
                            }
                        }
                    }
                }
                if let Some(Type::Generic {
                    name: marker,
                    args: effects,
                }) = args.last()
                {
                    if marker == "effects" {
                        let mut seen = BTreeSet::new();
                        for effect in effects {
                            let effect = effect.to_agent();
                            if !is_known_effect(&effect) {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "unknown_fn_effect",
                                        node,
                                        format!("unknown function-value effect '{effect}'"),
                                    )
                                    .suggest("replace_fn_effect"),
                                );
                            } else if !seen.insert(effect.clone()) {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "duplicate_fn_effect",
                                        node,
                                        format!("function-value effect '{effect}' is duplicated"),
                                    )
                                    .suggest("remove_duplicate_fn_effect"),
                                );
                            }
                        }
                    }
                }
                let signature_len = args.len().saturating_sub(usize::from(matches!(
                    args.last(),
                    Some(Type::Generic { name, .. }) if name == "effects"
                )));
                for arg in &args[..signature_len] {
                    self.validate_fn_effect_type(arg, node);
                }
            }
            Type::Generic { args, .. } => {
                for arg in args {
                    self.validate_fn_effect_type(arg, node);
                }
            }
            Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
                self.validate_fn_effect_type(elem, node)
            }
            Type::Bool | Type::Primitive(_) | Type::Named(_) | Type::Void | Type::Unknown => {}
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
            Expr::TraitConst {
                trait_name,
                name,
                for_type,
            } => self.infer_trait_const(trait_name, name, for_type, node),
            Expr::Lambda { params, body } => {
                // A closure (lang.4) types as `Fn[P…, R]`: parameters are
                // declared explicitly, and the body is checked with them in
                // scope alongside the enclosing locals (which the closure
                // captures by value at runtime).
                let mut lambda_locals = locals.clone();
                for param in params.iter() {
                    lambda_locals.insert(param.name.clone(), param.ty.clone());
                }
                let body_ty = self.infer_expr(body, &lambda_locals, node);
                let effects = infer_expression_effects(self.program, body, &lambda_locals.bindings);
                Type::function_type(
                    params.iter().map(|param| param.ty.clone()).collect(),
                    body_ty,
                    &effects,
                )
            }
            Expr::Var(name) if name.contains("::") => {
                // An enum variant value, e.g. `Color::Red`.
                let (enum_name, variant) = name.split_once("::").unwrap();
                match self.program.enums().find(|e| e.name == enum_name) {
                    Some(decl) if decl.variants.iter().any(|v| v.name == variant) => {
                        Type::Named(enum_name.to_string())
                    }
                    Some(_) => {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_enum_variant",
                                node,
                                format!("enum '{enum_name}' has no variant '{variant}'"),
                            )
                            .suggest("choose_a_declared_variant"),
                        );
                        Type::Unknown
                    }
                    None => {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_enum",
                                node,
                                format!("unknown enum '{enum_name}'"),
                            )
                            .suggest("declare_the_enum"),
                        );
                        Type::Unknown
                    }
                }
            }
            Expr::Var(name) => locals.get(name).cloned().unwrap_or_else(|| {
                // A bare reference to a top-level function is a function value
                // (lang.4), typed `Fn[P…, R]`. Locals shadow (checked above).
                if let Some(function) = self.type_env.functions.get(name) {
                    return fn_value_type(function);
                }
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
                    "==" | "!=" => {
                        let generic_params = [&left_ty, &right_ty]
                            .into_iter()
                            .filter_map(|ty| match ty {
                                Type::Named(name) if locals.is_type_param(name) => {
                                    Some(name.clone())
                                }
                                _ => None,
                            })
                            .collect::<BTreeSet<_>>();
                        for type_param in generic_params {
                            if !locals.has_trait_bound(&type_param, "Eq") {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "missing_trait_bound",
                                        node,
                                        format!(
                                            "equality on generic type '{type_param}' requires an Eq bound"
                                        ),
                                    )
                                    .suggest("add_eq_trait_bound"),
                                );
                            }
                        }
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
                    "<" | ">" | "<=" | ">=" => {
                        if left_ty != Type::Unknown
                            && right_ty != Type::Unknown
                            && (!left_ty.is_numeric()
                                || !right_ty.is_numeric()
                                || !numeric_operands_compatible(&left_ty, &right_ty))
                        {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "comparison_type_mismatch",
                                    node,
                                    "ordered comparison operands must be compatible numeric types",
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
                if let Some(ty) = self.infer_concurrency_call(name, args, locals, node) {
                    return ty;
                }
                if let Some(ty) = self.infer_trait_method_call(name, args, locals, node) {
                    return ty;
                }
                if is_simd_intrinsic(name) {
                    let arg_types = args
                        .iter()
                        .map(|arg| self.infer_expr(arg, locals, node))
                        .collect::<Vec<_>>();
                    let (ty, diagnostics) = check_intrinsic_call(name, &arg_types, node);
                    self.diagnostics.extend(diagnostics);
                    return ty;
                }
                if matches!(name.as_str(), "str_len" | "str_contains") {
                    let expected = if name == "str_len" { 1 } else { 2 };
                    if args.len() != expected {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "call_arity",
                                node,
                                format!("builtin '{name}' expects {expected} argument(s)"),
                            )
                            .suggest("fix_call_arguments"),
                        );
                    }
                    let str_type = Type::Primitive("str".to_string());
                    for arg in args {
                        let found = self.infer_expr(arg, locals, node);
                        self.check_expr_assignable(
                            &str_type,
                            arg,
                            &found,
                            node,
                            TypeDiagnostic::new(
                                "argument_type_mismatch",
                                format!("argument to builtin '{name}' must be str"),
                                &["replace_call_argument"],
                            ),
                        );
                    }
                    return if name == "str_len" {
                        Type::Primitive("usize".to_string())
                    } else {
                        Type::Bool
                    };
                }
                if let Some(ty) = builtin_call_type(name) {
                    for arg in args {
                        self.infer_expr(arg, locals, node);
                    }
                    return ty;
                }
                // An indirect call through a local (lang.4): `f(x)` where `f`
                // is a function value. Locals shadow constructors/capsules/
                // functions, matching the interpreter's dispatch order.
                if let Some(local_ty) = locals.get(name).cloned() {
                    let Some((params, ret, _effects)) = local_ty.function_parts() else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "not_callable",
                                node,
                                format!("'{name}' is not a function value"),
                            )
                            .with_types(&Type::Named("Fn".to_string()), &local_ty)
                            .suggest("bind_a_function_value"),
                        );
                        for arg in args {
                            self.infer_expr(arg, locals, node);
                        }
                        return Type::Unknown;
                    };
                    if args.len() != params.len() {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "fn_value_arity",
                                node,
                                format!("'{name}' expects {} argument(s)", params.len()),
                            )
                            .suggest("fix_call_arguments"),
                        );
                    }
                    let binders = local_ty.function_binders().unwrap_or_default();
                    let type_params = binders
                        .iter()
                        .map(|(param, _)| param.clone())
                        .collect::<Vec<_>>();
                    let found_types = args
                        .iter()
                        .map(|arg| self.infer_expr(arg, locals, node))
                        .collect::<Vec<_>>();
                    let mut bindings = HashMap::new();
                    for (param_ty, found) in params.iter().zip(&found_types) {
                        unify_type_params(param_ty, found, &type_params, &mut bindings);
                    }
                    for (param, bounds) in &binders {
                        let Some(found) = bindings.get(param) else {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "unresolved_fn_binder",
                                    node,
                                    format!(
                                        "cannot infer function-value binder '{param}' for call to '{name}'"
                                    ),
                                )
                                .suggest("pass_typed_function_argument"),
                            );
                            continue;
                        };
                        for bound in bounds {
                            if is_known_trait(self.program, bound)
                                && !type_satisfies_trait(
                                    found,
                                    bound,
                                    locals,
                                    self.program,
                                    &mut BTreeSet::new(),
                                )
                            {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "trait_bound_not_satisfied",
                                        node,
                                        format!(
                                            "type '{}' does not satisfy {bound} required by function value '{name}'",
                                            found.to_human()
                                        ),
                                    )
                                    .with_types(&Type::Named(bound.clone()), found)
                                    .suggest("pass_value_satisfying_bound"),
                                );
                            }
                        }
                    }
                    for ((param_ty, arg), found) in params.iter().zip(args.iter()).zip(&found_types)
                    {
                        let expected = substitute_type_params(param_ty, &type_params, &bindings);
                        self.check_expr_assignable(
                            &expected,
                            arg,
                            found,
                            node,
                            TypeDiagnostic::new(
                                "fn_value_arg_type",
                                format!("argument to function value '{name}' has wrong type"),
                                &["insert_explicit_cast", "replace_call_argument"],
                            ),
                        );
                    }
                    return substitute_type_params(ret, &type_params, &bindings);
                }
                // A payload-carrying enum variant constructor, `Shape::Circle(r)`.
                if let Some((enum_name, variant_name)) = name.split_once("::") {
                    let Some(decl) = self.program.enums().find(|e| e.name == enum_name) else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_enum",
                                node,
                                format!("unknown enum '{enum_name}'"),
                            )
                            .suggest("declare_the_enum"),
                        );
                        for arg in args {
                            self.infer_expr(arg, locals, node);
                        }
                        return Type::Unknown;
                    };
                    let Some(variant) = decl.variants.iter().find(|v| v.name == variant_name)
                    else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unknown_enum_variant",
                                node,
                                format!("enum '{enum_name}' has no variant '{variant_name}'"),
                            )
                            .suggest("choose_a_declared_variant"),
                        );
                        for arg in args {
                            self.infer_expr(arg, locals, node);
                        }
                        return Type::Unknown;
                    };
                    match &variant.payload {
                        Some(payload_ty) => {
                            if args.len() != 1 {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "variant_arity",
                                        node,
                                        format!(
                                            "variant '{variant_name}' takes one payload argument"
                                        ),
                                    )
                                    .suggest("fix_variant_arguments"),
                                );
                            }
                            if let Some(arg) = args.first() {
                                let found = self.infer_expr(arg, locals, node);
                                self.check_expr_assignable(
                                    payload_ty,
                                    arg,
                                    &found,
                                    node,
                                    TypeDiagnostic::new(
                                        "variant_payload_type",
                                        format!("variant '{variant_name}' payload has wrong type"),
                                        &["insert_explicit_cast", "replace_variant_argument"],
                                    ),
                                );
                            }
                        }
                        None => {
                            if !args.is_empty() {
                                self.diagnostics.push(
                                    Diagnostic::new(
                                        "variant_arity",
                                        node,
                                        format!("variant '{variant_name}' takes no payload"),
                                    )
                                    .suggest("remove_variant_arguments"),
                                );
                                for arg in args {
                                    self.infer_expr(arg, locals, node);
                                }
                            }
                        }
                    }
                    return Type::Named(enum_name.to_string());
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
                // A capsule invocation, `to_fahrenheit(c)`: check the arguments
                // against the capsule's typed inputs and yield its output type.
                if let Some(capsule) = self.program.capsules().find(|c| c.name == *name) {
                    if args.len() != capsule.inputs.len() {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "capsule_call_arity",
                                node,
                                format!(
                                    "capsule '{name}' expects {} argument(s)",
                                    capsule.inputs.len()
                                ),
                            )
                            .suggest("fix_capsule_arguments"),
                        );
                    }
                    for (input, arg) in capsule.inputs.iter().zip(args.iter()) {
                        let found = self.infer_expr(arg, locals, node);
                        self.check_expr_assignable(
                            &input.ty,
                            arg,
                            &found,
                            node,
                            TypeDiagnostic::new(
                                "capsule_arg_type",
                                format!("capsule '{name}' input '{}' has wrong type", input.name),
                                &["insert_explicit_cast", "replace_capsule_argument"],
                            ),
                        );
                    }
                    // A single typed output is the call's type; otherwise it is
                    // opaque at the boundary for now.
                    return match capsule.outputs.as_slice() {
                        [single] => single.ty.clone(),
                        _ => Type::Unknown,
                    };
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
                let type_params = function.type_params.clone();
                let type_bounds = function.type_bounds.clone();
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
                // Infer all argument types once, then — for a generic callee
                // (lang.4) — bind its type parameters by structural unification
                // and check against the substituted signature. For monomorphic
                // callees the substitution is the identity.
                let found_types: Vec<Type> = args
                    .iter()
                    .map(|arg| self.infer_expr(arg, locals, node))
                    .collect();
                let mut bindings: HashMap<String, Type> = HashMap::new();
                if !type_params.is_empty() {
                    for (param, found) in params.iter().zip(found_types.iter()) {
                        unify_type_params(&param.ty, found, &type_params, &mut bindings);
                    }
                }
                for bound in &type_bounds {
                    let Some(found) = bindings.get(&bound.type_param) else {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "unresolved_trait_bound",
                                node,
                                format!(
                                    "cannot infer bounded type parameter '{}' for function '{name}'",
                                    bound.type_param
                                ),
                            )
                            .suggest("add_typed_argument"),
                        );
                        continue;
                    };
                    for trait_name in &bound.traits {
                        if is_known_trait(self.program, trait_name)
                            && !type_satisfies_trait(
                                found,
                                trait_name,
                                locals,
                                self.program,
                                &mut BTreeSet::new(),
                            )
                        {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    "trait_bound_not_satisfied",
                                    node,
                                    format!(
                                        "type '{}' does not satisfy {} required by function '{name}'",
                                        found.to_human(),
                                        trait_name
                                    ),
                                )
                                .with_types(&Type::Named(trait_name.clone()), found)
                                .suggest("pass_eq_value"),
                            );
                        }
                    }
                }
                for ((param, arg), found) in params.iter().zip(args.iter()).zip(found_types.iter())
                {
                    let expected = substitute_type_params(&param.ty, &type_params, &bindings);
                    self.check_expr_assignable(
                        &expected,
                        arg,
                        found,
                        node,
                        TypeDiagnostic::new(
                            "argument_type_mismatch",
                            format!("argument '{}' has wrong type", param.name),
                            &["insert_explicit_cast", "replace_call_argument"],
                        ),
                    );
                }
                let substituted = substitute_type_params(&ret, &type_params, &bindings);
                self.normalize_associated_type_projections(&substituted, locals)
            }
            Expr::Cast { ty, expr } => {
                self.validate_associated_type_projections(ty, locals, node);
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
                let is_len_container = matches!(base_ty, Type::Slice { .. } | Type::Array { .. })
                    || matches!(&base_ty, Type::Generic { name, .. } if name == "Vec" || name == "Map")
                    || matches!(&base_ty, Type::Primitive(name) if name == "str");
                if field == "len" && is_len_container {
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

    fn infer_trait_const(
        &mut self,
        trait_name: &str,
        name: &str,
        for_type: &Type,
        node: &str,
    ) -> Type {
        let Some(trait_decl) = self.program.traits().find(|item| item.name == trait_name) else {
            self.diagnostics.push(
                Diagnostic::new(
                    "unknown_trait",
                    node,
                    format!("unknown trait '{trait_name}'"),
                )
                .suggest("declare_trait"),
            );
            return Type::Unknown;
        };
        let Some(contract) = trait_decl.constants.iter().find(|item| item.name == name) else {
            self.diagnostics.push(
                Diagnostic::new(
                    "unknown_trait_const_access",
                    node,
                    format!("trait '{trait_name}' has no constant '{name}'"),
                )
                .suggest("choose_a_required_trait_constant"),
            );
            return Type::Unknown;
        };
        let contract_ty = contract.ty.clone();
        let declared_types = trait_decl
            .associated_types
            .iter()
            .map(|item| item.name.clone())
            .collect::<BTreeSet<_>>();
        let empty_locals = LocalTypeEnv::default();
        let implementation = self.program.trait_impls().find_map(|implementation| {
            (implementation.trait_name == trait_name
                && implementation
                    .constants
                    .iter()
                    .any(|item| item.name == name))
            .then(|| {
                trait_impl_bindings(
                    implementation,
                    for_type,
                    &empty_locals,
                    self.program,
                    &mut BTreeSet::new(),
                )
                .map(|bindings| (implementation, bindings))
            })
            .flatten()
        });
        if implementation.is_none() {
            self.diagnostics.push(
                Diagnostic::new(
                    "trait_const_impl_missing",
                    node,
                    format!(
                        "trait constant '{trait_name}::{name}' is not implemented for '{}'",
                        for_type.to_human()
                    ),
                )
                .suggest("add_trait_impl"),
            );
        }
        let self_param = "Self".to_string();
        let mut type_params = vec![self_param.clone()];
        let mut bindings = HashMap::from([(self_param.clone(), for_type.clone())]);
        if let Some((implementation, impl_bindings)) = implementation {
            for type_param in &implementation.type_params {
                type_params.push(type_param.clone());
                if let Some(binding) = impl_bindings.get(type_param) {
                    bindings.insert(type_param.clone(), binding.clone());
                }
            }
            for binding in &implementation.associated_types {
                if !declared_types.contains(&binding.name) {
                    continue;
                }
                let projection = format!("Self::{}", binding.name);
                let resolved = substitute_type_params(&binding.ty, &type_params, &bindings);
                type_params.push(projection.clone());
                bindings.insert(projection, resolved);
            }
        }
        substitute_type_params(&contract_ty, &type_params, &bindings)
    }

    fn infer_concurrency_call(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &LocalTypeEnv,
        node: &str,
    ) -> Option<Type> {
        let expected_arity = match name {
            "task_spawn" | "channel_send" => 2,
            "task_group_spawn" => 3,
            "task_await"
            | "task_cancel"
            | "task_group_new"
            | "task_group_await_all"
            | "task_group_cancel"
            | "channel_new"
            | "channel_recv"
            | "channel_close" => 1,
            _ => return None,
        };
        if args.len() != expected_arity {
            self.diagnostics.push(
                Diagnostic::new(
                    "call_arity",
                    node,
                    format!("{name} expects {expected_arity} argument(s)"),
                )
                .suggest("fix_call_arguments"),
            );
        }
        let found = args
            .iter()
            .map(|arg| self.infer_expr(arg, locals, node))
            .collect::<Vec<_>>();
        match name {
            "task_spawn" => {
                let Some((params, ret, _callable_effects)) =
                    found.first().and_then(Type::function_parts)
                else {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "task_not_callable",
                            node,
                            "task_spawn requires a typed one-argument function value",
                        )
                        .suggest("pass_function_or_closure"),
                    );
                    return Some(Type::Generic {
                        name: "Task".to_string(),
                        args: vec![Type::Unknown],
                    });
                };
                if params.len() != 1 {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "task_arity",
                            node,
                            "task_spawn callable must take exactly one argument",
                        )
                        .suggest("wrap_callable_in_one_argument_closure"),
                    );
                    return Some(Type::Generic {
                        name: "Task".to_string(),
                        args: vec![Type::Unknown],
                    });
                }
                if let Some(input_ty) = found.get(1) {
                    self.check_expr_assignable(
                        &params[0],
                        &args[1],
                        input_ty,
                        node,
                        TypeDiagnostic::new(
                            "task_input_type",
                            "task input does not match the callable parameter",
                            &["replace_task_input", "insert_explicit_cast"],
                        ),
                    );
                }
                Some(Type::Generic {
                    name: "Task".to_string(),
                    args: vec![ret.clone()],
                })
            }
            "task_await" => Some(generic_element_type(
                found.first(),
                "Task",
                "task_await",
                node,
                &mut self.diagnostics,
            )),
            "task_cancel" => {
                expect_generic_value(
                    found.first(),
                    "Task",
                    "task_cancel",
                    node,
                    &mut self.diagnostics,
                );
                Some(Type::Bool)
            }
            "task_group_new" => {
                if let Some(capacity) = found.first() {
                    if !capacity.is_integer() && *capacity != Type::Unknown {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "task_group_capacity_type",
                                node,
                                "task group capacity must be an integer",
                            )
                            .suggest("replace_task_group_capacity"),
                        );
                    }
                }
                Some(Type::Generic {
                    name: "TaskGroup".to_string(),
                    args: vec![Type::Unknown],
                })
            }
            "task_group_spawn" => {
                let group_elem = generic_element_type(
                    found.first(),
                    "TaskGroup",
                    "task_group_spawn",
                    node,
                    &mut self.diagnostics,
                );
                let Some((params, ret, _callable_effects)) =
                    found.get(1).and_then(Type::function_parts)
                else {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "task_not_callable",
                            node,
                            "task_group_spawn requires a typed one-argument function value",
                        )
                        .suggest("pass_function_or_closure"),
                    );
                    return Some(Type::Bool);
                };
                if params.len() != 1 {
                    self.diagnostics.push(
                        Diagnostic::new(
                            "task_arity",
                            node,
                            "task_group_spawn callable must take exactly one argument",
                        )
                        .suggest("wrap_callable_in_one_argument_closure"),
                    );
                    return Some(Type::Bool);
                }
                if !is_assignable(&group_elem, ret) {
                    self.push_type_diagnostic(
                        "task_group_output_type",
                        node,
                        "task result does not match the task group element type",
                        &group_elem,
                        ret,
                        &["replace_task_group_callable"],
                    );
                }
                if let Some(input_ty) = found.get(2) {
                    self.check_expr_assignable(
                        &params[0],
                        &args[2],
                        input_ty,
                        node,
                        TypeDiagnostic::new(
                            "task_input_type",
                            "task input does not match the callable parameter",
                            &["replace_task_input", "insert_explicit_cast"],
                        ),
                    );
                }
                Some(Type::Bool)
            }
            "task_group_await_all" => Some(Type::Generic {
                name: "Vec".to_string(),
                args: vec![generic_element_type(
                    found.first(),
                    "TaskGroup",
                    "task_group_await_all",
                    node,
                    &mut self.diagnostics,
                )],
            }),
            "task_group_cancel" => {
                expect_generic_value(
                    found.first(),
                    "TaskGroup",
                    "task_group_cancel",
                    node,
                    &mut self.diagnostics,
                );
                Some(Type::Primitive("usize".to_string()))
            }
            "channel_new" => {
                if let Some(capacity) = found.first() {
                    if !capacity.is_integer() && *capacity != Type::Unknown {
                        self.diagnostics.push(
                            Diagnostic::new(
                                "channel_capacity_type",
                                node,
                                "channel capacity must be an integer",
                            )
                            .suggest("replace_channel_capacity"),
                        );
                    }
                }
                Some(Type::Generic {
                    name: "Channel".to_string(),
                    args: vec![Type::Unknown],
                })
            }
            "channel_send" => {
                let elem = generic_element_type(
                    found.first(),
                    "Channel",
                    "channel_send",
                    node,
                    &mut self.diagnostics,
                );
                if let Some(value_ty) = found.get(1) {
                    self.check_expr_assignable(
                        &elem,
                        &args[1],
                        value_ty,
                        node,
                        TypeDiagnostic::new(
                            "channel_value_type",
                            "channel_send value does not match the channel element type",
                            &["replace_channel_value"],
                        ),
                    );
                }
                Some(Type::Bool)
            }
            "channel_recv" => Some(Type::Generic {
                name: "Option".to_string(),
                args: vec![generic_element_type(
                    found.first(),
                    "Channel",
                    "channel_recv",
                    node,
                    &mut self.diagnostics,
                )],
            }),
            "channel_close" => {
                expect_generic_value(
                    found.first(),
                    "Channel",
                    "channel_close",
                    node,
                    &mut self.diagnostics,
                );
                Some(Type::Void)
            }
            _ => None,
        }
    }

    fn infer_trait_method_call(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &LocalTypeEnv,
        node: &str,
    ) -> Option<Type> {
        let (trait_name, method_name) = name.split_once("::")?;
        let decl = self.program.traits().find(|decl| decl.name == trait_name)?;
        let Some(method) = decl
            .methods
            .iter()
            .find(|method| method.name == method_name)
            .cloned()
        else {
            self.diagnostics.push(
                Diagnostic::new(
                    "unknown_trait_method_call",
                    node,
                    format!("trait '{trait_name}' has no required method '{method_name}'"),
                )
                .suggest("choose_a_required_trait_method"),
            );
            for arg in args {
                self.infer_expr(arg, locals, node);
            }
            return Some(Type::Unknown);
        };

        let found_types = args
            .iter()
            .map(|arg| self.infer_expr(arg, locals, node))
            .collect::<Vec<_>>();
        if args.len() != method.params.len() {
            self.diagnostics.push(
                Diagnostic::new(
                    "trait_method_arity",
                    node,
                    format!(
                        "trait method '{name}' expects {} argument(s)",
                        method.params.len()
                    ),
                )
                .suggest("fix_call_arguments"),
            );
        }

        let self_param = "Self".to_string();
        let mut type_params = vec![self_param.clone()];
        let mut bindings = HashMap::new();
        for (param, found) in method.params.iter().zip(&found_types) {
            unify_type_params(&param.ty, found, &type_params, &mut bindings);
        }
        let Some(self_ty) = bindings.get(&self_param).cloned() else {
            self.diagnostics.push(
                Diagnostic::new(
                    "unresolved_trait_self",
                    node,
                    format!("trait method call '{name}' cannot infer Self from its arguments"),
                )
                .suggest("pass_a_self_typed_argument"),
            );
            return Some(Type::Unknown);
        };

        if let Some((implementation, impl_bindings)) =
            self.program.trait_impls().find_map(|implementation| {
                (implementation.trait_name == trait_name)
                    .then(|| {
                        trait_impl_bindings(
                            implementation,
                            &self_ty,
                            locals,
                            self.program,
                            &mut BTreeSet::new(),
                        )
                        .map(|bindings| (implementation, bindings))
                    })
                    .flatten()
            })
        {
            for type_param in &implementation.type_params {
                type_params.push(type_param.clone());
                if let Some(binding) = impl_bindings.get(type_param) {
                    bindings.insert(type_param.clone(), binding.clone());
                }
            }
            for binding in &implementation.associated_types {
                if !decl
                    .associated_types
                    .iter()
                    .any(|item| item.name == binding.name)
                {
                    continue;
                }
                let projection = format!("Self::{}", binding.name);
                let resolved = substitute_type_params(&binding.ty, &type_params, &bindings);
                type_params.push(projection.clone());
                bindings.insert(projection, resolved);
            }
        }

        if !type_satisfies_trait(
            &self_ty,
            trait_name,
            locals,
            self.program,
            &mut BTreeSet::new(),
        ) {
            self.diagnostics.push(
                Diagnostic::new(
                    "trait_method_bound_not_satisfied",
                    node,
                    format!(
                        "type '{}' does not implement trait '{trait_name}' required by '{name}'",
                        self_ty.to_human()
                    ),
                )
                .with_types(&Type::Named(trait_name.to_string()), &self_ty)
                .suggest("add_trait_bound_or_impl"),
            );
        }

        for ((param, arg), found) in method.params.iter().zip(args).zip(&found_types) {
            let expected = substitute_type_params(&param.ty, &type_params, &bindings);
            let expected = expose_trait_associated_projections(&expected, trait_name, &self_ty);
            let expected = self.normalize_associated_type_projections(&expected, locals);
            self.check_expr_assignable(
                &expected,
                arg,
                found,
                node,
                TypeDiagnostic::new(
                    "trait_method_arg_type",
                    format!(
                        "argument '{}' to trait method '{name}' has wrong type",
                        param.name
                    ),
                    &["insert_explicit_cast", "replace_call_argument"],
                ),
            );
        }
        let resolved = substitute_type_params(&method.ret, &type_params, &bindings);
        let resolved = expose_trait_associated_projections(&resolved, trait_name, &self_ty);
        Some(self.normalize_associated_type_projections(&resolved, locals))
    }

    fn validate_associated_type_projections(
        &mut self,
        ty: &Type,
        locals: &LocalTypeEnv,
        node: &str,
    ) {
        if let Some((trait_name, item_name, subject)) = ty.associated_projection_parts() {
            self.validate_associated_type_projections(subject, locals, node);
            let Some(trait_decl) = self.program.traits().find(|decl| decl.name == trait_name)
            else {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_projection_trait",
                        node,
                        format!(
                            "associated-type projection references unknown trait '{trait_name}'"
                        ),
                    )
                    .suggest("replace_projection_trait"),
                );
                return;
            };
            if !trait_decl
                .associated_types
                .iter()
                .any(|item| item.name == item_name)
            {
                self.diagnostics.push(
                    Diagnostic::new(
                        "unknown_associated_type_projection",
                        node,
                        format!("trait '{trait_name}' has no associated type '{item_name}'"),
                    )
                    .suggest("replace_associated_type_name"),
                );
                return;
            }
            if matches!(subject, Type::Named(name) if locals.is_type_param(name) && locals.has_trait_bound(name, trait_name))
                || self
                    .resolve_associated_type_projection(trait_name, item_name, subject, locals)
                    .is_some()
            {
                return;
            }
            self.diagnostics.push(
                Diagnostic::new(
                    "associated_type_impl_missing",
                    node,
                    format!(
                        "associated type '{trait_name}::{item_name}' has no applicable implementation for '{}'",
                        subject.to_human()
                    ),
                )
                .with_types(ty, subject)
                .suggest("add_trait_bound_or_impl"),
            );
            return;
        }
        match ty {
            Type::Generic { args, .. } => {
                for arg in args {
                    self.validate_associated_type_projections(arg, locals, node);
                }
            }
            Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
                self.validate_associated_type_projections(elem, locals, node)
            }
            Type::Bool | Type::Primitive(_) | Type::Named(_) | Type::Void | Type::Unknown => {}
        }
    }

    fn normalize_associated_type_projections(&self, ty: &Type, locals: &LocalTypeEnv) -> Type {
        self.normalize_associated_type_projections_inner(ty, locals, &mut BTreeSet::new())
    }

    fn normalize_associated_type_projections_inner(
        &self,
        ty: &Type,
        locals: &LocalTypeEnv,
        visiting: &mut BTreeSet<String>,
    ) -> Type {
        if let Some((trait_name, item_name, subject)) = ty.associated_projection_parts() {
            let subject =
                self.normalize_associated_type_projections_inner(subject, locals, visiting);
            if matches!(&subject, Type::Named(name) if locals.is_type_param(name) && locals.has_trait_bound(name, trait_name))
            {
                return Type::Generic {
                    name: format!("{trait_name}::{item_name}"),
                    args: vec![subject],
                };
            }
            let key = format!("{trait_name}::{item_name}[{}]", subject.to_agent());
            if !visiting.insert(key.clone()) {
                return Type::Generic {
                    name: format!("{trait_name}::{item_name}"),
                    args: vec![subject],
                };
            }
            if let Some(resolved) =
                self.resolve_associated_type_projection(trait_name, item_name, &subject, locals)
            {
                let normalized =
                    self.normalize_associated_type_projections_inner(&resolved, locals, visiting);
                visiting.remove(&key);
                return normalized;
            }
            visiting.remove(&key);
            return Type::Generic {
                name: format!("{trait_name}::{item_name}"),
                args: vec![subject],
            };
        }
        match ty {
            Type::Generic { name, args } => Type::Generic {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| {
                        self.normalize_associated_type_projections_inner(arg, locals, visiting)
                    })
                    .collect(),
            },
            Type::Array { elem, len } => Type::Array {
                elem: Box::new(
                    self.normalize_associated_type_projections_inner(elem, locals, visiting),
                ),
                len: *len,
            },
            Type::Slice { mutable, elem } => Type::Slice {
                mutable: *mutable,
                elem: Box::new(
                    self.normalize_associated_type_projections_inner(elem, locals, visiting),
                ),
            },
            Type::Pointer { mutable, elem } => Type::Pointer {
                mutable: *mutable,
                elem: Box::new(
                    self.normalize_associated_type_projections_inner(elem, locals, visiting),
                ),
            },
            other => other.clone(),
        }
    }

    fn resolve_associated_type_projection(
        &self,
        trait_name: &str,
        item_name: &str,
        subject: &Type,
        locals: &LocalTypeEnv,
    ) -> Option<Type> {
        let (implementation, impl_bindings) =
            self.program.trait_impls().find_map(|implementation| {
                (implementation.trait_name == trait_name)
                    .then(|| {
                        trait_impl_bindings(
                            implementation,
                            subject,
                            locals,
                            self.program,
                            &mut BTreeSet::new(),
                        )
                        .map(|bindings| (implementation, bindings))
                    })
                    .flatten()
            })?;
        let binding = implementation
            .associated_types
            .iter()
            .find(|binding| binding.name == item_name)?;
        let mut type_params = implementation.type_params.clone();
        let mut bindings = impl_bindings;
        type_params.push("Self".to_string());
        bindings.insert("Self".to_string(), subject.clone());
        Some(substitute_type_params(&binding.ty, &type_params, &bindings))
    }

    fn check_expr_assignable(
        &mut self,
        expected: &Type,
        expr: &Expr,
        found: &Type,
        node: &str,
        diagnostic: TypeDiagnostic<'_>,
    ) -> bool {
        let empty_locals = LocalTypeEnv::default();
        let normalized_expected =
            self.normalize_associated_type_projections(expected, &empty_locals);
        let normalized_found = self.normalize_associated_type_projections(found, &empty_locals);
        if !is_assignable(&normalized_expected, &normalized_found) {
            self.push_type_diagnostic(
                diagnostic.code,
                node,
                diagnostic.message,
                &normalized_expected,
                &normalized_found,
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
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
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
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
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
                // Both branches are checked; their moves and borrows are merged
                // conservatively into the parent so a value moved in either
                // branch is treated as possibly-moved afterward (sound join).
                let mut then_env = env.clone();
                self.check_safety_body(then_body, ret, &mut then_env, function, future_uses);
                let mut else_env = env.clone();
                self.check_safety_body(else_body, ret, &mut else_env, function, future_uses);
                env.merge_moved_from(&then_env);
                env.merge_branch_borrows_from(&then_env);
                env.merge_moved_from(&else_env);
                env.merge_branch_borrows_from(&else_env);
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
            Stmt::While { cond, body } => {
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
                let mut loop_env = env.clone();
                let outer_bindings = env.bindings.keys().cloned().collect::<BTreeSet<_>>();
                let mut loop_future_uses = future_uses.clone();
                // Both the body's reads and the re-evaluated condition's reads
                // are loop-carried, so a move of a value needed next iteration
                // is caught.
                let mut loop_carried_uses = collect_stmt_var_reads(body);
                collect_expr_var_reads(cond, &mut loop_carried_uses);
                loop_carried_uses.retain(|name| outer_bindings.contains(name));
                loop_future_uses.extend(loop_carried_uses);
                self.check_safety_body(body, ret, &mut loop_env, function, &loop_future_uses);
                env.merge_moved_from(&loop_env);
            }
            Stmt::Match { scrutinee, arms } => {
                let mut current_uses = BTreeSet::new();
                collect_expr_var_reads(scrutinee, &mut current_uses);
                let liveness = BorrowLiveness {
                    current: &current_uses,
                    future: future_uses,
                };
                self.check_expr_ownership(
                    scrutinee,
                    env,
                    function,
                    &function.id,
                    ValueUse::Read,
                    liveness,
                );
                // Each arm is checked from a pristine snapshot; the moves and
                // borrows of every arm merge conservatively into the parent
                // (sound N-way join, generalizing the two-branch `if`).
                let scrut_ty = self.safety_expr_type(scrutinee, env);
                let enum_decl = match &scrut_ty {
                    Type::Named(name) => self.program.enums().find(|e| &e.name == name),
                    _ => None,
                };
                let base_env = env.clone();
                for arm in arms {
                    let mut arm_env = base_env.clone();
                    // A payload binding is a fresh owned local in the arm, so
                    // ownership tracking catches misuse of a moved payload.
                    if let Pattern::Variant {
                        path,
                        binding: Some(name),
                    } = &arm.pattern
                    {
                        let variant_name = path.rsplit("::").next().unwrap_or("");
                        if let Some(payload) = enum_decl.and_then(|decl| {
                            decl.variants
                                .iter()
                                .find(|v| v.name == variant_name)
                                .and_then(|v| v.payload.clone())
                        }) {
                            arm_env.insert(
                                name.clone(),
                                payload,
                                ReferenceOrigin::Local,
                                None,
                                None,
                                None,
                            );
                        }
                    }
                    self.check_safety_body(&arm.body, ret, &mut arm_env, function, future_uses);
                    env.merge_moved_from(&arm_env);
                    env.merge_branch_borrows_from(&arm_env);
                }
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
            Expr::Lambda { body, .. } => {
                // Captured free variables are read (copied) at creation.
                self.check_expr_ownership(body, env, function, node, ValueUse::Read, liveness);
            }
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::TraitConst { .. } => {}
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

        if name == "task_spawn" {
            if let Some(callable) = args.first() {
                self.check_expr_ownership(callable, env, function, node, ValueUse::Read, liveness);
            }
            if let Some(input) = args.get(1) {
                let input_ty = self.safety_expr_type(input, env);
                let value_use = if moves_into_type(&input_ty) {
                    ValueUse::Move
                } else {
                    ValueUse::Read
                };
                self.check_expr_ownership(input, env, function, node, value_use, liveness);
            }
            return;
        }

        if name == "task_group_spawn" {
            if let Some(group) = args.first() {
                self.check_expr_ownership(group, env, function, node, ValueUse::Read, liveness);
            }
            if let Some(callable) = args.get(1) {
                self.check_expr_ownership(callable, env, function, node, ValueUse::Read, liveness);
            }
            if let Some(input) = args.get(2) {
                let input_ty = self.safety_expr_type(input, env);
                let value_use = if moves_into_type(&input_ty) {
                    ValueUse::Move
                } else {
                    ValueUse::Read
                };
                self.check_expr_ownership(input, env, function, node, value_use, liveness);
            }
            return;
        }

        if matches!(
            name,
            "task_await" | "task_cancel" | "task_group_await_all" | "task_group_cancel"
        ) {
            for arg in args {
                self.check_expr_ownership(arg, env, function, node, ValueUse::Move, liveness);
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
            Expr::Lambda { .. } => Vec::new(),
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
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::TraitConst { .. } => Vec::new(),
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
            Expr::Lambda { .. } => Vec::new(),
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
            | Expr::TraitConst { .. }
            | Expr::Binary { .. }
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_) => Vec::new(),
        }
    }

    fn borrow_parent(&self, expr: &Expr, env: &OwnershipEnv) -> Option<String> {
        match expr {
            Expr::Lambda { .. } => None,
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
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::TraitConst { .. } => None,
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
            Expr::Lambda { params, .. } => {
                let mut args: Vec<Type> = params.iter().map(|param| param.ty.clone()).collect();
                args.push(Type::Unknown);
                Type::Generic {
                    name: "Fn".to_string(),
                    args,
                }
            }
            Expr::Int(_) => Type::Primitive("$int".to_string()),
            Expr::Float(_) => Type::Primitive("$float".to_string()),
            Expr::Bool(_) => Type::Bool,
            Expr::String(_) => Type::Primitive("str".to_string()),
            Expr::TraitConst {
                trait_name,
                name,
                for_type,
            } => self
                .program
                .traits()
                .find(|item| item.name == *trait_name)
                .and_then(|item| item.constants.iter().find(|item| item.name == *name))
                .map(|constant| {
                    substitute_type_params(
                        &constant.ty,
                        &["Self".to_string()],
                        &HashMap::from([("Self".to_string(), for_type.clone())]),
                    )
                })
                .unwrap_or(Type::Unknown),
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
    if let (
        Some((expected_params, expected_ret, expected_effects)),
        Some((found_params, found_ret, found_effects)),
    ) = (expected.function_parts(), found.function_parts())
    {
        let expected_binders = expected.function_binders().unwrap_or_default();
        let found_binders = found.function_binders().unwrap_or_default();
        if expected_binders.len() != found_binders.len()
            || expected_binders.iter().zip(&found_binders).any(
                |((_, expected_bounds), (_, found_bounds))| {
                    let expected_bounds = expected_bounds.iter().collect::<BTreeSet<_>>();
                    found_bounds
                        .iter()
                        .any(|bound| !expected_bounds.contains(bound))
                },
            )
        {
            return false;
        }
        let found_type_params = found_binders
            .iter()
            .map(|(param, _)| param.clone())
            .collect::<Vec<_>>();
        let binder_bindings = found_binders
            .iter()
            .zip(&expected_binders)
            .map(|((found, _), (expected, _))| (found.clone(), Type::Named(expected.clone())))
            .collect::<HashMap<_, _>>();
        let found_params = found_params
            .iter()
            .map(|ty| substitute_type_params(ty, &found_type_params, &binder_bindings))
            .collect::<Vec<_>>();
        let found_ret = substitute_type_params(found_ret, &found_type_params, &binder_bindings);
        if expected_params.len() != found_params.len()
            || !expected_params
                .iter()
                .zip(&found_params)
                .all(|(expected, found)| is_assignable(expected, found))
            || !is_assignable(expected_ret, &found_ret)
        {
            return false;
        }
        let expected_effects = expected_effects
            .iter()
            .map(Type::to_agent)
            .collect::<BTreeSet<_>>();
        return found_effects
            .iter()
            .map(Type::to_agent)
            .all(|effect| expected_effects.contains(&effect));
    }
    // Same-constructor generics (e.g. `Vec<i32>` vs `Vec<$unknown>`): assignable
    // when the arguments are pairwise assignable. Lets a polymorphic constructor
    // like `vec_new()` bind to an annotated `Vec<T>`.
    if let (
        Type::Generic {
            name: expected_name,
            args: expected_args,
        },
        Type::Generic {
            name: found_name,
            args: found_args,
        },
    ) = (expected, found)
    {
        return expected_name == found_name
            && expected_args.len() == found_args.len()
            && expected_args
                .iter()
                .zip(found_args)
                .all(|(e, f)| is_assignable(e, f));
    }
    false
}

fn expose_trait_associated_projections(ty: &Type, trait_name: &str, self_ty: &Type) -> Type {
    match ty {
        Type::Named(name) if name.starts_with("Self::") => Type::Generic {
            name: format!("{trait_name}::{}", name.trim_start_matches("Self::")),
            args: vec![self_ty.clone()],
        },
        Type::Generic { name, args } => Type::Generic {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| expose_trait_associated_projections(arg, trait_name, self_ty))
                .collect(),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(expose_trait_associated_projections(
                elem, trait_name, self_ty,
            )),
            len: *len,
        },
        Type::Slice { mutable, elem } => Type::Slice {
            mutable: *mutable,
            elem: Box::new(expose_trait_associated_projections(
                elem, trait_name, self_ty,
            )),
        },
        Type::Pointer { mutable, elem } => Type::Pointer {
            mutable: *mutable,
            elem: Box::new(expose_trait_associated_projections(
                elem, trait_name, self_ty,
            )),
        },
        other => other.clone(),
    }
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
        // A function value is a name, not owned data (lang.4).
        Type::Generic { name, .. } if name == "Fn" => true,
        Type::Named(_) | Type::Generic { .. } | Type::Array { .. } => false,
    }
}

fn is_const_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => true,
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => is_const_expr(expr),
        Expr::Var(_)
        | Expr::TraitConst { .. }
        | Expr::Call { .. }
        | Expr::Binary { .. }
        | Expr::Field { .. }
        | Expr::Index { .. }
        | Expr::Lambda { .. } => false,
    }
}

/// Structural unification for generic calls (lang.4): walk the declared
/// parameter type against the argument type, binding each of the callee's type
/// parameters to the first concrete type found in its position. Mismatches are
/// not reported here — they surface in the assignability check against the
/// substituted signature.
fn unify_type_params(
    param: &Type,
    found: &Type,
    type_params: &[String],
    bindings: &mut HashMap<String, Type>,
) {
    match (param, found) {
        // An Unknown argument type must not pin a type parameter.
        (Type::Named(name), _) if type_params.contains(name) && !matches!(found, Type::Unknown) => {
            bindings
                .entry(name.clone())
                .or_insert_with(|| found.clone());
        }
        (
            Type::Generic {
                name: left,
                args: left_args,
            },
            Type::Generic {
                name: right,
                args: right_args,
            },
        ) if left == right => {
            for (l, r) in left_args.iter().zip(right_args) {
                unify_type_params(l, r, type_params, bindings);
            }
        }
        (Type::Slice { elem: l, .. }, Type::Slice { elem: r, .. })
        | (Type::Array { elem: l, .. }, Type::Array { elem: r, .. })
        | (Type::Pointer { elem: l, .. }, Type::Pointer { elem: r, .. }) => {
            unify_type_params(l, r, type_params, bindings);
        }
        _ => {}
    }
}

/// Substitute bound type parameters into a declared type. A declared-but-unbound
/// type parameter becomes `Unknown` (lenient) rather than leaking as a phantom
/// named type.
fn substitute_type_params(
    ty: &Type,
    type_params: &[String],
    bindings: &HashMap<String, Type>,
) -> Type {
    match ty {
        Type::Named(name) if type_params.contains(name) => {
            bindings.get(name).cloned().unwrap_or(Type::Unknown)
        }
        Type::Generic { name, args } => Type::Generic {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute_type_params(arg, type_params, bindings))
                .collect(),
        },
        Type::Slice { mutable, elem } => Type::Slice {
            mutable: *mutable,
            elem: Box::new(substitute_type_params(elem, type_params, bindings)),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(substitute_type_params(elem, type_params, bindings)),
            len: *len,
        },
        Type::Pointer { mutable, elem } => Type::Pointer {
            mutable: *mutable,
            elem: Box::new(substitute_type_params(elem, type_params, bindings)),
        },
        other => other.clone(),
    }
}

/// The `Fn[P…, R]` type of a function used as a value (lang.4): the parameter
/// types followed by the return type, as a bracket generic.
fn fn_value_type(function: &FunctionDecl) -> Type {
    Type::generic_function_type(
        &function.type_params,
        &function.type_bounds,
        function
            .params
            .iter()
            .map(|param| param.ty.clone())
            .collect(),
        function.ret.clone(),
        &function.effects,
    )
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
    ) || matches!(ty, Type::Generic { name, .. } if name == "Vec")
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
        Stmt::Return(None) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_expr_var_reads(cond, out);
            for stmt in then_body.iter().chain(else_body) {
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
        Stmt::While { cond, body } => {
            collect_expr_var_reads(cond, out);
            for stmt in body {
                collect_stmt_var_reads_into(stmt, out);
            }
        }
        Stmt::Match { scrutinee, arms } => {
            collect_expr_var_reads(scrutinee, out);
            for arm in arms {
                for stmt in &arm.body {
                    collect_stmt_var_reads_into(stmt, out);
                }
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
        Expr::Lambda { params, body } => {
            // Free variables are captured (read); parameters are not reads.
            let mut inner = BTreeSet::new();
            collect_expr_var_reads(body, &mut inner);
            for param in params {
                inner.remove(&param.name);
            }
            out.extend(inner);
        }
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
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
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
        // Growable Vec<T> (lang.3). `vec_new` yields an empty vec whose element
        // type is pinned by the binding; `push` returns void; `pop` yields the
        // removed element as an `Option` (opaque element for now).
        "vec_new" => Some(Type::Generic {
            name: "Vec".to_string(),
            args: vec![Type::Unknown],
        }),
        "push" => Some(Type::Void),
        "pop" => Some(Type::Unknown),
        // String library (lang.3).
        "str_concat" | "int_to_str" | "str_slice" => Some(Type::Primitive("str".to_string())),
        "str_len" => Some(Type::Primitive("usize".to_string())),
        "str_find" => Some(Type::Unknown),
        "str_contains" => Some(Type::Bool),
        "str_split" => Some(Type::Generic {
            name: "Vec".to_string(),
            args: vec![Type::Primitive("str".to_string())],
        }),
        // Growable Map[K, V] (lang.3).
        "map_new" => Some(Type::Generic {
            name: "Map".to_string(),
            args: vec![Type::Unknown, Type::Unknown],
        }),
        "map_set" => Some(Type::Void),
        "map_get" | "map_remove" => Some(Type::Unknown),
        "map_has" => Some(Type::Bool),
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

fn generic_element_type(
    found: Option<&Type>,
    constructor: &str,
    operation: &str,
    node: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match found {
        Some(Type::Generic { name, args }) if name == constructor && args.len() == 1 => {
            args[0].clone()
        }
        Some(found) => {
            diagnostics.push(
                Diagnostic::new(
                    "concurrency_handle_type",
                    node,
                    format!("{operation} requires a {constructor}[T] handle"),
                )
                .with_types(&Type::Named(constructor.to_string()), found)
                .suggest("replace_concurrency_handle"),
            );
            Type::Unknown
        }
        None => Type::Unknown,
    }
}

fn expect_generic_value(
    found: Option<&Type>,
    constructor: &str,
    operation: &str,
    node: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let _ = generic_element_type(found, constructor, operation, node, diagnostics);
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

fn is_known_trait(program: &Program, trait_name: &str) -> bool {
    trait_name == "Eq" || program.traits().any(|decl| decl.name == trait_name)
}

fn type_mentions_param(ty: &Type, param: &str) -> bool {
    match ty {
        Type::Named(name) => name == param,
        Type::Generic { args, .. } => args.iter().any(|arg| type_mentions_param(arg, param)),
        Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
            type_mentions_param(elem, param)
        }
        Type::Bool | Type::Primitive(_) | Type::Void | Type::Unknown => false,
    }
}

fn impl_patterns_overlap(
    left: &Type,
    left_params: &[String],
    right: &Type,
    right_params: &[String],
) -> bool {
    if matches!(left, Type::Named(name) if left_params.contains(name))
        || matches!(right, Type::Named(name) if right_params.contains(name))
    {
        return true;
    }
    match (left, right) {
        (
            Type::Generic {
                name: left_name,
                args: left_args,
            },
            Type::Generic {
                name: right_name,
                args: right_args,
            },
        ) => {
            left_name == right_name
                && left_args.len() == right_args.len()
                && left_args.iter().zip(right_args).all(|(left, right)| {
                    impl_patterns_overlap(left, left_params, right, right_params)
                })
        }
        (
            Type::Array {
                elem: left_elem,
                len: left_len,
            },
            Type::Array {
                elem: right_elem,
                len: right_len,
            },
        ) => {
            left_len == right_len
                && impl_patterns_overlap(left_elem, left_params, right_elem, right_params)
        }
        (
            Type::Slice {
                elem: left_elem, ..
            },
            Type::Slice {
                elem: right_elem, ..
            },
        ) => impl_patterns_overlap(left_elem, left_params, right_elem, right_params),
        (
            Type::Pointer {
                mutable: left_mutable,
                elem: left_elem,
            },
            Type::Pointer {
                mutable: right_mutable,
                elem: right_elem,
            },
        ) => {
            left_mutable == right_mutable
                && impl_patterns_overlap(left_elem, left_params, right_elem, right_params)
        }
        _ => left == right || is_assignable(left, right) || is_assignable(right, left),
    }
}

fn impl_pattern_matches(
    pattern: &Type,
    found: &Type,
    type_params: &[String],
    bindings: &mut HashMap<String, Type>,
) -> bool {
    if matches!(found, Type::Unknown) {
        return false;
    }
    if let Type::Named(name) = pattern {
        if type_params.contains(name) {
            return match bindings.get(name) {
                Some(existing) => {
                    existing == found
                        || is_assignable(existing, found)
                        || is_assignable(found, existing)
                }
                None => {
                    bindings.insert(name.clone(), found.clone());
                    true
                }
            };
        }
    }
    match (pattern, found) {
        (
            Type::Generic {
                name: pattern_name,
                args: pattern_args,
            },
            Type::Generic {
                name: found_name,
                args: found_args,
            },
        ) => {
            pattern_name == found_name
                && pattern_args.len() == found_args.len()
                && pattern_args.iter().zip(found_args).all(|(pattern, found)| {
                    impl_pattern_matches(pattern, found, type_params, bindings)
                })
        }
        (
            Type::Array {
                elem: pattern_elem,
                len: pattern_len,
            },
            Type::Array {
                elem: found_elem,
                len: found_len,
            },
        ) => {
            pattern_len == found_len
                && impl_pattern_matches(pattern_elem, found_elem, type_params, bindings)
        }
        (
            Type::Slice {
                mutable: pattern_mutable,
                elem: pattern_elem,
            },
            Type::Slice {
                mutable: found_mutable,
                elem: found_elem,
            },
        ) => {
            (*pattern_mutable == *found_mutable || (!pattern_mutable && *found_mutable))
                && impl_pattern_matches(pattern_elem, found_elem, type_params, bindings)
        }
        (
            Type::Pointer {
                mutable: pattern_mutable,
                elem: pattern_elem,
            },
            Type::Pointer {
                mutable: found_mutable,
                elem: found_elem,
            },
        ) => {
            pattern_mutable == found_mutable
                && impl_pattern_matches(pattern_elem, found_elem, type_params, bindings)
        }
        _ => is_assignable(pattern, found),
    }
}

fn trait_impl_applies(
    implementation: &crate::ast::TraitImplDecl,
    ty: &Type,
    locals: &LocalTypeEnv,
    program: &Program,
    visiting: &mut BTreeSet<String>,
) -> bool {
    trait_impl_bindings(implementation, ty, locals, program, visiting).is_some()
}

fn trait_impl_bindings(
    implementation: &crate::ast::TraitImplDecl,
    ty: &Type,
    locals: &LocalTypeEnv,
    program: &Program,
    visiting: &mut BTreeSet<String>,
) -> Option<HashMap<String, Type>> {
    let mut bindings = HashMap::new();
    if !impl_pattern_matches(
        &implementation.for_type,
        ty,
        &implementation.type_params,
        &mut bindings,
    ) {
        return None;
    }
    let bounds_hold = implementation.type_bounds.iter().all(|bound| {
        let Some(found) = bindings.get(&bound.type_param) else {
            return false;
        };
        bound
            .traits
            .iter()
            .all(|trait_name| type_satisfies_trait(found, trait_name, locals, program, visiting))
    });
    bounds_hold.then_some(bindings)
}

fn type_satisfies_trait(
    ty: &Type,
    trait_name: &str,
    locals: &LocalTypeEnv,
    program: &Program,
    visiting: &mut BTreeSet<String>,
) -> bool {
    if let Type::Named(name) = ty {
        if locals.is_type_param(name) {
            return locals.has_trait_bound(name, trait_name);
        }
    }
    if trait_name != "Eq" {
        let key = format!("{trait_name}:{}", ty.to_agent());
        if !visiting.insert(key.clone()) {
            return false;
        }
        let result = program.trait_impls().any(|implementation| {
            implementation.trait_name == trait_name
                && trait_impl_applies(implementation, ty, locals, program, visiting)
        });
        visiting.remove(&key);
        return result;
    }
    match ty {
        Type::Unknown => true,
        Type::Bool | Type::Primitive(_) => true,
        Type::Named(name) => {
            let key = format!("named:{name}");
            if !visiting.insert(key.clone()) {
                return false;
            }
            let result = if let Some(decl) = program.structs().find(|decl| decl.name == *name) {
                decl.fields
                    .iter()
                    .all(|field| type_satisfies_trait(&field.ty, "Eq", locals, program, visiting))
            } else if let Some(decl) = program.enums().find(|decl| decl.name == *name) {
                decl.variants.iter().all(|variant| {
                    variant.payload.as_ref().is_none_or(|payload| {
                        type_satisfies_trait(payload, "Eq", locals, program, visiting)
                    })
                })
            } else {
                false
            };
            visiting.remove(&key);
            result
        }
        Type::Generic { name, args }
            if matches!(name.as_str(), "Vec" | "Map" | "Option" | "Result") =>
        {
            args.iter()
                .all(|arg| type_satisfies_trait(arg, "Eq", locals, program, visiting))
        }
        Type::Array { elem, .. } => type_satisfies_trait(elem, "Eq", locals, program, visiting),
        Type::Generic { .. } | Type::Slice { .. } | Type::Pointer { .. } | Type::Void => false,
    }
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
