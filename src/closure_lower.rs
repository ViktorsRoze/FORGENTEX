//! Conservative closure lifting for the C and Zig source backends.
//!
//! The native closure subset has no environment: a contextually typed lambda
//! with a monomorphic scalar `Fn` type and no captured locals becomes a
//! deterministic private top-level function. Context currently comes from an
//! annotated local or a statically known direct-call parameter. Unsupported
//! lambdas are left in place so FIR preflight retains `op:lambda`.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Expr, FunctionDecl, Item, MatchArm, Pattern, Stmt, Type};
use crate::backend::is_native_function_type;
use crate::effects::effect_requires_unsafe;
use crate::project::Project;

/// Lift every eligible contextually typed non-capturing lambda in `project`.
/// Returns the number of generated private functions.
pub(crate) fn lower_non_capturing_closures(project: &mut Project) -> usize {
    let mut occupied = project
        .modules
        .iter()
        .flat_map(|module| module.program.functions())
        .map(|function| function.name.clone())
        .collect::<BTreeSet<_>>();
    let function_params = project
        .modules
        .iter()
        .flat_map(|module| module.program.functions())
        .map(|function| {
            (
                function.name.clone(),
                function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut lifted = 0;

    for (module_index, module) in project.modules.iter_mut().enumerate() {
        let original_len = module.program.items.len();
        let mut generated = Vec::new();
        for item in module.program.items.iter_mut().take(original_len) {
            let Item::Function(function) = item else {
                continue;
            };
            let parent = function.name.clone();
            let return_type = function.ret.clone();
            let mut bindings = function
                .params
                .iter()
                .map(|param| (param.name.clone(), param.ty.clone()))
                .collect::<BTreeMap<_, _>>();
            let mut ordinal = 0;
            lower_body(
                &mut function.body,
                &mut bindings,
                &return_type,
                module_index,
                &parent,
                &mut ordinal,
                &mut occupied,
                &function_params,
                &mut generated,
            );
        }
        lifted += generated.len();
        module
            .program
            .items
            .extend(generated.into_iter().map(Item::Function));
    }

    lifted
}

#[allow(clippy::too_many_arguments)]
fn lower_body(
    body: &mut [Stmt],
    bindings: &mut BTreeMap<String, Type>,
    expected_return: &Type,
    module_index: usize,
    parent: &str,
    ordinal: &mut usize,
    occupied: &mut BTreeSet<String>,
    function_params: &BTreeMap<String, Vec<Type>>,
    generated: &mut Vec<FunctionDecl>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                lower_expr(
                    expr,
                    ty.as_ref(),
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                bindings.insert(name.clone(), ty.clone().unwrap_or(Type::Unknown));
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                lower_expr(
                    cond,
                    None,
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                let mut then_bindings = bindings.clone();
                lower_body(
                    then_body,
                    &mut then_bindings,
                    expected_return,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                let mut else_bindings = bindings.clone();
                lower_body(
                    else_body,
                    &mut else_bindings,
                    expected_return,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                lower_expr(
                    start,
                    None,
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                lower_expr(
                    end,
                    None,
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                let mut nested_bindings = bindings.clone();
                nested_bindings.insert(var.clone(), Type::Unknown);
                lower_body(
                    body,
                    &mut nested_bindings,
                    expected_return,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
            }
            Stmt::While { cond, body } => {
                lower_expr(
                    cond,
                    None,
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                let mut nested_bindings = bindings.clone();
                lower_body(
                    body,
                    &mut nested_bindings,
                    expected_return,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
            }
            Stmt::Match { scrutinee, arms } => {
                lower_expr(
                    scrutinee,
                    None,
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                for MatchArm { pattern, body } in arms {
                    let mut arm_bindings = bindings.clone();
                    if let Pattern::Variant {
                        binding: Some(binding),
                        ..
                    } = pattern
                    {
                        arm_bindings.insert(binding.clone(), Type::Unknown);
                    }
                    lower_body(
                        body,
                        &mut arm_bindings,
                        expected_return,
                        module_index,
                        parent,
                        ordinal,
                        occupied,
                        function_params,
                        generated,
                    );
                }
            }
            Stmt::Return(Some(expr)) => lower_expr(
                expr,
                Some(expected_return),
                bindings,
                module_index,
                parent,
                ordinal,
                occupied,
                function_params,
                generated,
            ),
            Stmt::Require(expr) | Stmt::Expr(expr) => lower_expr(
                expr,
                None,
                bindings,
                module_index,
                parent,
                ordinal,
                occupied,
                function_params,
                generated,
            ),
            Stmt::Assign { target, expr } => {
                let expected = match target {
                    Expr::Var(name) => bindings.get(name).cloned(),
                    _ => None,
                };
                lower_expr(
                    target,
                    None,
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
                lower_expr(
                    expr,
                    expected.as_ref(),
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
            }
            Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_expr(
    expr: &mut Expr,
    expected: Option<&Type>,
    bindings: &BTreeMap<String, Type>,
    module_index: usize,
    parent: &str,
    ordinal: &mut usize,
    occupied: &mut BTreeSet<String>,
    function_params: &BTreeMap<String, Vec<Type>>,
    generated: &mut Vec<FunctionDecl>,
) {
    if let Some(expected) = expected {
        if let Some(function) = lift_lambda(
            expr,
            expected,
            bindings,
            module_index,
            parent,
            ordinal,
            occupied,
        ) {
            generated.push(function);
            return;
        }
    }

    match expr {
        Expr::Call { name, args } => {
            let params = (!bindings.contains_key(name))
                .then(|| function_params.get(name).cloned())
                .flatten();
            for (index, arg) in args.iter_mut().enumerate() {
                lower_expr(
                    arg,
                    params.as_ref().and_then(|params| params.get(index)),
                    bindings,
                    module_index,
                    parent,
                    ordinal,
                    occupied,
                    function_params,
                    generated,
                );
            }
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            lower_expr(
                expr,
                None,
                bindings,
                module_index,
                parent,
                ordinal,
                occupied,
                function_params,
                generated,
            );
        }
        Expr::Binary { left, right, .. }
        | Expr::Index {
            base: left,
            index: right,
        } => {
            lower_expr(
                left,
                None,
                bindings,
                module_index,
                parent,
                ordinal,
                occupied,
                function_params,
                generated,
            );
            lower_expr(
                right,
                None,
                bindings,
                module_index,
                parent,
                ordinal,
                occupied,
                function_params,
                generated,
            );
        }
        Expr::Lambda { .. }
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
    }
}

fn lift_lambda(
    expr: &mut Expr,
    expected: &Type,
    outer_bindings: &BTreeMap<String, Type>,
    module_index: usize,
    parent: &str,
    ordinal: &mut usize,
    occupied: &mut BTreeSet<String>,
) -> Option<FunctionDecl> {
    if !is_native_function_type(expected) {
        return None;
    }
    let (expected_params, ret, effect_types) = expected.function_parts()?;
    let effects = effect_types
        .iter()
        .map(|effect| match effect {
            Type::Named(name) if !effect_requires_unsafe(name) => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    let (params, lambda_body) = match expr {
        Expr::Lambda { params, body } => (params.clone(), (**body).clone()),
        _ => return None,
    };
    if params.len() != expected_params.len()
        || params
            .iter()
            .zip(expected_params)
            .any(|(param, expected)| param.ty != *expected)
    {
        return None;
    }
    let lambda_params = params
        .iter()
        .map(|param| param.name.clone())
        .collect::<BTreeSet<_>>();
    if expression_captures(&lambda_body, &lambda_params, outer_bindings) {
        return None;
    }

    let name = fresh_name(module_index, parent, ordinal, occupied);
    *expr = Expr::Var(name.clone());
    let function_body = if *ret == Type::Void {
        vec![Stmt::Expr(lambda_body), Stmt::Return(None)]
    } else {
        vec![Stmt::Return(Some(lambda_body))]
    };
    Some(FunctionDecl {
        id: format!("@function.{name}"),
        name,
        type_params: Vec::new(),
        type_bounds: Vec::new(),
        unsafe_fn: false,
        params,
        ret: ret.clone(),
        effects,
        attrs: Vec::new(),
        body: function_body,
    })
}

fn fresh_name(
    module_index: usize,
    parent: &str,
    ordinal: &mut usize,
    occupied: &mut BTreeSet<String>,
) -> String {
    let parent = parent
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    loop {
        let candidate = format!("__fgx_lambda_m{module_index}_{parent}_{}", *ordinal);
        *ordinal += 1;
        if occupied.insert(candidate.clone()) {
            return candidate;
        }
    }
}

fn expression_captures(
    expr: &Expr,
    lambda_params: &BTreeSet<String>,
    outer_bindings: &BTreeMap<String, Type>,
) -> bool {
    match expr {
        Expr::Var(name) => !lambda_params.contains(name),
        Expr::Lambda { .. } => true,
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            expression_captures(expr, lambda_params, outer_bindings)
        }
        Expr::Binary { left, right, .. } => {
            expression_captures(left, lambda_params, outer_bindings)
                || expression_captures(right, lambda_params, outer_bindings)
        }
        Expr::Call { name, args } => {
            (outer_bindings.contains_key(name) && !lambda_params.contains(name))
                || args
                    .iter()
                    .any(|arg| expression_captures(arg, lambda_params, outer_bindings))
        }
        Expr::Index { base, index } => {
            expression_captures(base, lambda_params, outer_bindings)
                || expression_captures(index, lambda_params, outer_bindings)
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_human;
    use crate::project::{ProjectManifest, ProjectModule, ProjectSecurity};
    use std::path::PathBuf;

    fn project(source: &str) -> Project {
        Project {
            manifest_path: PathBuf::from("Forgentex.toml"),
            package_root: PathBuf::new(),
            manifest: ProjectManifest {
                name: "closures".to_string(),
                version: "0.1.0".to_string(),
                source_root: PathBuf::from("src"),
                dependencies: Vec::new(),
                targets: Vec::new(),
                profiles: Vec::new(),
                security: ProjectSecurity::default(),
            },
            modules: vec![ProjectModule {
                package: "closures".to_string(),
                name: "m".to_string(),
                path: PathBuf::from("src/m.fgx"),
                imports: Vec::new(),
                public_symbols: Vec::new(),
                program: parse_human(source).unwrap(),
                source_hash: String::new(),
                token_cost: 0,
                dependency: false,
                standard: false,
            }],
        }
    }

    #[test]
    fn lifts_contextually_typed_non_capturing_scalar_lambdas() {
        let mut project = project(
            "fn apply(f: Fn[i32, i32], x: i32) -> i32:\n    return f(x)\n\nfn returned() -> Fn[i32, i32]:\n    return |x: i32| x + 3\n\nfn captured_return(n: i32) -> Fn[i32, i32]:\n    return |x: i32| x + n\n\nfn f() -> i32:\n    let n: i32 = 2\n    let lifted: Fn[i32, i32] = |x: i32| x + 1\n    let captured: Fn[i32, i32] = |x: i32| x + n\n    let reassigned: Fn[i32, i32] = lifted\n    reassigned = |x: i32| x + 4\n    return lifted(captured(1)) + reassigned(1) + apply(|x: i32| x + 2, 1) + apply(|x: i32| x + n, 1)\n",
        );
        assert_eq!(lower_non_capturing_closures(&mut project), 4);
        let program = &project.modules[0].program;
        assert!(program
            .find_function("__fgx_lambda_m0_returned_0")
            .is_some());
        assert!(program.find_function("__fgx_lambda_m0_f_0").is_some());
        assert!(program.find_function("__fgx_lambda_m0_f_1").is_some());
        assert!(program.find_function("__fgx_lambda_m0_f_2").is_some());
        let source = crate::format::format_human(program);
        assert!(source.contains("return __fgx_lambda_m0_returned_0"));
        assert!(source.contains("return |x: i32| x + n"));
        assert!(source.contains("let lifted: Fn[i32, i32] = __fgx_lambda_m0_f_0"));
        assert!(source.contains("let captured: Fn[i32, i32] = |x: i32| x + n"));
        assert!(source.contains("reassigned = __fgx_lambda_m0_f_1"));
        assert!(source.contains("apply(__fgx_lambda_m0_f_2, 1)"));
        assert!(source.contains("apply(|x: i32| x + n, 1)"));
    }
}
