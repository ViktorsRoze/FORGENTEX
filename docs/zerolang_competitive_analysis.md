# Zero Language Competitive Analysis

Reviewed: 2026-05-29

Source:

- <https://github.com/vercel-labs/zerolang>
- <https://zerolang.ai/>
- <https://zerolang.ai/reference>
- <https://zerolang.ai/cli>
- <https://zerolang.ai/diagnostics>
- <https://zerolang.ai/testing>
- <https://zerolang.ai/optimization>
- <https://zerolang.ai/cross-compilation>

## Executive Summary

Zero is a serious agent-language competitor. It is not production-ready, and its
own docs warn that security vulnerabilities should be expected, but it has a
clear story: one regular syntax, a native compiler, broad standard library,
direct artifacts, and compiler-native structured facts for agents.

Forgentex is strongest where Zero is currently less distinctive:

- dual human/agent syntax over one AST
- semantic patch protocol as a first-class language workflow
- explicit review reports for unsafe assembly, ABI, SIMD, security, and
  performance
- architecture review for x86_64, ARM/AArch64, and RISC-V
- agent context packets and release-readiness reports

Zero is ahead, or at least louder and more coherent, in:

- bundled agent skills matched to the compiler binary
- direct native executable/object emission
- ProgramGraph artifacts with patch/roundtrip/build/check/size workflows
- typed repair plans from diagnostics
- inline test blocks with expected-fail handling
- size and memory budget reports
- a broader current standard library surface
- deterministic compile-time evaluation and static reflection facts

The best response is not to copy Zero wholesale. Forgentex should lean into a
different identity: **reviewable agent-native systems programming**, where every
agent action leaves a human-readable semantic trail.

## What Zero Has

### Positioning

Zero describes itself as an experimental programming language for agent
workflows, built for reliable autonomous software generation. Its public
positioning emphasizes token efficiency, low memory usage, fast startup, fast
builds, low runtime latency, and zero dependencies.

Zero explicitly prefers one current syntax and one formatted style before 1.0.
Its contributor notes say not to preserve legacy behavior by default when a
clearer agent-facing design wins.

### Syntax And Language Surface

Zero currently has:

- `.0` source files
- single-file programs and package manifests
- explicit `pub fn main`
- `World` capability object for runtime I/O
- explicit fallibility with `!`, named error sets, `check`, `raise`, and
  `rescue`
- `let`, `mut`, `set`, `if`, `else`, `while`, range `for`, `break`, `continue`,
  and `ret`
- records through `type`
- field defaults
- enums, choices, and exhaustive/fallback `match`
- generics with type parameters
- static value parameters for monomorphized layouts
- static interfaces erased before direct emission
- receiver-style method calls lowered to direct functions
- `defer`
- `owned<T>` cleanup
- `ref<T>` and `mutref<T>` borrows
- `Span<T>`, `MutSpan<T>`, fixed arrays, strings, `Maybe<T>`

Important design theme: Zero avoids hidden registries, vtables, exception
objects, global error state, and hidden allocation where possible.

### Compile-Time Evaluation

Zero has a bounded deterministic compile-time evaluator. It supports literal
arithmetic, comparisons, Bool logic, target facts, and typed reflection facts
such as field and enum/choice case queries. It reports compile-time facts in
`zero check --json` and `zero graph --json`.

This is a major agent affordance: an agent can ask the compiler for target and
type facts instead of guessing from source.

### Tooling And Agent Interfaces

Zero exposes many compiler-native structured commands:

- `zero tokens --json`
- `zero parse --json`
- `zero check --json`
- `zero graph --json`
- `zero graph dump|validate|view|check|size|build|patch|roundtrip`
- `zero size --json`
- `zero mem --json`
- `zero doc --json`
- `zero explain --json <code>`
- `zero fix --plan --json`
- `zero doctor --json`
- `zero time --json`
- `zero dev --json --trace`
- `zero skills list`
- `zero skills get language`
- `zero skills get diagnostics`
- `zero skills get stdlib`

The standout feature is the ProgramGraph workflow. Zero can dump a deterministic
graph artifact, validate it, view it as Zero-shaped source, patch it, check it,
size it, build it, run it, and prove roundtrip stability.

