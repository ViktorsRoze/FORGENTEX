# Forgentex Mature Language Development Plan

This roadmap extends the first demo into a mature systems language and agent-native
toolchain. It assumes the current v0 project already proves the seed workflow:

```text
.fgx human syntax <-> shared AST <-> .fgai agent syntax
semantic patch -> type check -> review report -> tiny interpreter
```

Architecture note: this document treats "RISC-5" as **RISC-V**, the open
instruction set architecture. The target architecture family should include
`x86_64`, ARM/AArch64, 32-bit ARM where useful, `riscv64`, and `riscv32`.

## Guiding Principles

- Preserve the core identity: agent-native, human-auditable, systems-oriented.
- Add a feature only when it exists in `.fgx`, `.fgai`, AST, formatter, checker,
  diagnostics, tests, and review reports.
- Prefer small vertical slices over large speculative subsystems.
- Keep unsafe, assembly, ABI, allocation, crypto, networking, and architecture
  behavior visible in machine-readable diagnostics and human review output.
- Make every major compiler decision reproducible through tests, snapshots, and
  canonical serialization.

## Stage 1: Stabilize the v0 Demo

Goal: turn the current demo into a dependable foundation.

Core work:

- Add `rustfmt` and `clippy` to the local toolchain and CI.
- Split parser, formatter, checker, patcher, interpreter, and CLI tests by area.
- Add snapshot tests for `.fgx`, `.fgai`, `.fgp`, `.fgmap`, AST JSON, diagnostics,
  and review reports.
- Add parser error recovery for common syntax mistakes.
- Add a formal compatibility promise for v0 examples.
- Deduplicate project-map entries when both `.fgx` and `.fgai` views exist.

Deliverables:

- `cargo test` and `cargo clippy -- -D warnings` pass.
- `scripts/demo.ps1` is the canonical smoke test.
- A `docs/testing_strategy.md` file describing required tests for each feature.

Acceptance gates:

- All current examples parse, format, roundtrip, check, map, and run or review.
- Every CLI command has at least one success test and one failure test.

## Stage 2: Formal Grammar and AST Contract

Goal: define the language in a way that agents and compiler code can trust.

Core work:

- Write formal grammar docs for `.fgx`, `.fgai`, and `.fgp`.
- Define the AST schema as versioned data, not only Rust structs.
- Add stable node ID rules for functions, structs, fields, statements, expressions,
  assembly blocks, loops, and unsafe regions.
- Add canonical AST serialization for semantic equality checks.
- Define how comments and review annotations attach to AST nodes.

Deliverables:

- `docs/grammar_human_fgx.md`
- `docs/grammar_agent_fgai.md`
- `docs/ast_schema_v1.md`
- `docs/node_id_policy.md`
- `fgx ast <file> --canonical`

Acceptance gates:

- Roundtrip equality uses canonical AST serialization.
- Node IDs are stable across formatting and unrelated edits.

## Stage 3: Parser Infrastructure v1

Goal: replace the line-oriented prototype parser with production-grade parsing.

Core work:

- Introduce a lexer with token spans, source files, and diagnostics.
- Implement real parsers for `.fgx`, `.fgai`, and `.fgp`.
- Preserve source spans for every AST node.
- Add useful parse errors with machine-readable repair hints.
- Support comments, doc comments, and review annotations.

Deliverables:

- `fgx parse <file> --emit-ast --spans`
- `fgx diagnose <file> --json`
- parser fuzz harness

Acceptance gates:

- Invalid input produces structured diagnostics instead of panics.
- Fuzzing does not crash the parser.

## Stage 4: Type System v1

Goal: make Forgentex meaningfully statically typed.

Core work:

- Add explicit type environments for modules, imports, structs, functions, and locals.
- Add integer width/range checking for literals.
- Add implicit literal typing rules, but keep variable and API types explicit.
- Add `Option[T]`, `Result[T, E]`, arrays, slices, pointers, and named generic types.
- Add struct constructor checking and field initialization rules.
- Add exhaustive diagnostics with suggested semantic patches.

Deliverables:

- `fgx check <file> --json`
- negative tests for every type rule
- diagnostics documentation with stable error codes

Acceptance gates:

- Every type error includes node ID, expected type, found type, message, and suggested patch kind.

## Stage 5: Ownership, Borrowing, and Lifetimes v0

Goal: establish the first real systems-language safety model.

Core work:

- Add owned values, shared borrows, mutable borrows, raw pointers, and moves.
- Track borrow scopes through blocks, calls, loops, and returns.
- Add diagnostics for use-after-move, aliasing mutable borrows, and escaping references.
- Keep lifetime syntax minimal at first; infer local lifetimes where possible.
- Require `unsafe` for raw pointer dereference and unchecked aliasing.

Deliverables:

- `docs/ownership_model.md`
- `docs/borrow_checker_v0.md`
- borrow-check diagnostics with repair hints

Acceptance gates:

- Safe code cannot mutate through immutable references.
- Safe code cannot use moved values.
- Raw pointer behavior is rejected outside `unsafe`.

## Stage 6: Effects and Capability Safety

Goal: make side effects visible to agents and human reviewers.

Core work:

- Add effect declarations and inference for `alloc`, `io.read`, `io.write`,
  `network.read`, `network.write`, `crypto.sign`, `memory.raw`, `cpu.simd`,
  `cpu.asm`, and target-specific effects.
- Add capability requirements at function and module boundaries.
- Include effect changes in review reports and semantic patches.
- Add checks so safe functions cannot silently gain dangerous effects.

Deliverables:

- `docs/effects_model.md`
- `fgx effects <file> --json`
- review report section for effect deltas

Acceptance gates:

- Adding network, crypto, allocation, raw memory, or assembly behavior changes the review report.

## Stage 7: Semantic Patch Protocol v1

Goal: make agent edits robust, reviewable, and conflict-aware.

Core work:

- Define a structured patch format with version, author, target nodes, preconditions,
  operations, expected diagnostics, and review metadata.
