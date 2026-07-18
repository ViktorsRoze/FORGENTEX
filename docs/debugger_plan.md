# Forgentex Debugger (design + slice ladder)

User-directed priority (2026-07-08): the dev-loop gap moves ahead of `rt.3`.
This is the design record; each slice updates it. Ledger: `v3_progress.md`.

## Thesis fit: an agent-native debugger

Traditional debuggers are interactive UIs for humans. Forgentex's primary
"debugger user" is a **coding agent** that needs machine-readable runtime
evidence to repair a program. So v0 is not a UI — it is a **deterministic
execution trace** whose every step names the **same canonical node id that
semantic patches target** (`@function.f.stmt.2`, `….then.0`, `….body.1`,
`….arm.0.1`). The loop this enables:

```text
fgx debug → failing node id + env at failure → agent emits .fgp patch against
that exact node → fgx patch → fgx debug again
```

Humans get the same trace as readable text; agents consume the JSON.

## v0 surface (`forgentex.debug_trace.v1`)

`fgx debug <file> --entry <fn> [--json-input <in>] [--break <node-id>]
[--max-steps N] [--json]`

- **Step records** — one per executed statement: `seq`, `node` (canonical id),
  `kind`, `text` (readable statement rendering), and `changed` (the env delta:
  bindings created or mutated by that statement, as text values).
- **Call records** — function enter (name, args) / exit (name, return value),
  so the trace shows the dynamic call tree.
- **Run-to-breakpoint** — `--break <node-id>` stops *before* executing that
  node and dumps the full local environment; status `breakpoint`. Non-
  interactive by design (v0): agents re-run cheaply; determinism makes
  "restart + break later" equivalent to interactive stepping.
- **Trap post-mortem** — on any trap, each unwinding frame records the
  function, its locals at failure, and the failing node: a full stack
  snapshot; status `trap`.
- **Step cap** — `--max-steps` (default 10k) turns a runaway loop into status
  `step_limit` with the trace so far, instead of an unbounded dump.

## Design decisions

- **Capability pattern**: the tracer is an `Option<RefCell<DebugTracer>>` on
  the interpreter, exactly like io/net/capsules — `None` costs nothing on
  normal runs (no env cloning, no bookkeeping).
- **AST-path execution**: the debugger runs the interpreter's AST path
  (`fir: None`). Semantics are identical to FIR-backed runs (same tree
  walker underneath); statement-level canonical ids are the AST's native
  vocabulary.
- **Node-id threading**: `run_body`/`exec_stmt` carry an id path mirroring
  the canonical-JSON scheme byte-for-byte, so trace ids == patch targets ==
  `fgx get` targets. This is the whole point.
- **Env deltas by diff**: before/after snapshot per step (tracer-only cost),
  reporting created/changed bindings. Removals (match-arm binding restores)
  are not reported in v0.

## Honest limitations (v0)

- Closure bodies are expressions, so they execute inside a single step —
  no sub-steps inside a lambda (fix: expression-level stepping, later).
- Live capsule calls appear as one step; their transcripts stay in the hosted
  runtime's audit (`capsule_calls`), not duplicated in the trace.
- Non-interactive: no pause/resume process; re-run with a different `--break`
  instead (cheap because runs are deterministic).
- No watch expressions or conditional breakpoints yet (ladder below).

## Slice ladder

| # | Slice | Status |
| --- | --- | --- |
| 1 | **Trace + breakpoint + post-mortem** (this doc's v0 surface, CLI, JSON schema, tests) | ✅ shipped — 6 tests in `tests/debugger.rs`: canonical node ids on every step with per-iteration loop-body repeats and env deltas; call tree enter/exit with arg/return values; breakpoint stops *before* the node with full locals (and the node provably never executes); trap post-mortem stacks innermost-first with per-frame locals; step cap yields `step_limit` with a bounded trace; JSON schema + human text render. CLI verified live: a div-by-zero trap reports failing node `@function.inner.stmt.1` with the full stack, and `--break` dumps locals at the target node. |
| 2 | Conditional breakpoints (`--break-if <node> <expr>`) + watch expressions evaluated per step | 🚧 next in this track |
| 3 | Expression-level stepping (inside closures/complex exprs); capsule-call step detail | ⬜ |
| 4 | Interactive session (`fgx debug --interactive`: step/next/continue/print) on the same tracer | ⬜ |
| 5 | DAP bridge for VS Code (the workbench cockpit) | ⬜ |
