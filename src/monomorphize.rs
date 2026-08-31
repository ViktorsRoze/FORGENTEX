//! Conservative, deterministic monomorphization for the C and Zig source
//! backends.
//!
//! The interpreter continues to execute generic declarations directly. Before
//! source-backend lowering, this pass clones eligible private generic functions
//! for every concrete direct-call instantiation and rewrites those calls. Cases
//! that cannot be resolved stay generic so the existing FIR contract reports an
//! honest blocker instead of emitting an invalid source program.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{
    Expr, FunctionDecl, Item, Pattern, Stmt, StructDecl, TraitDecl, TraitImplDecl, Type,
};
use crate::project::Project;

#[derive(Clone, Debug)]
struct Template {
    module: usize,
    function: FunctionDecl,
    public: bool,
}

#[derive(Clone, Debug)]
struct Request {
    template: String,
    bindings: BTreeMap<String, Type>,
    specialized_name: String,
}

#[derive(Default)]
struct Engine {
    templates: BTreeMap<String, Template>,
    functions: BTreeMap<String, FunctionDecl>,
    structs: BTreeMap<String, StructDecl>,
    traits: BTreeMap<String, TraitDecl>,
    trait_impls: Vec<TraitImplDecl>,
    requests: BTreeMap<String, Request>,
    pending: BTreeSet<String>,
    processed: BTreeSet<String>,
    rewritten: BTreeSet<String>,
    blocked: BTreeSet<String>,
    occupied_names: BTreeSet<String>,
}

/// Specialize every eligible generic direct call in `project` in place.
///
/// This pass intentionally returns success for unsupported generic shapes. It
/// leaves those declarations/calls untouched so backend contract preflight
/// remains the single user-facing source of lowering blockers.
pub(crate) fn monomorphize_project(project: &mut Project) -> Result<usize, String> {
    let mut engine = Engine::from_project(project);

    // Calls reachable from ordinary functions seed the specialization worklist.
    // Generic templates themselves are visited only after concrete substitution.
    for module in &mut project.modules {
        for item in &mut module.program.items {
            let Item::Function(function) = item else {
                continue;
            };
            if function.type_params.is_empty() {
                engine.normalize_function_types(function);
                engine.rewrite_function(function);
            }
        }
    }

    let mut generated: BTreeMap<(usize, String), Vec<FunctionDecl>> = BTreeMap::new();
    while let Some(key) = engine.pending.iter().next().cloned() {
        engine.pending.remove(&key);
        if !engine.processed.insert(key.clone()) {
            continue;
        }
        let Some(request) = engine.requests.get(&key).cloned() else {
            continue;
        };
        let Some(template) = engine.templates.get(&request.template).cloned() else {
            continue;
        };
        let mut function = specialize_function(&template.function, &request);
        engine.normalize_function_types(&mut function);
        engine.rewrite_function(&mut function);
        engine
            .functions
            .insert(function.name.clone(), function.clone());
        generated
            .entry((template.module, template.function.name.clone()))
            .or_default()
            .push(function);
    }

    let removable = engine
        .rewritten
        .difference(&engine.blocked)
        .cloned()
        .collect::<BTreeSet<_>>();
    let generated_count = generated
        .iter()
        .filter(|((_, template), _)| removable.contains(template))
        .map(|(_, functions)| functions.len())
        .sum();

    for (module_index, module) in project.modules.iter_mut().enumerate() {
        let mut items = Vec::with_capacity(module.program.items.len() + generated_count);
        for item in module.program.items.drain(..) {
            match item {
                Item::Function(function)
                    if !function.type_params.is_empty() && removable.contains(&function.name) =>
                {
                    if let Some(mut functions) =
                        generated.remove(&(module_index, function.name.clone()))
                    {
                        functions.sort_by(|left, right| left.name.cmp(&right.name));
                        items.extend(functions.into_iter().map(Item::Function));
                    }
                }
                other => items.push(other),
            }
        }
        module.program.items = items;
    }

    Ok(generated_count)
}

