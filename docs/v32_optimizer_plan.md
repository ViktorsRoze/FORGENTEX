# v3.2 — Dependency-Free Optimizer (design + slice ladder)

Backend-track milestone `v3.2` (`v3_backend_plan.md`): const-folding, dead-code
elimination, inlining — pure Rust, zero dependencies — improving both the
interpreter and the emitted C. Entry point for critical gap #3
(`v3_progress.md`): the fast path.

## Design

**Where it runs.** Over the shared AST, *in memory, opt-in* — the pass never
rewrites source files, never changes canonical form on disk, and runs after
`check` so diagnostics always point at the original program. Because both the
interpreter and FIR lowering consume the AST, one optimizer improves every
path.

**The prime directive: trap semantics are preserved.** Forgentex traps on
integer overflow and division by zero; the optimizer must never "optimize away"
a trap:

- integer `+ - *` fold via `checked_*` — if folding would overflow, the
  expression is left alone and the runtime traps as before;
- `/` and `%` fold only when the literal divisor is non-zero;
- shifts and bit-ops are not folded in slice 1 (semantic subtleties);
- `&&`/`||` fold freely — the interpreter evaluates both sides eagerly, so
  there is no short-circuit behavior to preserve;
- a statement expression is dropped only when it is a bare literal or
  variable — `v[i]` (bounds trap) and calls (effects) always stay.

**Passes (slice 1).**
1. **Const folding** — post-order over every expression: unary and binary ops
   on same-typed literals become literals.
2. **Branch elimination** — `if true/false` splices the taken branch;
   `while false` is dropped (`while true` stays — it's a loop).
3. **Dead code elimination** — statements after an unconditional
   `return`/`break`/`continue` in a block are unreachable and dropped;
   pure statement expressions (literal/var) are dropped.

**Report.** `forgentex.optimize.v1`: per-pass counts (folded expressions,
eliminated branches, eliminated statements). Surfaces: `fgx optimize <file>
[--json] [--emit]` (report + optionally the optimized `.fgx`) and
`fgx run … --opt` (optimize before executing).

## Honest limitations (slice 1)

- ~~No inlining~~ Inlining shipped (slice 3); no scalar replacement yet.
- ~~`fgx build --opt` deferred~~ shipped in slice 2 (project-level hook after
  load, before FIR lowering).
- `match` scrutinee folding is not attempted.
- Correctness net: a differential test runs programs pre- and post-optimize
  and asserts identical results, including that preserved traps still fire.

## Slice ladder

| # | Slice | Status |
| --- | --- | --- |
| 1 | Const folding + branch elimination + DCE, `fgx optimize`, `fgx run --opt`, differential equivalence tests | ✅ shipped — 7 tests in `tests/optimizer.rs`: `2 + 3 * 4` folds to `14`; **overflow and div-by-zero folds are refused and still trap post-optimize**; literal branches splice with unreachable code dropped; `while false` removed; a mixed program (loops/match/Vec/strings) is differentially equivalent pre/post at three inputs; report renders `forgentex.optimize.v1`. CLI verified: a 7-line function with `if 1 < 2:` / `6 * 7` / dead `return 99` optimized to 2 lines and ran identically. |
| 2 | Build-pipeline integration (`fgx build --opt`) + measured C-size/compile-time deltas | ✅ shipped — the optimizer runs after project load and **before FIR lowering**, so emitted bodies, contracts, and the `forgentex.fgir.json` receipt all derive from the optimized program (proven: the folded literal appears in the receipt, the unfolded addition does not). Works for `--backend c` and `--backend zig`; `fgx build --opt` prints the pass report **and the measured source delta**. CLI-verified on a demo project: **462 → 320 bytes (-31%)**, with the emitted C showing `int32_t scale = 42;` and the dead branch gone. 2 tests |
| 3 | Function inlining (small, non-recursive callees) | ✅ shipped — a candidate is exactly `return <expr>`, safe, monomorphic, ≤24 nodes, and **call-free** (so substitution can never reorder or duplicate an effect); whole-program rounds let chains collapse (`triple(5)` → inline → fold → literal `15`). Four safety guards, each proven by test: recursion is never a candidate; a **shadowing local `Fn` blocks inlining** (runtime dispatches through the local — result stays 9, not 6); impure arguments block; compound arguments never duplicate into multi-use parameters (`sq(y)` inlines, `sq(a + b)` does not). 6 tests |
| 4 | Simple scalar replacement | 🚧 next in this track |
