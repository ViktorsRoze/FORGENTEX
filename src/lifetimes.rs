use std::collections::{BTreeSet, HashMap};

use crate::ast::{Expr, FunctionDecl, Item, Program, Stmt, Type};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifetimeContract {
    pub function: String,
    pub returns: Vec<LifetimeParam>,
    pub mutates: Vec<LifetimeParam>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifetimeParam {
    pub index: usize,
    pub name: String,
    pub mutable: bool,
    pub ty: String,
}

pub fn analyze_contracts(program: &Program) -> Vec<LifetimeContract> {
    let functions = function_index(program);
    program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(function) => Some(lifetime_contract(function, &functions)),
            Item::Struct(_)
            | Item::Enum(_)
            | Item::Trait(_)
            | Item::TraitImpl(_)
            | Item::Test(_)
            | Item::Capsule(_)
            | Item::Share(_)
            | Item::Pipeline(_) => None,
        })
        .collect()
}

pub fn lifetime_contract(
    function: &FunctionDecl,
    functions: &HashMap<String, &FunctionDecl>,
) -> LifetimeContract {
    let returned = returned_borrow_param_indices(function, functions).unwrap_or_default();
    let mutated = mutated_borrow_param_indices(function, functions);
    LifetimeContract {
        function: function.id.clone(),
        returns: lifetime_params(function, &returned),
        mutates: lifetime_params(function, &mutated),
    }
}

pub fn returned_borrow_param_indices(
    function: &FunctionDecl,
    functions: &HashMap<String, &FunctionDecl>,
) -> Option<BTreeSet<usize>> {
    returned_borrow_param_indices_inner(function, functions, &mut Vec::new())
}

pub fn mutated_borrow_param_indices(
    function: &FunctionDecl,
    functions: &HashMap<String, &FunctionDecl>,
) -> BTreeSet<usize> {
    mutated_borrow_param_indices_inner(function, functions, &mut Vec::new())
}

fn returned_borrow_param_indices_inner(
    function: &FunctionDecl,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
) -> Option<BTreeSet<usize>> {
    if !type_can_carry_borrow(&function.ret, &function.type_params)
        || visiting.iter().any(|name| name == &function.name)
    {
        return None;
    }

    visiting.push(function.name.clone());
    let mut aliases = initial_borrow_aliases(function);
    let result =
        returned_borrow_param_indices_from_body(&function.body, &mut aliases, functions, visiting)
            .ok()
            .filter(|indices| !indices.is_empty());
    visiting.pop();
    result
}

fn returned_borrow_param_indices_from_body(
    body: &[Stmt],
    aliases: &mut HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
) -> Result<BTreeSet<usize>, ()> {
    let mut returned = BTreeSet::new();
    for stmt in body {
        match stmt {
            Stmt::Let { name, expr, .. } => update_alias(name, expr, aliases, functions, visiting),
            Stmt::Assign {
                target: Expr::Var(name),
                expr,
            } => update_alias(name, expr, aliases, functions, visiting),
            Stmt::Return(Some(expr)) => {
                let indices =
                    borrow_param_indices_for_expr(expr, aliases, functions, visiting).ok_or(())?;
                returned.extend(indices);
                return Ok(returned);
            }
            Stmt::Return(None) => return Err(()),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for branch in [then_body, else_body] {
                    let mut branch_aliases = aliases.clone();
                    let branch_returns = returned_borrow_param_indices_from_body(
                        branch,
                        &mut branch_aliases,
                        functions,
                        visiting,
                    )?;
                    returned.extend(branch_returns);
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                let mut loop_aliases = aliases.clone();
                let loop_returns = returned_borrow_param_indices_from_body(
                    body,
                    &mut loop_aliases,
                    functions,
                    visiting,
                )?;
                returned.extend(loop_returns);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    let mut arm_aliases = aliases.clone();
                    let arm_returns = returned_borrow_param_indices_from_body(
                        &arm.body,
                        &mut arm_aliases,
                        functions,
                        visiting,
                    )?;
                    returned.extend(arm_returns);
                }
            }
            Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Asm(_)
            | Stmt::Break
            | Stmt::Continue => {}
        }
    }
    Ok(returned)
}

fn mutated_borrow_param_indices_inner(
    function: &FunctionDecl,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
) -> BTreeSet<usize> {
    if visiting.iter().any(|name| name == &function.name) {
        return mutable_borrow_param_indices(function);
    }

    visiting.push(function.name.clone());
    let mut aliases = initial_borrow_aliases(function);
    let mut mutated = BTreeSet::new();
    collect_mutations_from_body(
        &function.body,
        &mut aliases,
        functions,
        visiting,
        &mut mutated,
    );
    visiting.pop();
    mutated
}