impl Engine {
    fn from_project(project: &Project) -> Self {
        let mut engine = Self::default();
        for (module_index, module) in project.modules.iter().enumerate() {
            let public = module
                .public_symbols
                .iter()
                .filter(|symbol| symbol.kind == "fn")
                .map(|symbol| symbol.name.clone())
                .collect::<BTreeSet<_>>();
            for item in &module.program.items {
                match item {
                    Item::Function(function) => {
                        engine.occupied_names.insert(function.name.clone());
                        engine
                            .functions
                            .insert(function.name.clone(), function.clone());
                        if !function.type_params.is_empty() {
                            engine.templates.insert(
                                function.name.clone(),
                                Template {
                                    module: module_index,
                                    function: function.clone(),
                                    public: public.contains(&function.name),
                                },
                            );
                        }
                    }
                    Item::Struct(decl) => {
                        engine.structs.insert(decl.name.clone(), decl.clone());
                    }
                    Item::Trait(decl) => {
                        engine.traits.insert(decl.name.clone(), decl.clone());
                    }
                    Item::TraitImpl(decl) => {
                        engine.trait_impls.push(decl.clone());
                    }
                    Item::Enum(_)
                    | Item::Test(_)
                    | Item::Capsule(_)
                    | Item::Share(_)
                    | Item::Pipeline(_) => {}
                }
            }
        }
        engine
    }

    fn rewrite_function(&mut self, function: &mut FunctionDecl) {
        let mut locals = function
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect::<BTreeMap<_, _>>();
        self.rewrite_body(&mut function.body, &function.ret, &mut locals);
    }

    fn normalize_function_types(&self, function: &mut FunctionDecl) {
        for param in &mut function.params {
            param.ty = self.normalize_associated_type_projections(&param.ty);
        }
        function.ret = self.normalize_associated_type_projections(&function.ret);
        self.normalize_body_types(&mut function.body);
    }