- Support add/remove/replace operations for declarations, fields, statements,
  expressions, effects, unsafe blocks, assembly blocks, and tests.
- Add patch conflict detection when source node hashes do not match.
- Add patch preview mode and patch application logs.
- Add signed patch envelope fields, but use mock keys until crypto is mature.

Deliverables:

- `docs/semantic_patch_protocol_v1.md`
- `fgx patch <source> <patch> --check-only`
- `fgx patch <source> <patch> --emit-review-json`

Acceptance gates:

- Patches fail safely when node IDs or preconditions do not match.
- Patch reports distinguish public API, unsafe, ABI, allocation, effect, and architecture changes.

## Stage 8: Module System and Package Layout

Goal: support real multi-file projects.

Core work:

- Add modules, imports, exports, visibility, and project manifests.
- Define package root layout and dependency graph rules.
- Add project-wide checking and incremental parsing.
- Add explicit public API summaries.
- Add cycle diagnostics and import-resolution hints.

Deliverables:

- `Forgentex.toml`
- `fgx check --project`
- `fgx api --public`
- `fgx map --project`

Acceptance gates:

- A multi-module media library checks as one project.
- Public API changes are detected in review reports.

## Stage 9: Interpreter v1 and Reference Semantics

Goal: use the interpreter as the executable spec for safe language semantics.

Core work:

- Support structs, arrays, slices, references, loops, branches, pattern matching,
  `Option`, `Result`, and checked arithmetic modes.
- Add deterministic runtime traps for invalid demo behavior.
- Add an interpreter test DSL for golden input/output cases.
- Keep assembly and target-specific intrinsics review-only until backend stages.

Deliverables:

- `fgx run <file> --entry <function> --json-input <file>`
- reference semantics tests

Acceptance gates:

- Core safe semantics can be validated without native code generation.

## Stage 10: C Transpiler Backend

Goal: produce simple native artifacts before building a serious backend.

Core work:

- Lower safe Forgentex subset to portable C11.
- Generate headers for public APIs.
- Map primitive types to fixed-width C types.
- Generate runtime helper functions for slices, bounds checks, and traps.
- Integrate with platform C compilers through a clear backend interface.

Deliverables:

- `fgx build <project> --backend c`
- generated `.c` and `.h` output
- C backend snapshot tests

Acceptance gates:

- The media/DSP examples compile through C and pass the same tests as the interpreter.

## Stage 11: Target Architecture Model

Goal: make CPU architecture support explicit and extensible.

Core work:

- Define canonical target triples and feature sets:
  - `x86_64-pc-windows-msvc`
  - `x86_64-unknown-linux-gnu`
  - `aarch64-apple-darwin`
  - `aarch64-unknown-linux-gnu`
  - `aarch64-pc-windows-msvc`
  - `armv7-unknown-linux-gnueabihf`
  - `riscv64gc-unknown-linux-gnu`
  - `riscv32imac-unknown-none-elf`
- Add architecture families:
  - `x86_64`
  - `arm`
  - `aarch64`
  - `riscv32`
  - `riscv64`
- Add feature detection names:
  - x86: `sse2`, `avx2`, `avx512f`, `bmi2`, `popcnt`
  - ARM 32-bit: `neon`, `vfpv3`, `thumb2`
  - AArch64: `neon`, `sve`, `sve2`, `crc`, `crypto`
  - RISC-V: `i`, `m`, `a`, `f`, `d`, `c`, `v`, `zbb`, `zba`, `zbs`, `crypto`
- Add architecture metadata to AST, project maps, diagnostics, and review reports.

Deliverables:

- `docs/target_architecture_model.md`
- `fgx targets`
- `fgx check <file> --target <triple>`
- `fgx map --include-targets`

Acceptance gates:

- Target-specific code cannot be called without compatible target guards.
- Review reports identify architecture-specific paths and fallback behavior.

## Stage 12: Structured Assembly v1 for x86_64, ARM, and RISC-V

Goal: evolve assembly from a parsed block into a typed, target-aware sublanguage.

Core work:

- Define structured assembly dialects:
  - `asm x86_64`
  - `asm armv7`
  - `asm aarch64`
  - `asm riscv32`
  - `asm riscv64`
- Validate registers by target:
  - x86_64: `rax`, `rbx`, `rcx`, `rdx`, `rsi`, `rdi`, `rsp`, `rbp`, `r8-r15`,
    `xmm0-xmm31`, flags.
  - ARMv7: `r0-r15`, `sp`, `lr`, `pc`, `s0-s31`, `d0-d31`, `q0-q15`, flags.
  - AArch64: `x0-x30`, `w0-w30`, `sp`, `v0-v31`, `p0-p15` for SVE where enabled.
  - RISC-V: `x0-x31`, ABI aliases like `zero`, `ra`, `sp`, `gp`, `tp`, `t0-t6`,
    `s0-s11`, `a0-a7`, `f0-f31`, vector registers `v0-v31` when `v` is enabled.
- Validate inputs, outputs, clobbers, target features, and unsafe requirements.
- Add assembly review summaries with changed registers, clobbers, memory effects,
  and ABI risk.
- Add target-specific examples:
  - `popcount_x86_64`
  - `popcount_aarch64`
  - `copy_neon_aarch64`
  - `add_riscv64`
  - `vector_add_riscv64_v`

Deliverables:

- `docs/assembly_model_v1.md`
- `examples/arch/x86_64_popcount.fgx`
- `examples/arch/aarch64_popcount.fgx`
- `examples/arch/riscv64_add.fgx`
- `fgx asm-check <file> --target <triple>`

Acceptance gates:

- Assembly outside `unsafe` fails.
- Invalid registers for a target fail.
- Target features are required before feature-specific instructions are accepted.

## Stage 13: Portable Intrinsics and SIMD Layer

Goal: let agents write optimized code without immediately dropping to assembly.

Core work:

