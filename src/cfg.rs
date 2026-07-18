//! Control-flow graph and dataflow foundation.
//!
//! Lowers a function body into basic blocks with explicit edges, extracts
//! def/use facts per instruction, and provides fixpoint dataflow analyses
//! over that graph (currently backward liveness, plus back-edge discovery).
//!
//! This module is the intended foundation for ownership and borrow
//! checking. The staged borrow rules (loop-carried liveness, conditional
//! borrow-state joins, non-lexical endings) each re-derive a control-flow
//! fact on the AST; computed here once, those facts fall out of the same
//! graph for every present and future control-flow shape. Instruction node
//! IDs follow the existing node-ID policy (`@function.name.stmt.N`,
//! `.then.N`, `.body.N`) so CFG evidence joins against source maps,
//! borrow reports, and semantic patches.

use std::collections::BTreeSet;

use crate::ast::{Expr, FunctionDecl, Stmt};
use crate::json;

pub type BlockId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Entry,
    Body,
    LoopHeader,
    Join,
    Exit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cfg {
    pub function: String,
    pub entry: BlockId,
    pub exit: BlockId,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub kind: BlockKind,
    pub instructions: Vec<Inst>,
    pub successors: Vec<BlockId>,
    pub predecessors: Vec<BlockId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstKind {
    Param,
    Let,
    Assign,
    WriteThrough,
    Require,
    Branch,
    LoopInit,
    LoopIterate,
    Return,
    Expr,
    Asm,
}

/// One lowered instruction with its dataflow facts.
///
/// `defs` are whole-variable definitions. `uses` are variable reads,
/// including reads required to evaluate projections. `write_roots` are
/// variables written *through* (indexed or field assignment); they appear
/// in `uses` as well because a write-through keeps its root live.
#[derive(Clone, Debug, PartialEq)]
pub struct Inst {
    pub node: String,
    pub kind: InstKind,
    pub defs: BTreeSet<String>,
    pub uses: BTreeSet<String>,
    pub write_roots: BTreeSet<String>,
    /// The lowered source statement, for analyses that need expression
    /// structure beyond the def/use sets. Branch instructions carry their
    /// condition and loop-init instructions carry their range as a
    /// synthetic expression statement.
    pub source: Option<Stmt>,
}

impl Inst {
    fn new(node: String, kind: InstKind) -> Self {
        Self {
            node,
            kind,
            defs: BTreeSet::new(),
            uses: BTreeSet::new(),
            write_roots: BTreeSet::new(),
            source: None,
        }
    }
}

pub fn build_cfg(function: &FunctionDecl) -> Cfg {
    let mut builder = Builder {
        blocks: Vec::new(),
        loop_targets: Vec::new(),
    };
    let entry = builder.new_block(BlockKind::Entry);
    let exit = builder.new_block(BlockKind::Exit);
    for param in &function.params {
        let mut inst = Inst::new(
            format!("{}.param.{}", function.id, param.name),
            InstKind::Param,
        );
        inst.defs.insert(param.name.clone());
        builder.blocks[entry].instructions.push(inst);
    }
    let first = builder.new_block(BlockKind::Body);
    builder.edge(entry, first);
    let prefix = format!("{}.stmt", function.id);
    if let Some(end) = builder.lower_body(&function.body, first, &prefix, exit) {
        builder.edge(end, exit);
    }
    Cfg {
        function: function.id.clone(),
        entry,
        exit,
        blocks: builder.blocks,
    }
}

struct Builder {
    blocks: Vec<Block>,
    /// Stack of the innermost loop's (continue target = header, break target =
    /// after) blocks, so `continue`/`break` can edge to the right place.
    loop_targets: Vec<(BlockId, BlockId)>,
}

impl Builder {
    fn new_block(&mut self, kind: BlockKind) -> BlockId {
        let id = self.blocks.len();
        self.blocks.push(Block {
            id,
            kind,
            instructions: Vec::new(),
            successors: Vec::new(),
            predecessors: Vec::new(),
        });
        id
    }

    fn edge(&mut self, from: BlockId, to: BlockId) {
        if !self.blocks[from].successors.contains(&to) {
            self.blocks[from].successors.push(to);
        }
        if !self.blocks[to].predecessors.contains(&from) {
            self.blocks[to].predecessors.push(from);
        }
    }

    fn push(&mut self, block: BlockId, inst: Inst) {
        self.blocks[block].instructions.push(inst);
    }

    /// Lower a statement list starting in `current`. Returns the block that
    /// control falls out of, or `None` when every path has returned.
    fn lower_body(
        &mut self,
        body: &[Stmt],
        mut current: BlockId,
        prefix: &str,
        exit: BlockId,
    ) -> Option<BlockId> {
        for (idx, stmt) in body.iter().enumerate() {
            let node = format!("{prefix}.{idx}");
            match stmt {
                Stmt::Let { name, expr, .. } => {
                    let mut inst = Inst::new(node, InstKind::Let);
                    collect_uses(expr, &mut inst.uses);
                    inst.defs.insert(name.clone());
                    inst.source = Some(stmt.clone());
                    self.push(current, inst);
                }
                Stmt::Assign { target, expr } => {
                    let mut inst = Inst::new(node, InstKind::Assign);
                    collect_uses(expr, &mut inst.uses);
                    match target {
                        Expr::Var(name) => {
                            inst.defs.insert(name.clone());
                        }
                        _ => {
                            inst.kind = InstKind::WriteThrough;
                            collect_uses(target, &mut inst.uses);
                            if let Some(root) = place_root(target) {
                                inst.write_roots.insert(root);
                            }
                        }
                    }
                    inst.source = Some(stmt.clone());
                    self.push(current, inst);
                }
                Stmt::Require(expr) => {
                    let mut inst = Inst::new(node, InstKind::Require);
                    collect_uses(expr, &mut inst.uses);
                    inst.source = Some(stmt.clone());
                    self.push(current, inst);
                }
                Stmt::Expr(expr) => {
                    let mut inst = Inst::new(node, InstKind::Expr);
                    collect_uses(expr, &mut inst.uses);
                    inst.source = Some(stmt.clone());
                    self.push(current, inst);
                }
                Stmt::Return(expr) => {
                    let mut inst = Inst::new(node, InstKind::Return);
                    if let Some(expr) = expr {
                        collect_uses(expr, &mut inst.uses);
                    }
                    inst.source = Some(stmt.clone());
                    self.push(current, inst);
                    self.edge(current, exit);
                    if idx + 1 < body.len() {
                        // Statements after a return are unreachable; give
                        // them a block with no predecessors so analyses see
                        // them but no facts flow through.
                        current = self.new_block(BlockKind::Body);
                        continue;
                    }
                    return None;
                }
                Stmt::Break | Stmt::Continue => {
                    // `break` jumps to the loop's exit (after) block; `continue`
                    // jumps to the loop header. Both end the current path, so
                    // following statements are unreachable (like a return).
                    if let Some(&(header, after)) = self.loop_targets.last() {
                        let target = if matches!(stmt, Stmt::Break) {
                            after
                        } else {
                            header
                        };
                        self.edge(current, target);
                    }
                    if idx + 1 < body.len() {
                        current = self.new_block(BlockKind::Body);
                        continue;
                    }
                    return None;
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    let mut inst = Inst::new(node.clone(), InstKind::Branch);
                    collect_uses(cond, &mut inst.uses);
                    inst.source = Some(Stmt::Expr(cond.clone()));
                    self.push(current, inst);
                    let then_entry = self.new_block(BlockKind::Body);
                    let join = self.new_block(BlockKind::Join);
                    self.edge(current, then_entry);
                    if let Some(then_exit) =
                        self.lower_body(then_body, then_entry, &format!("{node}.then"), exit)
                    {
                        self.edge(then_exit, join);
                    }
                    if else_body.is_empty() {
                        // No else: the cond-false path flows straight to the join.
                        self.edge(current, join);
                    } else {
                        // The cond-false path runs the else branch, then joins.
                        let else_entry = self.new_block(BlockKind::Body);
                        self.edge(current, else_entry);
                        if let Some(else_exit) =
                            self.lower_body(else_body, else_entry, &format!("{node}.else"), exit)
                        {
                            self.edge(else_exit, join);
                        }
                    }
                    current = join;
                }
                Stmt::For {
                    var,
                    start,
                    end,
                    body,
                } => {
                    let mut init = Inst::new(format!("{node}.range"), InstKind::LoopInit);
                    collect_uses(start, &mut init.uses);
                    collect_uses(end, &mut init.uses);
                    init.source = Some(Stmt::Expr(Expr::Binary {
                        op: "..".to_string(),
                        left: Box::new(start.clone()),
                        right: Box::new(end.clone()),
                    }));
                    self.push(current, init);
                    let header = self.new_block(BlockKind::LoopHeader);
                    self.edge(current, header);
                    let mut iterate = Inst::new(format!("{node}.loop"), InstKind::LoopIterate);
                    iterate.defs.insert(var.clone());
                    self.push(header, iterate);
                    let body_entry = self.new_block(BlockKind::Body);
                    let after = self.new_block(BlockKind::Join);
                    self.edge(header, body_entry);
                    self.edge(header, after);
                    self.loop_targets.push((header, after));
                    let body_exit =
                        self.lower_body(body, body_entry, &format!("{node}.body"), exit);
                    self.loop_targets.pop();
                    if let Some(body_exit) = body_exit {
                        // Loop back edge.
                        self.edge(body_exit, header);
                    }
                    current = after;
                }
                Stmt::While { cond, body } => {
                    // A loop header that branches on the condition each
                    // iteration; the body loops back to it. Same shape as a
                    // `for` loop so the borrow checker's loop-carried liveness
                    // and back-edge handling apply (sound).
                    let header = self.new_block(BlockKind::LoopHeader);
                    self.edge(current, header);
                    let mut branch = Inst::new(format!("{node}.cond"), InstKind::Branch);
                    collect_uses(cond, &mut branch.uses);
                    branch.source = Some(Stmt::Expr(cond.clone()));
                    self.push(header, branch);
                    let body_entry = self.new_block(BlockKind::Body);
                    let after = self.new_block(BlockKind::Join);
                    self.edge(header, body_entry);
                    self.edge(header, after);
                    self.loop_targets.push((header, after));
                    let body_exit =
                        self.lower_body(body, body_entry, &format!("{node}.body"), exit);
                    self.loop_targets.pop();
                    if let Some(body_exit) = body_exit {
                        self.edge(body_exit, header);
                    }
                    current = after;
                }
                Stmt::Match { scrutinee, arms } => {
                    // An N-way branch: the scrutinee is read once, then each arm
                    // is its own block, all joining after. Like `if`/`else`
                    // generalized, so the borrow checker (which runs over the
                    // CFG) sees every arm and merges their effects (sound).
                    let mut inst = Inst::new(node.clone(), InstKind::Branch);
                    collect_uses(scrutinee, &mut inst.uses);
                    inst.source = Some(Stmt::Expr(scrutinee.clone()));
                    self.push(current, inst);
                    let join = self.new_block(BlockKind::Join);
                    for (arm_idx, arm) in arms.iter().enumerate() {
                        let arm_entry = self.new_block(BlockKind::Body);
                        self.edge(current, arm_entry);
                        // A payload binding is a local defined at the arm entry,
                        // taking its value from the scrutinee.
                        if let crate::ast::Pattern::Variant {
                            binding: Some(name),
                            ..
                        } = &arm.pattern
                        {
                            let mut bind =
                                Inst::new(format!("{node}.arm{arm_idx}.bind"), InstKind::Let);
                            collect_uses(scrutinee, &mut bind.uses);
                            bind.defs.insert(name.clone());
                            self.push(arm_entry, bind);
                        }
                        if let Some(arm_exit) = self.lower_body(
                            &arm.body,
                            arm_entry,
                            &format!("{node}.arm{arm_idx}"),
                            exit,
                        ) {
                            self.edge(arm_exit, join);
                        }
                    }
                    current = join;
                }
                Stmt::Asm(block) => {
                    let mut inst = Inst::new(node, InstKind::Asm);
                    for input in &block.inputs {
                        inst.uses.insert(input.value.clone());
                    }
                    for output in &block.outputs {
                        inst.defs.insert(output.value.clone());
                    }
                    inst.source = Some(stmt.clone());
                    self.push(current, inst);
                }
            }
        }
        Some(current)
    }
}

/// Collect every variable read by an expression, including the roots of
/// field and index projections.
pub fn collect_uses(expr: &Expr, uses: &mut BTreeSet<String>) {
    match expr {
        Expr::Lambda { params, body } => {
            // A closure's free variables are captured (read) at creation; its
            // parameters are not outer uses.
            let mut inner = BTreeSet::new();
            collect_uses(body, &mut inner);
            for param in params {
                inner.remove(&param.name);
            }
            uses.extend(inner);
        }
        Expr::Var(name) => {
            uses.insert(name.clone());
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => collect_uses(expr, uses),
        Expr::Binary { left, right, .. } => {
            collect_uses(left, uses);
            collect_uses(right, uses);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_uses(arg, uses);
            }
        }
        Expr::Field { base, .. } => collect_uses(base, uses),
        Expr::Index { base, index } => {
            collect_uses(base, uses);
            collect_uses(index, uses);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
    }
}

/// The root variable of an assignable place (`pair.left[i]` -> `pair`).
pub fn place_root(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Var(name) => Some(name.clone()),
        Expr::Field { base, .. } | Expr::Index { base, .. } => place_root(base),
        _ => None,
    }
}

/// Per-block live-variable sets, computed by backward may-analysis to a
/// worklist fixpoint. A variable is live at a point when some path from
/// that point reads it before redefining it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Liveness {
    pub live_in: Vec<BTreeSet<String>>,
    pub live_out: Vec<BTreeSet<String>>,
}

