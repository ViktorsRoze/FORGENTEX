# Forgentex Agent Tooling Superiority Plan

Reviewed: 2026-05-29

This plan turns the Zero competitive review into a concrete Forgentex roadmap.
It starts after the current Stage 27 borrow-checker work and focuses on
features that are realistic to implement while making Forgentex stronger than a
compiler that only exposes structured agent commands.

Zero has a strong agent-facing tool surface: `check --json`, `graph`, `size`,
`mem`, `explain`, `fix --plan`, `skills`, inline tests, target facts, and direct
build artifacts. Forgentex should match the useful parts, but make them more
powerful through its own identity:

```text
one AST
two source views: .fgx for humans and .fgai for agents
semantic patches: .fgp
stable node IDs
target-aware safety review
human approval reports
architecture facts for x86_64, ARM/AArch64, and RISC-V
```

The main product idea is the Forgentex Trust Loop:

```text
inspect -> propose -> simulate -> review -> apply -> verify -> sign
```

An agent should be able to inspect a small slice of code, propose a semantic
patch, see typed diagnostics and repair plans, run tests and target checks,
compare size/memory/performance effects, and produce a human-readable review
packet before anything is accepted.

## Source Baseline

The plan is based on the current Forgentex repo and these public Zero surfaces:

- <https://github.com/vercel-labs/zerolang>
- <https://zerolang.ai/>
- <https://zerolang.ai/cli>
- <https://zerolang.ai/diagnostics>
- <https://zerolang.ai/testing>
- <https://zerolang.ai/optimization>
- <https://zerolang.ai/cross-compilation>
- <https://zerolang.ai/reference>

## Design Rules

- Every new feature must have a human `.fgx` form, agent `.fgai` form when it is
  language syntax, AST representation, formatter behavior, diagnostics, tests,
  and review output.
- JSON schemas must be versioned and stable enough for agents.
- Human output must remain short and readable. JSON output carries full detail.
- Repair features should emit `.fgp` semantic patches first. Automatic file edits
  should come later and require explicit approval.
- Target checks must include x86_64, ARMv7 where useful, AArch64, riscv32, and
  riscv64/riscv64gc.
- Risky behavior must be visible in review reports: unsafe, assembly, ABI,
  allocation, filesystem, network, crypto, target-specific code, and performance
  regressions.
- The compiler should be the source of truth. Agent instructions, repair plans,
  graph facts, and release reports should be generated from the same internal
  rules instead of copied into disconnected docs.

## Feature Matrix

| Area | Zero-style feature | Forgentex stronger version |
| --- | --- | --- |
| Agent skills | `zero skills get` | Versioned `fgx skills` generated from docs and compiler facts, with role, target, and patch-specific packs |
| Diagnostics | stable codes, `explain` | Stable codes plus patchable node IDs, `.fgp` repair candidates, target-aware consequences |
| Repair plans | `fix --plan` | Multi-candidate semantic patch plans with safety labels, preconditions, review impact, and rollback hints |
| Inline tests | `test`, xfail | Inline executable specs that can be attached to patches, context packets, target matrices, and review reports |
| Graph | ProgramGraph | `.fggraph` containing AST, symbol graph, effects, borrow facts, targets, dual source views, and patch history |
| Size/memory | JSON size and memory reports | Patch delta reports with target, ABI, SIMD, allocation, and release-budget consequences |
| Target facts | cross-target checks | Target facts tied to unsafe/ASM/SIMD/ABI review across x86_64, ARM/AArch64, and RISC-V |
| Capability object | `World` | `RuntimeWorld` connected to effects, security policy, tests, sandboxing, and patch review |
| Build artifacts | direct native output | Auditable C/native output with provenance, review packets, target readiness, and reproducibility gates |

## Stage 28: Bundled Agent Skills v1

Goal: make the compiler teach agents how to use this exact Forgentex version.

Commands:

```text
fgx skills list [--json]
fgx skills get language [--json|--full]
fgx skills get diagnostics [--json|--full]
fgx skills get stdlib [--json|--full]
fgx skills get patches [--json|--full]
fgx skills get architecture [--json|--full]
fgx skills get security [--json|--full]
fgx skills path <name>
```