- Add portable vector types such as `vec[i16, 8]`, `vec[f32, 4]`, and `mask[N]`.
- Add target-independent intrinsics for add, multiply, saturating arithmetic,
  shuffle, load/store, widening/narrowing, and reductions.
- Lower intrinsics to:
  - x86 SSE/AVX
  - ARM NEON/SVE
  - RISC-V Vector Extension
  - scalar fallback
- Add equivalence tests across scalar and vector paths.

Deliverables:

- `docs/simd_intrinsics.md`
- `examples/media/audio_mix_simd.fgx`
- `fgx check --target aarch64-unknown-linux-gnu`
- `fgx check --target riscv64gc-unknown-linux-gnu`

Acceptance gates:

- SIMD code must have a scalar fallback unless explicitly marked target-only.
- Review reports state whether scalar/vector equivalence tests exist.

## Stage 14: ABI, Layout, and FFI

Goal: support C-compatible systems boundaries safely.

Core work:

- Add `repr(C)`, `packed`, `aligned(N)`, endian-aware fields, and explicit padding.
- Add C ABI import/export declarations.
- Validate struct layout against target architecture ABI rules.
- Add target-aware pointer size, alignment, calling convention, and register ABI metadata.
- Generate C headers and ABI reports.

Deliverables:

- `docs/abi_layout_model.md`
- `fgx abi <file> --target <triple>`
- `fgx header <file> --target <triple>`

Acceptance gates:

- Public layout changes appear in review reports.
- The same type can produce different layout reports for x86_64, AArch64, ARMv7, and RISC-V.

## Stage 15: Native Backend Strategy

Goal: move from interpretation/transpilation toward real native compilation.

Core work:

- Define a backend IR between typed AST and machine backends.
- Start with C backend as reference.
- Add a Zig source backend track after C and before heavier native backends.
- Evaluate Cranelift for JIT/native experiments.
- Evaluate LLVM/MLIR for long-term optimizing backend.
- Keep target lowering modular so x86_64, ARM/AArch64, and RISC-V do not become special cases in the frontend.
- Integrate Zig-inspired speed ideas into the language plan: explicit allocator
  effects, allocation-free APIs, comptime-like specialization metadata,
  target-feature dispatch, vector lowering plans, exact ABI/layout review, and
  reviewed bounds-check policy.

Recommended sequence:

1. C transpiler backend.
2. Zig source backend for safe-subset native and cross-target experiments.
3. Cranelift or simple SSA IR backend for fast iteration.
4. LLVM backend for broad target support.
5. MLIR dialects for tensor/GPU/media experiments.

Deliverables:

- `docs/backend_strategy.md`
- `docs/zig_backend_strategy.md`
- `fgx build --backend c`
- `fgx build --backend zig`
- `fgx build --backend llvm` later

Acceptance gates:

- Backend choice does not alter frontend AST, type checking, effect checking, or patch semantics.
- Zig output agrees with interpreter and C reference behavior before it is used
  for performance claims.

## Stage 16: Standard Library v0

Goal: provide enough library surface for real demos while preserving safety.

Core work:

- Add core modules:
  - `core.mem`
  - `core.slice`
  - `core.math`
  - `core.result`
  - `core.option`
  - `core.bits`
  - `core.simd`
  - `core.target`
- Add allocation-free APIs first.
- Add explicit allocator and fixed-buffer policy surfaces before heap allocation
  becomes a standard-library default.
- Add checked arithmetic and bit manipulation primitives.
- Add target feature queries with compile-time and runtime forms.
- Plan tiny follow-on modules for deterministic demos and token-efficient agent
  workflows: `core.bytes`, `core.fixed`, `core.stats`, `core.geom`,
  `core.grid`, `core.hash`, `core.rng`, `core.text`, `core.json`,
  `core.test`, and `core.bench`.

Deliverables:

- `docs/standard_library_v0.md`
- standard library source package
- conformance tests
- small-library roadmap entries with demo and benchmark consumers

Acceptance gates:

- Media/DSP examples use standard APIs instead of ad hoc compiler magic.
- Planned tiny libraries do not become stable APIs until they have source
  modules, catalog entries, checker/interpreter coverage, and documented
  compatibility notes.

## Stage 17: I/O and Networking Runtime

Goal: support agent systems and real tooling workloads.

Core work:

- Add filesystem APIs with capability scopes.
- Add sync I/O first, then async I/O.
- Add TCP, UDP, HTTP, WebSocket, and eventually QUIC-style transports.
- Add structured serialization and streaming APIs.
- Require explicit effects for every I/O and network operation.

Deliverables:

- `docs/io_networking_model.md`
- `examples/io/file_copy.fgx`
- `examples/network/agent_ping.fgx`

Acceptance gates:

- Review reports flag added filesystem and network access.

## Stage 18: Crypto and Agent Economy Types

Goal: make cryptographic and agent-to-agent workflows safe by construction.

Core work:

- Add typed crypto primitives:
  - `Hash[Blake3]`
  - `Signature[Ed25519]`
  - `SecretKey[Ed25519]`
  - `PublicKey[Ed25519]`
  - `Nonce`
  - `Receipt`
- Add safe secret handling rules: no accidental print, log, serialize, copy, or unsafe move.
- Add signed semantic patch envelopes.
- Add typed token/payment abstractions without binding to one chain.
- Keep early demos on mock keys and mock ledgers.

Deliverables:

- `docs/crypto_agent_economy.md`
- `examples/agent/signed_patch.fgx`
- `examples/agent/payment_receipt.fgx`

Acceptance gates:

- Secret values cannot be logged or serialized without explicit unsafe/approved APIs.
- Signed patch verification is part of the patch workflow.

## Stage 19: Agent Workbench Protocol

Goal: give AI agents efficient, stable compiler-native interfaces.

Core work:

- Define commands for compact context retrieval:
  - project summaries
  - symbol maps
  - dependency slices
  - unsafe/assembly slices
  - diagnostics with suggested patches
  - review summaries
