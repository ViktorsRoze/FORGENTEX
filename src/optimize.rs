//! v3.2 — the dependency-free optimizer (slice 1).
//!
//! Const folding, branch elimination, and dead-code elimination over the
//! shared AST. Runs in memory and opt-in (`fgx optimize`, `fgx run --opt`) —
//! source files and canonical form are never rewritten.
//!
//! The prime directive: **trap semantics are preserved**. Integer folding uses
//! checked arithmetic (an overflowing fold is left for the runtime to trap);
//! division/modulo fold only for non-zero literal divisors; statement
//! expressions with possible side effects (calls, indexing) are never dropped.
//! See docs/v32_optimizer_plan.md.

use crate::ast::{Expr, Item, Program, Stmt};

/// Optimize every module program in a loaded project (build-pipeline entry,
/// `fgx build --opt`). In memory only — module sources on disk are untouched.
pub fn optimize_project(project: &mut crate::project::Project) -> OptReport {
    let mut report = OptReport::default();
    for module in &mut project.modules {
        let module_report = optimize_program(&mut module.program);
        report.folded_exprs += module_report.folded_exprs;
        report.eliminated_branches += module_report.eliminated_branches;
        report.eliminated_stmts += module_report.eliminated_stmts;
    }
    report
}

/// Per-pass counts for the optimize report (`forgentex.optimize.v1`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OptReport {
    pub folded_exprs: usize,
    pub eliminated_branches: usize,
    pub eliminated_stmts: usize,
    pub inlined_calls: usize,
}

impl OptReport {
    pub fn total(&self) -> usize {
        self.folded_exprs + self.eliminated_branches + self.eliminated_stmts + self.inlined_calls
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"schema\": \"forgentex.optimize.v1\",\n  \"passes\": [\"inline\", \"const_fold\", \"branch_elim\", \"dce\"],\n  \"inlined_calls\": {},\n  \"folded_exprs\": {},\n  \"eliminated_branches\": {},\n  \"eliminated_stmts\": {}\n}}",
            self.inlined_calls, self.folded_exprs, self.eliminated_branches, self.eliminated_stmts
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "optimize: inline replaced {} call(s); const_fold folded {} expr(s); branch_elim removed {} branch(es); dce removed {} stmt(s)\n",
            self.inlined_calls, self.folded_exprs, self.eliminated_branches, self.eliminated_stmts
        )
    }
}

/// Optimize every function body in place. Iterates folding + structural
/// passes to a fixed point (bounded), since each enables the other
/// (`if 1 < 2:` folds to `if true:` which then splices).
pub fn optimize_program(program: &mut Program) -> OptReport {
    let mut report = OptReport::default();
    // Whole-program rounds: inlining can turn a body call-free, making it a
    // candidate for the next round; folding can expose literal branches. A
    // small bound is plenty — each round strictly shrinks or stops.
    for _ in 0..8 {
        let before = report.clone();
        let candidates = collect_inline_candidates(program);
        for item in &mut program.items {
            if let Item::Function(function) = item {
                let mut scope: std::collections::BTreeSet<String> = function
                    .params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect();
                for stmt in &mut function.body {
                    inline_stmt(stmt, &candidates, &mut scope, &mut report);
                }
                for stmt in &mut function.body {
                    fold_stmt(stmt, &mut report);
                }
                function.body = eliminate(std::mem::take(&mut function.body), &mut report);
            }
        }
        if report == before {
            break;
        }
    }
    report
}

/// A callee eligible for inlining (v3.2 slice 3): exactly `return <expr>`,
/// safe, monomorphic, and its body contains **no calls at all** — so
/// substitution can never reorder or duplicate an effect.
struct InlineCandidate {
    params: Vec<String>,
    body: Expr,
}

fn collect_inline_candidates(
    program: &Program,
) -> std::collections::BTreeMap<String, InlineCandidate> {
    let mut candidates = std::collections::BTreeMap::new();
    for function in program.functions() {
        if function.unsafe_fn || !function.type_params.is_empty() {
            continue;
        }
        let [Stmt::Return(Some(body))] = function.body.as_slice() else {
            continue;
        };
        if !expr_is_pure(body) || expr_size(body) > 24 {
            continue;
        }
        candidates.insert(
            function.name.clone(),
            InlineCandidate {
                params: function.params.iter().map(|p| p.name.clone()).collect(),
                body: body.clone(),
            },
        );
    }
    candidates
}

