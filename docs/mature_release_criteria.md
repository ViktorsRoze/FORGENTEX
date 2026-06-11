# Forgentex Mature Release Criteria

Stage 26 defines what "mature enough for serious use" means. It does not claim
that the current v0 demo is production-ready; it gives the project a stable
release-readiness contract that humans, CI, and agents can inspect.

## Command

```powershell
fgx release-check --json
```

The command emits `forgentex.release_check.v1` with:

- target release label
- current development track
- release decision
- maturity gates
- evidence for each gate
- explicit gaps
- release-candidate hardening checks
- release label roadmap

Text output is available without `--json`:

```powershell
fgx release-check
```

Use `--strict` when a CI job should fail if the requested target is not ready:

```powershell
fgx release-check --target v1.0 --strict
```

## v1.0 Maturity Gates

Forgentex v1.0 requires:

- stable grammar and AST schema
- stable semantic patch protocol
- project-wide type checking
- ownership and borrow checking for safe code
- effects and capability model
- at least one native or transpiler backend suitable for production experiments
- architecture model for x86_64, ARM/AArch64, and RISC-V
- structured assembly validation for x86_64 and AArch64, with RISC-V in active support
- standard library v0
- LSP and VS Code integration
- CI, fuzzing, and differential backend tests
- documentation for language users, compiler contributors, and agent tool authors

## Current Decision

The current report defaults to `--target v1.0` and returns:

```text
decision=v1_ready
```

All fifteen maturity gates and five hardening checks pass. Stage 27 adds borrow-alias root tracking and conservative
conditional/loop move merging, v0.14 adds field-sensitive move paths for owned
struct values, and v0.15 adds borrow-return provenance summaries for direct
parameter lifetimes. v0.16 extends those summaries through conditional
borrow-return joins with multiple possible parameter origins. v0.18 adds
lifetime region graph evidence, and v0.19 uses simple last-use checks to end
local borrow aliases before later root writes. v0.20 adds local reborrow
parent-child chains, shared reborrows from mutable slices, and two-phase mutable
call activation for simple helper calls. v0.21 adds local place/projection alias
analysis for obvious field and constant-index disjointness. v0.22 adds explicit
interprocedural lifetime contracts for returned and mutating borrow parameters.
v0.23 adds a built-in borrow-checker conformance suite with positive, negative,
and contract-summary cases.
v0.24 adds release-candidate hardening checks to release-check, freezes the
diagnostic catalog shape, requires evidence completeness for every maturity
gate, and closes the raw-pointer-parameter safety diagnostic edge.
v0.25 adds loop-carried borrow liveness so aliases created before a loop remain
live across loop back-edges when the loop body reads them.
v0.26 adds conditional borrow-state joins so aliases reassigned in branch bodies
keep every possible root for later writes and call-boundary checks.
v1.0 closes the ownership gate with the CFG dataflow borrow checker
(`src/borrowck.rs`, `docs/borrow_checker_v1.md`): moved paths, alias places,
reborrow ancestry, and liveness are fixpoint dataflow analyses over the
control-flow graph, pinned by the conformance suite's `dataflow.*` cases.
Stage 28 adds Zig source backend output, Stage 29 adds diagnostic repair
planning, Stage 30 adds inline executable specs, and Stage 31 adds the
`.fggraph` artifact. Native compiler invocation is still deferred.

The completed v0.26-to-v1.0 implementation roadmap is tracked in
[docs/v1_roadmap.md](v1_roadmap.md). Keep that document, this release label
table, and `fgx release-check --json` in sync when a milestone changes status.

## Release Candidate Hardening

`fgx release-check --json` includes `hardening_checks` in addition to maturity
gates. v0.24 requires:

- `strict_release_gate`: strict mode remains the CI-facing failure path when a
  requested target is not ready.
- `gate_evidence_complete`: every gate has a requirement, evidence, a valid
  status, and explicit gaps when not passing.
- `diagnostic_shape_freeze`: diagnostic catalog entries keep stable fields for
  code, canonical code, family, rule, summary, safety, patch suggestions,
  review gates, and target consequences.
- `borrow_escape_hatch_conformance`: borrow conformance covers the RC cases for
  raw pointer parameters, conditional lifetime escape, projection aliasing, and
  loop-carried borrow liveness, and conditional borrow-state joins.
- `target_decision_consistency`: the release decision is derived from the
  requested target and current gate statuses.

The v0.24 through v0.26 hardening target is healthy when `hardening_fail=0`. That
does not make v1.0 ready by itself; the ownership and borrowing gate must still
move from `partial` to `pass`.

## Release Labels

The release labels are:

| Label | Meaning | Current status |
| --- | --- | --- |
| `v0.1` | first demo | completed |
| `v0.2` | formal parser, formatter, and checker | completed |
| `v0.3` | semantic patches v1 and project maps | completed |
| `v0.4` | ownership and effects prototype | completed |
| `v0.5` | C backend and package projects | completed |
| `v0.6` | architecture model and structured assembly for x86_64, AArch64, and RISC-V | completed |
| `v0.7` | SIMD and ABI/layout reports | completed |
| `v0.8` | LSP and VS Code workbench | completed |
| `v0.9` | security, fuzzing, package manager, and backend differential tests | completed |
| `v0.10` | Zig source backend for safe project experiments | completed |
| `v0.11` | diagnostic catalog, explain, and semantic repair plans | completed |
| `v0.12` | inline tests and executable source specs | completed |
| `v0.13` | deterministic graph artifacts and patchable program graph reports | completed |
| `v0.14` | field-sensitive move paths for owned struct values | completed |
| `v0.15` | borrow-return provenance summaries for parameter lifetimes | completed |
| `v0.16` | conditional borrow-return joins for multiple parameter lifetimes | completed |
| `v0.17` | lexical borrow alias locks for indexed writes | completed |
| `v0.18` | lifetime region graph foundation | completed |
| `v0.19` | non-lexical borrow endings | completed |
| `v0.20` | reborrows and two-phase mutable access | completed |
| `v0.21` | place and projection alias analysis | completed |
| `v0.22` | interprocedural lifetime contracts | completed |
| `v0.23` | borrow checker conformance suite | completed |
| `v0.24` | v1.0 release candidate hardening | completed |
| `v0.25` | loop-carried borrow liveness | completed |
| `v0.26` | conditional borrow state joins | completed |
| `v1.0` | stable language core, tooling contracts, and production-quality safety guarantees | planned |

## Required Release Check Sequence

Before a release tag, run:

```powershell
cargo fmt --check
cargo clippy -- -D warnings
cargo test
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1
fgx fuzz --iterations 128 --json
fgx diff-test --json
fgx graph roundtrip examples/smoke/inline_tests.fgx --json
fgx borrow-conformance --json
fgx release-check --target v1.0 --json
```

Use `fgx release-check --strict` only when the branch is expected to satisfy the
requested release target.