- Add token-cost estimates to maps and context retrieval.
- Add semantic patch negotiation: propose, check, review, apply, sign.
- Add machine-readable explanation packets for human review.

Deliverables:

- `docs/agent_workbench_protocol.md`
- `fgx context <symbol> --deps types,effects,tests`
- `fgx review <patch> --json`

Acceptance gates:

- An agent can modify one function using `.fgmap` plus symbol context, without reading the whole project.

## Stage 20: VS Code Extension

Goal: make the human/agent workflow usable.

Core work:

- Add syntax highlighting for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`.
- Add side-by-side human/agent source views.
- Add semantic node ID hover and navigation.
- Add diagnostics, review reports, unsafe/assembly highlights, ABI changes,
  effect changes, and patch approval UI.
- Add architecture filters for x86_64, ARM/AArch64, and RISC-V code paths.

Deliverables:

- VS Code extension package
- language server integration
- patch review WebView

Acceptance gates:

- A human can review and approve a semantic patch from the editor.
- Architecture-specific unsafe code is visually isolated and searchable.

## Stage 21: Language Server and IDE Protocol

Goal: support editor tooling beyond VS Code.

Core work:

- Implement LSP for diagnostics, hover, go-to-definition, rename, formatting,
  semantic tokens, code actions, and document symbols.
- Add node-ID-aware navigation.
- Add semantic patch code actions.
- Add target-aware diagnostics in the editor.

Deliverables:

- `forgentex-lsp`
- editor protocol docs

Acceptance gates:

- LSP can serve `.fgx`, `.fgai`, and `.fgp`.

## Stage 22: Testing, Fuzzing, and Formal Invariants

Goal: improve compiler trustworthiness.

Core work:

- Fuzz parsers, formatters, patch application, AST serialization, and type checker.
- Add roundtrip property tests:
  - `.fgx -> AST -> .fgai -> AST`
  - `.fgai -> AST -> .fgx -> AST`
  - `patch -> check -> format -> check`
- Add differential tests between interpreter and C/native backends.
- Add target-specific ABI and assembly validation tests.

Deliverables:

- fuzz harnesses
- property test suite
- backend differential test runner

Acceptance gates:

- Fuzzing is part of CI for nightly or scheduled runs.
- Backend output agrees with interpreter for safe programs.

## Stage 23: Package Manager and Build System

Goal: make Forgentex projects shareable and reproducible.

Core work:

- Add package manifests, lockfiles, dependency resolution, target matrices, and build profiles.
- Support local path dependencies first.
- Add signed package metadata later.
- Integrate target-specific build artifacts.

Deliverables:

- `fgx new`
- `fgx build`
- `fgx test`
- `fgx publish` later

Acceptance gates:

- A multi-package workspace can build and test for x86_64, AArch64, and RISC-V targets.

## Stage 24: Security, Sandboxing, and Supply Chain

Goal: make agent-generated code safer to consume.

Core work:

- Add sandbox policies for build scripts, codegen, tests, and agent tools.
- Add signed patches, signed packages, and reproducible build metadata.
- Add supply-chain review reports.
- Add permission manifests for I/O, network, crypto, and filesystem capabilities.

Deliverables:

- `docs/security_model.md`
- `fgx audit`
- `fgx verify`

Acceptance gates:

- Untrusted packages cannot run arbitrary code during check/build without declared permissions.

## Stage 25: Performance Tooling

Goal: make optimization visible and reviewable.

Core work:

- Add benchmark harnesses.
- Add compiler-generated performance reports.
- Add a token-efficiency benchmark corpus that compares source, agent syntax,
  AST, project-map, context-packet, semantic-patch, and verified-behavior token
  counts.
- Add architecture-specific benchmark comparison:
  - scalar vs x86 AVX
  - scalar vs ARM NEON/SVE
  - scalar vs RISC-V Vector
- Add allocation and bounds-check reports.
- Add backend-specific performance evidence once Zig/native generated artifacts
  can be compiled and run.
- Report debug/safe versus optimized bounds-check and trap policy explicitly.
- Add profile-guided patch suggestions later.

Deliverables:

- `fgx bench`
- `fgx perf-report`
- `docs/token_efficiency_benchmarks.md`
- benchmark examples for media/DSP kernels

Acceptance gates:

- Performance patches include correctness tests and target-specific benchmark evidence.
- The butterfly ray-grid benchmark uses the planned small libraries to show
  token-efficiency gains without hiding safety checks or deterministic behavior.

## Stage 26: Mature Release Criteria

Goal: define what "mature enough for serious use" means.

Required maturity gates:

- Stable grammar and AST schema.
- Stable semantic patch protocol.
- Project-wide type checking.
- Ownership and borrow checking for safe code.
- Effects and capability model.
- At least one native or transpiler backend suitable for production experiments.
- Architecture model for x86_64, ARM/AArch64, and RISC-V.
- Structured assembly validation for at least x86_64 and AArch64, with RISC-V in active support.
- Standard library v0.
- LSP and VS Code integration.
- CI, fuzzing, and differential backend tests.
- Documentation for language users, compiler contributors, and agent tool authors.

Release labels:

- `v0.1`: first demo, current milestone.
- `v0.2`: formal parser/formatter/checker.
- `v0.3`: semantic patches v1 and project maps.
- `v0.4`: ownership/effects prototype.
- `v0.5`: C backend and package projects.
- `v0.6`: architecture model and structured assembly for x86_64/AArch64/RISC-V.
- `v0.7`: SIMD and ABI/layout reports.
- `v0.8`: LSP and VS Code workbench.
- `v0.9`: security, fuzzing, package manager, and backend differential tests.
- `v0.10`: Zig source backend for safe project experiments.
- `v1.0`: stable language core, tooling contracts, and production-quality safety guarantees.

Deliverables:

- `docs/mature_release_criteria.md`
- `fgx release-check`
- `forgentex.release_check.v1`

Acceptance gates:

- The release check distinguishes current demo readiness from v1.0 readiness
  and lists explicit gaps for partial gates.

## Stage 27: Borrow Checker v1 Demo

Goal: begin closing the v1.0 ownership gap with more precise review tooling.

Core work:

- Preserve local borrow aliases back to their original root.
- Detect mutable alias conflicts through local aliases.
- Conservatively merge moves from `if` and `for` bodies into later reads.
- Add a focused borrow report for humans, CI, and agents.

Deliverables:

- `docs/borrow_checker_v1.md`
- `fgx borrow-report`
- `forgentex.borrow_report.v1`

Acceptance gates:

- `let alias: &mut [T] = xs; fill(xs, alias)` reports
  `mutable_borrow_alias`.
- Moving an owned value inside an `if` or `for` body and reading it afterward
  reports `use_after_move`.
- `fgx borrow-report --json` lists Stage 27 rules and filtered diagnostics.

## Stage 28: Zig Source Backend v0

Goal: add the first concrete Zig backend slice without adding external
toolchain dependencies.

Core work:

- Generate deterministic `.zig` source for the same safe project subset as the
  C backend.
- Keep C as the reference transpiler while Zig acts as a native-oriented source
  backend.
- Reject unsafe functions, structured assembly, unsupported generic runtime
  values, and runtime-only effects in the Zig backend.
- Expose Zig as an available backend in `fgx backends`.
- Keep Zig compiler invocation and `build.zig` generation deferred.

Deliverables:

- `docs/zig_backend_strategy.md`
- `fgx build <project> --backend zig`
- Zig backend source snapshot tests

Acceptance gates:

- `fgx build . --backend zig` writes a generated `.zig` artifact.
- Generated Zig source preserves traps and bounds checks for safe/debug
  behavior.
- Zig backend choice does not alter frontend AST, checking, effects, target, or
  patch semantics.
- Unsafe and assembly review-only code is rejected by the Zig backend.

## v2 Track: Foreign Code Capsules

Goal: let agents reuse existing programming languages, libraries, and pinned
starting projects inside Forgentex while preserving typed boundaries, effects,
sandbox policy, and human review.

Core work:

- Define foreign-language capsule syntax and AST nodes.
- Require language, version, runtime, target environment, dependencies, effects,
  and sandbox policy on every capsule.
- Start with a Python 3.13 capsule prototype.
- Plan the broader foreign-language catalog around JavaScript, TypeScript,
  Java, C#, C, C++, CUDA, Go, Bash, PowerShell, Solidity, and Vyper.
- Add dependency pinning for package managers and GitHub repositories through
  lockfile entries, commits, checksums, and audit reports.
- Type-check Forgentex inputs and outputs at the capsule boundary.
- Report foreign-code changes in semantic patch review, graph artifacts,
  package audit, and agent context packets.
- Treat shell, native, GPU, and smart-contract capsules as high-risk capsule
  classes with stricter sandbox, target, ABI, gas, and secret-safety review.
- Reject, stub, or explicitly bridge capsules in native backends rather than
  silently lowering them.

Deliverables:

- `docs/v2_roadmap.md`
- foreign capsule design spec
- shared typed namespace design (`docs/v2_shared_namespace.md`)
- token optimization design: capsule compression, doc-compact register, and
  the generation/reading idea catalog (`docs/token_optimization_v2.md`)
- audio DSP track: native DSP core + miniaudio capsule contract, with
  realtime-safety as a checked effect property (`docs/audio_dsp_track.md`)
- numeric determinism and precision: exact fixed-point path, deterministic
  float across backends, explicit rounding, checked arithmetic
  (`docs/numeric_determinism.md`)
- foreign language catalog for Python, JavaScript, TypeScript, Java, C#, C,
  C++, CUDA, Go, Bash, PowerShell, Solidity, and Vyper
- Python 3.13 capsule examples
- capsule review report schema
- capsule dependency locking and audit evidence

Acceptance gates:

- A Python 3.13 capsule declares exact runtime, target environment, typed
  boundary, effects, pinned dependencies, and sandbox policy.
- Review reports identify changed foreign code, dependency changes, effects,
  target environments, and caller functions.
- Review reports classify high-risk languages such as Bash, PowerShell, C,
  C++, CUDA, Solidity, and Vyper.
- Unpinned packages, floating GitHub branches, undeclared effects, and missing
  sandbox policy are rejected.
- Foreign code cannot bypass the package manager, audit, effect, graph, or
  review systems.

## v0.14 Ownership Increment: Field-Sensitive Move Paths

Goal: remove one named v1.0 ownership gap by tracking moves through owned struct
fields instead of only whole local bindings.

Core work:

- Extend the ownership environment with moved field paths.
- Treat owned field moves as partial moves of the root binding.
- Preserve access to unrelated sibling fields.
- Reject whole-root use while any owned field path is missing.
- Clear a moved field path when that field is assigned again.

Deliverables:

- `docs/borrow_checker_v1.md` v0.14 section
- `fgx check` field-move diagnostics
- `fgx borrow-report` rule evidence

Acceptance gates:

- `let left: Packet = pair.left; return pair.right.value` passes.
- `let left: Packet = pair.left; return pair.left.value` reports
  `use_after_move`.
- `let left: Packet = pair.left; return pair` reports `use_after_move` with
  `partially_moved_value`.
- Reassigning `pair.left` makes the whole `pair` usable again.

## v0.15 Ownership Increment: Borrow Return Provenance

Goal: remove the most obvious false negative in v0 reference returns by
summarizing functions that return one of their borrow parameters.

Core work:

- Summarize borrow-return functions whose returned expression is a borrow
  parameter or local alias of one.
- Preserve summaries through direct call chains.
- Preserve parameter-origin evidence at the caller when the corresponding
  argument is parameter-origin.
- Keep unknown calls, field/index returns, and complex control-flow returns
  conservatively rejected.

Deliverables:

- `docs/borrow_checker_v1.md` v0.15 section
- `fgx check` accepts direct borrow-return helper calls
- `fgx borrow-report` rule evidence

Acceptance gates:

- `fn same(xs: &[T]) -> &[T]: return xs` still passes.
- `fn caller(xs: &[T]) -> &[T]: return same(xs)` now passes.
- Local aliases of summarized borrow-return calls remain parameter-origin.
- Unknown borrow-return calls still report `escaping_reference`.

## v0.16 Ownership Increment: Conditional Borrow Return Joins

Goal: extend borrow-return summaries from one parameter to a set of possible
parameter origins across simple conditional control flow.

Core work:

- Represent borrow-return summaries as sets of possible parameter indices.
- Join proven return paths from `if` and loop bodies.
- Preserve call-site safety only when every possible returned argument is
  parameter-origin.
- Keep field/index, unknown, and unproven return paths rejected.

Deliverables:

- `docs/borrow_checker_v1.md` v0.16 section
- `fgx check` accepts simple conditional borrow-return helpers
- `fgx borrow-report` rule evidence

Acceptance gates:

- `choose(flag, xs, ys)` can return either borrow parameter.
- `return choose(flag, xs, ys)` passes when `xs` and `ys` are caller
  parameters.
- Passing an unproven local borrow as one possible returned argument reports
  `escaping_reference`.

## v0.17 Ownership Increment: Lexical Borrow Alias Locks

Goal: make local borrow aliases affect later indexed writes, not only call
arguments.

Core work:

- Treat local borrow aliases as live until reassigned or function end.
- Reject indexed writes through a root while another live alias points at that
  root.
- Allow indexed writes through the live alias itself.
- Release the old root when the alias is reassigned to a different borrow root.

Deliverables:

- `docs/borrow_checker_v1.md` v0.17 section
- `fgx check` reports `mutable_borrow_alias` for live alias write conflicts
- `fgx borrow-report` rule evidence

Acceptance gates:

- `let alias: &mut [T] = xs; xs[0] = value` reports
  `mutable_borrow_alias`.
- `let alias: &mut [T] = xs; alias[0] = value` passes.
- Reassigning `alias` before writing through `xs` releases the old root.

## v0.18 Ownership Increment: Lifetime Region Graph

Goal: create stable region/loan evidence that future lifetime inference and
non-lexical borrow work can consume.

Core work:

- Add a `regions` section to `forgentex.borrow_report.v1`.
- Create one region per function-local borrow root.
- Record parameter and local-alias loans with root, mutability, type, origin,
  and node evidence.
- Record borrow uses for indexed reads, indexed writes, call arguments,
  returns, assignment targets, and assignment sources.
- Keep v0.18 evidence-only: diagnostics and pass/fail behavior remain governed
  by the existing borrow rules.

Deliverables:

- `docs/borrow_checker_v1.md` v0.18 section
- `fgx borrow-report --json` region evidence
- `lifetime_region_graph` borrow-report rule evidence

Acceptance gates:

- Borrow parameters appear as parameter loans in region graph output.
- `let alias: &mut [T] = xs` adds a local loan tied to the `xs` root.
- Indexed writes through the alias are recorded as `index_write` uses.
- Immutable borrowed inputs record separate immutable regions and `index_read`
  uses.

## v0.19 Ownership Increment: Non-Lexical Borrow Endings

Goal: use simple last-use evidence to end local borrow aliases before the end of
the lexical scope when no later alias use exists.

Core work:

- Compute future variable uses for each statement body.
- Treat local borrow aliases as active only when they are used in the current
  assignment or a later statement.
- Allow indexed writes through a root after the alias's last use.
- Keep same-statement alias reads and later alias writes blocked.
- Keep the rule intentionally narrow: it does not yet handle local reborrow
  ancestry, two-phase mutable activation, or projection disjointness.

Deliverables:

- `docs/borrow_checker_v1.md` v0.19 section
- `fgx check` accepts simple post-last-use root writes
- `fgx borrow-report` lists `non_lexical_borrow_endings`

Acceptance gates:

- `let alias: &mut [T] = xs; alias[0] = value; xs[0] = value` passes.
- `let alias: &mut [T] = xs; xs[0] = value; alias[0] = value` reports
  `mutable_borrow_alias`.
- `let alias: &mut [T] = xs; xs[0] = alias[0]` reports
  `mutable_borrow_alias`.
- An unused alias does not block a later indexed write through its root.

## v0.20 Ownership Increment: Reborrows and Two-Phase Mutable Access

Goal: let simple local mutable reborrows behave like parent-child loans while
keeping mutable call activation visible and conservative.

Core work:

- Record the immediate parent of each local borrow alias in addition to its
  canonical root.
- Allow writes through a child reborrow before the parent/root is used again.
- Reject parent/root writes and mutable call activations while a child reborrow
  has a future use.
- Reject same-statement parent/child alias use for indexed writes.
- Keep sibling reborrows from the same parent conservative.
- Allow `&mut [T]` where `&[T]` is expected as a shared reborrow.
- Treat mutable call arguments as reserved during helper argument evaluation and
  activated at the call boundary.

Deliverables:

- `docs/borrow_checker_v1.md` v0.20 section
- `fgx check` accepts simple child-then-parent reborrow flows
- `fgx borrow-report` lists `reborrows_and_two_phase_access`

Acceptance gates:

- `let child: &mut [T] = alias; child[0] = value; alias[0] = value` passes.
- `let child: &mut [T] = alias; alias[0] = value; child[0] = value` reports
  `mutable_borrow_alias`.
- A mutable call through the parent reports `mutable_borrow_alias` while a child
  reborrow has a future use.
- `set_first(xs, first(xs))` passes when `first` takes a shared slice and
  `set_first` takes a mutable slice.
- Passing the same root as multiple borrow arguments to one call remains
  rejected.

## v0.21 Ownership Increment: Place and Projection Alias Analysis

Goal: represent local borrow aliases as places so the checker can accept
obviously disjoint projections while keeping overlapping projections rejected
with the same stable `mutable_borrow_alias` diagnostic.

Core work:

- Store a borrow place beside each alias root.
- Represent field projections and constant index projections in local places.
- Treat different fields under the same root as disjoint.
- Treat different known constant indexes under the same root as disjoint.
- Treat parent/child places, identical places, unknown indexes, and unsupported
  projection combinations as overlapping.
- Use place overlap for mutable call-argument alias checks and live-alias write
  activation checks.

Deliverables:

- `docs/borrow_checker_v1.md` v0.21 section
- `fgx check` accepts disjoint field borrow aliases such as `pair.left` and
  `pair.right`
- `fgx check` rejects overlapping projections such as two aliases of
  `pair.left`
- `fgx borrow-report` lists `place_projection_alias_analysis`

Acceptance gates:

- `fill(pair.left, pair.right)` passes when both fields are mutable borrow
  fields.
- `fill(pair.left, pair.left)` reports `mutable_borrow_alias`.
- `let left = pair.left; let right = pair.right; left[0] = value; right[0] =
  value` passes.
- `let left = pair.left; let also_left = pair.left; left[0] = value;
  also_left[0] = value` reports `mutable_borrow_alias`.

## v0.22 Ownership Increment: Interprocedural Lifetime Contracts

Goal: make helper lifetime relationships explicit by summarizing which borrow
parameters can be returned and which mutable borrow parameters are actually
mutated, then use those summaries in the checker and borrow report.

Core work:

- Add an AST-only lifetime contract analyzer shared by typecheck and
  borrow-report.
- Record returned borrow parameter sets for each function.
- Record mutating mutable-borrow parameter sets for each function.
- Propagate mutating contracts through helper calls and wrappers.
- Keep recursive mutating helpers conservative.
- Use mutating contracts to avoid activating read-only `&mut [T]` helper calls
  against live child reborrows.
- Emit contract evidence in `forgentex.borrow_report.v1`.

Deliverables:

- `docs/borrow_checker_v1.md` v0.22 section
- `fgx check` accepts read-only `&mut [T]` helper calls while a child reborrow
  has a future use
- `fgx check` still rejects wrappers around mutating helpers in the same case
- `fgx borrow-report --json` emits `lifetime_contracts`
- `fgx borrow-report` lists `interprocedural_lifetime_contracts`

Acceptance gates:

- A helper that only reads from `&mut [T]` has no mutating parameter in its
  contract.
- A helper that writes through `xs[0]` has `xs` in its mutating parameter
  contract.
- A wrapper calling that mutating helper propagates the mutating contract.
- Conditional borrow-return helpers still report every possible returned
  parameter in the explicit contract.

## v0.23 Ownership Increment: Borrow Checker Conformance Suite

Goal: turn the staged borrow-checker rules into a compact built-in conformance
suite that agents and release checks can run before treating a borrow change as
stable.

Core work:

- Add `fgx borrow-conformance` with text and JSON output.
- Emit `forgentex.borrow_conformance.v1` for machine-readable release evidence.
- Run each case through the real parser and type/borrow checker.
- Cover positive and negative examples for moves, local borrow aliases,
  reborrows, lifetime returns, place/projection aliasing, interprocedural
  lifetime contracts, and raw pointer unsafe gates.
- Include contract-summary cases for returned borrow parameters and transitive
  mutating mutable-borrow parameters.
- Wire the command into release-check evidence.

Deliverables:

- `src/borrow_conformance.rs`
- `docs/borrow_checker_v1.md` v0.23 section
- `fgx borrow-conformance --json`
- release labels mark `v0.23` completed

Acceptance gates:

- The built-in suite reports `status=pass` and `failed=0`.
- The suite includes categories for moves, borrows, reborrows, lifetimes,
  projections, contracts, and raw pointer gates.
- JSON output includes representative case IDs such as
  `moves.use_after_move`, `lifetimes.conditional_return_ok`, and
  `contracts.report_wrapper_mutates`.
- `fgx release-check --json` lists `fgx borrow-conformance --json` as
  ownership/borrow evidence.

## v0.24 Release Candidate Hardening

Goal: make release readiness auditable before v1.0 by turning hardening checks
into release-check data and closing the latest borrow-checker edge cases.

Core work:

- Extend `forgentex.release_check.v1` with `hardening_checks`.
- Add hardening checks for strict release gating, maturity-gate evidence
  completeness, diagnostic shape freeze, borrow escape-hatch conformance, and
  target decision consistency.
- Keep `fgx release-check --target v1.0 --strict` failing while ownership and
  borrowing remain partial.
- Add explicit `raw_pointer_requires_unsafe` diagnostics for raw pointer
  parameters in safe functions.
- Expand borrow conformance with raw pointer parameter, conditional lifetime
  escape, constant-index projection, and unknown-index projection cases.
- Mark v0.24 completed while keeping v1.0 planned until the ownership gate can
  move from partial to pass.

Deliverables:

- `forgentex.release_check.v1` includes `hardening_checks`,
  `hardening_pass`, and `hardening_fail`
- `fgx borrow-conformance --json` includes the v0.24 RC cases
- release labels mark `v0.24` completed
- `docs/v1_roadmap.md` lists only v1.0 as the remaining roadmap item

Acceptance gates:

- `hardening_fail=0` in `fgx release-check --json`.
- Every release gate has non-empty evidence and explicit gaps when not passing.
- Diagnostic catalog entries keep stable machine-readable fields.
- Safe raw pointer parameters report `raw_pointer_requires_unsafe`.
- Borrow conformance includes `raw.raw_pointer_param_safe_fn_rejected`,
  `lifetimes.unproven_conditional_arg_rejected`,
  `projections.constant_indices_disjoint_ok`, and
  `projections.unknown_index_overlap_rejected`.

## v0.25 Ownership Increment: Loop-Carried Borrow Liveness

Goal: close the loop back-edge false negative where an alias created before a
loop is read in the loop body and must stay live for later iterations.

Core work:

- Treat reads of bindings that existed before a loop as future uses across the
  loop back-edge.
- Reject root writes inside the loop body while an outer borrow alias will be
  read again in a later iteration.
- Keep aliases declared inside the loop body local to that iteration so simple
  alias-then-root-write flows still pass.
- Add loop liveness cases to the built-in borrow conformance suite.
- Expose `loop_carried_borrow_liveness` in borrow-report rule evidence.

Deliverables:

- `docs/borrow_checker_v1.md` v0.25 section
- `fgx check` rejects loop-carried alias/root conflicts with
  `mutable_borrow_alias`
- `fgx borrow-conformance --json` includes loop-carried and loop-local cases
- release labels mark `v0.25` completed

Acceptance gates:

- `let alias: &mut [T] = xs; for ...: alias[0] = value; xs[0] = value`
  reports `mutable_borrow_alias`.
- `for ...: let alias: &mut [T] = xs; alias[0] = value; xs[0] = value`
  passes.
- `fgx borrow-report --json` lists `loop_carried_borrow_liveness`.
- `fgx release-check --json` reports
  `current_track=v0.25-loop-carried-borrow-liveness`.

## v0.26 Ownership Increment: Conditional Borrow-State Joins

Goal: close the conditional-control-flow false negative where an existing
borrow alias is reassigned inside an `if` body and later checks forget that the
alias may now point at the branch-assigned root.

Core work:

- Preserve possible borrow places for existing aliases after conditional branch
  reassignment.
- Keep branch-local aliases scoped to the branch body.
- Check later indexed writes against every possible joined alias root.
- Check mutable call arguments against every possible joined alias root.
- Keep simple non-lexical endings so root writes after the alias's last use
  still pass.
- Add branch join cases to the built-in borrow conformance suite.
- Expose `conditional_borrow_state_join` in borrow-report rule evidence.

Deliverables:

- `docs/borrow_checker_v1.md` v0.26 section
- `fgx check` rejects conditional alias/root conflicts with
  `mutable_borrow_alias`
- `fgx borrow-conformance --json` includes branch reassignment, call, and
  last-use cases
- release labels mark `v0.26` completed

Acceptance gates:

- `let alias = ys; if flag: alias = xs; xs[0] = value; alias[0] = value`
  reports `mutable_borrow_alias`.
- `let alias = ys; if flag: alias = xs; fill(xs, alias)` reports
  `mutable_borrow_alias`.
- `let alias = ys; if flag: alias = xs; alias[0] = value; xs[0] = value`
  passes.
- `fgx borrow-report --json` lists `conditional_borrow_state_join`.
- `fgx release-check --json` reports
  `current_track=v0.26-conditional-borrow-state-joins`.

## v1.0: CFG Dataflow Borrow Checker and Release

Goal: close the ownership/borrow gate by computing the staged borrow rules'
properties uniformly over the control-flow graph, then declare v1.0.

Core work:

- Add `src/cfg.rs` instruction-level source statements so dataflow passes can
  walk expression structure.
- Add `src/borrowck.rs`: forward may-analysis for moved paths, alias place
  sets with field/constant-index projections, reborrow ancestry, and
  liveness-based conflict checks over the CFG.
- Run the dataflow pass behind the staged rules so existing diagnostics stay
  byte-stable while new control-flow shapes become errors.
- Add `dataflow.*` conformance cases: loop re-moves, reborrows of
  conditionally-joined aliases, nested-loop alias liveness, and matching
  precision cases.
- Move the ownership gate to pass, set `current_track=v1.0-cfg-dataflow-borrowck`,
  and mark the v1.0 release label completed.

Acceptance gates:

- `fgx borrow-conformance --json` reports `status=pass` with the `dataflow`
  category included.
- `fgx release-check --target v1.0 --strict` exits zero with
  `decision=v1_ready`, fifteen gates passing, and `hardening_fail=0`.
- Re-moving an owned value across a loop back edge reports `use_after_move`.
- A reborrow of a conditionally-joined alias keeps every possible root at
  later writes.

## Architecture Support Matrix

| Stage | x86_64 | ARMv7 | AArch64 | RISC-V 32 | RISC-V 64 |
| --- | --- | --- | --- | --- | --- |
| Target metadata | Stage 11 | Stage 11 | Stage 11 | Stage 11 | Stage 11 |
| Assembly parsing | Stage 12 | Stage 12 | Stage 12 | Stage 12 | Stage 12 |
| Register validation | Stage 12 | Stage 12 | Stage 12 | Stage 12 | Stage 12 |
| ABI/layout reports | Stage 14 | Stage 14 | Stage 14 | Stage 14 | Stage 14 |
| C backend target checks | Stage 10-14 | Stage 14 | Stage 14 | Stage 14 | Stage 14 |
| Zig source backend | Stage 28 | Stage 28 | Stage 28 | Stage 28 | Stage 28 |
| SIMD intrinsics | Stage 13 | Stage 13 | Stage 13 | Stage 13 | Stage 13 |
| Native backend | Stage 15+ | Stage 15+ | Stage 15+ | Stage 15+ | Stage 15+ |

## Near-Term Implementation Queue

The next practical development sequence should be:

1. Install `rustfmt` and `clippy`; make CI-style checks pass locally.
2. Expand tests around the current parser, formatter, checker, patcher, and CLI.
3. Add `docs/target_architecture_model.md` with x86_64, ARM/AArch64, and RISC-V names.
4. Add target metadata to the AST for assembly blocks and fast paths.
5. Add examples for `asm aarch64` and `asm riscv64`.
6. Add target validation diagnostics for unknown architecture names.
7. Add `fgx targets`.
8. Add `fgx check --target <triple>`.
9. Add architecture-specific review report sections.
10. Begin replacing the prototype parser with a lexer/span-aware parser.
11. Extend backend differential checks to include generated Zig entry-symbol
    evidence alongside interpreter, C, and FGIR checks.
12. Add real Zig compiler discovery only after source snapshots and
    differential hooks stay stable.