/// Pure and non-effectful: literals, vars, and unary/binary/cast over pure.
/// Calls (effects), indexing/fields (traps/projections), and lambdas are out.
fn expr_is_pure(expr: &Expr) -> bool {
    match expr {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => true,
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => expr_is_pure(expr),
        Expr::Binary { left, right, .. } => expr_is_pure(left) && expr_is_pure(right),
        Expr::Call { .. }
        | Expr::Field { .. }
        | Expr::Index { .. }
        | Expr::Lambda { .. }
        | Expr::TraitConst { .. } => false,
    }
}

fn expr_size(expr: &Expr) -> usize {
    1 + match expr {
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => expr_size(expr),
        Expr::Binary { left, right, .. } => expr_size(left) + expr_size(right),
        Expr::Call { args, .. } => args.iter().map(expr_size).sum(),
        Expr::Field { base, .. } => expr_size(base),
        Expr::Index { base, index } => expr_size(base) + expr_size(index),
        Expr::Lambda { body, .. } => expr_size(body),
        _ => 0,
    }
}

fn count_var_uses(expr: &Expr, name: &str) -> usize {
    match expr {
        Expr::Var(var) => usize::from(var == name),
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => count_var_uses(expr, name),
        Expr::Binary { left, right, .. } => {
            count_var_uses(left, name) + count_var_uses(right, name)
        }
        Expr::Call { args, .. } => args.iter().map(|arg| count_var_uses(arg, name)).sum(),
        Expr::Field { base, .. } => count_var_uses(base, name),
        Expr::Index { base, index } => count_var_uses(base, name) + count_var_uses(index, name),
        Expr::Lambda { params, body } => {
            if params.iter().any(|param| param.name == name) {
                0
            } else {
                count_var_uses(body, name)
            }
        }
        _ => 0,
    }
}

fn substitute_var(expr: &mut Expr, name: &str, replacement: &Expr) {
    match expr {
        Expr::Var(var) if var == name => *expr = replacement.clone(),
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
            substitute_var(expr, name, replacement)
        }
        Expr::Binary { left, right, .. } => {
            substitute_var(left, name, replacement);
            substitute_var(right, name, replacement);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                substitute_var(arg, name, replacement);
            }
        }
        Expr::Field { base, .. } => substitute_var(base, name, replacement),
        Expr::Index { base, index } => {
            substitute_var(base, name, replacement);
            substitute_var(index, name, replacement);
        }
        // The lambda's own parameter shadows the substituted name.
        Expr::Lambda { params, body } if !params.iter().any(|param| param.name == name) => {
            substitute_var(body, name, replacement);
        }
        _ => {}
    }
}

/// Inline eligible calls inside one statement, tracking the local scope so a
/// call through a shadowing local (`let double: Fn[…] = …`) is never inlined —
/// the runtime dispatches through the local, and so must we.
fn inline_stmt(
    stmt: &mut Stmt,
    candidates: &std::collections::BTreeMap<String, InlineCandidate>,
    scope: &mut std::collections::BTreeSet<String>,
    report: &mut OptReport,
) {
    match stmt {
        Stmt::Let { name, expr, .. } => {
            inline_expr(expr, candidates, scope, report);
            scope.insert(name.clone());
        }
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            inline_expr(expr, candidates, scope, report)
        }
        Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            inline_expr(cond, candidates, scope, report);
            let mut then_scope = scope.clone();
            for stmt in then_body {
                inline_stmt(stmt, candidates, &mut then_scope, report);
            }
            let mut else_scope = scope.clone();
            for stmt in else_body {
                inline_stmt(stmt, candidates, &mut else_scope, report);
            }
        }
        Stmt::For {
            var,
            start,
            end,
            body,
        } => {
            inline_expr(start, candidates, scope, report);
            inline_expr(end, candidates, scope, report);
            let mut loop_scope = scope.clone();
            loop_scope.insert(var.clone());
            for stmt in body {
                inline_stmt(stmt, candidates, &mut loop_scope, report);
            }
        }
        Stmt::While { cond, body } => {
            inline_expr(cond, candidates, scope, report);
            let mut loop_scope = scope.clone();
            for stmt in body {
                inline_stmt(stmt, candidates, &mut loop_scope, report);
            }
        }
        Stmt::Match { scrutinee, arms } => {
            inline_expr(scrutinee, candidates, scope, report);
            for arm in arms {
                let mut arm_scope = scope.clone();
                if let crate::ast::Pattern::Variant {
                    binding: Some(binding),
                    ..
                } = &arm.pattern
                {
                    arm_scope.insert(binding.clone());
                }
                for stmt in &mut arm.body {
                    inline_stmt(stmt, candidates, &mut arm_scope, report);
                }
            }
        }
        Stmt::Assign { target, expr } => {
            inline_expr(target, candidates, scope, report);
            inline_expr(expr, candidates, scope, report);
        }
    }
}

