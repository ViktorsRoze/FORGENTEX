# Forgentex v1.0 Roadmap

This roadmap tracks the path from the current v0.26 development track to the
v1.0 release target. It is intentionally status-oriented: completed work stays
marked as done, planned work names the exit criteria that should be satisfied
before the label moves forward.

## Current Snapshot

Current track:

```text
v1.0-cfg-dataflow-borrowck
```

Current release decision:

```text
v1_ready
```

Release gate status:

| Gate group | Status |
| --- | --- |
| Grammar and AST schema | Done |
| Semantic patch protocol | Done |
| Project-wide type checking | Done |
| Effects and capabilities | Done |
| C and Zig source backend experiments | Done |
| Architecture model | Done |
| Structured assembly validation | Done |
| Standard library v0 | Done |
| LSP and VS Code integration | Done |
| CI, fuzzing, and differential tests | Done |
| Documentation | Done |
| Diagnostic catalog and repair plans | Done |
| Inline executable specs | Done |
| Graph artifact workflows | Done |
| Ownership and borrow checking | Done |

The ownership and borrow checking gate closed with the CFG dataflow borrow
checker (`src/borrowck.rs`): moved paths, alias places, reborrow ancestry, and
liveness are computed as fixpoint dataflow analyses over the control-flow
graph, behind the staged rules, so every control-flow shape (nested loops,
conditional reassignments, re-moves across loop back edges, reborrows of
joined aliases) is checked uniformly. The built-in conformance suite covers
moves, borrows, reborrows, loops, branches, lifetimes, projections,
contracts, dataflow shapes, and raw pointer gates.

## Completed Milestones

| Label | Status | Scope |
| --- | --- | --- |
| `v0.1` | Done | First demo |
| `v0.2` | Done | Formal parser, formatter, and checker |
| `v0.3` | Done | Semantic patches v1 and project maps |
| `v0.4` | Done | Ownership and effects prototype |
| `v0.5` | Done | C backend and package projects |
| `v0.6` | Done | Architecture model and structured assembly for x86_64, AArch64, and RISC-V |
| `v0.7` | Done | SIMD and ABI/layout reports |
| `v0.8` | Done | LSP and VS Code workbench |
| `v0.9` | Done | Security, fuzzing, package manager, and backend differential tests |
| `v0.10` | Done | Zig source backend for safe project experiments |
| `v0.11` | Done | Diagnostic catalog, explain, and semantic repair plans |
| `v0.12` | Done | Inline tests and executable source specs |
| `v0.13` | Done | Deterministic graph artifacts and patchable program graph reports |
| `v0.14` | Done | Field-sensitive move paths for owned struct values |
| `v0.15` | Done | Borrow-return provenance summaries for parameter lifetimes |
| `v0.16` | Done | Conditional borrow-return joins for multiple parameter lifetimes |
| `v0.17` | Done | Lexical borrow alias locks for indexed writes |
| `v0.18` | Done | Lifetime region graph foundation |
| `v0.19` | Done | Non-lexical borrow endings |
| `v0.20` | Done | Reborrows and two-phase mutable access |
| `v0.21` | Done | Place and projection alias analysis |
| `v0.22` | Done | Interprocedural lifetime contracts |
| `v0.23` | Done | Borrow checker conformance suite |
| `v0.24` | Done | v1.0 release candidate hardening |
| `v0.25` | Done | Loop-carried borrow liveness |
| `v0.26` | Done | Conditional borrow-state joins |
| `v1.0` | Done | Stable language core, tooling contracts, and production-quality safety guarantees: CFG dataflow borrow checker, strict release check passes, all gates and hardening checks green |

## Remaining Roadmap

v1.0 is complete: `fgx release-check --target v1.0 --strict` exits zero with
all fifteen gates and five hardening checks passing. Post-v1 work continues in
[`docs/v2_roadmap.md`](v2_roadmap.md) (foreign code capsules and the shared
typed namespace).

## Supporting Small-Library Plan

The v0.20-v0.26 work should also grow the standard library just enough to make
benchmarks and agent demos concise without hiding compiler behavior. The planned
tiny modules are tracked in
[`docs/standard_library_v0.md`](standard_library_v0.md#small-library-roadmap).

Near-term planned modules:

- `core.bytes`
- `core.fixed`
- `core.stats`
- `core.geom`
- `core.grid`
- `core.hash`
- `core.rng`
- `core.text`
- `core.json`
- `core.test`
- `core.bench`

These modules are supporting roadmap items, not stable v0 APIs yet. Each one
must land with source modules, catalog entries, checker/interpreter tests, and
at least one demo or token-efficiency benchmark use case before it can be marked
done.

## Tracking Rules

When a milestone is implemented:

- move its status from `Planned` or `Next` to `Done`
- update `current_track` if the milestone becomes the active track
- update `fgx release-check --json` release labels
- add or update tests that prove the exit criteria
- update the relevant design docs, especially `docs/borrow_checker_v1.md`
- run the required release check sequence from `docs/mature_release_criteria.md`

The roadmap should stay conservative. A milestone is not done because the code
accepts one example; it is done when the implementation, diagnostics, tests,
borrow report evidence, and docs all agree.