pub fn liveness(cfg: &Cfg) -> Liveness {
    let block_count = cfg.blocks.len();
    let mut gen_sets = vec![BTreeSet::<String>::new(); block_count];
    let mut kill_sets = vec![BTreeSet::<String>::new(); block_count];
    for block in &cfg.blocks {
        let gen_set = &mut gen_sets[block.id];
        let kill_set = &mut kill_sets[block.id];
        for inst in &block.instructions {
            for used in &inst.uses {
                if !kill_set.contains(used) {
                    gen_set.insert(used.clone());
                }
            }
            for defined in &inst.defs {
                kill_set.insert(defined.clone());
            }
        }
    }

    let mut live_in = vec![BTreeSet::new(); block_count];
    let mut live_out = vec![BTreeSet::new(); block_count];
    let mut worklist: Vec<BlockId> = (0..block_count).rev().collect();
    while let Some(id) = worklist.pop() {
        let mut out = BTreeSet::<String>::new();
        for &succ in &cfg.blocks[id].successors {
            out.extend(live_in[succ].iter().cloned());
        }
        let mut input = gen_sets[id].clone();
        for var in &out {
            if !kill_sets[id].contains(var) {
                input.insert(var.clone());
            }
        }
        let changed = input != live_in[id] || out != live_out[id];
        live_out[id] = out;
        live_in[id] = input;
        if changed {
            for &pred in &cfg.blocks[id].predecessors {
                if !worklist.contains(&pred) {
                    worklist.push(pred);
                }
            }
        }
    }
    Liveness { live_in, live_out }
}