    fn normalize_body_types(&self, body: &mut [Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Let { ty, expr, .. } => {
                    if let Some(ty) = ty {
                        *ty = self.normalize_associated_type_projections(ty);
                    }
                    self.normalize_expr_types(expr);
                }
                Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
                    self.normalize_expr_types(expr)
                }
                Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.normalize_expr_types(cond);
                    self.normalize_body_types(then_body);
                    self.normalize_body_types(else_body);
                }
                Stmt::For {
                    start, end, body, ..
                } => {
                    self.normalize_expr_types(start);
                    self.normalize_expr_types(end);
                    self.normalize_body_types(body);
                }
                Stmt::While { cond, body } => {
                    self.normalize_expr_types(cond);
                    self.normalize_body_types(body);
                }
                Stmt::Match { scrutinee, arms } => {
                    self.normalize_expr_types(scrutinee);
                    for arm in arms {
                        self.normalize_body_types(&mut arm.body);
                    }
                }
                Stmt::Assign { target, expr } => {
                    self.normalize_expr_types(target);
                    self.normalize_expr_types(expr);
                }
            }
        }
    }

    fn normalize_expr_types(&self, expr: &mut Expr) {
        match expr {
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => {}
            Expr::TraitConst { for_type, .. } => {
                *for_type = self.normalize_associated_type_projections(for_type)
            }
            Expr::Unary { expr, .. } => self.normalize_expr_types(expr),
            Expr::Binary { left, right, .. } => {
                self.normalize_expr_types(left);
                self.normalize_expr_types(right);
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    self.normalize_expr_types(arg);
                }
            }
            Expr::Cast { ty, expr } => {
                *ty = self.normalize_associated_type_projections(ty);
                self.normalize_expr_types(expr);
            }
            Expr::Field { base, .. } => self.normalize_expr_types(base),
            Expr::Index { base, index } => {
                self.normalize_expr_types(base);
                self.normalize_expr_types(index);
            }
            Expr::SliceRange { base, start, end } => {
                self.normalize_expr_types(base);
                self.normalize_expr_types(start);
                self.normalize_expr_types(end);
            }
            Expr::Lambda { params, body } => {
                for param in params {
                    param.ty = self.normalize_associated_type_projections(&param.ty);
                }
                self.normalize_expr_types(body);
            }
        }
    }

    fn normalize_associated_type_projections(&self, ty: &Type) -> Type {
        self.normalize_associated_type_projections_inner(ty, &mut BTreeSet::new())
    }

    fn normalize_associated_type_projections_inner(
        &self,
        ty: &Type,
        visiting: &mut BTreeSet<String>,
    ) -> Type {
        if let Some((trait_name, item_name, subject)) = ty.associated_projection_parts() {
            let subject = self.normalize_associated_type_projections_inner(subject, visiting);
            let key = format!("{trait_name}::{item_name}[{}]", subject.to_agent());
            if !visiting.insert(key.clone()) {
                return Type::Generic {
                    name: format!("{trait_name}::{item_name}"),
                    args: vec![subject],
                };
            }
            if let Some(resolved) =
                self.resolve_associated_type_projection(trait_name, item_name, &subject)
            {
                let normalized =
                    self.normalize_associated_type_projections_inner(&resolved, visiting);
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
                    .map(|arg| self.normalize_associated_type_projections_inner(arg, visiting))
                    .collect(),
            },
            Type::Array { elem, len } => Type::Array {
                elem: Box::new(self.normalize_associated_type_projections_inner(elem, visiting)),
                len: *len,
            },
            Type::Slice { mutable, elem } => Type::Slice {
                mutable: *mutable,
                elem: Box::new(self.normalize_associated_type_projections_inner(elem, visiting)),
            },
            Type::Pointer { mutable, elem } => Type::Pointer {
                mutable: *mutable,
                elem: Box::new(self.normalize_associated_type_projections_inner(elem, visiting)),
            },
            other => other.clone(),
        }
    }

    fn resolve_associated_type_projection(
        &self,
        trait_name: &str,
        item_name: &str,
        subject: &Type,
    ) -> Option<Type> {
        let (implementation, mut bindings) =
            self.trait_impls.iter().find_map(|implementation| {
                if implementation.trait_name != trait_name {
                    return None;
                }
                let mut bindings = BTreeMap::new();
                impl_pattern_matches(
                    &implementation.for_type,
                    subject,
                    &implementation.type_params,
                    &mut bindings,
                )
                .then_some((implementation, bindings))
            })?;
        let binding = implementation
            .associated_types
            .iter()
            .find(|binding| binding.name == item_name)?;
        let mut type_params = implementation.type_params.clone();
        type_params.push("Self".to_string());
        bindings.insert("Self".to_string(), subject.clone());
        Some(substitute_type(&binding.ty, &type_params, &bindings))
    }

    fn rewrite_body(
        &mut self,
        body: &mut [Stmt],
        return_type: &Type,
        locals: &mut BTreeMap<String, Type>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Let { name, ty, expr } => {
                    let found = self.rewrite_expr(expr, locals, ty.as_ref());
                    locals.insert(name.clone(), ty.clone().unwrap_or(found));
                }
                Stmt::Return(Some(expr)) => {
                    self.rewrite_expr(expr, locals, Some(return_type));
                }
                Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
                Stmt::Require(expr) => {
                    self.rewrite_expr(expr, locals, Some(&Type::Bool));
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.rewrite_expr(cond, locals, Some(&Type::Bool));
                    self.rewrite_body(then_body, return_type, &mut locals.clone());
                    self.rewrite_body(else_body, return_type, &mut locals.clone());
                }
                Stmt::For {
                    var,
                    start,
                    end,
                    body,
                } => {
                    let index_type = Type::Primitive("usize".to_string());
                    self.rewrite_expr(start, locals, Some(&index_type));
                    self.rewrite_expr(end, locals, Some(&index_type));
                    let mut loop_locals = locals.clone();
                    loop_locals.insert(var.clone(), index_type);
                    self.rewrite_body(body, return_type, &mut loop_locals);
                }
                Stmt::While { cond, body } => {
                    self.rewrite_expr(cond, locals, Some(&Type::Bool));
                    self.rewrite_body(body, return_type, &mut locals.clone());
                }
                Stmt::Match { scrutinee, arms } => {
                    self.rewrite_expr(scrutinee, locals, None);
                    for arm in arms {
                        let mut arm_locals = locals.clone();
                        if let Pattern::Variant {
                            binding: Some(binding),
                            ..
                        } = &arm.pattern
                        {
                            arm_locals.insert(binding.clone(), Type::Unknown);
                        }
                        self.rewrite_body(&mut arm.body, return_type, &mut arm_locals);
                    }
                }
                Stmt::Assign { target, expr } => {
                    let expected = self.infer_expr_type(target, locals);
                    self.rewrite_expr(expr, locals, Some(&expected));
                    self.rewrite_expr(target, locals, None);
                }
                Stmt::Expr(expr) => {
                    self.rewrite_expr(expr, locals, None);
                }
            }
        }
    }

    fn rewrite_expr(
        &mut self,
        expr: &mut Expr,
        locals: &BTreeMap<String, Type>,
        expected: Option<&Type>,
    ) -> Type {
        match expr {
            Expr::Int(_) => expected
                .filter(|ty| ty.is_integer() || ty.is_float())
                .cloned()
                .unwrap_or_else(|| Type::Primitive("$int".to_string())),
            Expr::Float(_) => expected
                .filter(|ty| ty.is_float())
                .cloned()
                .unwrap_or_else(|| Type::Primitive("$float".to_string())),
            Expr::Bool(_) => Type::Bool,
            Expr::String(_) => Type::Primitive("str".to_string()),
            Expr::Var(name) => locals
                .get(name)
                .cloned()
                .or_else(|| self.function_value_type(name))
                .unwrap_or(Type::Unknown),
            Expr::TraitConst {
                trait_name,
                name,
                for_type,
            } => {
                let replacement = self
                    .trait_impls
                    .iter()
                    .find(|implementation| {
                        if implementation.trait_name != *trait_name {
                            return false;
                        }
                        let mut bindings = BTreeMap::new();
                        impl_pattern_matches(
                            &implementation.for_type,
                            for_type,
                            &implementation.type_params,
                            &mut bindings,
                        )
                    })
                    .and_then(|implementation| {
                        implementation
                            .constants
                            .iter()
                            .find(|constant| constant.name == *name)
                    })
                    .map(|constant| constant.value.clone());
                if let Some(replacement) = replacement {
                    *expr = replacement;
                    self.rewrite_expr(expr, locals, expected)
                } else {
                    Type::Unknown
                }
            }
            Expr::Lambda { params, body } => {
                let mut lambda_locals = locals.clone();
                for param in params.iter() {
                    lambda_locals.insert(param.name.clone(), param.ty.clone());
                }
                let ret = self.rewrite_expr(body, &lambda_locals, None);
                let mut args = params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect::<Vec<_>>();
                args.push(ret);
                Type::Generic {
                    name: "Fn".to_string(),
                    args,
                }
            }
            Expr::Unary { op, expr } => {
                if op == "!" {
                    self.rewrite_expr(expr, locals, Some(&Type::Bool));
                    Type::Bool
                } else {
                    self.rewrite_expr(expr, locals, expected)
                }
            }
            Expr::Binary { op, left, right } => {
                let left_ty = self.rewrite_expr(left, locals, expected);
                let right_ty = self.rewrite_expr(right, locals, Some(&left_ty));
                if matches!(
                    op.as_str(),
                    "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||"
                ) {
                    Type::Bool
                } else if !matches!(left_ty, Type::Unknown) {
                    left_ty
                } else {
                    right_ty
                }
            }
            Expr::Call { name, args } => {
                if let Some(template) = self.templates.get(name).cloned() {
                    return self.rewrite_generic_call(name, args, locals, expected, &template);
                }
                if let Some(ty) = self.rewrite_trait_call(name, args, locals) {
                    return ty;
                }

                let function = self.functions.get(name).cloned();
                let constructor = self.structs.get(name).cloned();
                for (index, arg) in args.iter_mut().enumerate() {
                    let arg_expected = function
                        .as_ref()
                        .and_then(|decl| decl.params.get(index))
                        .map(|param| &param.ty)
                        .or_else(|| {
                            constructor
                                .as_ref()
                                .and_then(|decl| decl.fields.get(index))
                                .map(|field| &field.ty)
                        });
                    self.rewrite_expr(arg, locals, arg_expected);
                }
                function
                    .map(|decl| decl.ret)
                    .or_else(|| constructor.map(|decl| Type::Named(decl.name)))
                    .unwrap_or(Type::Unknown)
            }
            Expr::Cast { ty, expr } => {
                self.rewrite_expr(expr, locals, Some(ty));
                ty.clone()
            }
            Expr::Field { base, field } => {
                let base_ty = self.rewrite_expr(base, locals, None);
                let name = match base_ty {
                    Type::Named(name) => Some(name),
                    _ => None,
                };
                name.and_then(|name| self.structs.get(&name))
                    .and_then(|decl| decl.fields.iter().find(|item| item.name == *field))
                    .map(|item| item.ty.clone())
                    .unwrap_or(Type::Unknown)
            }
            Expr::Index { base, index } => {
                let index_type = Type::Primitive("usize".to_string());
                self.rewrite_expr(index, locals, Some(&index_type));
                self.rewrite_expr(base, locals, None)
                    .element_type()
                    .unwrap_or(Type::Unknown)
            }
            Expr::SliceRange { base, start, end } => {
                let index_type = Type::Primitive("usize".to_string());
                self.rewrite_expr(start, locals, Some(&index_type));
                self.rewrite_expr(end, locals, Some(&index_type));
                match self.rewrite_expr(base, locals, None) {
                    Type::Slice { mutable, elem } => Type::Slice { mutable, elem },
                    Type::Array { elem, .. } => Type::Slice {
                        mutable: false,
                        elem,
                    },
                    _ => Type::Unknown,
                }
            }
        }
    }

    fn rewrite_trait_call(
        &mut self,
        name: &mut String,
        args: &mut [Expr],
        locals: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let (trait_name, method_name) = name.split_once("::")?;
        let trait_decl = self.traits.get(trait_name)?.clone();
        let Some(method) = trait_decl
            .methods
            .iter()
            .find(|method| method.name == method_name)
            .cloned()
        else {
            for arg in args {
                self.rewrite_expr(arg, locals, None);
            }
            return Some(Type::Unknown);
        };

        let mut found_types = Vec::with_capacity(args.len());
        for arg in args.iter_mut() {
            found_types.push(self.rewrite_expr(arg, locals, None));
        }
        let self_param = "Self".to_string();
        let type_params = vec![self_param.clone()];
        let mut bindings = BTreeMap::new();
        for (param, found) in method.params.iter().zip(&found_types) {
            unify_type_params(&param.ty, found, &type_params, &mut bindings);
        }
        let Some(self_ty) = bindings.get(&self_param) else {
            return Some(Type::Unknown);
        };
        let unresolved_ret = substitute_type(&method.ret, &type_params, &bindings);

        let candidates = self
            .trait_impls
            .iter()
            .filter(|implementation| implementation.trait_name == trait_name)
            .filter_map(|implementation| {
                let mut impl_bindings = BTreeMap::new();
                if !impl_pattern_matches(
                    &implementation.for_type,
                    self_ty,
                    &implementation.type_params,
                    &mut impl_bindings,
                ) {
                    return None;
                }
                let target = implementation
                    .methods
                    .iter()
                    .find(|mapping| mapping.method == method_name)
                    .map(|mapping| mapping.function.as_str())
                    .or(method.default_function.as_deref())?;
                self.functions
                    .get(target)
                    .cloned()
                    .map(|function| (function, implementation.clone(), impl_bindings))
            })
            .collect::<Vec<_>>();
        let [(function, implementation, impl_bindings)] = candidates.as_slice() else {
            return Some(unresolved_ret);
        };

        let mut concrete_type_params = vec![self_param.clone()];
        let concrete_self = substitute_type(
            &implementation.for_type,
            &implementation.type_params,
            impl_bindings,
        );
        let mut concrete_bindings = BTreeMap::from([(self_param.clone(), concrete_self)]);
        for type_param in &implementation.type_params {
            concrete_type_params.push(type_param.clone());
            if let Some(binding) = impl_bindings.get(type_param) {
                concrete_bindings.insert(type_param.clone(), binding.clone());
            }
        }
        for associated_type in &implementation.associated_types {
            let projection = format!("Self::{}", associated_type.name);
            let resolved = substitute_type(
                &associated_type.ty,
                &concrete_type_params,
                &concrete_bindings,
            );
            concrete_type_params.push(projection.clone());
            concrete_bindings.insert(projection, resolved);
        }
        let concrete_ret = substitute_type(&method.ret, &concrete_type_params, &concrete_bindings);

        if !function.type_params.is_empty() {
            let Some(template) = self.templates.get(&function.name).cloned() else {
                return Some(concrete_ret);
            };
            *name = function.name.clone();
            return Some(self.rewrite_generic_call(
                name,
                args,
                locals,
                Some(&concrete_ret),
                &template,
            ));
        }
        *name = function.name.clone();
        Some(function.ret.clone())
    }

    fn rewrite_generic_call(
        &mut self,
        name: &mut String,
        args: &mut [Expr],
        locals: &BTreeMap<String, Type>,
        expected: Option<&Type>,
        template: &Template,
    ) -> Type {
        if template.public {
            self.blocked.insert(template.function.name.clone());
            for arg in args {
                self.rewrite_expr(arg, locals, None);
            }
            return Type::Unknown;
        }

        let type_params = &template.function.type_params;
        let mut bindings = BTreeMap::new();
        if let Some(expected) = expected {
            unify_type_params(&template.function.ret, expected, type_params, &mut bindings);
        }
        for (index, arg) in args.iter_mut().enumerate() {
            let param = template.function.params.get(index).map(|param| &param.ty);
            let provisional = param
                .map(|ty| substitute_type(ty, type_params, &bindings))
                .filter(is_concrete_type);
            let found = self.rewrite_expr(arg, locals, provisional.as_ref());
            if let Some(param) = param {
                unify_type_params(param, &found, type_params, &mut bindings);
            }
        }

        for type_param in type_params {
            if let Some(bound) = bindings.get_mut(type_param) {
                *bound = default_literal_type(bound);
            }
        }
        if type_params
            .iter()
            .any(|param| bindings.get(param).is_none_or(|ty| !is_concrete_type(ty)))
        {
            self.blocked.insert(template.function.name.clone());
            return self.normalize_associated_type_projections(&substitute_type(
                &template.function.ret,
                type_params,
                &bindings,
            ));
        }

        let signature = type_params
            .iter()
            .map(|param| bindings[param].to_agent())
            .collect::<Vec<_>>()
            .join(",");
        let key = format!("{}<{signature}>", template.function.name);
        let specialized_name = format!(
            "{}__fgx_mono_{}",
            template.function.name,
            type_params
                .iter()
                .map(|param| mangle_type(&bindings[param]))
                .collect::<Vec<_>>()
                .join("__")
        );
        if self.occupied_names.contains(&specialized_name) && !self.requests.contains_key(&key) {
            self.blocked.insert(template.function.name.clone());
            return Type::Unknown;
        }
        self.occupied_names.insert(specialized_name.clone());
        self.requests.entry(key.clone()).or_insert_with(|| Request {
            template: template.function.name.clone(),
            bindings: bindings.clone(),
            specialized_name: specialized_name.clone(),
        });
        if !self.processed.contains(&key) {
            self.pending.insert(key);
        }
        self.rewritten.insert(template.function.name.clone());
        *name = specialized_name;
        self.normalize_associated_type_projections(&substitute_type(
            &template.function.ret,
            type_params,
            &bindings,
        ))
    }

    fn infer_expr_type(&self, expr: &Expr, locals: &BTreeMap<String, Type>) -> Type {
        match expr {
            Expr::Var(name) => locals.get(name).cloned().unwrap_or(Type::Unknown),
            Expr::Field { base, field } => {
                let Type::Named(name) = self.infer_expr_type(base, locals) else {
                    return Type::Unknown;
                };
                self.structs
                    .get(&name)
                    .and_then(|decl| decl.fields.iter().find(|item| item.name == *field))
                    .map(|item| item.ty.clone())
                    .unwrap_or(Type::Unknown)
            }
            Expr::Index { base, .. } => self
                .infer_expr_type(base, locals)
                .element_type()
                .unwrap_or(Type::Unknown),
            _ => Type::Unknown,
        }
    }

    fn function_value_type(&self, name: &str) -> Option<Type> {
        let function = self.functions.get(name)?;
        Some(Type::generic_function_type(
            &function.type_params,
            &function.type_bounds,
            function
                .params
                .iter()
                .map(|param| param.ty.clone())
                .collect(),
            function.ret.clone(),
            &function.effects,
        ))
    }
}