Implementation:

- Add `src/skills.rs`.
- Embed short skill packs with `include_str!` or generate them from docs at build
  time.
- Add `docs/skills/` with source markdown:
  - `language.md`
  - `diagnostics.md`
  - `stdlib.md`
  - `patches.md`
  - `architecture.md`
  - `security.md`
- Add a `forgentex.skills.v1` JSON schema.
- Include compiler version, stage number, skill name, summary, source doc hashes,
  supported commands, and "do not do this" safety notes.

Better than Zero:

- Skills are not just documentation. Each skill should include the exact command
  contracts, schema names, current target list, diagnostic families, and patch
  operations supported by this compiler.
- Add `recommended_context_budget` so agents know whether to load the short or
  full skill.
- Add `related_review_gates` so dangerous skills point to required review
  commands.

Acceptance gates:

- `fgx skills list --json` is stable and snapshot tested.
- Every skill has short text and JSON output.
- `fgx release-check` verifies all skill source hashes.

## Stage 29: Diagnostic Catalog, Explain, and Repair Plans

Goal: turn diagnostics into structured next actions.

Commands:

```text
fgx explain <diagnostic-code> [--json]
fgx diagnostics list [--json]
fgx fix --plan <file|project> [--json]
fgx fix --plan <file|project> --target <triple> [--json]
```

Implementation:

- Add `src/diagnostic_catalog.rs`.
- Normalize all diagnostic codes into families:
  - `PAR`: parser
  - `NAM`: names/imports
  - `TYP`: type system
  - `BOR`: borrow/ownership
  - `EFF`: effects/capabilities
  - `ASM`: assembly
  - `ABI`: layout and headers
  - `TAR`: target architecture
  - `SEC`: security/sandboxing
  - `PKG`: package/dependency
  - `PERF`: performance budgets
- Add a `forgentex.diagnostic_explain.v1` JSON schema.
- Add a `forgentex.fix_plan.v1` JSON schema.
- Extend diagnostics with:
  - stable code
  - rule
  - source span when available
  - node ID when available
  - expected/found
  - suggested patch kind
  - fix safety label
  - target consequences
  - review gates
  - related diagnostics

Repair plan shape:

```json
{
  "schema": "forgentex.fix_plan.v1",
  "source": "examples/broken.fgx",
  "ok": false,
  "plans": [
    {
      "id": "plan-001",
      "diagnostics_fixed": ["TYP002"],
      "safety": "local-edit",
      "confidence": "high",
      "operation": "replace_return_type",
      "patch_preview": "PATCH @function.mix_samples\nOP REPLACE_RETURN_TYPE i32\n",
      "review_gates": ["typecheck", "roundtrip"],
      "target_impact": ["x86_64", "aarch64", "riscv64"]
    }
  ]
}
```

Better than Zero:

- Repair plans should be semantic `.fgp` candidates, not text edits.
- Each candidate should state which review gates must pass before application.
- The planner should explain why a diagnostic is not safely fixable.
- Plans should be ranked by smallest API impact, then by local scope, then by
  human-review cost.

Acceptance gates:

- `fgx explain TYP001 --json` returns a stable schema.
- At least five common diagnostics produce repair plans.
- Invalid assembly outside `unsafe fn` produces a plan that requires human review
  instead of pretending it is a safe edit.

## Stage 30: Inline Tests and Executable Specs

Goal: let examples carry small tests inside Forgentex source.

Human syntax:

```forgentex
test "clamp high":
    expect clamp_i16(40000) == 32767

xfail test "asm outside unsafe":
    expect_diagnostic ASM001
```

Agent syntax:

```text
TEST "clamp high" { EXPECT EQ CALL clamp_i16 40000 32767 }
XTEST "asm outside unsafe" { EXPECT_DIAGNOSTIC ASM001 }
```

Commands:

```text
fgx test <file> [--json]
fgx test --project <path> [--target <triple>|--all-targets] [--json]
fgx test <file> --filter <name> [--json]
```

Implementation:

- Extend AST with `TestBlock`.
- Extend both parsers and formatters.
- Add a test runner over the interpreter for safe tests.
- Add diagnostic expectation tests for intentionally invalid snippets.
- Add test discovery to project loading.
- Add `forgentex.test_report.v1`.