/// Loop back edges, found by depth-first search from the entry block.
/// An edge is a back edge when it targets a block on the current DFS stack.
pub fn back_edges(cfg: &Cfg) -> Vec<(BlockId, BlockId)> {
    let mut edges = Vec::new();
    let mut visited = vec![false; cfg.blocks.len()];
    let mut on_stack = vec![false; cfg.blocks.len()];
    dfs_back_edges(cfg, cfg.entry, &mut visited, &mut on_stack, &mut edges);
    edges
}

fn dfs_back_edges(
    cfg: &Cfg,
    id: BlockId,
    visited: &mut [bool],
    on_stack: &mut [bool],
    edges: &mut Vec<(BlockId, BlockId)>,
) {
    visited[id] = true;
    on_stack[id] = true;
    for &succ in &cfg.blocks[id].successors {
        if on_stack[succ] {
            edges.push((id, succ));
        } else if !visited[succ] {
            dfs_back_edges(cfg, succ, visited, on_stack, edges);
        }
    }
    on_stack[id] = false;
}

/// Variables that are live across a loop back edge: defined or borrowed
/// before the loop and read again by a later iteration. This is the
/// uniform dataflow form of the staged "loop-carried borrow liveness"
/// rule.
pub fn loop_carried_live(cfg: &Cfg) -> BTreeSet<String> {
    let live = liveness(cfg);
    let mut carried = BTreeSet::new();
    for (_, header) in back_edges(cfg) {
        carried.extend(live.live_in[header].iter().cloned());
    }
    carried
}