### Diagnostics And Repair

Zero diagnostics include:

- stable diagnostic codes
- spans
- expected/actual fields
- help text
- fix safety labels
- repair metadata
- related facts
- `zero explain`
- `zero fix --plan --json`

Fix safety labels include categories such as format-only, behavior-preserving,
local-edit, api-changing, and requires-human-review.

Borrow diagnostics include borrow traces with active borrows so agents can see
which live binding blocks the edit.

### Testing And Reliability

Zero includes:

- inline test blocks via `zero test`
- JSON test reports
- package test discovery
- filterable tests
- expected-fail tests
- golden output workflows
- snapshot workflows
- fuzz guidance
- crasher repro policy
- sanitizer smoke checks

### Builds, Targets, Size, And Memory

Zero builds native executables and object files through direct emitters. Its
public target list includes macOS, Linux, musl Linux, and Windows variants for
x64 and ARM64.

It separates language validity from target buildability through
`targetReadiness` fields in JSON reports.

Zero also has strong size/memory introspection:

- profile contracts: debug, fast, small, tiny
- `zero size --json`
- `zero mem --json`
- size breakdowns
- retention reasons
- helper budgets
- stack/static/heap/arena/fixed-buffer memory facts
- allocation instrumentation

### Standard Library

Zero's documented module surface is broad:

- `std.parse`
- `std.codec`
- `std.mem`
- `std.args`
- `std.math`
- `std.path`
- `std.str`
- `std.io`
- `std.fs`
- `std.json`
- `std.env`
- `std.time`
- `std.rand`
- `std.proc`
- `std.crypto`
- `std.net`
- `std.http`

This broad stdlib is a real advantage for agents because it reduces dependency
search and lets examples stay inside the language/toolchain.

## What Forgentex Already Has

Forgentex already has several differentiators:

- `.fgx` human syntax and `.fgai` agent syntax roundtripping through one AST
- canonical AST equality and stable node IDs
- semantic patch files `.fgp`
- patch review reports with public API, unsafe, ABI/layout, allocation,
  filesystem, network, effect, architecture, and signature categories
- compact project maps and agent context packets
- target architecture catalog for x86_64, ARM/AArch64, and RISC-V
- structured assembly review for x86_64, ARMv7, AArch64, RISC-V 32-bit, and
  RISC-V 64-bit
- portable SIMD review and performance reports
- ABI/layout reports and generated C headers
- C backend as a reference transpiler
- stdlib v0 source package
- deterministic mock I/O, networking, crypto, and agent-economy APIs
- package manager, local path dependencies, target matrix checks, publish
  dry-run
- security and supply-chain audit/verify reports
- LSP and VS Code package
- fuzz, differential backend checks, release-check reports
- borrow-report v1 demo

Forgentex is much more review-centric than Zero. That matters for agent code
because the dangerous step is not code generation; it is accepting a change.

## Gaps We Should Close

### 1. Bundled Agent Skills

Zero's `zero skills get language|diagnostics|stdlib` is excellent. It lets an
agent load the exact rules for the compiler binary it is using.

Forgentex should add:

```text
fgx skills list
fgx skills get language
fgx skills get diagnostics
fgx skills get stdlib
fgx skills get patches
```

Output should have text and JSON modes. These skills should be generated from
repo docs or embedded static strings so they cannot drift from the compiler.

### 2. Typed Repair Plans

Forgentex diagnostics currently include suggested patch kinds. Zero goes further
with `zero fix --plan --json`.

Forgentex should add:

```text
fgx explain <diagnostic-code> [--json]
fgx fix --plan <file> [--json]
```

`fgx fix --plan` should not edit files. It should emit candidate `.fgp`
operations, safety labels, target node IDs, expected diagnostics fixed, and
human review notes.

### 3. Program Graph Artifact

Forgentex has `.fgmap`, canonical AST, and backend IR, but not a saved,
patchable graph artifact equivalent to Zero's ProgramGraph workflow.

Forgentex should add:

```text
fgx graph dump <project> --out build/project.fggraph
fgx graph validate build/project.fggraph --json
fgx graph view build/project.fggraph --to fgx
fgx graph patch build/project.fggraph patch.fgp --json
fgx graph roundtrip <project> --json
```

