//! Exact-arithmetic opt-in checking.
//!
//! Forgentex's numeric policy is **fast by default, exact on request**:
//! ordinary code uses IEEE-754 `f32`/`f64` (fast, approximate, the same
//! rounding as every other language), and a function opts into exact,
//! deterministic arithmetic by carrying the `@exact` attribute.
//!
//! `@exact` is a cheap, one-token, *checked* contract: an `@exact` function
//! may not use floating point anywhere in its signature or body. The
//! checker proves the function is exact so a reviewer does not have to
//! audit it. Exact functions use integer or fixed-point math (for example
//! the Q15 helpers in the audio DSP core), which is bit-exact and identical
//! on every backend and platform.

use crate::ast::{Expr, Program, Stmt, Type};
use crate::diag::Diagnostic;

const EXACT_ATTR: &str = "exact";

pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for function in program.functions() {
        if !function.attrs.iter().any(|attr| attr.name == EXACT_ATTR) {
            continue;
        }
        let uses_float = function
            .params
            .iter()
            .any(|param| type_has_float(&param.ty))
            || type_has_float(&function.ret)
            || body_has_float(&function.body);
        if uses_float {
            diagnostics.push(
                Diagnostic::new(
                    "exact_arithmetic_uses_float",
                    &function.id,
                    format!(
                        "function '{}' is marked @exact but uses floating point; exact functions must use integer or fixed-point math",
                        function.name
                    ),
                )
                .suggest("remove f32/f64 and use integer or fixed-point (for example Q15) math for exact, deterministic results"),
            );
        }
    }
    diagnostics
}

fn type_has_float(ty: &Type) -> bool {
    match ty {
        Type::Primitive(name) => name == "f32" || name == "f64",
        Type::Slice { elem, .. } | Type::Array { elem, .. } | Type::Pointer { elem, .. } => {
            type_has_float(elem)
        }
        Type::Generic { args, .. } => args.iter().any(type_has_float),
        _ => false,
    }
}

fn body_has_float(body: &[Stmt]) -> bool {
    body.iter().any(stmt_has_float)
}

fn stmt_has_float(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { ty, expr, .. } => {
            ty.as_ref().map(type_has_float).unwrap_or(false) || expr_has_float(expr)
        }
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => expr_has_float(expr),
        Stmt::Return(None) => false,
        Stmt::If { cond, then_body } => expr_has_float(cond) || body_has_float(then_body),
        Stmt::For {
            start, end, body, ..
        } => expr_has_float(start) || expr_has_float(end) || body_has_float(body),
        Stmt::Assign { target, expr } => expr_has_float(target) || expr_has_float(expr),
        Stmt::Asm(_) => false,
    }
}

fn expr_has_float(expr: &Expr) -> bool {
    match expr {
        Expr::Float(_) => true,
        Expr::Cast { ty, expr } => type_has_float(ty) || expr_has_float(expr),
        Expr::Unary { expr, .. } => expr_has_float(expr),
        Expr::Binary { left, right, .. } => expr_has_float(left) || expr_has_float(right),
        Expr::Call { args, .. } => args.iter().any(expr_has_float),
        Expr::Field { base, .. } => expr_has_float(base),
        Expr::Index { base, index } => expr_has_float(base) || expr_has_float(index),
        Expr::Int(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => false,
    }
}