fn inline_expr(
    expr: &mut Expr,
    candidates: &std::collections::BTreeMap<String, InlineCandidate>,
    scope: &std::collections::BTreeSet<String>,
    report: &mut OptReport,
) {
    // Inline children first (innermost calls before enclosing ones).
    match expr {
        Expr::Unary { expr: inner, .. } | Expr::Cast { expr: inner, .. } => {
            inline_expr(inner, candidates, scope, report)
        }
        Expr::Binary { left, right, .. } => {
            inline_expr(left, candidates, scope, report);
            inline_expr(right, candidates, scope, report);
        }
        Expr::Call { args, .. } => {
            for arg in args.iter_mut() {
                inline_expr(arg, candidates, scope, report);
            }
        }
        Expr::Field { base, .. } => inline_expr(base, candidates, scope, report),
        Expr::Index { base, index } => {
            inline_expr(base, candidates, scope, report);
            inline_expr(index, candidates, scope, report);
        }
        Expr::Lambda { body, .. } => inline_expr(body, candidates, scope, report),
        _ => {}
    }

    let Expr::Call { name, args } = expr else {
        return;
    };
    // A local of the same name shadows the program function at runtime.
    if scope.contains(name.as_str()) {
        return;
    }
    let Some(candidate) = candidates.get(name.as_str()) else {
        return;
    };
    if candidate.params.len() != args.len() {
        return;
    }
    // Every argument must be pure; a compound (potentially trapping /
    // work-carrying) argument may substitute only into a single use.
    for (param, arg) in candidate.params.iter().zip(args.iter()) {
        if !expr_is_pure(arg) {
            return;
        }
        let atom = matches!(
            arg,
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_)
        );
        if !atom && count_var_uses(&candidate.body, param) > 1 {
            return;
        }
    }
    let mut body = candidate.body.clone();
    for (param, arg) in candidate.params.iter().zip(args.iter()) {
        substitute_var(&mut body, param, arg);
    }
    *expr = body;
    report.inlined_calls += 1;
}

/// Fold every expression inside a statement (recursing into nested bodies).
fn fold_stmt(stmt: &mut Stmt, report: &mut OptReport) {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Return(Some(expr))
        | Stmt::Require(expr)
        | Stmt::Expr(expr) => fold_expr(expr, report),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            fold_expr(cond, report);
            for stmt in then_body.iter_mut().chain(else_body.iter_mut()) {
                fold_stmt(stmt, report);
            }
        }
        Stmt::For {
            start, end, body, ..
        } => {
            fold_expr(start, report);
            fold_expr(end, report);
            for stmt in body {
                fold_stmt(stmt, report);
            }
        }
        Stmt::While { cond, body } => {
            fold_expr(cond, report);
            for stmt in body {
                fold_stmt(stmt, report);
            }
        }
        Stmt::Match { scrutinee, arms } => {
            fold_expr(scrutinee, report);
            for arm in arms {
                for stmt in &mut arm.body {
                    fold_stmt(stmt, report);
                }
            }
        }
        Stmt::Assign { target, expr } => {
            fold_expr(target, report);
            fold_expr(expr, report);
        }
    }
}

/// Post-order constant folding. Only folds when the runtime result is
/// identical — including *not* folding anything that would trap.
fn fold_expr(expr: &mut Expr, report: &mut OptReport) {
    // Fold children first.
    match expr {
        Expr::Unary { expr: inner, .. } | Expr::Cast { expr: inner, .. } => {
            fold_expr(inner, report)
        }
        Expr::Binary { left, right, .. } => {
            fold_expr(left, report);
            fold_expr(right, report);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                fold_expr(arg, report);
            }
        }
        Expr::Field { base, .. } => fold_expr(base, report),
        Expr::Index { base, index } => {
            fold_expr(base, report);
            fold_expr(index, report);
        }
        Expr::Lambda { body, .. } => fold_expr(body, report),
        _ => {}
    }

    let folded = match expr {
        Expr::Unary { op, expr: inner } => match (op.as_str(), inner.as_ref()) {
            ("-", Expr::Int(value)) => value.checked_neg().map(Expr::Int),
            ("-", Expr::Float(value)) => Some(Expr::Float(-value)),
            ("!", Expr::Bool(value)) => Some(Expr::Bool(!value)),
            _ => None,
        },
        Expr::Binary { op, left, right } => fold_binary(op, left, right),
        _ => None,
    };
    if let Some(folded) = folded {
        *expr = folded;
        report.folded_exprs += 1;
    }
}