This can be our answer to ProgramGraph while preserving our semantic patch
model and node IDs.

### 4. Inline Test Blocks

Zero has `zero test` over inline test blocks and expected-fail tests. Forgentex
currently has external JSON run inputs and Rust tests.

Forgentex should add small language-level tests:

```forgentex
test "clamp high":
    expect clamp_i16(40000.0) == 32767

xfail test "bad alias":
    expect_diagnostic mutable_borrow_alias
```

CLI:

```text
fgx test <file> --json
fgx test --project examples/project --json
```

### 5. Size And Memory Reports

Zero's `size` and `mem` JSON are strong agent hooks. Forgentex has performance
estimates but not memory/size budgets.

Add:

```text
fgx size <project|file> --target <triple> --json
fgx mem <project|file> --json
```

Reports should include:

- estimated code size
- helper retention reasons
- string/constant sizes
- stack/static/heap estimates
- allocation sites
- buffer ownership
- bounds-check counts
- profile budget status

### 6. Compile-Time Facts

Zero's bounded compile-time evaluator and typed reflection facts are a strong
agent feature.

Forgentex can start smaller:

```forgentex
const LANES: usize = target.lanes("i16")
const PACKET_FIELDS: usize = meta.field_count(AudioBlock)
```

CLI:

```text
fgx facts <file> --json
```

Include target pointer width, endianness, ABI, field counts, field types, known
effects, and stdlib helper availability.

### 7. Capability Object For Runtime Entry Points

Zero's `World` capability object makes I/O explicit. Forgentex already has
effects and deterministic mock runtime APIs, but entry points do not yet use a
unified capability object.

Possible Forgentex equivalent:

```forgentex
fn main(world: RuntimeWorld) -> Result[i32,str] effects [io.write]:
    return world_stdout_write(world, "hello\n")
```

This would make runtime authority more visible and easier for agents to reason
about.

## Where Forgentex Can Be Better

### Human Review As A First-Class Output

Zero is very good at structured facts. Forgentex should be better at human
approval. Every risky operation should have:

- compact JSON
- readable review text
- source/regenerated `.fgx`
- source/regenerated `.fgai`
- semantic patch trail
- safety/effect/architecture/security/performance summary

### Dual Representation

Zero chooses one syntax. Forgentex can own dual representation:

- `.fgx` for reviewers
- `.fgai` for agents
- one AST
- deterministic roundtrip
- semantic patch and graph patch on node IDs

This is a sharp differentiator. We should say it more clearly.

### Safety And Supply Chain

Zero warns that security vulnerabilities should be expected. Forgentex already
has package audit/verify, permission manifests, sandbox labels, mock signatures,
and secret-leak diagnostics. We should build this into the positioning:

> Forgentex is the agent-native language where generated systems changes are
> reviewable, auditable, and capability-scoped before they run.

### Architecture And Media/DSP

Forgentex has a stronger story around assembly review, ABI/layout, SIMD review,
and architecture-specific performance evidence. Zero has direct native emitters,
but Forgentex can be the better language for low-level review workflows.

## Recommended Roadmap Response

Priority order:

1. Add bundled `fgx skills` command.
2. Add `fgx explain` and `fgx fix --plan --json`.
3. Add inline `test` / `xfail test` blocks and `fgx test <file> --json`.
4. Add `fgx graph dump|validate|view|patch|roundtrip`.
5. Add `fgx size` and `fgx mem`.
6. Add `fgx facts --json` for target/type/compile-time facts.
7. Add `RuntimeWorld` capability-object pattern for executable entry points.
8. Later, add a native backend story beyond the C reference backend.

## Proposed Positioning

Short version:

> Forgentex is a review-first systems language for agents: one AST, two
> representations, semantic patches, deterministic diagnostics, and human
> approval reports for low-level code.

Longer version:

> Zero optimizes for direct agent productivity and native output. Forgentex
> should optimize for trustworthy agent collaboration: agents can propose,
> patch, inspect, benchmark, and explain systems code while humans can review
> exactly what changed, which capabilities were added, and which architecture
> paths are affected.

That is a defensible lane. It lets us learn from Zero without becoming a weaker
copy of it.