impl BlockKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockKind::Entry => "entry",
            BlockKind::Body => "body",
            BlockKind::LoopHeader => "loop_header",
            BlockKind::Join => "join",
            BlockKind::Exit => "exit",
        }
    }
}

impl InstKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstKind::Param => "param",
            InstKind::Let => "let",
            InstKind::Assign => "assign",
            InstKind::WriteThrough => "write_through",
            InstKind::Require => "require",
            InstKind::Branch => "branch",
            InstKind::LoopInit => "loop_init",
            InstKind::LoopIterate => "loop_iterate",
            InstKind::Return => "return",
            InstKind::Expr => "expr",
            InstKind::Asm => "asm",
        }
    }
}

pub fn cfg_to_json(cfg: &Cfg) -> String {
    let live = liveness(cfg);
    let mut out = String::from("{\n");
    out.push_str("  \"schema\": \"forgentex.cfg.v1\",\n");
    out.push_str(&format!(
        "  \"function\": {},\n  \"entry\": {},\n  \"exit\": {},\n",
        json::string(&cfg.function),
        cfg.entry,
        cfg.exit
    ));
    out.push_str("  \"blocks\": [");
    for (idx, block) in cfg.blocks.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"id\": {},\n      \"kind\": {},\n      \"successors\": {:?},\n      \"predecessors\": {:?},",
            block.id,
            json::string(block.kind.as_str()),
            block.successors,
            block.predecessors
        ));
        out.push_str(&format!(
            "\n      \"live_in\": {},\n      \"live_out\": {},",
            json::string_array(&live.live_in[block.id].iter().cloned().collect::<Vec<_>>()),
            json::string_array(&live.live_out[block.id].iter().cloned().collect::<Vec<_>>())
        ));
        out.push_str("\n      \"instructions\": [");
        for (inst_idx, inst) in block.instructions.iter().enumerate() {
            if inst_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!(
                "{{\"node\": {}, \"kind\": {}, \"defs\": {}, \"uses\": {}, \"write_roots\": {}}}",
                json::string(&inst.node),
                json::string(inst.kind.as_str()),
                json::string_array(&inst.defs.iter().cloned().collect::<Vec<_>>()),
                json::string_array(&inst.uses.iter().cloned().collect::<Vec<_>>()),
                json::string_array(&inst.write_roots.iter().cloned().collect::<Vec<_>>())
            ));
        }
        out.push_str("]\n    }");
    }
    out.push_str("\n  ]\n}");
    out
}