fn specialize_function(template: &FunctionDecl, request: &Request) -> FunctionDecl {
    let mut function = template.clone();
    function.id = format!(
        "{}#mono<{}>",
        template.id,
        template
            .type_params
            .iter()
            .map(|param| request.bindings[param].to_agent())
            .collect::<Vec<_>>()
            .join(",")
    );
    function.name = request.specialized_name.clone();
    for param in &mut function.params {
        param.ty = substitute_type(&param.ty, &template.type_params, &request.bindings);
    }
    function.ret = substitute_type(&function.ret, &template.type_params, &request.bindings);
    substitute_body_types(&mut function.body, &template.type_params, &request.bindings);
    function.type_params.clear();
    function.type_bounds.clear();
    function
}

fn substitute_body_types(
    body: &mut [Stmt],
    type_params: &[String],
    bindings: &BTreeMap<String, Type>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let { ty, expr, .. } => {
                if let Some(ty) = ty {
                    *ty = substitute_type(ty, type_params, bindings);
                }
                substitute_expr_types(expr, type_params, bindings);
            }
            Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
                substitute_expr_types(expr, type_params, bindings);
            }
            Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                substitute_expr_types(cond, type_params, bindings);
                substitute_body_types(then_body, type_params, bindings);
                substitute_body_types(else_body, type_params, bindings);
            }
            Stmt::For {
                start, end, body, ..
            } => {
                substitute_expr_types(start, type_params, bindings);
                substitute_expr_types(end, type_params, bindings);
                substitute_body_types(body, type_params, bindings);
            }
            Stmt::While { cond, body } => {
                substitute_expr_types(cond, type_params, bindings);
                substitute_body_types(body, type_params, bindings);
            }
            Stmt::Match { scrutinee, arms } => {
                substitute_expr_types(scrutinee, type_params, bindings);
                for arm in arms {
                    substitute_body_types(&mut arm.body, type_params, bindings);
                }
            }
            Stmt::Assign { target, expr } => {
                substitute_expr_types(target, type_params, bindings);
                substitute_expr_types(expr, type_params, bindings);
            }
        }
    }
}

