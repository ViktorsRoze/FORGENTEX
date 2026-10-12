//! CFG-based borrow and move checker.
//!
//! This is the unified dataflow form of the staged borrow rules
//! (v0.14 through v0.26). Where the staged rules in `typecheck.rs`
//! re-derive a control-flow fact per construct, this pass computes the
//! same safety properties once over the control-flow graph:
//!
//! - moved paths are a forward may-analysis joined at branch and loop
//!   edges, so a move on any path to a read is rejected for every
//!   control-flow shape (nested loops, branches inside loops, re-moves
//!   across loop back edges);
//! - borrow aliases carry sets of possible target places joined at merge
//!   points, so conditional reassignment chains and reborrows of joined
//!   aliases stay tracked;
//! - alias liveness is real CFG liveness, so "last use" and "live across
//!   a back edge" need no special cases.
//!
//! The pass runs behind the staged rules as a second net: it only adds a
//! diagnostic when the staged checker reported nothing of the same code
//! in the same function, so existing behavior is preserved while
//! previously-unreachable control-flow shapes become errors.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ast::{Expr, FunctionDecl, Item, Program, Stmt, StructDecl, Type};
use crate::cfg::{build_cfg, liveness, Block, Cfg, Inst};
use crate::diag::Diagnostic;
use crate::lifetimes::analyze_contracts;

pub fn check_program(program: &Program) -> Vec<Diagnostic> {
    let context = ProgramContext::new(program);
    let mut diagnostics = Vec::new();
    for function in program.functions() {
        diagnostics.extend(check_function_with_context(function, &context));
    }
    diagnostics
}

pub fn check_function(function: &FunctionDecl, program: &Program) -> Vec<Diagnostic> {
    let context = ProgramContext::new(program);
    check_function_with_context(function, &context)
}

struct ProgramContext<'a> {
    functions: HashMap<String, &'a FunctionDecl>,
    structs: HashMap<String, &'a StructDecl>,
    mutating_params: HashMap<String, BTreeSet<usize>>,
}

impl<'a> ProgramContext<'a> {
    fn new(program: &'a Program) -> Self {
        let mut functions = HashMap::new();
        let mut structs = HashMap::new();
        for item in &program.items {
            match item {
                // Data, blob, and note sections carry content, not behaviour.
                Item::Data(_) | Item::Blob(_) | Item::Note(_) => {}
                Item::Function(function) => {
                    functions.insert(function.name.clone(), function);
                }
                Item::Struct(decl) => {
                    structs.insert(decl.name.clone(), decl);
                }
                Item::Enum(_)
                | Item::Trait(_)
                | Item::TraitImpl(_)
                | Item::Test(_)
                | Item::Capsule(_)
                | Item::Share(_)
                | Item::Pipeline(_) => {}
            }
        }
        let mut mutating_params = HashMap::new();
        for contract in analyze_contracts(program) {
            let indices = contract
                .mutates
                .iter()
                .map(|param| param.index)
                .collect::<BTreeSet<_>>();
            mutating_params.insert(contract.function.clone(), indices);
        }
        Self {
            functions,
            structs,
            mutating_params,
        }
    }

