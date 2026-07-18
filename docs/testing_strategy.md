# Forgentex Testing Strategy

This document defines the required test coverage for the v0 demo and future
language stages. Every language feature must be tested across the full Forgentex
workflow, not only in one parser or CLI command.

## Required Checks

Run these before treating a change as stable:

```powershell
cargo fmt --check
cargo clippy -- -D warnings
cargo test
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1
```

If the current shell does not see Cargo on `PATH`, use:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
```

## Test Areas

Parser tests:

- `.fgx` success cases for structs, functions, control flow, assignments, and assembly.
- `.fgai` success cases for compact declarations, nested blocks, and assembly.
- Module metadata parses without changing single-file AST behavior.
- `.fgp` semantic patch success cases.
- Common syntax mistakes produce stable, useful errors.
- Lexer tokens include source spans.
- Source maps include node spans for declarations, statements, expressions, and assembly.
- Comments, doc comments, and review annotations are preserved in source maps.

Formatter tests:

- `.fgx` formatting is deterministic.
- `.fgai` formatting is deterministic.
- Formatting is idempotent.
- Formatting preserves AST equality after parsing again.

Type-checker tests:

- Valid examples produce no diagnostics.
- Invalid examples produce stable diagnostic codes.
- Diagnostics include node ID, message, expected/found type when useful, and suggested patches.
- Type syntax covers primitives, named types, generics, arrays, slices, and pointers.
- Literal checks cover integer width/range, unsigned negative values, and float-to-integer rejection.
- Calls, constructors, arithmetic, field access, indexing, local bindings, and unsafe assembly each have negative tests.

Ownership and borrow-checker tests:

- Owned structs, arrays, and named generics cannot be used after move.
- Mutable borrow call arguments cannot alias the same root value.
- Local borrow aliases preserve their original root for call alias checks.
- Moves inside `if` and `for` bodies are conservatively visible to later reads.
- Field-sensitive moves preserve unmoved sibling fields, reject repeated field
  moves, reject whole-struct use after a partial move, and allow explicit field
  reinitialization.
- Borrow-return provenance accepts direct parameter returns, local aliases of
  parameter returns, and call-through helpers whose returned borrow parameter is
  provable.
- Conditional borrow-return joins accept helpers that can return one of several
  borrow parameters only when every possible returned argument is
  parameter-origin at the caller.
- Live local borrow aliases block indexed writes through another handle to the
  same root until the alias is reassigned.
- Borrow reports include v0.18 lifetime region graph evidence with roots,
  parameter/local loans, and use sites for indexed access, calls, returns, and
  assignments.
- Simple v0.19 non-lexical borrow endings allow indexed writes after a local
  alias's last use while still rejecting same-statement and later alias uses.
- v0.20 reborrow tests cover child-before-parent writes, parent/root writes
  while a child has a future use, same-statement parent/child uses, sibling
  reborrow conflicts, shared reborrows from mutable slices, and mutable call
  activation around helper calls.
- v0.21 projection alias tests cover disjoint mutable borrow fields, overlapping
  field aliases, and call arguments that use the same place-overlap rule.
- v0.22 interprocedural lifetime contract tests cover read-only `&mut [T]`
  helpers, mutating helpers, and wrappers that propagate mutating contracts.
- v0.23 borrow conformance tests cover a deterministic built-in suite for
  moves, borrow aliases, reborrows, lifetime returns, raw pointer gates,
  projection aliasing, and lifetime contract summaries.
- v0.25 loop-carried borrow liveness tests reject aliases created before a loop
  when they are read across the loop back-edge and allow aliases declared inside
  the loop body to end before a later root write in the same iteration.
- v0.26 conditional borrow-state join tests preserve possible alias roots after
  branch reassignment, reject later writes and calls that may alias, and still
  allow root writes after the joined alias's last use.
- Immutable slices cannot be written through.
- Reference returns must be tied to a parameter lifetime in v0.
- Raw pointer dereference, raw pointer arguments, and raw pointer returns require `unsafe fn`.
- Raw pointer parameters in safe functions report `raw_pointer_requires_unsafe`
  even when the pointer is not dereferenced.
- `fgx borrow-report <file> --json` emits `forgentex.borrow_report.v1` with
  filtered ownership and borrow diagnostics.
- `fgx borrow-conformance --json` emits
  `forgentex.borrow_conformance.v1` with all built-in cases passing.

Effect tests:

- Function effect declarations parse, format, roundtrip, and appear in canonical AST.
- Calls to effectful functions require matching caller declarations.
- Effectful named functions and closures type as
  `Fn[P…, R, effects[E…]]`; indirect calls propagate that annotated set.
- Pure/wider `Fn` contracts enforce effect-subset assignment, and unknown,
  duplicate, or misplaced effect markers produce stable diagnostics.
- Builtin capability functions infer effects such as `io.read`, `fs.read`, `network.write`, `crypto.sign`, and `alloc`.
- `memory.raw` and `cpu.asm` require an unsafe boundary.
- `fgx effects <file> --json` has a stable schema.
- Semantic patch review reports include effect/capability changes, including
  filesystem and network-specific flags.

Target tests:

- `fgx targets` and `fgx targets --json` list every Stage 11 triple, family, and feature set.
- AST JSON and project maps expose target metadata for effects and structured assembly.
- `fgx check <file> --target <triple> --json` reports unknown triples, unknown features, architecture mismatches, incompatible feature use, and missing feature guards.
- Guarded target-specific calls using `cpu_has_<feature>()` pass when the selected target family supports that feature.
- Semantic patch review reports include architecture path notes and scalar fallback behavior.

Assembly tests:

- `fgx asm-check <file> --target <triple>` passes for matching x86_64, AArch64, and RISC-V examples.
- Assembly outside `unsafe fn` fails.
- Invalid registers and clobbers fail for the selected target.
- Feature-specific instructions require matching `cpu.<feature>` effects unless the target provides the feature by default.
- Assembly review summaries include changed registers, clobbers, required features, memory effects, and ABI risk.

SIMD tests:

- `vec[T,N]` and `mask[N]` parse, format, roundtrip, and type-check.
- Intrinsic diagnostics cover lane/type mismatches, arity mismatches, vector requirements, and mutable store destinations.
- `fgx check <file> --target <triple> --json` enforces scalar fallback requirements for SIMD functions.
- `effects [cpu.simd, target.only]` intentionally bypasses the fallback requirement and appears in review output.
- `fgx simd-review <file> --target <triple>` covers AArch64 and RISC-V target plans, scalar fallback status, and equivalence-test status.
- Interpreter tests execute portable SIMD arithmetic, shuffle, reductions, load, and store using lane-array semantics.

ABI/layout tests:

- `@repr(C)`, `@packed`, `@aligned(N)`, field `@endian(...)`, and `@extern(C)` parse, format, roundtrip, and appear in canonical annotations.
- `padding[N]` reports explicit byte padding and emits as `uint8_t name[N]` in headers.
- `fgx abi <file> --target <triple>` covers x86_64, ARMv7, AArch64, and RISC-V pointer/calling-convention metadata.
- The same struct with pointer-sized fields reports different sizes on 32-bit and 64-bit targets.
- ABI diagnostics cover missing `@repr(C)`, invalid alignment, invalid endian, endian on non-integer fields, recursive structs, unknown types, and unsupported generic types.
- `fgx header <file> --target <triple>` emits target macros, alignment macros, packed pragmas, struct typedefs, and C ABI function prototypes.

Patch tests:

- Semantic patches apply to AST nodes, not raw text.
- Patch review reports include API, unsafe, assembly, ABI/layout, allocation,
  filesystem, network, effect, architecture, and fallback details.
- Patches are rejected when targets are missing.
- Patch protocol v1 envelopes validate node existence and node hash preconditions.
- Check-only mode and JSON review output are covered by CLI tests.

Interpreter tests:

- Safe demo functions run with deterministic input/output.
- Unsafe or assembly examples are review-only and cannot execute.
- Entrypoint execution accepts JSON input and emits stable JSON output.
- JSON golden expectations validate returns and mutable binding updates.
- Mutable slice updates propagate through nested safe calls.
- Struct, Option, and Result runtime values execute in safe code.
- String values execute in safe code and can flow through JSON inputs, JSON
  outputs, and Result values.
- Invalid indexes, division by zero, and expectation mismatches produce deterministic traps.

I/O and networking tests:

- `examples/io/file_copy.fgx` checks and runs through deterministic filesystem mocks.
- `examples/network/agent_ping.fgx` checks and runs through deterministic network mocks.
- Missing `fs.*` and `network.*` declarations produce `undeclared_effect`.
- Patch review text and JSON flag added filesystem and network access.

Crypto and agent-economy tests:

- `examples/agent/signed_patch.fgx` checks, reports `crypto.*` and
  `agent.patch` effects, and executes signing through deterministic mock keys.
- `examples/agent/payment_receipt.fgx` checks, reports `ledger.mock` and
  `agent.payment` effects, and executes receipt creation through a mock ledger.
- Safe code cannot log, serialize/export, or copy `SecretKey[...]` values
  without `secret_leak` diagnostics.
- Signed `.fgp` envelopes are verified before semantic patch operations apply.

Project-map tests:

- `.fgx` and `.fgai` views of the same source are deduplicated.
- Maps include modules, structs, functions, node IDs, unsafe markers, assembly markers, and token estimates.
- Project maps include manifest package data, imports, exports, hashes, and token estimates.
- Symbol lookup returns useful agent context.

Package manager and build-system tests:

- `fgx new <path> --name <package>` creates `Forgentex.toml` and `src/main.fgx`.
- Local path dependencies load into project graphs and lock reports.
- `fgx lock <project> --json` emits `forgentex.lock.v1` with deterministic
  package order, manifest checksums, dependency names, and target matrices.
- `fgx test --project <path> --all-targets --json` checks x86_64, AArch64, and
  RISC-V targets for root packages and local path dependencies.
- `fgx build <project> --backend c --all-targets --out <dir> --json` writes
  target-specific artifacts and FIR receipt metadata under
  `<out>/<profile>/<target>`.
- `fgx publish <project> --dry-run --json` emits package readiness, source file
  lists, lock graph size, package verification status, and deferred registry
  upload checks.

Security and supply-chain tests:

- `fgx audit <project> --json` emits `forgentex.audit.v1` with package trust,
  permissions, sandbox policy, actions, mock signature status, reproducible
  hash, and issue severities.
- `fgx verify <project> --json` emits `forgentex.verify.v1` with package
  signature status and reproducible metadata checks.
- Signed untrusted local path packages pass audit and verify when their
  manifest permissions and sandbox labels are declared.
- Untrusted packages with `build_script`, `codegen`, `test_runner`, or
  `agent_tool` entries fail audit when the matching permission is missing.
- Untrusted packages with `fs.*`, `network.*`, `crypto.*`, `io.*`, `agent.*`,
  or `ledger.*` effects fail audit when the matching permission category is
  missing.
- `fgx test` and matrix-aware `fgx build` fail before package action when the
  supply-chain audit has high-severity issues.

Performance tooling tests:

- `fgx bench <file> --entry <function> --json-input <file> --target <triple> --json`
  emits `forgentex.bench.v1` with correctness, target path, deterministic
  estimates, operation counts, allocation reports, and bounds-check reports.
- `fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> --json`
  emits `forgentex.perf_report.v1` with scalar-vs-optimized comparisons for
  x86_64, AArch64, and RISC-V.
- Unknown benchmark target triples fail with stable diagnostics.
- Performance patch gates require correctness to pass before benchmark evidence
  is accepted.
- Planned token-efficiency benchmarks should record stable source, agent syntax,
  AST, project-map, context-packet, patch-token, and verified-behavior counts.
  See `docs/token_efficiency_benchmarks.md`.
- The butterfly ray-grid benchmark should declare its planned small-library
  dependencies: `core.fixed`, `core.stats`, `core.geom`, `core.grid`,
  `core.hash`, and `core.rng`.

Release criteria tests:

- `fgx release-check --json` emits `forgentex.release_check.v1` with target
  release label, current track, decision, maturity gates, release labels,
  evidence, explicit gaps, hardening checks, and hardening pass/fail counts.
- `fgx release-check` text output lists the same gate decisions for human
  release review.
- `fgx release-check --strict` fails when the requested release target is not
  ready.
- v0.24 hardening checks cover strict release gating, gate evidence
  completeness, diagnostic shape freeze, borrow escape-hatch conformance, and
  target decision consistency.
- v0.25 release metadata covers loop-carried borrow liveness evidence and keeps
  `hardening_fail=0`.
- v0.26 release metadata covers conditional borrow-state join evidence and
  keeps `hardening_fail=0`.
- The default v1.0 report is `v1_ready`: the ownership gate passes with the
  CFG dataflow borrow checker, and `fgx release-check --target v1.0 --strict`
  exits zero.

Agent workbench tests:

- `fgx context <symbol> --deps types,effects,tests --json` emits
  `forgentex.agent_context.v1`.
- Context packets include symbol source, compact map lines, dependency slices,
  diagnostics, unsafe/assembly summaries, and token estimates.
- `fgx review <patch> --json` emits `forgentex.agent_review.v1`.
- Review packets auto-locate patch targets from the manifest project, run
  check-only semantic patch previews, and embed `forgentex.patch_review.v1`.

VS Code extension tests:

- Extension manifest declares `.fgx`, `.fgai`, `.fgp`, and `.fgmap` languages.
- TextMate grammars cover semantic node IDs, unsafe assembly, patch
  operations, and x86_64, ARM/AArch64, and RISC-V architecture labels.
- Extension commands bridge to `fgx fmt`, `fgx context`, `fgx review`,
  `fgx patch`, and `fgx check`.
- Patch review WebView, hover provider, document symbol provider, diagnostics,
  and architecture filter hooks remain present.

Language server tests:

- `forgentex-lsp` capabilities advertise diagnostics, hover, definition,
  rename, formatting, document symbols, semantic tokens, code actions, node-ID
  navigation, semantic patch actions, and target-aware diagnostics.
- LSP open/change diagnostics serve `.fgx`, `.fgai`, and `.fgp` documents.
- Hover, document symbols, definition, rename, formatting, and semantic tokens
  work on `.fgx` documents.
- `.fgp` patch documents expose semantic target hovers, document symbols, and
  semantic patch code actions.
- Target-aware diagnostics report architecture mismatches for x86_64,
  ARM/AArch64, and RISC-V-targeted code paths.

Stage 22 fuzz/property tests:

- `fgx fuzz --iterations <n> --json` emits `forgentex.fuzz.v1`.
- Generated programs satisfy `.fgx -> AST -> .fgai -> AST` and
  `.fgai -> AST -> .fgx -> AST` canonical equality.
- Human and agent formatters remain idempotent over generated and corpus inputs.
- Mutated parser inputs and malformed patch inputs do not panic.
- Semantic patches satisfy `patch -> check -> format -> parse -> check`.
- Target invariants exercise ABI and assembly validation across x86_64,
  ARM/AArch64, and RISC-V.
- `fgx diff-test --json` emits `forgentex.backend_differential.v1` and links
  interpreter golden execution, generated C output, and FGIR symbol presence.

Project tests:

- Manifest-backed projects load from `Forgentex.toml`.
- Multi-module examples check as one program.
- Public API summaries include exported symbols only.
- Missing imports and import cycles produce stable diagnostics.

C backend tests:

- Manifest-backed safe projects generate deterministic C header and source output.
- Generated headers include fixed-width includes, slice typedefs, public structs, and public function prototypes.
- Generated sources include runtime trap calls, bounds checks, and safe function definitions.
- Backend output is written to the requested directory.
- Unsafe functions, structured assembly, and runtime-only effects are rejected by the C backend.
- `cpu_has_<feature>()` probes lower conservatively while target-specific lowering remains future work.

Backend strategy tests:

- `fgx backends` and `fgx backends --json` list the available C and Zig backends and planned Cranelift, LLVM, and MLIR backends.
- `fgx ir <project|file>` emits deterministic human-readable FGIR with module, enum, struct, function, effect, target, and review-only assembly metadata.
- `fgx ir <project|file> --json` emits the `forgentex.backend_ir.v1` schema.
- `fgx ir <project|file> --contract-json` emits the frontend contract used to prove backend choice does not alter AST/check/effects semantics.
- `fgx run <file> --entry <function> --backend interp`, including `--live`,
  preflights the selected entry against its FIR backend contract and executes
  FIR-backed interpreter bodies.
- `fgx run <file>` default demo execution generates FGIR and runs the demo
  entry from the FIR body.
- `fgx io-run` and `fgx net-run` preserve their gated runtime transcripts while
  executing helper entries from generated FIR.
- Direct interpreter helper tests exercise in-memory FGIR lowering by default,
  including JSON entry runs, live capsule runs, and gated I/O/network hosts.
- `fgx build <project> --backend zig` writes deterministic Zig source while `--backend c` remains the reference build path.
- `fgx build <project> --backend c|zig` writes `forgentex.fgir.json` and
  `forgentex.backend_build.json` metadata for successful source builds.
- FIR backend contracts report source-backend blockers for runtime-only
  effects, generic runtime types, interpreter-only intrinsics, and foreign
  capsule calls before C/Zig source emission.
- C/Zig build preflight also reports FIR declaration blockers such as generic
  struct fields before source/header emission.
- Eligible private generic functions with fully concrete direct calls are
  deterministically monomorphized before C/Zig FIR preflight; nested calls,
  literal context, raw/source/optimized entry paths, and retained blockers for
  exported or uncalled templates are covered in `tests/fn_values.rs`.
- `T: Eq` bounds round-trip in `.fgx`, `.fgai`, and canonical AST data; generic
  equality requires the bound and concrete call sites must satisfy it.
- Bounds propagate through generic wrappers and eligible private bounded helpers
  still monomorphize. Generic function values preserve ordered parameters and
  bounds in `Fn[forall[...], ...]`; tests cover dual-syntax round-trip,
  alpha-renamed higher-order assignment, bound narrowing/rejection, fresh
  per-call inference, interpreter execution, malformed binder diagnostics, and
  retained C/Zig `generic_type:Fn` blockers.
- User marker traits and explicit impls round-trip in both syntaxes, canonical
  AST, source maps, graph/project/workbench summaries, and enforce every bound.
- Reserved/unknown/duplicate trait declarations and impls have stable
  diagnostics; marker metadata erases before eligible C/Zig specialization.
- Required trait methods round-trip and impl mappings are checked for
  completeness, uniqueness, backing-function existence, purity, and exact
  `Self`-substituted signatures.
- Trait-qualified calls round-trip and execute through concrete and bounded
  generic interpreter paths; missing bounds/impls and unknown methods have
  stable diagnostics. Unique concrete calls, including calls inside generated
  generic clones, rewrite to backing functions for C/Zig; ambiguous calls retain
  `op:trait_dispatch` before emission.
- Associated constants round-trip as contracts, impl values, and canonical
  `trait_const` expressions; tests enforce `Self` substitution, completeness,
  uniqueness, literal constness, interpreter evaluation, and C/Zig inlining.
- Associated types round-trip as required names and concrete impl bindings;
  tests enforce declaration, completeness, uniqueness, `Self::Item` resolution
  in method/constant contracts, interpreter execution, and C/Zig specialization.
- Generic/blanket impls round-trip with bounds; tests cover recursive structural
  matching, bound rejection, unconstrained parameters, conservative coherence,
  symbolic method/constant/associated-type contracts, interpreter selection,
  and C/Zig associated-item resolution plus backing-function specialization.
- Standalone `Trait::Item[Type]` projections round-trip in both syntaxes; tests
  cover generic-bound preservation, nested `Self::Item` exposure, concrete
  normalization, unknown trait/item/missing-impl diagnostics, interpreter
  execution, and projection-free C/Zig specialization.
- Default trait methods round-trip with pure backing functions; tests cover
  missing/impure/mismatched defaults, concrete overrides, interpreter fallback,
  impl-selected numeric specialization, and C/Zig generic-default lowering.
- The allocation-free borrowed String subset (`str_len`, `.len`,
  `str_contains`, `==`, `!=`) is generated for C/Zig with a sub-16-MiB bound;
  host-gated C compilation/execution proves UTF-8 byte length, empty-needle,
  and value-equality behavior. Owned String operations remain FIR-blocked.
- Generated C and Zig source artifacts include FIR declaration/signature markers
  and keep public/private declarations aligned with `forgentex.backend_ir.v1`.
- Generated C and Zig source include FIR body markers for straight-line
  functions emitted from entry-block FIR instructions.
- `fgx build <project> --backend llvm` and other planned native backends fail explicitly.
- Unknown backend names produce stable diagnostics.

Zig backend tests:

- Manifest-backed safe projects generate deterministic Zig source output.
- Generated source includes trap and bounds-check helpers.
- Safe exported structs and functions are lowered from manifest projects.
- Backend output is written to the requested directory.
- Unsafe functions, structured assembly, unsupported generic runtime values,
  and runtime-only effects are rejected by the Zig backend.

v2 foreign capsule tests:

- Foreign-language capsules parse into stable AST nodes with language, version,
  runtime, target environment, dependency, effect, sandbox, and typed-boundary
  metadata.
- The foreign-language catalog snapshot includes Python, JavaScript,
  TypeScript, Java, C#, C, C++, CUDA, Go, Bash, PowerShell, Solidity, and
  Vyper.
- Python 3.13 capsule examples reject missing runtime versions, unpinned
  dependencies, floating GitHub branches, undeclared effects, missing sandbox
  policy, and type mismatches at the Forgentex boundary.
- JavaScript, TypeScript, Java, C#, and Go capsule examples require exact
  toolchain/runtime versions, package lock evidence, target environments, and
  typed Forgentex boundaries.
- C, C++, and CUDA capsule examples require native/unsafe classification,
  compiler or toolkit versions, target triples or GPU targets, ABI/memory
  boundary review, and deterministic fixtures or fallbacks.
- Bash and PowerShell capsule examples require command allowlists,
  environment-variable review, host-path limits, and strict sandbox policies.
- Solidity and Vyper capsule examples require exact compiler versions, EVM
  targets, ABI hashes, gas evidence, chain/network effects, and secret-key
  safety gates.
- Capsule review reports identify changed foreign code, changed dependencies,
  target environments, effects, sandbox permissions, and caller functions.
- Package audit reports include capsule dependency pins, checksums, source
  repositories, and permission risks.
- Native backends reject, stub, or explicitly bridge foreign capsules with
  stable diagnostics instead of silently lowering them.

Standard library tests:

- `fgx stdlib` and `fgx stdlib --json` list every Stage 16/17/18 core module.
- `stdlib/Forgentex.toml` type-checks as a source package.
- Manifest projects resolve imported `core.*` modules automatically.
- Media/DSP examples use `core.math`, `core.slice`, `core.bits`, `core.simd`, and `core.target` APIs.
- Checked arithmetic and bit helpers have interpreter conformance tests.
- Target feature guard aliases under `core.target` are accepted by target checks.
- `core.fs`, `core.net`, `core.serialize`, and `core.stream` APIs have effect
  and interpreter conformance tests.
- `core.crypto` and `core.agent` APIs have effect, interpreter, and secret
  safety tests.
- Planned tiny modules such as `core.bytes`, `core.fixed`, `core.stats`,
  `core.geom`, `core.grid`, `core.hash`, `core.rng`, `core.text`,
  `core.json`, `core.test`, and `core.bench` require source modules,
  `fgx stdlib --json` catalog entries, checker/interpreter coverage, and at
  least one demo or token-efficiency benchmark use case before they become
  stable.
- `core.stats` tests must cover exact counts, checked sums, explicit
  fixed-point rounding, min/max, variance, histogram behavior, empty-input
  diagnostics, and golden summary records.

CLI tests:

- Every public command has at least one success test.
- Every public command has at least one failure test.
- `fgx diagnose <file> --json` reports parser infrastructure output.
- `fgx borrow-report <file> --json` covers Stage 27 borrow-checker review
  workflows, v0.18 region graph evidence, v0.19 non-lexical borrow ending
  rules, v0.20 reborrow/two-phase activation rules, and the v0.21
  `place_projection_alias_analysis` rule, v0.22 lifetime contract evidence,
  the v0.25 `loop_carried_borrow_liveness` rule, and the v0.26
  `conditional_borrow_state_join` rule.
- `fgx borrow-conformance --json` covers v0.23 positive and negative
  conformance cases plus lifetime contract summary cases.
- v0.24 borrow conformance includes raw pointer parameter, conditional lifetime
  escape, constant-index projection, and unknown-index projection cases.
- v0.25 borrow conformance includes loop-carried alias rejection and loop-local
  alias acceptance cases.
- v0.26 borrow conformance includes conditional alias reassignment rejection,
  conditional alias call rejection, and conditional alias last-use acceptance
  cases.
- safe.1 adds `forgentex.borrow_reference.v1`: an implementation-independent
  move-availability model generates 510 owned root/field programs across linear
  flow, branch joins, and two-iteration loops. `fgx borrow-conformance` and
  strict release checks require all 510 results to match the production checker.
- Its independent alias/liveness model generates 277 cases across descendant
  loans, shallow and nested fields, branch joins, loop-carried aliases,
  constant/dynamic index pairs, and conditional target joins inside loops.
  Only differing constant indices prove disjointness; all dynamic pairs remain
  conservatively overlapping.
- Its independent lifetime lattice generates 132 parameter/unproven provenance
  cases across direct/control-flow/helper/wrapper returns, field/index
  projections, and self/mutual recursive cycles. Selective helper contracts
  prove that unreturned projection arguments do not taint a result.
- safe.1 runtime campaigns accept a bounded caller seed and case count, run 128
  cases by default, and emit minimized source plus replay metadata if a
  differential mismatch survives model-equivalent shrinking.
- `--failure-dir <path>` persists mismatch source and
  `forgentex.borrow_regression.v1` metadata; four checked-in minimal fixtures
  replay on every run. Weekly CI covers four seeds at 64 cases per family and
  retains any minimized failure artifacts.
- `fgx effects <file> --json` reports declared, inferred, missing, invalid, and capability effects.
- `fgx check --project --json`, `fgx api --public`, and `fgx map --project` cover Stage 8 project workflows.
- `fgx run <file> --entry <function> --json-input <file> --backend interp` covers Stage 9 interpreter workflows through the v3.0/v3.1 backend boundary.
- `fgx build <project> --backend c` covers Stage 10 backend workflows.
- `fgx build <project> --backend zig` covers Stage 28 backend workflows.
- `fgx targets`, `fgx check --target`, and `fgx map --include-targets` cover Stage 11 target workflows.
- `fgx asm-check <file> --target <triple>` covers Stage 12 structured assembly workflows.
- `fgx simd-review <file> --target <triple>` covers Stage 13 portable SIMD workflows.
- `fgx abi <file> --target <triple>` and `fgx header <file> --target <triple>` cover Stage 14 ABI/layout workflows.
- `fgx backends` and `fgx ir <project|file>` cover Stage 15 backend strategy workflows.
- `fgx stdlib` covers Stage 16 standard library workflows.
- `fgx check`, `fgx effects`, and `fgx run --entry` cover Stage 17 I/O and
  networking workflows.
- `fgx check`, `fgx effects`, `fgx run --entry`, and signed `fgx patch` cover
  Stage 18 crypto and agent-economy workflows.
- `fgx context` and `fgx review` cover Stage 19 agent workbench workflows.
- Static VS Code extension tests cover Stage 20 editor workflow packaging.
- `forgentex-lsp --probe` and LSP unit tests cover Stage 21 editor protocol
  workflows.
- `fgx fuzz` and `fgx diff-test` cover Stage 22 fuzz/property and backend
  differential workflows.
- `fgx new`, `fgx lock`, `fgx test`, matrix-aware `fgx build`, and
  `fgx publish --dry-run` cover Stage 23 package manager and build-system
  workflows.
- `fgx audit` and `fgx verify` cover Stage 24 security, sandboxing, and
  supply-chain workflows.
- `fgx bench` and `fgx perf-report` cover Stage 25 performance tooling
  workflows.
- `fgx release-check` covers Stage 26 mature release criteria workflows.
- `fgx graph dump`, `fgx graph validate`, `fgx graph roundtrip`,
  `fgx graph context`, `fgx graph patch`, and `fgx graph diff` cover Stage 31
  graph artifact workflows.
- Future v2 capsule commands should cover foreign-language capsule parsing,
  review, audit, and sandbox checks before any execution command is considered
  stable.
- PowerShell-sensitive symbols such as `@function.name` are documented as quoted.

## Snapshot Policy

Snapshot tests are plain string assertions in v0, without an external snapshot
framework. Stable snapshots should cover:

- AST JSON
- canonical AST JSON
- parser diagnostics JSON
- token span JSON
- source-map JSON
- `.fgx` formatted output
- `.fgai` formatted output
- diagnostics JSON
- semantic patch review text
- semantic patch review JSON
- `.fgmap` output
- public API summary output
- interpreter JSON output
- generated C header/source output
- target catalog JSON output
- target-aware diagnostics JSON
- assembly review text and `forgentex.asm_check.v1` JSON
- SIMD review text and `forgentex.simd_review.v1` JSON
- ABI report text, `forgentex.abi.v1` JSON, and generated C headers
- backend catalog text and `forgentex.backend_info.v1` JSON
- FGIR text and `forgentex.backend_ir.v1` JSON
- backend build receipt `forgentex.backend_build.v1` JSON
- `forgentex.frontend_contract.v1` JSON
- standard library catalog text and `forgentex.stdlib.v1` JSON
- agent context `forgentex.agent_context.v1` JSON
- agent review `forgentex.agent_review.v1` JSON
- VS Code extension package manifest and grammar coverage
- language server capabilities and representative JSON-RPC responses
- fuzz report `forgentex.fuzz.v1` JSON
- backend differential `forgentex.backend_differential.v1` JSON
- package lock `forgentex.lock.v1` JSON
- package test `forgentex.package_test.v1` JSON
- package build `forgentex.package_build.v1` JSON
- publish dry-run `forgentex.publish_dry_run.v1` JSON
- supply-chain audit `forgentex.audit.v1` JSON
- package verification `forgentex.verify.v1` JSON
- benchmark report `forgentex.bench.v1` JSON
- performance report `forgentex.perf_report.v1` JSON
- release check `forgentex.release_check.v1` JSON
- borrow report `forgentex.borrow_report.v1` JSON
- graph validation `forgentex.graph_validate.v1` JSON
- graph roundtrip `forgentex.graph_roundtrip.v1` JSON
- graph context `forgentex.graph_context.v1` JSON
- graph patch `forgentex.graph_patch.v1` JSON
- graph diff `forgentex.graph_diff.v1` JSON

When intentionally changing a snapshot, update the corresponding language doc or
roadmap note so the change is reviewable.

## Acceptance Rule

A new feature is not complete until it is represented in:

- parser
- AST
- formatter
- type checker or validator
- diagnostics
- tests
- human review/reporting path when relevant