fn substitute_expr_types(
    expr: &mut Expr,
    type_params: &[String],
    bindings: &BTreeMap<String, Type>,
) {
    match expr {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => {}
        Expr::TraitConst { for_type, .. } => {
            *for_type = substitute_type(for_type, type_params, bindings);
        }
        Expr::Unary { expr, .. } => substitute_expr_types(expr, type_params, bindings),
        Expr::Binary { left, right, .. } => {
            substitute_expr_types(left, type_params, bindings);
            substitute_expr_types(right, type_params, bindings);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                substitute_expr_types(arg, type_params, bindings);
            }
        }
        Expr::Cast { ty, expr } => {
            *ty = substitute_type(ty, type_params, bindings);
            substitute_expr_types(expr, type_params, bindings);
        }
        Expr::Field { base, .. } => substitute_expr_types(base, type_params, bindings),
        Expr::Index { base, index } => {
            substitute_expr_types(base, type_params, bindings);
            substitute_expr_types(index, type_params, bindings);
        }
        Expr::SliceRange { base, start, end } => {
            substitute_expr_types(base, type_params, bindings);
            substitute_expr_types(start, type_params, bindings);
            substitute_expr_types(end, type_params, bindings);
        }
        Expr::Lambda { params, body } => {
            for param in params {
                param.ty = substitute_type(&param.ty, type_params, bindings);
            }
            substitute_expr_types(body, type_params, bindings);
        }
    }
}

