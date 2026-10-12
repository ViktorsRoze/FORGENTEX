//! Which width each integer operation is checked at.
//!
//! Integer arithmetic traps at the **declared** width, so `i32 + i32` that
//! overflows an `i32` is an error even though the sum would fit an `i64`. The
//! C backend gets the width from the types it is already carrying while it
//! lowers. The interpreter has no type environment — its environment maps names
//! to values — so this module supplies one: it walks the body about to be
//! executed, tracks the type of every binding, and records the width of each
//! arithmetic node.
//!
//! Nodes are keyed by address. That is sound only for the exact body handed to
//! [`annotate`], and only for as long as that body is alive, which is why the
//! interpreter keeps one map per call frame rather than one per program: the
//! FIR path rebuilds statements per call, so the same address can belong to a
//! different node once the frame is gone.
//!
//! The width rule mirrors `typecheck::arithmetic_result`: an integer literal
//! has no width of its own and takes the other operand's, two literals stay
//! width-less, and operands of one type give that type. A width-less result is
//! deliberately absent from the map — it is checked where it lands instead.

use std::collections::HashMap;

use crate::ast::{Expr, Param, Stmt, Type};

/// The width of every arithmetic node in `body`, keyed by node address.
pub(crate) fn annotate(body: &[Stmt], params: &[Param], ret: &Type) -> HashMap<usize, String> {
    let mut widths = HashMap::new();
    let mut locals: HashMap<String, Type> = params
        .iter()
        .map(|param| (param.name.clone(), param.ty.clone()))
        .collect();
    // The return type is not a binding, but it is the context a bare `return`
    // expression is checked against, so it is recorded under a name no source
    // identifier can collide with.
    locals.insert("$return".to_string(), ret.clone());
    walk_body(body, &mut locals, &mut widths);
    widths
}

fn walk_body(body: &[Stmt], locals: &mut HashMap<String, Type>, out: &mut HashMap<usize, String>) {
    for stmt in body {
        walk_stmt(stmt, locals, out);
    }
}

fn walk_stmt(stmt: &Stmt, locals: &mut HashMap<String, Type>, out: &mut HashMap<usize, String>) {
    match stmt {
        Stmt::Let { name, ty, expr } => {
            let declared = ty.clone().unwrap_or_else(|| infer(expr, locals));
            // A literal-only initialiser takes the binding's width, which is the
            // only place that width can come from.
            walk_expr_in(expr, ty.as_ref(), locals, out);
            locals.insert(name.clone(), declared);
        }
        Stmt::Return(Some(expr)) => {
            let expected = locals.get("$return").cloned();
            walk_expr_in(expr, expected.as_ref(), locals, out);
        }
        Stmt::Return(None) => {}
        Stmt::Require(expr) | Stmt::Expr(expr) => walk_expr(expr, locals, out),
        Stmt::Assign { target, expr } => {
            let expected = infer(target, locals);
            walk_expr(target, locals, out);
            walk_expr_in(expr, Some(&expected), locals, out);
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            walk_expr(cond, locals, out);
            let mut branch = locals.clone();
            walk_body(then_body, &mut branch, out);
            let mut branch = locals.clone();
            walk_body(else_body, &mut branch, out);
        }
        Stmt::For {
            var,
            start,
            end,
            body,
        } => {
            walk_expr(start, locals, out);
            walk_expr(end, locals, out);
            let mut scope = locals.clone();
            // A range bound gives the loop variable its width; two literals
            // leave it width-less, which is the same answer the checker gives.
            let element = match (infer(start, locals), infer(end, locals)) {
                (ty, _) if is_sized_integer(&ty) => ty,
                (_, ty) if is_sized_integer(&ty) => ty,
                _ => Type::Unknown,
            };
            scope.insert(var.clone(), element);
            walk_body(body, &mut scope, out);
        }
        Stmt::While { cond, body } => {
            walk_expr(cond, locals, out);
            let mut scope = locals.clone();
            walk_body(body, &mut scope, out);
        }
        Stmt::Match { scrutinee, arms } => {
            walk_expr(scrutinee, locals, out);
            for arm in arms {
                let mut scope = locals.clone();
                // A binding introduced by a pattern has no declared type here,
                // so arithmetic on it is left unchecked rather than checked at a
                // guessed width.
                if let crate::ast::Pattern::Variant {
                    binding: Some(binding),
                    ..
                } = &arm.pattern
                {
                    scope.insert(binding.clone(), Type::Unknown);
                }
                walk_body(&arm.body, &mut scope, out);
            }
        }
        Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
    }
}