fn collect_mutations_from_body(
    body: &[Stmt],
    aliases: &mut HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
    out: &mut BTreeSet<usize>,
) {
    for stmt in body {
        collect_mutations_from_stmt(stmt, aliases, functions, visiting, out);
    }
}

fn collect_mutations_from_stmt(
    stmt: &Stmt,
    aliases: &mut HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
    out: &mut BTreeSet<usize>,
) {
    match stmt {
        Stmt::Let { name, expr, .. } => {
            collect_mutations_from_expr(expr, aliases, functions, visiting, out);
            update_alias(name, expr, aliases, functions, visiting);
        }
        Stmt::Assign { target, expr } => {
            collect_mutations_from_assignment_target(target, aliases, functions, visiting, out);
            collect_mutations_from_expr(expr, aliases, functions, visiting, out);
            if let Expr::Var(name) = target {
                update_alias(name, expr, aliases, functions, visiting);
            }
        }
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            collect_mutations_from_expr(expr, aliases, functions, visiting, out);
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_mutations_from_expr(cond, aliases, functions, visiting, out);
            for branch in [then_body, else_body] {
                let mut branch_aliases = aliases.clone();
                collect_mutations_from_body(branch, &mut branch_aliases, functions, visiting, out);
            }
        }
        Stmt::For {
            start, end, body, ..
        } => {
            collect_mutations_from_expr(start, aliases, functions, visiting, out);
            collect_mutations_from_expr(end, aliases, functions, visiting, out);
            let mut loop_aliases = aliases.clone();
            collect_mutations_from_body(body, &mut loop_aliases, functions, visiting, out);
        }
        Stmt::While { cond, body } => {
            collect_mutations_from_expr(cond, aliases, functions, visiting, out);
            let mut loop_aliases = aliases.clone();
            collect_mutations_from_body(body, &mut loop_aliases, functions, visiting, out);
        }
        Stmt::Match { scrutinee, arms } => {
            collect_mutations_from_expr(scrutinee, aliases, functions, visiting, out);
            for arm in arms {
                let mut arm_aliases = aliases.clone();
                collect_mutations_from_body(&arm.body, &mut arm_aliases, functions, visiting, out);
            }
        }
        Stmt::Return(None) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
    }
}

fn collect_mutations_from_assignment_target(
    target: &Expr,
    aliases: &HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
    out: &mut BTreeSet<usize>,
) {
    match target {
        Expr::Index { base, index } => {
            if let Some(indices) = borrow_param_indices_for_expr(base, aliases, functions, visiting)
            {
                out.extend(indices);
            }
            collect_mutations_from_expr(base, aliases, functions, visiting, out);
            collect_mutations_from_expr(index, aliases, functions, visiting, out);
        }
        Expr::SliceRange { base, start, end } => {
            collect_mutations_from_expr(base, aliases, functions, visiting, out);
            collect_mutations_from_expr(start, aliases, functions, visiting, out);
            collect_mutations_from_expr(end, aliases, functions, visiting, out);
        }
        Expr::Field { base, .. } => {
            collect_mutations_from_assignment_target(base, aliases, functions, visiting, out)
        }
        Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => {
            collect_mutations_from_assignment_target(expr, aliases, functions, visiting, out)
        }
        _ => collect_mutations_from_expr(target, aliases, functions, visiting, out),
    }
}