fn unify_type_params(
    param: &Type,
    found: &Type,
    type_params: &[String],
    bindings: &mut BTreeMap<String, Type>,
) {
    match (param, found) {
        (Type::Named(name), found)
            if type_params.contains(name) && !matches!(found, Type::Unknown) =>
        {
            match bindings.get(name) {
                Some(existing) if is_literal_type(existing) && !is_literal_type(found) => {
                    bindings.insert(name.clone(), found.clone());
                }
                None => {
                    bindings.insert(name.clone(), found.clone());
                }
                _ => {}
            }
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
            for (left, right) in left_args.iter().zip(right_args) {
                unify_type_params(left, right, type_params, bindings);
            }
        }
        (Type::Slice { elem: left, .. }, Type::Slice { elem: right, .. })
        | (Type::Array { elem: left, .. }, Type::Array { elem: right, .. })
        | (Type::Pointer { elem: left, .. }, Type::Pointer { elem: right, .. }) => {
            unify_type_params(left, right, type_params, bindings);
        }
        _ => {}
    }
}

fn substitute_type(ty: &Type, type_params: &[String], bindings: &BTreeMap<String, Type>) -> Type {
    match ty {
        Type::Named(name) if type_params.contains(name) => {
            bindings.get(name).cloned().unwrap_or(Type::Unknown)
        }
        Type::Generic { name, args } => Type::Generic {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute_type(arg, type_params, bindings))
                .collect(),
        },
        Type::Array { elem, len } => Type::Array {
            elem: Box::new(substitute_type(elem, type_params, bindings)),
            len: *len,
        },
        Type::Slice { mutable, elem } => Type::Slice {
            mutable: *mutable,
            elem: Box::new(substitute_type(elem, type_params, bindings)),
        },
        Type::Pointer { mutable, elem } => Type::Pointer {
            mutable: *mutable,
            elem: Box::new(substitute_type(elem, type_params, bindings)),
        },
        other => other.clone(),
    }
}