    fn callee(&self, name: &str) -> Option<&'a FunctionDecl> {
        self.functions.get(name).copied()
    }

    fn callee_mutates(&self, function: &FunctionDecl, param_index: usize) -> bool {
        self.mutating_params
            .get(&function.id)
            .map(|indices| indices.contains(&param_index))
            .unwrap_or(true)
    }

    fn field_type(&self, root_ty: &Type, path: &[String]) -> Option<Type> {
        let mut ty = root_ty.clone();
        for segment in path {
            let Type::Named(name) = &ty else {
                return None;
            };
            let decl = self.structs.get(name)?;
            ty = decl
                .fields
                .iter()
                .find(|field| field.name == *segment)?
                .ty
                .clone();
        }
        Some(ty)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Proj {
    Field(String),
    Index(Option<i64>),
    Range(Option<i64>, Option<i64>),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    root: String,
    projs: Vec<Proj>,
}

impl Place {
    fn root(name: &str) -> Self {
        Self {
            root: name.to_string(),
            projs: Vec::new(),
        }
    }

    fn label(&self) -> String {
        let mut out = self.root.clone();
        for proj in &self.projs {
            match proj {
                Proj::Field(field) => {
                    out.push('.');
                    out.push_str(field);
                }
                Proj::Index(Some(index)) => out.push_str(&format!("[{index}]")),
                Proj::Index(None) => out.push_str("[*]"),
                Proj::Range(Some(start), Some(end)) => out.push_str(&format!("[{start}..{end}]")),
                Proj::Range(_, _) => out.push_str("[*..*]"),
            }
        }
        out
    }
}

/// Two places overlap unless they prove disjointness through a differing
/// field or a differing constant index at the same projection depth.
fn places_overlap(a: &Place, b: &Place) -> bool {
    if a.root != b.root {
        return false;
    }
    for (pa, pb) in a.projs.iter().zip(b.projs.iter()) {
        match (pa, pb) {
            (Proj::Field(fa), Proj::Field(fb)) if fa != fb => return false,
            (Proj::Index(Some(ia)), Proj::Index(Some(ib))) if ia != ib => return false,
            (Proj::Range(Some(sa), Some(ea)), Proj::Range(Some(sb), Some(eb)))
                if ea <= sb || eb <= sa =>
            {
                return false;
            }
            (Proj::Index(Some(index)), Proj::Range(Some(start), Some(end)))
            | (Proj::Range(Some(start), Some(end)), Proj::Index(Some(index)))
                if index < start || index >= end =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

/// Abstract dataflow state: possible borrow targets per alias binding,
/// reborrow parents, and may-moved paths per owned root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    places: BTreeMap<String, BTreeSet<Place>>,
    parents: BTreeMap<String, BTreeSet<String>>,
    moved: BTreeMap<String, BTreeSet<Vec<String>>>,
}

impl State {
    fn join(&mut self, other: &State) -> bool {
        let mut changed = false;
        for (name, places) in &other.places {
            let entry = self.places.entry(name.clone()).or_default();
            for place in places {
                changed |= entry.insert(place.clone());
            }
        }
        for (name, parents) in &other.parents {
            let entry = self.parents.entry(name.clone()).or_default();
            for parent in parents {
                changed |= entry.insert(parent.clone());
            }
        }
        for (name, paths) in &other.moved {
            let entry = self.moved.entry(name.clone()).or_default();
            for path in paths {
                changed |= entry.insert(path.clone());
            }
        }
        changed
    }

    fn ancestors(&self, name: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = vec![name.to_string()];
        while let Some(current) = queue.pop() {
            if let Some(parents) = self.parents.get(&current) {
                for parent in parents {
                    if seen.insert(parent.clone()) {
                        queue.push(parent.clone());
                    }
                }
            }
        }
        seen
    }
}

fn check_function_with_context(
    function: &FunctionDecl,
    context: &ProgramContext,
) -> Vec<Diagnostic> {
    if function.unsafe_fn {
        return Vec::new();
    }
    let cfg = build_cfg(function);
    let types = collect_types(function, context);
    let entry_states = solve(&cfg, function, &types, context);
    let live = liveness(&cfg);
    let mut checker = FunctionChecker {
        context,
        types: types.clone(),
        diagnostics: Vec::new(),
        reported: BTreeSet::new(),
    };
    for block in &cfg.blocks {
        let live_after = live_after_per_inst(block, &live.live_out[block.id]);
        let mut state = entry_states[block.id].clone();
        for (idx, inst) in block.instructions.iter().enumerate() {
            checker.check_inst(inst, &state, &live_after[idx]);
            apply_inst(inst, &mut state, &types, context);
        }
    }
    checker.diagnostics
}

/// Forward worklist fixpoint over the CFG. Returns the entry state of
/// every block.
fn solve(
    cfg: &Cfg,
    function: &FunctionDecl,
    types: &HashMap<String, Type>,
    context: &ProgramContext,
) -> Vec<State> {
    let mut entry_states = vec![State::default(); cfg.blocks.len()];
    for param in &function.params {
        if is_mut_borrow(&param.ty) {
            entry_states[cfg.entry]
                .places
                .entry(param.name.clone())
                .or_default()
                .insert(Place::root(&param.name));
        }
    }
    let mut worklist: Vec<usize> = (0..cfg.blocks.len()).collect();
    while let Some(id) = worklist.pop() {
        let mut state = entry_states[id].clone();
        for inst in &cfg.blocks[id].instructions {
            apply_inst(inst, &mut state, types, context);
        }
        for &succ in &cfg.blocks[id].successors {
            if entry_states[succ].join(&state) && !worklist.contains(&succ) {
                worklist.push(succ);
            }
        }
    }
    entry_states
}

/// Live-after set for each instruction inside a block, derived from the
/// block's live-out by walking backward.
fn live_after_per_inst(block: &Block, live_out: &BTreeSet<String>) -> Vec<BTreeSet<String>> {
    let mut result = vec![BTreeSet::new(); block.instructions.len()];
    let mut live = live_out.clone();
    for idx in (0..block.instructions.len()).rev() {
        result[idx] = live.clone();
        let inst = &block.instructions[idx];
        for defined in &inst.defs {
            live.remove(defined);
        }
        for used in &inst.uses {
            live.insert(used.clone());
        }
    }
    result
}

/// Transfer function: update the abstract state for one instruction.
fn apply_inst(
    inst: &Inst,
    state: &mut State,
    types: &HashMap<String, Type>,
    context: &ProgramContext,
) {
    let Some(source) = &inst.source else {
        return;
    };
    match source {
        Stmt::Let { name, ty, expr } => {
            mark_value_moves(expr, state, types, context);
            if ty.as_ref().map(is_affine_function).unwrap_or(false) {
                mark_affine_capture_moves(expr, state, types);
            }
            apply_binding(name, expr, state, types);
        }
        Stmt::Assign { target, expr } => {
            mark_value_moves(expr, state, types, context);
            if matches!(target, Expr::Var(name) if types.get(name).map(is_affine_function).unwrap_or(false))
            {
                mark_affine_capture_moves(expr, state, types);
            }
            match target {
                Expr::Var(name) => {
                    apply_binding(name, expr, state, types);
                }
                _ => {
                    // A field write re-initializes that owned path.
                    if let Some((root, path)) = place_path(target) {
                        if let Some(paths) = state.moved.get_mut(&root) {
                            paths.retain(|moved| {
                                !path_covers(&path, moved) && !path_covers(moved, &path)
                            });
                        }
                    }
                }
            }
        }
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            mark_value_moves(expr, state, types, context);
        }
        _ => {}
    }
}

/// Bind `name` to the possible borrow places of `expr` (strong update).
fn apply_binding(name: &str, expr: &Expr, state: &mut State, types: &HashMap<String, Type>) {
    if types.get(name).map(is_mut_borrow).unwrap_or(false) {
        let places = expr_places(expr, state);
        if places.is_empty() {
            // Unknown borrow source: the binding becomes opaque rather
            // than keeping a stale target set.
            state.places.remove(name);
            state.parents.remove(name);
        } else {
            state.places.insert(name.to_string(), places);
            let mut parents = BTreeSet::new();
            if let Some(root) = expr_root(expr) {
                if root != name {
                    parents.insert(root);
                }
            }
            state.parents.insert(name.to_string(), parents);
        }
    } else {
        state.places.remove(name);
        state.parents.remove(name);
    }
    // Re-initializing an owned binding clears its moved paths.
    state.moved.remove(name);
}

/// Mark moves at value positions: the expression itself when it consumes
/// an owned struct, and every by-value call argument, recursively.
fn mark_value_moves(
    expr: &Expr,
    state: &mut State,
    types: &HashMap<String, Type>,
    context: &ProgramContext,
) {
    if let Some((root, path)) = owned_move(expr, types, context) {
        let entry = state.moved.entry(root).or_default();
        entry.retain(|existing| !path_covers(&path, existing));
        entry.insert(path);
        return;
    }
    match expr {
        Expr::Call { name, args } => {
            mark_affine_call(name, state, types);
            for arg in args {
                mark_value_moves(arg, state, types, context);
            }
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
            mark_call_moves_only(expr, state, types, context)
        }
        Expr::Binary { left, right, .. } => {
            mark_call_moves_only(left, state, types, context);
            mark_call_moves_only(right, state, types, context);
        }
        Expr::Index { base, index } => {
            mark_call_moves_only(base, state, types, context);
            mark_call_moves_only(index, state, types, context);
        }
        Expr::SliceRange { base, start, end } => {
            mark_call_moves_only(base, state, types, context);
            mark_call_moves_only(start, state, types, context);
            mark_call_moves_only(end, state, types, context);
        }
        Expr::Lambda { params, body } => {
            mark_owned_capture_moves(params, body, state, types, context)
        }
        _ => {}
    }
}

/// Inside operand positions only nested calls can consume values; plain
/// place reads there are copies.
fn mark_call_moves_only(
    expr: &Expr,
    state: &mut State,
    types: &HashMap<String, Type>,
    context: &ProgramContext,
) {
    match expr {
        Expr::Call { name, args } => {
            mark_affine_call(name, state, types);
            for arg in args {
                mark_value_moves(arg, state, types, context);
            }
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
            mark_call_moves_only(expr, state, types, context)
        }
        Expr::Binary { left, right, .. } => {
            mark_call_moves_only(left, state, types, context);
            mark_call_moves_only(right, state, types, context);
        }
        Expr::Index { base, index } => {
            mark_call_moves_only(base, state, types, context);
            mark_call_moves_only(index, state, types, context);
        }
        Expr::SliceRange { base, start, end } => {
            mark_call_moves_only(base, state, types, context);
            mark_call_moves_only(start, state, types, context);
            mark_call_moves_only(end, state, types, context);
        }
        Expr::Field { base, .. } => mark_call_moves_only(base, state, types, context),
        Expr::Lambda { params, body } => {
            mark_owned_capture_moves(params, body, state, types, context)
        }
        _ => {}
    }
}

/// A closure captures each free variable by value at creation. Capturing a
/// named owned value therefore consumes its whole root, even when the body
/// reads only one field; the interpreter stores the whole value in the closure
/// environment. Primitive and borrowed values remain copy captures.
fn mark_owned_capture_moves(
    params: &[crate::ast::Param],
    body: &Expr,
    state: &mut State,
    types: &HashMap<String, Type>,
    context: &ProgramContext,
) {
    for name in lambda_capture_names(params, body) {
        let capture = Expr::Var(name);
        if let Some((root, path)) = owned_move(&capture, types, context) {
            let entry = state.moved.entry(root).or_default();
            entry.retain(|existing| !path_covers(&path, existing));
            entry.insert(path);
        }
    }
}

/// Move mutable borrow roots into an explicitly affine closure environment.
/// Alias captures consume both the alias binding and every possible underlying
/// root, so the borrow cannot be activated outside the one-shot callable.
fn mark_affine_capture_moves(expr: &Expr, state: &mut State, types: &HashMap<String, Type>) {
    let Expr::Lambda { params, body } = expr else {
        return;
    };
    for name in lambda_capture_names(params, body) {
        if !types.get(&name).map(is_mut_borrow).unwrap_or(false) {
            continue;
        }
        state
            .moved
            .entry(name.clone())
            .or_default()
            .insert(Vec::new());
        for place in expr_places(&Expr::Var(name), state) {
            state
                .moved
                .entry(place.root)
                .or_default()
                .insert(Vec::new());
        }
    }
}

fn mark_affine_call(name: &str, state: &mut State, types: &HashMap<String, Type>) {
    if types.get(name).map(is_affine_function).unwrap_or(false) {
        state
            .moved
            .entry(name.to_string())
            .or_default()
            .insert(Vec::new());
    }
}

fn lambda_capture_names(params: &[crate::ast::Param], body: &Expr) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_expr_var_names(body, &mut names);
    for param in params {
        names.remove(&param.name);
    }
    names
}

fn collect_expr_var_names(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Var(name) => {
            out.insert(name.clone());
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => collect_expr_var_names(expr, out),
        Expr::Binary { left, right, .. } => {
            collect_expr_var_names(left, out);
            collect_expr_var_names(right, out);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_var_names(arg, out);
            }
        }
        Expr::Field { base, .. } => collect_expr_var_names(base, out),
        Expr::Index { base, index } => {
            collect_expr_var_names(base, out);
            collect_expr_var_names(index, out);
        }
        Expr::SliceRange { base, start, end } => {
            collect_expr_var_names(base, out);
            collect_expr_var_names(start, out);
            collect_expr_var_names(end, out);
        }
        Expr::Lambda { params, body } => {
            out.extend(lambda_capture_names(params, body));
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
    }
}

struct FunctionChecker<'a> {
    context: &'a ProgramContext<'a>,
    types: HashMap<String, Type>,
    diagnostics: Vec<Diagnostic>,
    reported: BTreeSet<(String, String, String)>,
}

