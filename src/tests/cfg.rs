use crate::cfg::{back_edges, build_cfg, cfg_to_json, liveness, loop_carried_live, BlockKind};
use crate::parser::parse_human;

fn function_cfg(source: &str, name: &str) -> crate::cfg::Cfg {
    let program = parse_human(source).expect("source should parse");
    let function = program
        .find_function(name)
        .expect("function should exist")
        .clone();
    build_cfg(&function)
}

#[test]
fn straight_line_function_has_linear_blocks() {
    let cfg = function_cfg(
        "fn area(width: i32, height: i32) -> i32:\n    return width * height\n",
        "area",
    );
    assert_eq!(cfg.blocks[cfg.entry].kind, BlockKind::Entry);
    assert_eq!(cfg.blocks[cfg.exit].kind, BlockKind::Exit);
    // entry -> body -> exit, no branches, no back edges.
    assert_eq!(cfg.blocks.len(), 3);
    assert!(back_edges(&cfg).is_empty());
    let live = liveness(&cfg);
    // Parameters are live entering the body block.
    let body = cfg.blocks[cfg.entry].successors[0];
    assert!(live.live_in[body].contains("width"));
    assert!(live.live_in[body].contains("height"));
    // Nothing is live entering the function before parameters are defined.
    assert!(live.live_in[cfg.entry].is_empty());
}

#[test]
fn if_statement_creates_branch_and_join() {
    let source = concat!(
        "fn clamp(x: i32) -> i32:\n",
        "    if x > 100:\n",
        "        return 100\n",
        "    return x\n",
    );
    let cfg = function_cfg(source, "clamp");
    // The branch block must have two successors: then-body and join.
    let branch = cfg.blocks[cfg.entry].successors[0];
    assert_eq!(cfg.blocks[branch].successors.len(), 2);
    assert!(cfg.blocks.iter().any(|block| block.kind == BlockKind::Join));
    // `x` stays live through the branch because the fallthrough returns it.
    let live = liveness(&cfg);
    assert!(live.live_in[branch].contains("x"));
    // The then-body returns, so the exit block has two predecessors.
    assert_eq!(cfg.blocks[cfg.exit].predecessors.len(), 2);
}

#[test]
fn for_loop_creates_header_and_back_edge() {
    let source = concat!(
        "fn fill(xs: &mut [i16], value: i16) -> void:\n",
        "    for i in 0 .. xs.len:\n",
        "        xs[i] = value\n",
    );
    let cfg = function_cfg(source, "fill");
    let edges = back_edges(&cfg);
    assert_eq!(edges.len(), 1);
    let (from, header) = edges[0];
    assert_eq!(cfg.blocks[header].kind, BlockKind::LoopHeader);
    assert!(cfg.blocks[from].kind == BlockKind::Body);
    // The loop body writes through `xs` and reads `value` each iteration,
    // so both are loop-carried: live across the back edge.
    let carried = loop_carried_live(&cfg);
    assert!(carried.contains("xs"));
    assert!(carried.contains("value"));
}

#[test]
fn loop_carried_liveness_distinguishes_used_and_dead_bindings() {
    // `kept` is read inside the loop on every iteration; `once` is only
    // read before the loop. The staged borrow rule v0.25 special-cased
    // exactly this; with a CFG it is plain liveness.
    let source = concat!(
        "fn demo(xs: &mut [i16], kept: i16, once: i16) -> i16:\n",
        "    let before: i16 = once\n",
        "    for i in 0 .. xs.len:\n",
        "        xs[i] = kept\n",
        "    return before\n",
    );
    let cfg = function_cfg(source, "demo");
    let carried = loop_carried_live(&cfg);
    assert!(carried.contains("kept"));
    assert!(!carried.contains("once"));
    // `before` is live across the loop (read after it) but not redefined
    // inside it, so it is also carried by the back edge.
    assert!(carried.contains("before"));
}

#[test]
fn early_return_isolates_unreachable_statements() {
    let source = concat!(
        "fn first(x: i32) -> i32:\n",
        "    return x\n",
        "    return 0\n",
    );
    let cfg = function_cfg(source, "first");
    // The trailing return lands in a block with no predecessors.
    let unreachable = cfg
        .blocks
        .iter()
        .find(|block| {
            block.id != cfg.entry && block.predecessors.is_empty() && !block.instructions.is_empty()
        })
        .expect("unreachable block should exist");
    assert_eq!(
        unreachable.instructions[0].kind,
        crate::cfg::InstKind::Return
    );
}

#[test]
fn write_through_records_root_as_use_and_write_root() {
    let source = concat!(
        "fn set(xs: &mut [i16], i: usize, value: i16) -> void:\n",
        "    xs[i] = value\n",
    );
    let cfg = function_cfg(source, "set");
    let inst = cfg
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|inst| inst.kind == crate::cfg::InstKind::WriteThrough)
        .expect("write-through instruction should exist");
    assert!(inst.write_roots.contains("xs"));
    assert!(inst.uses.contains("xs"));
    assert!(inst.uses.contains("i"));
    assert!(inst.uses.contains("value"));
    assert!(inst.defs.is_empty());
}

#[test]
fn instruction_nodes_follow_node_id_policy() {
    let source = concat!(
        "fn walk(xs: &mut [i16]) -> void:\n",
        "    if xs.len > 0:\n",
        "        xs[0] = 1\n",
    );
    let cfg = function_cfg(source, "walk");
    let nodes: Vec<&str> = cfg
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .map(|inst| inst.node.as_str())
        .collect();
    assert!(nodes.contains(&"@function.walk.stmt.0"));
    assert!(nodes.contains(&"@function.walk.stmt.0.then.0"));
}

#[test]
fn cfg_json_is_emitted_with_schema_and_liveness() {
    let cfg = function_cfg(
        "fn area(width: i32, height: i32) -> i32:\n    return width * height\n",
        "area",
    );
    let json = cfg_to_json(&cfg);
    assert!(json.contains("\"schema\": \"forgentex.cfg.v1\""));
    assert!(json.contains("\"function\": \"@function.area\""));
    assert!(json.contains("\"live_in\""));
    assert!(json.contains("\"write_roots\""));
}