Better than Zero:

- Inline tests can be referenced by `.fgp` patches:

  ```text
  REVIEW requires_tests=["mix scalar", "mix saturation"]
  ```

- Context packets can include only tests related to a symbol:

  ```text
  fgx context @function.mix_samples --deps tests --json
  ```

- Review reports can say exactly which tests protected a patch.
- Xfail tests should require a diagnostic code so failures do not become vague.

Acceptance gates:

- Inline tests parse, format, and roundtrip in `.fgx` and `.fgai`.
- `fgx test examples --json` reports pass, fail, xfail, and unexpected-pass.
- Patch review can list tests affected by a changed function.

## Stage 31: Forgentex Graph Artifact `.fggraph`

Goal: upgrade `.fgmap` into a deterministic, patchable graph artifact.

Commands:

```text
fgx graph dump <project|dir|file> --out build/project.fggraph
fgx graph validate build/project.fggraph [--json]
fgx graph view build/project.fggraph --to fgx|fgai
fgx graph patch build/project.fggraph patch.fgp [--json]
fgx graph diff before.fggraph after.fggraph [--json]
fgx graph roundtrip <project|dir|file> [--json]
fgx graph context build/project.fggraph @function.mix_samples --deps types,effects,tests [--json]
```

Graph content:

- schema version
- compiler version
- source file hashes
- canonical AST hashes
- symbol table
- stable node IDs
- imports/exports
- type facts
- effect facts
- borrow facts
- unsafe and assembly facts
- ABI/layout facts
- target compatibility facts
- inline test references
- patch history
- dual rendered snippets for selected nodes

Better than Zero:

- `.fggraph` should preserve the relationship between `.fgx`, `.fgai`, AST, and
  `.fgp`.
- Graph patching should use semantic node IDs instead of text ranges.
- Graph diffs should summarize human-review consequences:
  - public API changed
  - unsafe added
  - capability added
  - target support changed
  - size/memory budget changed
  - tests added/removed
- Graph artifacts should be signed or hashable for supply-chain review.

Acceptance gates:

- Dumping and validating a graph is deterministic.
- `fgx graph roundtrip examples --json` proves source -> graph -> source
  stability.
- A patch applied through a graph produces the same normalized AST as patching
  the source directly.

## Stage 32: Compile-Time Facts and Query Service

Goal: give agents compiler-owned facts instead of making them infer from source.

Commands:

```text
fgx facts <file|project> [--json]
fgx facts <file|project> --target aarch64-unknown-linux-gnu [--json]
fgx facts <file|project> --symbol @function.mix_samples [--json]
```

Initial facts:

- target pointer width
- target endianness
- target alignment rules
- ABI family
- supported CPU feature labels
- known SIMD lane support
- struct field names and types
- function signatures
- inferred effects
- required capabilities
- unsafe/assembly markers
- stdlib helper availability
- constant values that are safe to evaluate

Optional language syntax later:

```forgentex
const LANES: usize = target_simd_lanes("i16")
const HEADER_FIELDS: usize = meta_field_count(NalHeader)
```

Better than Zero:

- Facts should include `source`, `node_id`, and `review_impact`.
- Facts should say whether they are target-neutral, target-specific, inferred,
  declared, or approximated.
- Facts should be usable by patch review and release-check, not only by agents.

Acceptance gates:

- `fgx facts examples/media/audio_mix.fgx --json` includes signatures, effects,
  and target-neutral facts.
- Target-specific facts differ between x86_64, AArch64, and RISC-V where
  alignment, features, or assembly constraints differ.
- Facts are included in `.fggraph`.

## Stage 33: Size, Memory, and Budget Reports

Goal: make code growth and memory behavior visible before accepting patches.

Commands:

```text
fgx size <file|project> [--target <triple>] [--profile dev|fast|small|tiny] [--json]
fgx mem <file|project> [--target <triple>] [--json]
fgx budget <file|project> --check [--json]
```

Reports:

- estimated code size
- retained helper functions and reasons
- constant/string storage
- stack estimate per function
- static storage
- heap allocation sites
- arena/fixed-buffer usage
- bounds-check count estimates
- loop hot-path notes
- SIMD path notes
- profile budget status