impl<'a> FunctionChecker<'a> {
    fn check_inst(&mut self, inst: &Inst, state: &State, live_after: &BTreeSet<String>) {
        let Some(source) = inst.source.clone() else {
            return;
        };
        match &source {
            Stmt::Let { ty, expr, .. } => {
                self.check_reads(
                    inst,
                    expr,
                    state,
                    ty.as_ref().map(is_affine_function).unwrap_or(false),
                );
                self.check_calls(inst, expr, state, live_after);
            }
            Stmt::Assign { target, expr } => {
                let affine = matches!(target, Expr::Var(name) if self.types.get(name).map(is_affine_function).unwrap_or(false));
                self.check_reads(inst, expr, state, affine);
                self.check_calls(inst, expr, state, live_after);
                if !matches!(target, Expr::Var(_)) {
                    self.check_index_reads(inst, target, state);
                    self.check_write_through(inst, target, state, live_after);
                }
            }
            Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
                self.check_reads(inst, expr, state, false);
                self.check_calls(inst, expr, state, live_after);
            }
            _ => {}
        }
    }

    /// Reject reads through moved paths (forward may-moved analysis).
    /// A place expression is checked once as a whole; only non-place
    /// subexpressions recurse.
    fn check_reads(&mut self, inst: &Inst, expr: &Expr, state: &State, affine_lambda: bool) {
        if let Some((root, path)) = place_path(expr) {
            if let Some(moved_paths) = state.moved.get(&root) {
                for moved in moved_paths {
                    if path_covers(moved, &path) || path_covers(&path, moved) {
                        self.report_use_after_move(inst, &root, moved);
                        break;
                    }
                }
            }
            return;
        }
        match expr {
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
                self.check_reads(inst, expr, state, false)
            }
            Expr::Binary { left, right, .. } => {
                self.check_reads(inst, left, state, false);
                self.check_reads(inst, right, state, false);
            }
            Expr::Call { name, args } => {
                if state
                    .moved
                    .get(name)
                    .is_some_and(|paths| paths.iter().any(|path| path.is_empty()))
                {
                    self.report_use_after_move(inst, name, &[]);
                }
                for arg in args {
                    self.check_reads(inst, arg, state, false);
                }
            }
            Expr::Index { base, index } => {
                self.check_reads(inst, base, state, false);
                self.check_reads(inst, index, state, false);
            }
            Expr::SliceRange { base, start, end } => {
                self.check_reads(inst, base, state, false);
                self.check_reads(inst, start, state, false);
                self.check_reads(inst, end, state, false);
            }
            Expr::Field { base, .. } => self.check_reads(inst, base, state, false),
            Expr::Lambda { params, body } => {
                for name in lambda_capture_names(params, body) {
                    self.check_reads(inst, &Expr::Var(name.clone()), state, false);
                    if !affine_lambda && self.types.get(&name).map(is_mut_borrow).unwrap_or(false) {
                        self.report_alias(
                            inst,
                            &name,
                            "copyable Fn closure cannot capture a mutable borrow; an affine closure environment is required",
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn check_index_reads(&mut self, inst: &Inst, target: &Expr, state: &State) {
        match target {
            Expr::Index { base, index } => {
                self.check_reads(inst, index, state, false);
                self.check_index_reads(inst, base, state);
            }
            Expr::SliceRange { base, start, end } => {
                self.check_reads(inst, start, state, false);
                self.check_reads(inst, end, state, false);
                self.check_index_reads(inst, base, state);
            }
            Expr::Field { base, .. } => self.check_index_reads(inst, base, state),
            _ => {}
        }
    }

    /// Reject writes through a handle while another live alias may target
    /// an overlapping place. Ancestors of the handle (its loan chain) are
    /// exempt: writing through a child reborrow before the parent's next
    /// use is the supported pattern. A live descendant or sibling is a
    /// conflict.
    fn check_write_through(
        &mut self,
        inst: &Inst,
        target: &Expr,
        state: &State,
        live_after: &BTreeSet<String>,
    ) {
        let Some(root) = expr_root(target) else {
            return;
        };
        let written = expr_places(target, state);
        if written.is_empty() {
            return;
        }
        self.scan_conflicts(inst, &root, &written, state, live_after);
    }

    /// Reject conflicting mutable call arguments: pairwise overlapping
    /// mutable borrows regardless of callee behavior, and mutating-call
    /// activation against live descendants.
    fn check_calls(
        &mut self,
        inst: &Inst,
        expr: &Expr,
        state: &State,
        live_after: &BTreeSet<String>,
    ) {
        match expr {
            Expr::Call { name, args } => {
                for arg in args {
                    self.check_calls(inst, arg, state, live_after);
                }
                let Some(callee) = self.context.callee(name) else {
                    return;
                };
                let mut mut_args: Vec<(usize, &Expr, BTreeSet<Place>)> = Vec::new();
                for (idx, arg) in args.iter().enumerate() {
                    let Some(param) = callee.params.get(idx) else {
                        continue;
                    };
                    if is_mut_borrow(&param.ty) {
                        let places = expr_places(arg, state);
                        if !places.is_empty() {
                            mut_args.push((idx, arg, places));
                        }
                    }
                }
                for i in 0..mut_args.len() {
                    for j in (i + 1)..mut_args.len() {
                        let overlap = mut_args[i]
                            .2
                            .iter()
                            .any(|a| mut_args[j].2.iter().any(|b| places_overlap(a, b)));
                        if overlap {
                            let place = mut_args[i]
                                .2
                                .iter()
                                .next()
                                .map(Place::label)
                                .unwrap_or_default();
                            self.report_alias(
                                inst,
                                &place,
                                "mutable call arguments may alias the same place",
                            );
                        }
                    }
                }
                for (idx, arg, places) in &mut_args {
                    if !self.context.callee_mutates(callee, *idx) {
                        continue;
                    }
                    if let Some(root) = expr_root(arg) {
                        self.scan_conflicts(inst, &root, places, state, live_after);
                    }
                }
            }
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
                self.check_calls(inst, expr, state, live_after)
            }
            Expr::Binary { left, right, .. } => {
                self.check_calls(inst, left, state, live_after);
                self.check_calls(inst, right, state, live_after);
            }
            Expr::Index { base, index } => {
                self.check_calls(inst, base, state, live_after);
                self.check_calls(inst, index, state, live_after);
            }
            Expr::SliceRange { base, start, end } => {
                self.check_calls(inst, base, state, live_after);
                self.check_calls(inst, start, state, live_after);
                self.check_calls(inst, end, state, live_after);
            }
            Expr::Field { base, .. } => self.check_calls(inst, base, state, live_after),
            _ => {}
        }
    }

    fn scan_conflicts(
        &mut self,
        inst: &Inst,
        handle: &str,
        written: &BTreeSet<Place>,
        state: &State,
        live_after: &BTreeSet<String>,
    ) {
        let mut exempt = state.ancestors(handle);
        exempt.insert(handle.to_string());
        // The roots of the written places are the loan sources of the
        // handle; writing through the handle is their purpose.
        for place in written {
            exempt.insert(place.root.clone());
        }
        for (binding, places) in &state.places {
            if exempt.contains(binding) {
                continue;
            }
            let live = live_after.contains(binding) || inst.uses.contains(binding);
            if !live {
                continue;
            }
            let overlap = places
                .iter()
                .any(|place| written.iter().any(|w| places_overlap(place, w)));
            if overlap {
                let place = places.iter().next().map(Place::label).unwrap_or_default();
                self.report_alias(
                    inst,
                    &place,
                    &format!("write conflicts with live mutable alias '{binding}'"),
                );
            }
        }
    }

    fn report_use_after_move(&mut self, inst: &Inst, root: &str, moved: &[String]) {
        let label = if moved.is_empty() {
            root.to_string()
        } else {
            format!("{root}.{}", moved.join("."))
        };
        let key = (
            "use_after_move".to_string(),
            inst.node.clone(),
            label.clone(),
        );
        if !self.reported.insert(key) {
            return;
        }
        self.diagnostics.push(
            Diagnostic::new(
                "use_after_move",
                inst.node.clone(),
                format!("value '{label}' may be moved on a path reaching this use"),
            )
            .suggest("reinitialize the value before this use")
            .suggest("borrow instead of moving"),
        );
    }

    fn report_alias(&mut self, inst: &Inst, place: &str, message: &str) {
        let key = (
            "mutable_borrow_alias".to_string(),
            inst.node.clone(),
            place.to_string(),
        );
        if !self.reported.insert(key) {
            return;
        }
        self.diagnostics.push(
            Diagnostic::new(
                "mutable_borrow_alias",
                inst.node.clone(),
                format!("{message} (place '{place}')"),
            )
            .suggest("end the alias before this statement")
            .suggest("restructure so only one mutable path is live"),
        );
    }
}

/// Collect declared and inferable local types. Bindings whose type cannot
/// be determined stay absent and are treated as opaque (never flagged).
fn collect_types(function: &FunctionDecl, context: &ProgramContext) -> HashMap<String, Type> {
    let mut types = HashMap::new();
    for param in &function.params {
        types.insert(param.name.clone(), param.ty.clone());
    }
    collect_body_types(&function.body, context, &mut types);
    types
}

fn collect_body_types(body: &[Stmt], context: &ProgramContext, types: &mut HashMap<String, Type>) {
    for stmt in body {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                let resolved = ty.clone().or_else(|| infer_expr_type(expr, context, types));
                if let Some(resolved) = resolved {
                    types.insert(name.clone(), resolved);
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_body_types(then_body, context, types);
                collect_body_types(else_body, context, types);
            }
            Stmt::For { var, body, .. } => {
                types.insert(var.clone(), Type::Primitive("usize".to_string()));
                collect_body_types(body, context, types);
            }
            _ => {}
        }
    }
}

fn infer_expr_type(
    expr: &Expr,
    context: &ProgramContext,
    types: &HashMap<String, Type>,
) -> Option<Type> {
    match expr {
        Expr::Var(name) => types.get(name).cloned(),
        Expr::Cast { ty, .. } => Some(ty.clone()),
        Expr::Field { base, field } => {
            let base_ty = infer_expr_type(base, context, types)?;
            context.field_type(&base_ty, std::slice::from_ref(field))
        }
        Expr::Index { base, .. } => infer_expr_type(base, context, types)?.element_type(),
        Expr::SliceRange { base, .. } => match infer_expr_type(base, context, types)? {
            Type::Slice { mutable, elem } => Some(Type::Slice { mutable, elem }),
            Type::Array { elem, .. } => Some(Type::Slice {
                mutable: false,
                elem,
            }),
            _ => None,
        },
        Expr::Call { name, .. } => context.callee(name).map(|callee| callee.ret.clone()),
        _ => None,
    }
}

fn is_mut_borrow(ty: &Type) -> bool {
    matches!(ty, Type::Slice { mutable: true, .. })
}

fn is_affine_function(ty: &Type) -> bool {
    matches!(ty, Type::Generic { name, .. } if name == "FnOnce")
}

fn is_owned_struct(ty: &Type, context: &ProgramContext<'_>) -> bool {
    is_affine_function(ty)
        || matches!(
        ty,
        Type::Named(name)
            if !matches!(name.as_str(), "WakeToken" | "AsyncClock" | "IntervalTimer")
                || context.structs.contains_key(name)
        )
}

/// The possible places an expression refers to, substituting alias
/// bindings for their tracked target sets.
fn expr_places(expr: &Expr, state: &State) -> BTreeSet<Place> {
    let Some((root, projs)) = expr_projection(expr) else {
        return BTreeSet::new();
    };
    if let Some(targets) = state.places.get(&root) {
        targets
            .iter()
            .map(|target| {
                let mut place = target.clone();
                place.projs.extend(projs.clone());
                place
            })
            .collect()
    } else {
        let mut set = BTreeSet::new();
        set.insert(Place { root, projs });
        set
    }
}

fn expr_projection(expr: &Expr) -> Option<(String, Vec<Proj>)> {
    match expr {
        Expr::Var(name) => Some((name.clone(), Vec::new())),
        Expr::Field { base, field } => {
            let (root, mut projs) = expr_projection(base)?;
            projs.push(Proj::Field(field.clone()));
            Some((root, projs))
        }
        Expr::Index { base, index } => {
            let (root, mut projs) = expr_projection(base)?;
            let constant = match index.as_ref() {
                Expr::Int(value) => Some(*value),
                _ => None,
            };
            projs.push(Proj::Index(constant));
            Some((root, projs))
        }
        Expr::SliceRange { base, start, end } => {
            let (root, mut projs) = expr_projection(base)?;
            let literal = |expr: &Expr| match expr {
                Expr::Int(value) => Some(*value),
                _ => None,
            };
            projs.push(Proj::Range(literal(start), literal(end)));
            Some((root, projs))
        }
        _ => None,
    }
}

fn expr_root(expr: &Expr) -> Option<String> {
    expr_projection(expr).map(|(root, _)| root)
}

/// The owned root and field path of a pure field-place expression, used
/// for move tracking. Index projections end path sensitivity.
fn place_path(expr: &Expr) -> Option<(String, Vec<String>)> {
    match expr {
        Expr::Var(name) => Some((name.clone(), Vec::new())),
        Expr::Field { base, field } => {
            let (root, mut path) = place_path(base)?;
            path.push(field.clone());
            Some((root, path))
        }
        _ => None,
    }
}

/// An expression that consumes an owned struct value, returning the moved
/// root and field path.
fn owned_move(
    expr: &Expr,
    types: &HashMap<String, Type>,
    context: &ProgramContext,
) -> Option<(String, Vec<String>)> {
    let (root, path) = place_path(expr)?;
    let root_ty = types.get(&root)?;
    let value_ty = context.field_type(root_ty, &path)?;
    if is_owned_struct(&value_ty, context) {
        Some((root, path))
    } else {
        None
    }
}

/// True when `prefix` covers `path` (equal or an ancestor path).
fn path_covers(prefix: &[String], path: &[String]) -> bool {
    prefix.len() <= path.len() && prefix.iter().zip(path.iter()).all(|(a, b)| a == b)
}
