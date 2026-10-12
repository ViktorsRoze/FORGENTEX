//! Exact-arithmetic opt-in checking.
//!
//! Forgentex's numeric policy is **fast by default, exact on request**:
//! ordinary code uses IEEE-754 `f32`/`f64` (fast, approximate, the same
//! rounding as every other language), and a function opts into exact,
//! deterministic arithmetic by carrying the `@exact` attribute.
//!
//! `@exact` is a cheap, one-token, *checked* contract: an `@exact` function
//! may not use floating point anywhere in its signature or body, **nor reach
//! floating point through anything it calls**. The checker proves that so a
//! reviewer does not have to audit it. Exact functions use integer or
//! fixed-point math (for example the Q15 helpers in the audio DSP core), which
//! is bit-exact and identical on every backend and platform.
//!
//! # What the proof covers, and what it trusts
//!
//! The check is a reachability question over the call graph, so it holds for
//! whole call chains rather than one function body:
//!
//! * every function reachable from an `@exact` function is checked, and
//!   recursive groups are handled — the analysis is a fixpoint, not a walk;
//! * named types are resolved, so a struct with an `f32` field is floating
//!   point wherever it appears, however deeply nested;
//! * calls to intrinsics are judged by their declared contract. That contract
//!   is *trusted*, not proven, because the implementation is not Forgentex
//!   source. Today no intrinsic is float-typed, so this admits nothing.
//!
//! It deliberately does **not** claim anything about foreign capsules, whose
//! bodies are another language entirely, or about what integer arithmetic
//! *means*: exactness here is the absence of IEEE-754 rounding, not a promise
//! about overflow, division, or shift behaviour. Those are specified
//! separately in `docs/numeric_determinism.md`.

use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, FunctionDecl, Program, Stmt, Type};
use crate::diag::Diagnostic;

const EXACT_ATTR: &str = "exact";

/// The integer types the language checks arithmetic at.
///
/// Shared with the C backend and the interpreter so the three agree on which
/// widths exist; see `docs/numeric_determinism.md` for what is checked at each.
pub(crate) fn is_checked_integer_name(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize"
    )
}

/// Why a function is not float-free.
enum Cause {
    /// Its own signature or body uses floating point.
    Direct,
    /// It reaches floating point through `callee`, which bottoms out at `root`.
    Calls { callee: String, root: String },
}

pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let inexact = inexact_functions(program);
    let mut diagnostics = Vec::new();
    for function in program.functions() {
        if !function.attrs.iter().any(|attr| attr.name == EXACT_ATTR) {
            continue;
        }
        match inexact.get(&function.name) {
            None => {}
            Some(Cause::Direct) => diagnostics.push(
                Diagnostic::new(
                    "exact_arithmetic_uses_float",
                    &function.id,
                    format!(
                        "function '{}' is marked @exact but uses floating point; exact functions must use integer or fixed-point math",
                        function.name
                    ),
                )
                .suggest("remove f32/f64 and use integer or fixed-point (for example Q15) math for exact, deterministic results"),
            ),
            Some(Cause::Calls { callee, root }) => {
                // Naming the callee alone would send a reader to a function
                // that may itself only be a link in the chain, so the message
                // carries both ends of it.
                let chain = if callee == root {
                    format!("'{callee}' uses floating point")
                } else {
                    format!("'{callee}' reaches floating point through '{root}'")
                };
                diagnostics.push(
                    Diagnostic::new(
                        "exact_arithmetic_calls_inexact",
                        &function.id,
                        format!(
                            "function '{}' is marked @exact but calls '{callee}', which is not float-free: {chain}",
                            function.name
                        ),
                    )
                    .suggest("mark the called function @exact as well, or replace the call with integer or fixed-point (for example Q15) math"),
                )
            }
        }
    }
    diagnostics
}