/// Walk an expression flowing into a value of known type, so a literal-only
/// sub-expression is recorded at the context's width instead of being skipped.
fn walk_expr_in(
    expr: &Expr,
    expected: Option<&Type>,
    locals: &HashMap<String, Type>,
    out: &mut HashMap<usize, String>,
) {
    if let (Some(Type::Primitive(name)), Expr::Binary { op, left, right }) = (expected, expr) {
        if is_checked_op(op) && crate::numeric::is_checked_integer_name(name) {
            if let Kind::Literal = kind(expr, locals) {
                out.insert(expr as *const Expr as usize, name.clone());
                walk_expr_in(left, expected, locals, out);
                walk_expr_in(right, expected, locals, out);
                return;
            }
        }
    }
    walk_expr(expr, locals, out);
}

fn walk_expr(expr: &Expr, locals: &HashMap<String, Type>, out: &mut HashMap<usize, String>) {
    match expr {
        Expr::Binary { op, left, right } => {
            if is_checked_op(op) {
                if let Kind::Typed(ty) = kind(expr, locals) {
                    out.insert(expr as *const Expr as usize, ty);
                }
            }
            // A shift's amount is a value in its own right, not part of the
            // shifted operand's width, so both sides are walked normally.
            walk_expr(left, locals, out);
            walk_expr(right, locals, out);
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            walk_expr(expr, locals, out)
        }
        Expr::Call { args, .. } => {
            for arg in args {
                walk_expr(arg, locals, out);
            }
        }
        Expr::Index { base, index } => {
            walk_expr(base, locals, out);
            walk_expr(index, locals, out);
        }
        Expr::SliceRange { base, start, end } => {
            walk_expr(base, locals, out);
            walk_expr(start, locals, out);
            walk_expr(end, locals, out);
        }
        Expr::Lambda { params, body } => {
            let mut scope = locals.clone();
            for param in params {
                scope.insert(param.name.clone(), param.ty.clone());
            }
            walk_expr(body, &scope, out);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
    }
}

fn is_checked_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "%" | "<<" | ">>")
}

fn is_sized_integer(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(name) if crate::numeric::is_checked_integer_name(name))
}

/// What an operand contributes to the width of the operation containing it.
enum Kind {
    /// An untyped integer literal, or arithmetic built only from them.
    Literal,
    /// A value of a known, sized integer type.
    Typed(String),
    /// Anything else, including a type that could not be resolved.
    Other,
}

fn kind(expr: &Expr, locals: &HashMap<String, Type>) -> Kind {
    match expr {
        Expr::Int(_) => Kind::Literal,
        Expr::Unary { expr, .. } => kind(expr, locals),
        Expr::Binary { op, left, right } if is_checked_op(op) => {
            match (kind(left, locals), kind(right, locals)) {
                (Kind::Literal, Kind::Literal) => Kind::Literal,
                (Kind::Typed(ty), Kind::Literal) | (Kind::Literal, Kind::Typed(ty)) => {
                    Kind::Typed(ty)
                }
                (Kind::Typed(left), Kind::Typed(right)) if left == right => Kind::Typed(left),
                // Mixed widths are a type error the checker rejects, so reaching
                // here means a type was unresolved. Guessing a width could trap
                // a valid program, so none is recorded.
                _ => Kind::Other,
            }
        }
        other => match infer(other, locals) {
            Type::Primitive(name) if crate::numeric::is_checked_integer_name(&name) => {
                Kind::Typed(name)
            }
            _ => Kind::Other,
        },
    }
}

/// The type of an expression, to the extent this pass needs one: enough to
/// resolve integer widths, `Unknown` elsewhere.
fn infer(expr: &Expr, locals: &HashMap<String, Type>) -> Type {
    match expr {
        Expr::Cast { ty, .. } => ty.clone(),
        Expr::Var(name) => locals.get(name).cloned().unwrap_or(Type::Unknown),
        Expr::Unary { expr, .. } => infer(expr, locals),
        Expr::Index { base, .. } => match infer(base, locals) {
            Type::Slice { elem, .. } | Type::Array { elem, .. } => *elem,
            Type::Generic { name, args } if name == "Vec" => {
                args.into_iter().next().unwrap_or(Type::Unknown)
            }
            _ => Type::Unknown,
        },
        Expr::Field { field, .. } if field == "len" => Type::Primitive("usize".to_string()),
        Expr::Binary { op, left, right } if is_checked_op(op) => match kind(expr, locals) {
            Kind::Typed(name) => Type::Primitive(name),
            _ => {
                let _ = (left, right);
                Type::Unknown
            }
        },
        _ => Type::Unknown,
    }
}
