# CFG and Dataflow Foundation

This document describes the control-flow graph (CFG) foundation added in
`src/cfg.rs` and the migration plan for moving ownership and borrow checking
onto it.

## Why a CFG

The staged borrow-checker increments (v0.14 through v0.26) each re-derived a
control-flow fact directly on the AST:

- v0.19 non-lexical borrow endings re-derived *last-use liveness*.
- v0.25 loop-carried borrow liveness re-derived *liveness across loop back
  edges*.
- v0.26 conditional borrow-state joins re-derived *may-analysis joins at
  branch merge points*.

Each rule was correct but narrow, and every new control-flow shape (nested
loops, early returns inside branches, future `else`/`while`/`match` syntax)
would require another special case. A CFG with a generic fixpoint solver
computes these facts once, uniformly, for every present and future
control-flow construct. This is the same architectural step Rust took with
MIR before non-lexical lifetimes became tractable.

## Design

`src/cfg.rs` provides:

- `build_cfg(&FunctionDecl) -> Cfg` lowers a function body into basic blocks
  with explicit successor/predecessor edges.
- Block kinds: `entry`, `body`, `loop_header`, `join`, `exit`.
- Each instruction carries dataflow facts:
  - `defs`: whole-variable definitions.
  - `uses`: variable reads, including reads needed to evaluate projections.
  - `write_roots`: variables written *through* (indexed or field assignment);
    these also appear in `uses` because a write-through keeps its root live.
- `liveness(&Cfg) -> Liveness` runs backward may-analysis to a worklist
  fixpoint and returns per-block `live_in`/`live_out` sets.
- `back_edges(&Cfg)` finds loop back edges by depth-first search.
- `loop_carried_live(&Cfg)` returns variables live across any back edge —
  the uniform dataflow form of the v0.25 rule.

Lowering rules:

- `if` produces a branch block with two successors (then-entry and join);
  a then-body that returns does not edge into the join.
- `for` produces a loop-init instruction in the preheader, a `loop_header`
  block that defines the loop variable each iteration, a body with a back
  edge to the header, and an after-join.
- `return` edges to the exit block; trailing statements land in a block with
  no predecessors so analyses see them but no facts flow through them.
- `asm` blocks use their input bindings and define their output bindings.

## Node ID policy

CFG instruction nodes reuse the existing node-ID policy
(`@function.name.stmt.N`, `.then.N`, `.body.N`, `.range`, `.loop`) so CFG
evidence joins against source maps, borrow reports, diagnostics, and
semantic patches without translation.

## CLI

```text
fgx cfg <file> [--json]
```

Text mode prints one line per function with block count, back-edge count,
and loop-carried variables. JSON mode emits `forgentex.cfg_report.v1`
containing one `forgentex.cfg.v1` object per function with blocks, edges,
instructions, and per-block liveness.

## Migration plan for the borrow checker

Status: implemented in v1.0 as `src/borrowck.rs` — a CFG dataflow pass
computing moved paths, alias places, reborrow ancestry, and liveness that
runs behind the staged rules and is pinned by the conformance suite's
`dataflow.*` cases. The staged rules remain the first authority for their
diagnostics; the dataflow pass closes the control-flow shapes they miss.
The original step plan is kept below as the design record:

1. **Liveness substitution.** Replace the v0.19 last-use bookkeeping with
   `liveness` queries. Acceptance: `fgx borrow-conformance --json` reports
   `status=pass` with no case changes.
2. **Loop-carried substitution.** Replace the v0.25 loop back-edge special
   case with `loop_carried_live`. Acceptance: identical conformance results,
   plus new nested-loop cases that the AST rule could not express.
3. **Join substitution.** Re-express the v0.26 conditional borrow-state
   joins as a forward may-analysis over the CFG (borrow-state lattice per
   block). Acceptance: identical conformance results, plus early-return and
   nested-branch cases.
4. **Move analysis.** Track maybe-moved paths as a forward may-analysis,
   replacing the v0.14 field-path merging and the conservative `if`/`for`
   move merging. Acceptance: existing move cases pass; add
   move-in-one-branch-only cases.
5. **Region inference.** Feed the v0.18 region graph from CFG loans and
   uses instead of AST walks, preparing for full lifetime inference.

Each step must keep `fgx check`, `fgx borrow-report`, and
`fgx borrow-conformance` output schemas stable.

## Acceptance gates

- Every function in every example parses into a CFG without panics.
- `liveness` reaches a fixpoint on loops (back edges converge).
- Loop-carried liveness matches the v0.25 rule on its acceptance cases.
- CFG instruction node IDs resolve against the source map for `.fgx` and
  `.fgai` views of the same program.