/// Every function in the program that is not float-free, and why.
///
/// A worklist fixpoint over the reverse call graph rather than a recursive
/// walk: mutual and self recursion are ordinary graph edges here, whereas a
/// depth-first "does this reach a float" query has to special-case them and can
/// memoise an answer that was only correct for the path it was reached by.
fn inexact_functions(program: &Program) -> HashMap<String, Cause> {
    let mut callers: HashMap<String, Vec<String>> = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut inexact: HashMap<String, Cause> = HashMap::new();

    for function in program.functions() {
        let mut calls = Vec::new();
        collect_calls(&function.body, &mut calls);
        for callee in calls {
            callers
                .entry(callee)
                .or_default()
                .push(function.name.clone());
        }
        if uses_float_directly(function, program) {
            // A later definition of the same name cannot un-float an earlier
            // one, so the first verdict stands.
            if inexact
                .insert(function.name.clone(), Cause::Direct)
                .is_none()
            {
                pending.push(function.name.clone());
            }
        }
    }

    while let Some(callee) = pending.pop() {
        let root = match inexact.get(&callee) {
            Some(Cause::Direct) => callee.clone(),
            Some(Cause::Calls { root, .. }) => root.clone(),
            None => continue,
        };
        for caller in callers.get(&callee).cloned().unwrap_or_default() {
            if inexact.contains_key(&caller) {
                continue;
            }
            inexact.insert(
                caller.clone(),
                Cause::Calls {
                    callee: callee.clone(),
                    root: root.clone(),
                },
            );
            pending.push(caller);
        }
    }
    inexact
}

/// Floating point in this function's own signature or body, ignoring what it
/// calls — the call graph carries that.
fn uses_float_directly(function: &FunctionDecl, program: &Program) -> bool {
    function
        .params
        .iter()
        .any(|param| type_has_float(&param.ty, program))
        || type_has_float(&function.ret, program)
        || body_has_float(&function.body, program)
}

/// Whether a type is, or contains, floating point.
///
/// Named types are resolved through struct fields and enum payloads, so a
/// struct that merely *holds* an `f32` is floating point everywhere it is used.
/// Recursive types terminate: a type already being resolved contributes
/// nothing, because any float it holds is found at the field that declares it.
fn type_has_float(ty: &Type, program: &Program) -> bool {
    let mut visiting = HashSet::new();
    type_has_float_inner(ty, program, &mut visiting)
}

fn type_has_float_inner(ty: &Type, program: &Program, visiting: &mut HashSet<String>) -> bool {
    match ty {
        Type::Primitive(name) => name == "f32" || name == "f64",
        Type::Slice { elem, .. } | Type::Array { elem, .. } | Type::Pointer { elem, .. } => {
            type_has_float_inner(elem, program, visiting)
        }
        Type::Generic { args, .. } => args
            .iter()
            .any(|arg| type_has_float_inner(arg, program, visiting)),
        Type::Named(name) => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            let found = if let Some(decl) = program.structs().find(|decl| decl.name == *name) {
                decl.fields
                    .iter()
                    .any(|field| type_has_float_inner(&field.ty, program, visiting))
            } else if let Some(decl) = program.enums().find(|decl| decl.name == *name) {
                decl.variants.iter().any(|variant| {
                    variant
                        .payload
                        .as_ref()
                        .is_some_and(|payload| type_has_float_inner(payload, program, visiting))
                })
            } else {
                false
            };
            visiting.remove(name);
            found
        }
        _ => false,
    }
}

fn body_has_float(body: &[Stmt], program: &Program) -> bool {
    body.iter().any(|stmt| stmt_has_float(stmt, program))
}

fn stmt_has_float(stmt: &Stmt, program: &Program) -> bool {
    match stmt {
        Stmt::Let { ty, expr, .. } => {
            ty.as_ref()
                .map(|ty| type_has_float(ty, program))
                .unwrap_or(false)
                || expr_has_float(expr, program)
        }
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            expr_has_float(expr, program)
        }
        Stmt::Return(None) => false,
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_float(cond, program)
                || body_has_float(then_body, program)
                || body_has_float(else_body, program)
        }
        Stmt::For {
            start, end, body, ..
        } => {
            expr_has_float(start, program)
                || expr_has_float(end, program)
                || body_has_float(body, program)
        }
        Stmt::While { cond, body } => {
            expr_has_float(cond, program) || body_has_float(body, program)
        }
        Stmt::Match { scrutinee, arms } => {
            expr_has_float(scrutinee, program)
                || arms.iter().any(|arm| body_has_float(&arm.body, program))
        }
        Stmt::Assign { target, expr } => {
            expr_has_float(target, program) || expr_has_float(expr, program)
        }
        Stmt::Asm(_) | Stmt::Break | Stmt::Continue => false,
    }
}