fn collect_mutations_from_expr(
    expr: &Expr,
    aliases: &HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
    out: &mut BTreeSet<usize>,
) {
    match expr {
        Expr::Lambda { body, .. } => {
            collect_mutations_from_expr(body, aliases, functions, visiting, out)
        }
        Expr::Call { name, args } => {
            if let Some(callee) = functions.get(name) {
                let mutated = mutated_borrow_param_indices_inner(callee, functions, visiting);
                for callee_param_idx in mutated {
                    if let Some(arg) = args.get(callee_param_idx) {
                        if let Some(arg_indices) =
                            borrow_param_indices_for_expr(arg, aliases, functions, visiting)
                        {
                            out.extend(arg_indices);
                        }
                    }
                }
            }
            for arg in args {
                collect_mutations_from_expr(arg, aliases, functions, visiting, out);
            }
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
            collect_mutations_from_expr(expr, aliases, functions, visiting, out)
        }
        Expr::Binary { left, right, .. } => {
            collect_mutations_from_expr(left, aliases, functions, visiting, out);
            collect_mutations_from_expr(right, aliases, functions, visiting, out);
        }
        Expr::Field { base, .. } => {
            collect_mutations_from_expr(base, aliases, functions, visiting, out)
        }
        Expr::Index { base, index } => {
            collect_mutations_from_expr(base, aliases, functions, visiting, out);
            collect_mutations_from_expr(index, aliases, functions, visiting, out);
        }
        Expr::SliceRange { base, start, end } => {
            collect_mutations_from_expr(base, aliases, functions, visiting, out);
            collect_mutations_from_expr(start, aliases, functions, visiting, out);
            collect_mutations_from_expr(end, aliases, functions, visiting, out);
        }
        Expr::Var(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
    }
}

fn update_alias(
    name: &str,
    expr: &Expr,
    aliases: &mut HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
) {
    if let Some(indices) = borrow_param_indices_for_expr(expr, aliases, functions, visiting) {
        aliases.insert(name.to_string(), indices);
    } else {
        aliases.remove(name);
    }
}

fn borrow_param_indices_for_expr(
    expr: &Expr,
    aliases: &HashMap<String, BTreeSet<usize>>,
    functions: &HashMap<String, &FunctionDecl>,
    visiting: &mut Vec<String>,
) -> Option<BTreeSet<usize>> {
    match expr {
        Expr::Lambda { .. } => None,
        Expr::Var(name) => aliases.get(name).cloned(),
        Expr::Call { name, args } => {
            let callee = functions.get(name)?;
            let callee_param_indices =
                returned_borrow_param_indices_inner(callee, functions, visiting)?;
            let mut returned = BTreeSet::new();
            for callee_param_idx in callee_param_indices {
                let arg = args.get(callee_param_idx)?;
                let arg_indices = borrow_param_indices_for_expr(arg, aliases, functions, visiting)?;
                returned.extend(arg_indices);
            }
            if returned.is_empty() {
                None
            } else {
                Some(returned)
            }
        }
        Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => {
            borrow_param_indices_for_expr(expr, aliases, functions, visiting)
        }
        Expr::SliceRange { base, .. } => {
            borrow_param_indices_for_expr(base, aliases, functions, visiting)
        }
        Expr::Index { base, .. } => {
            borrow_param_indices_for_expr(base, aliases, functions, visiting)
        }
        Expr::Int(_)
        | Expr::TraitConst { .. }
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Binary { .. }
        | Expr::Field { .. } => None,
    }
}

fn initial_borrow_aliases(function: &FunctionDecl) -> HashMap<String, BTreeSet<usize>> {
    function
        .params
        .iter()
        .enumerate()
        .filter(|(_, param)| type_can_carry_borrow(&param.ty, &function.type_params))
        .map(|(idx, param)| (param.name.clone(), singleton_index(idx)))
        .collect()
}

fn mutable_borrow_param_indices(function: &FunctionDecl) -> BTreeSet<usize> {
    function
        .params
        .iter()
        .enumerate()
        .filter(|(_, param)| is_mutable_borrow_type(&param.ty))
        .map(|(idx, _)| idx)
        .collect()
}

fn lifetime_params(function: &FunctionDecl, indices: &BTreeSet<usize>) -> Vec<LifetimeParam> {
    indices
        .iter()
        .filter_map(|idx| {
            function.params.get(*idx).map(|param| LifetimeParam {
                index: *idx,
                name: param.name.clone(),
                mutable: is_mutable_borrow_type(&param.ty),
                ty: param.ty.to_human(),
            })
        })
        .collect()
}

fn function_index(program: &Program) -> HashMap<String, &FunctionDecl> {
    program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(function) => Some((function.name.clone(), function)),
            Item::Struct(_)
            | Item::Enum(_)
            | Item::Trait(_)
            | Item::TraitImpl(_)
            | Item::Test(_)
            | Item::Capsule(_)
            | Item::Share(_)
            | Item::Pipeline(_) => None,
        })
        .collect()
}

fn singleton_index(idx: usize) -> BTreeSet<usize> {
    let mut indices = BTreeSet::new();
    indices.insert(idx);
    indices
}

fn type_can_carry_borrow(ty: &Type, type_params: &[String]) -> bool {
    match ty {
        Type::Slice { .. } => true,
        Type::Array { elem, .. } => type_can_carry_borrow(elem, type_params),
        Type::Generic { args, .. } => args
            .iter()
            .any(|arg| type_can_carry_borrow(arg, type_params)),
        Type::Named(name) => type_params.contains(name),
        Type::Bool | Type::Primitive(_) | Type::Pointer { .. } | Type::Void | Type::Unknown => {
            false
        }
    }
}

fn is_mutable_borrow_type(ty: &Type) -> bool {
    matches!(ty, Type::Slice { mutable: true, .. })
}