fn fold_binary(op: &str, left: &Expr, right: &Expr) -> Option<Expr> {
    match (left, right) {
        (Expr::Int(a), Expr::Int(b)) => {
            let (a, b) = (*a, *b);
            match op {
                // Checked: an overflowing fold is left for the runtime trap.
                "+" => a.checked_add(b).map(Expr::Int),
                "-" => a.checked_sub(b).map(Expr::Int),
                "*" => a.checked_mul(b).map(Expr::Int),
                // Division/modulo by a zero literal must trap at runtime.
                "/" if b != 0 => a.checked_div(b).map(Expr::Int),
                "%" if b != 0 => a.checked_rem(b).map(Expr::Int),
                "==" => Some(Expr::Bool(a == b)),
                "!=" => Some(Expr::Bool(a != b)),
                "<" => Some(Expr::Bool(a < b)),
                ">" => Some(Expr::Bool(a > b)),
                "<=" => Some(Expr::Bool(a <= b)),
                ">=" => Some(Expr::Bool(a >= b)),
                _ => None,
            }
        }
        (Expr::Float(a), Expr::Float(b)) => {
            let (a, b) = (*a, *b);
            match op {
                "+" => Some(Expr::Float(a + b)),
                "-" => Some(Expr::Float(a - b)),
                "*" => Some(Expr::Float(a * b)),
                // f64 division matches the runtime exactly (0.0 → inf).
                "/" => Some(Expr::Float(a / b)),
                "==" => Some(Expr::Bool(a == b)),
                "!=" => Some(Expr::Bool(a != b)),
                "<" => Some(Expr::Bool(a < b)),
                ">" => Some(Expr::Bool(a > b)),
                "<=" => Some(Expr::Bool(a <= b)),
                ">=" => Some(Expr::Bool(a >= b)),
                _ => None,
            }
        }
        (Expr::Bool(a), Expr::Bool(b)) => {
            let (a, b) = (*a, *b);
            match op {
                // The interpreter evaluates both operands eagerly, so folding
                // introduces no short-circuit difference.
                "&&" => Some(Expr::Bool(a && b)),
                "||" => Some(Expr::Bool(a || b)),
                "==" => Some(Expr::Bool(a == b)),
                "!=" => Some(Expr::Bool(a != b)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Structural elimination over one block: splice literal `if` branches, drop
/// `while false`, drop unreachable statements after a terminator, and drop
/// pure statement expressions.
fn eliminate(body: Vec<Stmt>, report: &mut OptReport) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(body.len());
    let mut terminated = false;
    for mut stmt in body {
        if terminated {
            // Unreachable after return/break/continue.
            report.eliminated_stmts += 1;
            continue;
        }
        match stmt {
            Stmt::If {
                cond: Expr::Bool(taken),
                then_body,
                else_body,
            } => {
                report.eliminated_branches += 1;
                let branch = if taken { then_body } else { else_body };
                for inner in eliminate(branch, report) {
                    if matches!(inner, Stmt::Return(_) | Stmt::Break | Stmt::Continue) {
                        terminated = true;
                        out.push(inner);
                        break;
                    }
                    out.push(inner);
                }
                continue;
            }
            Stmt::While {
                cond: Expr::Bool(false),
                ..
            } => {
                report.eliminated_branches += 1;
                continue;
            }
            // Pure statement expressions have no effect. Only bare literals
            // and variables qualify — indexing traps, calls have effects.
            Stmt::Expr(
                Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_),
            ) => {
                report.eliminated_stmts += 1;
                continue;
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                stmt = Stmt::If {
                    cond,
                    then_body: eliminate(then_body, report),
                    else_body: eliminate(else_body, report),
                };
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                stmt = Stmt::For {
                    var,
                    start,
                    end,
                    body: eliminate(body, report),
                };
            }
            Stmt::While { cond, body } => {
                stmt = Stmt::While {
                    cond,
                    body: eliminate(body, report),
                };
            }
            Stmt::Match {
                scrutinee,
                mut arms,
            } => {
                for arm in &mut arms {
                    arm.body = eliminate(std::mem::take(&mut arm.body), report);
                }
                stmt = Stmt::Match { scrutinee, arms };
            }
            _ => {}
        }
        if matches!(stmt, Stmt::Return(_) | Stmt::Break | Stmt::Continue) {
            terminated = true;
        }
        out.push(stmt);
    }
    out
}