Manifest additions:

```toml
[budget.dev]
max_code_bytes = 65536
max_stack_bytes_per_function = 4096
max_heap_alloc_sites = 0

[budget.tiny]
max_code_bytes = 8192
max_stack_bytes_per_function = 512
max_heap_alloc_sites = 0
```

Better than Zero:

- Every report should support before/after patch deltas:

  ```text
  fgx size examples/media/audio_mix.fgx --patch patches/add_fast_path.fgp --json
  ```

- Review output should call out budget regressions in human language.
- Size/memory facts should be target-aware and architecture-aware.
- Reports should integrate with `fgx perf-report` and `fgx release-check`.

Acceptance gates:

- `fgx size` and `fgx mem` return stable JSON for examples.
- Patch review can display size and memory deltas.
- Release-check fails or warns on configured budget violations.

## Stage 34: RuntimeWorld Capability Object

Goal: make executable authority explicit and testable.

Language direction:

```forgentex
fn main(world: RuntimeWorld) -> Result[i32, str] effects [io.write]:
    return world_stdout_write(world, "hello\n")
```

Core capabilities:

- `world_stdout_write`
- `world_stderr_write`
- `world_args`
- `world_env_get`
- `world_fs_read`
- `world_fs_write`
- `world_time_now`
- `world_random_bytes`
- `world_net_connect`

Implementation:

- Add `RuntimeWorld` to stdlib declarations.
- Tie each operation to an effect.
- Allow tests to supply mock world data.
- Extend security audit to inspect `RuntimeWorld` usage.
- Extend target checks to reject capabilities unavailable on a target.

Better than Zero:

- RuntimeWorld should be connected to `.fgp` review. If a patch adds
  `world_fs_write`, the review must highlight a new filesystem write
  capability.
- RuntimeWorld should be deterministic under tests by default.
- Capability grants should be visible in `Forgentex.toml`.
- Capability deltas should appear in graph diffs, fix plans, and release-check.

Acceptance gates:

- Safe examples can run with a mock `RuntimeWorld`.
- Unsupported target capabilities produce target diagnostics.
- Patch review highlights newly added runtime capabilities.

## Stage 35: Agent Doctor, Dev Trace, and Timing

Goal: give agents a reliable self-check and trace loop.

Commands:

```text
fgx doctor [--json]
fgx dev <file|project> --trace [--json]
fgx time <file|project> [--json]
```

Doctor checks:

- Rust/compiler build availability
- parser/checker/formatter availability
- C backend compiler availability if configured
- target matrix readiness
- stdlib source presence
- docs/skills hash consistency
- package manifest validity
- writable build directory
- LSP/editor package status

Dev trace:

- parse time
- format time
- check time
- target-check time
- graph construction time
- test time
- patch review time
- backend time
- diagnostic counts by family

Better than Zero:

- `fgx doctor` should include review-readiness, not just build-readiness.
- `fgx dev --trace` should emit the trust-loop phases and where a patch would be
  blocked.
- Timing should include context size and graph size so agents can optimize their
  own workflows.

Acceptance gates:

- `fgx doctor --json` passes in a clean checkout.
- CI captures `fgx dev --trace --json examples/project`.
- Release-check includes doctor status.

## Stage 36: Patch-Aware Agent Workbench v2

Goal: make Forgentex better than a normal compiler for multi-step agent changes.

Commands:

```text
fgx context @function.mix_samples --deps types,effects,tests,facts,budgets --json
fgx propose <diagnostic-code> --source <file> --json
fgx simulate <source> <patch.fgp> --json
fgx review <patch.fgp> --source <file> --json
fgx apply <source> <patch.fgp> --out <file>
```

Implementation:

- Extend context packets with graph facts, inline tests, facts, and budgets.
- Add `simulate` as a no-write patch preview:
  - apply patch in memory
  - format both source modes
  - run type/effect/borrow/target checks
  - run related inline tests
  - compute size/memory/perf deltas
  - emit human review summary
- Add patch provenance:
  - proposed by
  - based on graph hash
  - diagnostics fixed
  - review gates run
  - signature status

Better than Zero:

- Zero has repair plans. Forgentex should have repair plans that flow directly
  into semantic patch simulation and human review.
- Agent context should be minimal but complete for a selected node.
- Every accepted change should have a review trail that can be audited later.

Acceptance gates:

- One command can simulate a `.fgp` patch and produce a complete review packet.
- Context packets include only the requested dependency categories.
- Applying a patch verifies the graph/source hash it was based on.

## Stage 37: Native Output Roadmap With Review Gates

Goal: catch up on native artifact story without losing auditability.

Near-term:

- Keep the C backend as the reference backend.
- Add stronger generated-C review:
  - source map to node IDs
  - helper retention reasons
  - target assumptions
  - ABI/layout assumptions
  - unsafe/assembly boundaries
- Add build provenance:
  - compiler version
  - backend version
  - target triple
  - source graph hash
  - stdlib hash
  - generated artifact hash

Later:

- Add direct object or native backend experiments only after graph, repair, and
  review flows are stable.
- Consider WASM as a safer first direct artifact target before full native
  object emission.

Better than Zero:

- Generated artifacts should be auditable back to `.fgx`, `.fgai`, AST node IDs,
  graph hashes, and semantic patches.
- Native backend output should never bypass security, capability, target,
  size/memory, or unsafe review.

Acceptance gates:

- `fgx build --backend c --json` includes provenance.
- Generated C can be traced back to function node IDs.
- Release-check records backend readiness per target.

## Stage 38: Conformance and Public Maturity Gates

Goal: make the language feel serious, measurable, and hard to regress.

Work:

- Add a `conformance/` suite:
  - parser pass/fail
  - type pass/fail
  - borrow pass/fail
  - effects pass/fail
  - target pass/fail
  - patch pass/fail
  - graph roundtrip
  - inline tests
  - size/memory budgets
- Add expected JSON snapshots for all public schemas.
- Add a public compatibility table.
- Add a feature maturity table:
  - experimental
  - preview
  - stable
  - deprecated
- Add release gates for every trust-loop phase.

Better than Zero:

- Conformance should include human-review outputs, not only compiler pass/fail.
- Compatibility should cover `.fgx`, `.fgai`, `.fgp`, `.fgmap`, `.fggraph`, JSON
  schemas, and review packet formats.

Acceptance gates:

- `cargo test` covers every public command.
- `fgx release-check --json` reports all Stage 28-38 evidence.
- A clean checkout can reproduce the complete trust-loop demo.

## Recommended Implementation Order

The fastest useful path is:

1. Stage 28: `fgx skills`
2. Stage 29: `fgx explain` and `fgx fix --plan`
3. Stage 30: inline tests
4. Stage 31: `.fggraph`
5. Stage 32: facts
6. Stage 33: size/memory budgets
7. Stage 34: RuntimeWorld
8. Stage 35: doctor/dev/time
9. Stage 36: patch-aware workbench v2
10. Stage 37: native-output provenance
11. Stage 38: conformance and maturity gates

This order is deliberate. Skills and diagnostics make agents immediately better.
Repair plans connect directly to existing `.fgp` patches. Inline tests and graph
artifacts give agents safe feedback. Facts, budgets, and RuntimeWorld deepen the
language model. Doctor, simulation, provenance, and conformance make the whole
toolchain trustworthy.

## First Four Implementation Tickets

These are the smallest high-value tasks to start:

1. Add `src/skills.rs`, `docs/skills/*.md`, and `fgx skills list|get`.
2. Add `src/diagnostic_catalog.rs` and `fgx explain <code>`.
3. Add a first `fgx fix --plan --json` that maps known diagnostics to `.fgp`
   patch candidates without editing files.
4. Add parser support for `test` and `xfail test` blocks, even before all test
   runner features are complete.

## Strategic Position

Forgentex should not try to be "Zero, but with another syntax." The stronger
claim is:

> Forgentex is the review-first systems language for agents. It gives agents
> structured ways to inspect, patch, test, and optimize code, while giving humans
> deterministic reports about what changed, which capabilities were added, which
> architectures are affected, and why a change is safe enough to accept.

That is a better lane for low-level, security-sensitive, media/DSP,
architecture-aware, and agent-generated systems code.