fn is_concrete_type(ty: &Type) -> bool {
    match ty {
        Type::Unknown => false,
        Type::Named(_) | Type::Bool | Type::Primitive(_) | Type::Void => true,
        Type::Generic { args, .. } => args.iter().all(is_concrete_type),
        Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
            is_concrete_type(elem)
        }
    }
}

fn impl_pattern_matches(
    pattern: &Type,
    found: &Type,
    type_params: &[String],
    bindings: &mut BTreeMap<String, Type>,
) -> bool {
    if let Type::Named(name) = pattern {
        if type_params.contains(name) {
            return match bindings.get(name) {
                Some(existing) => impl_type_matches(existing, found),
                None if !matches!(found, Type::Unknown) => {
                    bindings.insert(name.clone(), found.clone());
                    true
                }
                None => false,
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
        _ => impl_type_matches(pattern, found),
    }
}

fn impl_type_matches(expected: &Type, found: &Type) -> bool {
    if matches!(found, Type::Unknown) {
        return false;
    }
    if expected == found {
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
                .all(|(expected, found)| impl_type_matches(expected, found));
    }
    false
}

fn is_literal_type(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(name) if name == "$int" || name == "$float")
}

fn default_literal_type(ty: &Type) -> Type {
    match ty {
        Type::Primitive(name) if name == "$int" => Type::Primitive("i64".to_string()),
        Type::Primitive(name) if name == "$float" => Type::Primitive("f64".to_string()),
        other => other.clone(),
    }
}

fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::Primitive(name) => sanitize(name),
        Type::Named(name) => format!("named_{}", sanitize(name)),
        Type::Generic { name, args } => format!(
            "generic_{}_{}",
            sanitize(name),
            args.iter().map(mangle_type).collect::<Vec<_>>().join("_")
        ),
        Type::Array { elem, len } => format!("array_{len}_{}", mangle_type(elem)),
        Type::Slice { mutable, elem } => format!(
            "slice_{}_{}",
            if *mutable { "mut" } else { "const" },
            mangle_type(elem)
        ),
        Type::Pointer { mutable, elem } => format!(
            "ptr_{}_{}",
            if *mutable { "mut" } else { "const" },
            mangle_type(elem)
        ),
        Type::Void => "void".to_string(),
        Type::Unknown => "unknown".to_string(),
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
