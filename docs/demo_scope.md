# Forgentex Demo Scope

The first demo proves:

- `.fgx` and `.fgai` parse into one AST.
- The AST can be formatted back into both syntaxes.
- The checker emits machine-readable diagnostics.
- A semantic patch can insert an agent-proposed fast path.
- A tiny interpreter can execute safe scalar demo code.
- A project map can expose compact agent context.
- The C backend can act as a reference lowering path behind an explicit backend interface.
- The Zig backend can emit readable source artifacts for safe project
  experiments while compiler invocation remains deferred.
- A backend catalog and FGIR view can prepare future Cranelift, LLVM, and MLIR experiments.
- A small source-backed standard library can replace ad hoc media/DSP helper intrinsics.
- Planned tiny standard-library modules can support deterministic benchmark
  demos without turning the v0 standard library into a generic library surface.
- Deterministic mock filesystem and networking APIs can be checked, run, and
  reviewed through explicit effects.
- Deterministic mock crypto, signed patch envelopes, and agent-payment receipts
  can be checked, run, and reviewed through explicit effects.
- Agent workbench context and review packets can expose small symbol/dependency
  slices with token estimates.
- A local VS Code extension package can expose syntax highlighting,
  human/agent views, CLI diagnostics, patch review, node hovers, and
  architecture filters.
- A stdio language server can serve `.fgx`, `.fgai`, and `.fgp` with
  diagnostics, hover, symbols, formatting, semantic tokens, code actions,
  rename, definition, and target-aware diagnostics.
- A deterministic fuzz/property suite can check roundtrips, formatter
  idempotency, parser no-panic behavior, semantic patch invariants, target
  validation invariants, and interpreter-vs-C-backend agreement hooks.
- A local package manager and build flow can create packages, resolve local
  path dependencies, emit lock reports, test x86_64/AArch64/RISC-V target
  matrices, build target-specific C artifacts, and run publish dry-runs.
- A supply-chain security layer can audit package permission manifests,
  sandbox policies, mock signed package metadata, reproducible source hashes,
  and block untrusted package actions before test/build workflows.
- Deterministic performance tooling can attach correctness-gated benchmark
  evidence, allocation reports, bounds-check reports, and x86_64/ARM/RISC-V
  comparison paths to optimization reviews.
- A planned token-efficiency benchmark corpus can compare source, agent syntax,
  canonical AST, project-map, context-packet, and semantic-patch token counts
  for deterministic demo programs.
- A release-readiness report can state v1.0 maturity gates, current evidence,
  release labels, explicit gaps, and release-candidate hardening checks without
  pretending the demo is production ready.
- A borrow-checking report can expose alias-root checks, conditional/loop move
  merging, field-sensitive move paths, borrow-return provenance summaries,
  conditional borrow-return joins, lexical borrow alias locks, lifetime region
  graph evidence, simple non-lexical borrow endings, local reborrow chains,
  two-phase mutable call activation, place/projection alias analysis,
  interprocedural lifetime contracts, loop-carried borrow liveness,
  conditional borrow-state joins, filtered ownership diagnostics, and a
  built-in conformance suite for human and agent review.

Deferred:

- native compilation beyond C/Zig source transpilers
- full lifetime inference beyond v0.26 local reborrow, loop-carried liveness,
  conditional borrow-state joins, two-phase activation, projection, explicit
  contract, conformance, and RC hardening checks and
  production-grade borrow checking beyond alias liveness, local reborrow
  ancestry, field/constant-index projection rules, and explicit function
  lifetime contracts
- real assembly lowering
- real async I/O, real networking transports, real cryptography, wallet, and token APIs
- production cross-file LSP indexing and marketplace editor packaging
- long-lived agent workbench server
- remote package registry, package version solver, real package signature
  verification, artifact cache, and OS-level sandbox execution
- optimizer, real wall-clock benchmarking, CPU counters, and profile-guided
  optimization
- implemented token-efficiency benchmark corpus and dedicated `fgx token-report`
- implementation of planned tiny libraries beyond the current stable v0 catalog
- production-grade v1.0 safety guarantees
- Zig compiler invocation and `build.zig` emission
- Cranelift, LLVM, and MLIR code generation
- generic standard library
- v2 foreign-language capsules, including embedded Python 3.13, JavaScript,
  TypeScript, Java, C#, C, C++, CUDA, Go, Bash, PowerShell, Solidity, and Vyper
  blocks, package-manager dependencies, pinned GitHub projects, and sandboxed
  capsule execution