fn expr_has_float(expr: &Expr, program: &Program) -> bool {
    match expr {
        // A lambda's parameters are part of its signature just as a function's
        // are, so `|x: f32| x` is floating point even though its body is not.
        Expr::Lambda { params, body } => {
            params
                .iter()
                .any(|param| type_has_float(&param.ty, program))
                || expr_has_float(body, program)
        }
        Expr::Float(_) => true,
        Expr::Cast { ty, expr } => type_has_float(ty, program) || expr_has_float(expr, program),
        Expr::Unary { expr, .. } => expr_has_float(expr, program),
        Expr::Binary { left, right, .. } => {
            expr_has_float(left, program) || expr_has_float(right, program)
        }
        Expr::Call { name, args } => {
            // A call to a function defined in this program is an edge in the
            // call graph and is judged there. A call to anything else is an
            // intrinsic, judged by the contract it declares.
            let intrinsic_float = !program.functions().any(|function| function.name == *name)
                && crate::stdlib::standard_builtin_call_type(name)
                    .is_some_and(|ty| type_has_float(&ty, program));
            intrinsic_float || args.iter().any(|arg| expr_has_float(arg, program))
        }
        Expr::Field { base, .. } => expr_has_float(base, program),
        Expr::Index { base, index } => {
            expr_has_float(base, program) || expr_has_float(index, program)
        }
        Expr::SliceRange { base, start, end } => {
            expr_has_float(base, program)
                || expr_has_float(start, program)
                || expr_has_float(end, program)
        }
        Expr::Int(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) | Expr::TraitConst { .. } => {
            false
        }
    }
}

/// Every function name this body calls, including inside nested blocks and
/// lambda bodies.
fn collect_calls(body: &[Stmt], out: &mut Vec<String>) {
    for stmt in body {
        collect_calls_stmt(stmt, out);
    }
}

fn collect_calls_stmt(stmt: &Stmt, out: &mut Vec<String>) {
    match stmt {
        Stmt::Let { expr, .. } => collect_calls_expr(expr, out),
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            collect_calls_expr(expr, out)
        }
        Stmt::Return(None) => {}
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_calls_expr(cond, out);
            collect_calls(then_body, out);
            collect_calls(else_body, out);
        }
        Stmt::For {
            start, end, body, ..
        } => {
            collect_calls_expr(start, out);
            collect_calls_expr(end, out);
            collect_calls(body, out);
        }
        Stmt::While { cond, body } => {
            collect_calls_expr(cond, out);
            collect_calls(body, out);
        }
        Stmt::Match { scrutinee, arms } => {
            collect_calls_expr(scrutinee, out);
            for arm in arms {
                collect_calls(&arm.body, out);
            }
        }
        Stmt::Assign { target, expr } => {
            collect_calls_expr(target, out);
            collect_calls_expr(expr, out);
        }
        Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
    }
}

fn collect_calls_expr(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Call { name, args } => {
            out.push(name.clone());
            for arg in args {
                collect_calls_expr(arg, out);
            }
        }
        // A named function used as a value can be called anywhere it is passed,
        // so it is an edge too. Over-reporting here is the safe direction: it
        // can only reject an `@exact` function that mentions inexact code.
        Expr::Var(name) => out.push(name.clone()),
        Expr::Lambda { body, .. } => collect_calls_expr(body, out),
        Expr::Cast { expr, .. } | Expr::Unary { expr, .. } | Expr::Field { base: expr, .. } => {
            collect_calls_expr(expr, out)
        }
        Expr::Binary { left, right, .. } => {
            collect_calls_expr(left, out);
            collect_calls_expr(right, out);
        }
        Expr::Index { base, index } => {
            collect_calls_expr(base, out);
            collect_calls_expr(index, out);
        }
        Expr::SliceRange { base, start, end } => {
            collect_calls_expr(base, out);
            collect_calls_expr(start, out);
            collect_calls_expr(end, out);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
    }
}
