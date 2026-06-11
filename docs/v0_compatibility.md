# Forgentex v0 Compatibility Promise

Forgentex v0 is still a prototype, but the demo corpus is treated as a stable
contract for the current milestone.

## Stable v0 Examples

The following examples must continue to parse, format, roundtrip, check, and
appear in project maps:

- `examples/hello.fgx`
- `examples/hello.fgai`
- `examples/abi/layout_demo.fgx`
- `examples/media/audio_mix.fgx`
- `examples/media/audio_mix.fgai`
- `examples/media/audio_mix_simd.fgx`
- `examples/media/bitreader.fgx`
- `examples/media/h264_nal_header.fgx`
- `examples/media/idct4x4.fgx`
- `examples/media/popcount_x86.fgx`
- `examples/media/popcount_aarch64.fgx`
- `examples/media/popcount_riscv64.fgx`
- `examples/arch/x86_64_popcount.fgx`
- `examples/arch/aarch64_popcount.fgx`
- `examples/arch/riscv64_add.fgx`
- `examples/arch/aarch64_copy_neon.fgx`
- `examples/arch/riscv64_vector_add_v.fgx`
- `examples/io/file_copy.fgx`
- `examples/network/agent_ping.fgx`
- `examples/agent/signed_patch.fgx`
- `examples/agent/payment_receipt.fgx`
- `examples/packages/codec_common/Forgentex.toml`
- `examples/packages/codec_common/src/common.fgx`
- `examples/packages/codec_app/Forgentex.toml`
- `examples/packages/codec_app/src/app.fgx`

`examples/media/audio_mix.fgx` and `examples/media/audio_mix_simd.fgx` must
also run through interpreter entrypoint tests.
`examples/media/popcount_*.fgx` files are review-only because they contain
structured assembly.

## Stable v0 Commands

The following CLI commands are part of the v0 demo contract:

- `fgx parse <file> --emit-ast`
- `fgx ast <file> --canonical`
- `fgx diagnose <file> --json`
- `fgx fmt <file> --to fgx|fgai`
- `fgx roundtrip <file> --to fgx|fgai`
- `fgx check <file> --json`
- `fgx check <file> --target <triple> --json`
- `fgx borrow-report <file> [--json]`
- `fgx borrow-conformance [--json]`
- `fgx asm-check <file> --target <triple>`
- `fgx asm-check <file> --target <triple> --json`
- `fgx abi <file> --target <triple>`
- `fgx abi <file> --target <triple> --json`
- `fgx header <file> --target <triple>`
- `fgx simd-review <file> --target <triple>`
- `fgx simd-review <file> --target <triple> --json`
- `fgx check --project [path] --json`
- `fgx targets [--json]`
- `fgx backends [--json]`
- `fgx ir <project>`
- `fgx ir <project> --json`
- `fgx ir <project> --contract-json`
- `fgx stdlib [--json]`
- `fgx build <project> --backend c [--out <dir>]`
- `fgx build <project> --backend zig [--out <dir>]`
- `fgx new <path> [--name <package>]`
- `fgx lock [project] [--out <file>] [--json]`
- `fgx test [--project <path>] [--target <triple>|--all-targets] [--json]`
- `fgx build <project> --backend c --target <triple> [--profile <profile>] [--out <dir>] [--json]`
- `fgx build <project> --backend c --all-targets [--profile <profile>] [--out <dir>] [--json]`
- `fgx build <project> --backend zig --target <triple> [--profile <profile>] [--out <dir>] [--json]`
- `fgx build <project> --backend zig --all-targets [--profile <profile>] [--out <dir>] [--json]`
- `fgx publish <project> --dry-run [--json]`
- `fgx audit [project] [--json]`
- `fgx verify [project] [--json]`
- `fgx bench <file> --entry <function> --json-input <file> --target <triple> [--iterations <n>] [--json]`
- `fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]`
- `fgx release-check [--target v1.0] [--json] [--strict]`
- `fgx api --public [--project <path>]`
- `fgx effects <file> --json`
- `fgx patch <source> <patch> --review`
- `fgx patch <source> <patch> --check-only`
- `fgx patch <source> <patch> --emit-review-json`
- `fgx run <file>`
- `fgx run <file> --entry <function> --json-input <file>`
- `fgx map <dir> --out <file>`
- `fgx map <dir> --include-targets --out <file>`
- `fgx map --project [path] --out <file>`
- `fgx map --project [path] --include-targets --out <file>`
- `fgx get '@function.name' --from <dir> --deps types`
- `fgx context '@function.name' --deps types,effects,tests [--project <path>] [--json]`
- `fgx review <patch> [--source <file>] [--project <path>] [--json]`
- `fgx fuzz [--iterations <n>] [--json]`
- `fgx diff-test [--project <path>] [--source <file>] --entry <function> --json-input <file> [--json]`

Breaking these commands requires updating this file, the README, and the tests
in the same change.

## Stable Stage 4 Type Additions

The v0 demo now treats these type-system behaviors as covered by tests:

- canonical parsing and formatting for `Option[T]`, `Result[T,E]`, named generic
  types, fixed arrays `[T;N]`, slices, mutable slices, and pointers
- explicit local `let` types
- integer literal range checking for fixed-width integer destinations
- explicit casts for mixed concrete numeric arithmetic
- struct constructor arity/type checking
- stable JSON diagnostics with suggested patch kinds

## Stable Stage 5 Safety Additions

The v0 demo now runs a borrow-check pass as part of `fgx check`:

- owned structs, arrays, and named generics move into owned destinations
- use-after-move is rejected
- obvious mutable borrow aliasing in function calls is rejected
- immutable slice writes remain rejected
- reference returns must be tied to parameter-origin borrows
- raw pointer arguments, returns, and dereferences require `unsafe fn`

## Stable Stage 6 Effect Additions

The v0 demo now treats effect and capability behavior as part of `fgx check`:

- function headers can declare `effects [name]` in `.fgx`
- `.fgai` uses compact `effects[name]`
- `fgx effects <file> --json` reports declared, inferred, missing, invalid, and capability effects
- callers must declare effects inherited from effectful callees
- effectful builtin calls infer capabilities such as `io.read`, `fs.read`, `network.write`, `crypto.sign`, and `alloc`
- `memory.raw` and `cpu.asm` require an unsafe boundary
- semantic patch reports include effect/capability changes

## Stable Stage 7 Semantic Patch Additions

The v0 demo now accepts semantic patch protocol v1 while preserving v0 patches:

- `.fgp` envelopes can include version, author, mock signature, target, preconditions, expected diagnostics, and review metadata
- preconditions support node existence, node absence, and node hash conflict checks
- patch operations cover declarations, fields, function bodies, return expressions, unsafe markers, effects, assembly blocks, fast paths, assembly stubs, and test intents
- patch review JSON reports public API, unsafe/assembly, ABI/layout, allocation, filesystem, network, effect/capability, and architecture-specific change categories
- check-only previews apply and type-check patches in memory without emitting patched source

## Stable Stage 8 Project Additions

The v0 demo now includes a manifest-backed project layer:

- `Forgentex.toml` declares package name, version, and source root
- `.fgx` accepts `module`, `import`, `export`, and `pub` top-level markers
- `.fgai` accepts compact `M`, `I`, `IMPORT`, `EXPORT`, and `PUB` markers
- `fgx check --project --json` validates module graphs and checks the combined program
- `fgx api --public` emits exported structs and functions
- `fgx map --project` emits module/import/export metadata, hashes, token estimates, and symbols
- project diagnostics cover duplicate modules, unresolved imports, import cycles, and duplicate symbols

## Stable Stage 9 Interpreter Additions

The v0 demo now includes interpreter v1 behavior:

- `fgx run <file> --entry <function> --json-input <file>` executes a selected safe function
- JSON inputs can provide positional or named arguments plus golden expectations
- JSON output reports entry, status, return value, and final bindings
- mutable slice updates propagate through nested safe calls
- structs, arrays, slices, loops, branches, `require`, casts, indexing, and field access execute through the interpreter
- `Option` and `Result` runtime values are available through `Some`, `None`, `Ok`, `Err`, `is_some`, `is_none`, `is_ok`, `is_err`, and `unwrap_or`
- runtime failures are deterministic `trap: <code>: <message>` strings
- `str` runtime values are accepted in JSON inputs and outputs

## Stable Stage 10 C Backend Additions

The v0 demo now includes a conservative C11 backend:

- `fgx build <project> --backend c` writes generated `.h` and `.c` files
- safe exported structs and functions are lowered from a manifest project
- primitives map to fixed-width C types
- slices map to generated `{ data, len }` structs
- headers include runtime helpers for traps, bounds checks, and checked arithmetic contracts
- `require` lowers to deterministic `fgx_trap("require_failed")`
- unsafe functions and structured assembly are rejected by the backend
- runtime-only effects such as `fs.*`, `io.*`, `network.*`, `serialize`, and `stream` are rejected by the backend

## Stable Stage 11 Target Architecture Additions

The v0 demo now includes an explicit target architecture model:

- `fgx targets` lists canonical target triples for x86_64, ARMv7, AArch64, RISC-V 32-bit, and RISC-V 64-bit
- `fgx targets --json` emits the stable `forgentex.targets.v1` schema
- `fgx check <file> --target <triple> --json` adds target diagnostics after the normal checker
- target checks validate unknown triples, target families, CPU feature names, assembly target mismatches, incompatible target-specific calls, and missing `cpu_has_<feature>()` guards
- AST JSON includes function-level `target_metadata`
- `fgx map --include-targets` adds compact target labels to function entries
- semantic patch review reports include architecture path notes for fast paths, fallback behavior, and assembly review-only paths

## Stable Stage 12 Structured Assembly Additions

The v0 demo now validates structured assembly before any real lowering exists:

- `fgx asm-check <file> --target <triple>` emits a human review summary
- `fgx asm-check <file> --target <triple> --json` emits `forgentex.asm_check.v1`
- accepted dialects are `asm x86_64`, `asm armv7`, `asm aarch64`, `asm riscv32`, and `asm riscv64`
- register sets are validated for x86_64, ARMv7, AArch64, RISC-V 32-bit, and RISC-V 64-bit
- assembly outside `unsafe fn` reports `unsafe_required`
- invalid registers, invalid clobbers, reserved output/clobber registers, target mismatches, missing inputs, and missing target features produce stable diagnostics
- review summaries list changed registers, clobbers, required features, memory effects, and ABI risk

## Stable Stage 13 Portable SIMD Additions

The v0 demo now includes a portable SIMD frontend layer:

- `vec[T,N]` and `mask[N]` parse, format, roundtrip, type-check, and serialize as named generic types
- target-independent `simd_*` intrinsics cover vector constructors, splats, load/store, widening/narrowing, add, multiply, saturating add, equality masks, select, reductions, and shuffle
- `simd_*` calls infer the `cpu.simd` effect and can execute in the interpreter as deterministic lane arrays
- `fgx check <file> --target <triple> --json` reports `simd_fallback_required` when SIMD code lacks a scalar fallback or explicit `target.only` marker
- `fgx simd-review <file> --target <triple>` emits human review text with intrinsics, vector types, scalar fallback status, target-only status, equivalence-test status, and target lowering plans
- `fgx simd-review <file> --target <triple> --json` emits the stable `forgentex.simd_review.v1` schema
- review lowering plans cover x86_64 SSE/AVX, ARMv7 NEON, AArch64 NEON/SVE, RISC-V Vector Extension, and scalar fallback paths

## Stable Stage 14 ABI and Layout Additions

The v0 demo now includes target-aware ABI/layout tooling:

- `.fgx` accepts `@repr(C)`, `@packed`, `@aligned(N)`, field-level `@endian(...)`, and function-level `@extern(C)` attributes
- `.fgai` accepts compact `A name(args)` attribute lines and inline field attributes
- `padding[N]` is treated as explicit byte padding by ABI reports and generated headers
- `fgx abi <file> --target <triple>` emits human ABI reports with target model, struct layout, field offsets, padding, endian markers, and C ABI function summaries
- `fgx abi <file> --target <triple> --json` emits the stable `forgentex.abi.v1` schema
- `fgx header <file> --target <triple>` emits a C header with target macros, layout macros, struct typedefs, packed pragmas, explicit padding arrays, and function prototypes
- ABI reports expose target differences across x86_64, ARMv7, AArch64, RISC-V 32-bit, and RISC-V 64-bit pointer and calling-convention models
- semantic patch reports continue to mark struct/function API changes as `abi_or_layout`

## Stable Stage 15 Backend Strategy Additions

The v0 demo now includes a backend boundary before future native compiler work:

- `fgx backends` lists the backend catalog and marks `c` as available
- `fgx backends --json` emits the stable `forgentex.backends.v1` schema
- `fgx ir <project>` emits human-readable FGIR using the `forgentex.backend_ir.v1` schema
- `fgx ir <project> --json` emits machine-readable FGIR for backend experiments
- `fgx ir <project> --contract-json` emits the `forgentex.frontend_contract.v1` schema with canonical AST hash, type diagnostics, and effect analysis
- `fgx build <project> --backend c` continues to use the C backend as the reference transpiler
- `zig` is available as a source backend that emits generated `.zig` artifacts
- planned `cranelift`, `llvm`, and `mlir` backends fail explicitly instead of pretending to generate native artifacts
- backend catalog metadata covers x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, and RISC-V 64-bit target families
- backend choice must not alter frontend AST, type-checking, effect, patch, target, or ABI/layout semantics

## Stable Stage 16 Standard Library Additions

The v0 demo now includes a source-backed standard library:

- `stdlib/Forgentex.toml` and `stdlib/src/core/*.fgx` define the standard source package
- `fgx stdlib` lists the human-readable standard library catalog
- `fgx stdlib --json` emits the stable `forgentex.stdlib.v1` schema
- manifest projects automatically load imported `core.*` modules from the standard library package
- stable modules are `core.mem`, `core.slice`, `core.math`, `core.result`, `core.option`, `core.bits`, `core.simd`, `core.target`, `core.fs`, `core.net`, `core.serialize`, and `core.stream`
- planned modules such as `core.bytes`, `core.fixed`, `core.stats`,
  `core.geom`, `core.grid`, `core.hash`, `core.rng`, `core.text`,
  `core.json`, `core.test`, and `core.bench` are documented in the
  standard-library roadmap, but are not stable v0 APIs yet
- stable API prefixes are `core_mem_*`, `core_slice_*`, `core_math_*`, `core_option_*`, `core_result_*`, `core_bits_*`, `core_simd_*`, `core_target_*`, `core_fs_*`, `core_net_*`, `core_serialize_*`, and `core_stream_*`
- checked `i32` arithmetic and bit helpers execute through deterministic interpreter semantics
- `core_target_has_feature_*` and `core_target_feature_enabled_*` are recognized by target checks as feature guards
- `simd_*`, `cpu_has_*`, `Some`, `None`, `Ok`, `Err`, and `unwrap_or` remain compatibility aliases
- media/DSP examples now use standard APIs instead of direct ad hoc intrinsic names where a standard v0 API exists

## Stable Stage 17 I/O and Networking Additions

The v0 demo now includes deterministic I/O and networking runtime APIs:

- `docs/io_networking_model.md` defines capability scopes, mock runtime behavior, and effect requirements
- `examples/io/file_copy.fgx` demonstrates filesystem read/write effects through `core.fs`
- `examples/network/agent_ping.fgx` demonstrates HTTP-style network effects through `core.net`
- `fgx check` enforces explicit `fs.*`, `network.*`, `serialize`, and `stream` effects
- `fgx run --entry ... --json-input ...` can execute the I/O and network examples using mock runtime values
- semantic patch reviews separately flag added filesystem and network access

## Stable Stage 18 Crypto and Agent-Economy Additions

The v0 demo now includes deterministic mock crypto and agent-payment APIs:

- `docs/crypto_agent_economy.md` defines the typed mock crypto model, secret
  handling policy, signed patch envelope rules, and mock ledger limits
- `examples/agent/signed_patch.fgx` demonstrates typed `Hash[Blake3]`,
  `Signature[Ed25519]`, `SecretKey[Ed25519]`, and `PublicKey[Ed25519]` flows
- `examples/agent/payment_receipt.fgx` demonstrates `TokenAmount[MockLedger]`
  and `Receipt` values
- `fgx check` rejects safe logging, serialization/export, or copying of
  `SecretKey[...]` values with `secret_leak`
- signed `.fgp` envelopes using `SIGNED_BY`, `SIGNATURE_ALGORITHM Ed25519`, and
  `SIGNATURE mock-sig-*` are verified before patch operations apply
- semantic patch review JSON exposes signed envelope fields and
  `signed-mock-accepted` or `signed-mock-invalid` status

## Stable Stage 19 Agent Workbench Additions

The v0 demo now includes compact compiler-native packets for agent tooling:

- `docs/agent_workbench_protocol.md` defines context and review packet schemas
- `fgx context <symbol> --deps types,effects,tests --json` emits
  `forgentex.agent_context.v1`
- context packets include project/source summaries, `.fgmap`-style symbol map
  lines, symbol source, optional dependency slices, diagnostics with suggested
  patches, unsafe/assembly summaries, and token estimates
- `fgx review <patch> --json` emits `forgentex.agent_review.v1`
- review packets locate patch targets in the manifest project, apply patches to
  check-only previews, type-check the result, and embed
  `forgentex.patch_review.v1`
- `fgx review <patch> --source <file> --json` supports standalone source demos

## Stable Stage 20 VS Code Extension Additions

The v0 demo now includes a local editor workflow package:

- `tools/vscode/forgentex/package.json` declares VS Code language
  contributions for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`
- TextMate grammars highlight human syntax, agent syntax, semantic patches,
  project maps, node IDs, effects, unsafe assembly, x86_64, ARM/AArch64, and
  RISC-V labels
- the extension uses the `fgx` CLI as a language-service bridge for formatting,
  diagnostics, symbol context, patch review, and patch preview
- document symbols and hovers expose stable semantic node IDs such as
  `@function.mix_samples`
- the patch review WebView surfaces effect, ABI, unsafe, and assembly changes
- the architecture filter command isolates unsafe, assembly, ABI, x86_64,
  ARM/AArch64, and RISC-V lines across workspace `.fgx`, `.fgai`, and `.fgmap`
  files

## Stable Stage 21 Language Server Additions

The v0 demo now includes a dependency-free stdio LSP server:

- `forgentex-lsp --probe` reports `forgentex.lsp.v1` server capabilities
- `forgentex-lsp` speaks `Content-Length` framed JSON-RPC over stdin/stdout
- the server advertises diagnostics, hover, go-to-definition, rename,
  formatting, document symbols, semantic tokens, and code actions
- `.fgx`, `.fgai`, and `.fgp` documents are served by the LSP
- document symbols and hovers expose stable node IDs such as
  `@function.mix_samples`
- formatting reuses the canonical `.fgx` and `.fgai` formatters
- code actions expose semantic patch hooks and compiler-suggested patch kinds
- target-aware diagnostics can be enabled through initialize
  `initializationOptions.target` or the `forgentex/setTarget` notification
- target-aware diagnostics reuse the Stage 11/13 target and SIMD checks for
  x86_64, ARM/AArch64, and RISC-V code paths

## Stable Stage 22 Testing, Fuzzing, and Invariant Additions

The v0 demo now includes deterministic fuzz/property and differential tooling:

- `docs/testing_fuzzing_invariants.md` defines the Stage 22 harnesses and
  invariants
- `fgx fuzz --iterations <n> --json` emits `forgentex.fuzz.v1`
- fuzz cases cover corpus roundtrips, generated safe programs, formatter
  idempotency, canonical AST stability, parser no-panic mutations, malformed
  patch no-panic checks, semantic patch check/format/check invariants, and
  target-specific ABI/assembly invariants
- `fgx diff-test --json` emits `forgentex.backend_differential.v1`
- the backend differential runner links interpreter golden execution, generated
  C header/source output, and FGIR entry-symbol presence for safe programs
- `.github/workflows/ci.yml` includes a scheduled Stage 22 fuzz/differential
  job

## Stable Stage 23 Package Manager and Build-System Additions

The v0 demo now includes a local package manager and matrix-aware build flow:

- `docs/package_build_system.md` defines the Stage 23 package layout, manifest
  fields, lock output, target tests, matrix builds, and publish dry-runs
- `fgx new <path> [--name <package>]` creates a package skeleton with a default
  x86_64/AArch64/RISC-V target matrix
- `Forgentex.toml` accepts `[dependencies]`, `[targets]`, and `[profile.<name>]`
  sections in addition to package name, version, and source root
- local path dependencies are resolved before standard library modules
- `fgx lock` emits the stable `forgentex.lock.v1` schema
- `fgx test` emits the stable `forgentex.package_test.v1` schema
- matrix-aware `fgx build` emits the stable `forgentex.package_build.v1` schema
  and writes C backend artifacts below `<out>/<profile>/<target>`
- `fgx publish --dry-run` emits the stable `forgentex.publish_dry_run.v1`
  schema while registry upload and real package signatures remain deferred
- the example `codec_app` package depends on `codec_common` and builds/tests for
  x86_64, AArch64, and RISC-V targets

## Stable Stage 24 Security, Sandboxing, and Supply-Chain Additions

The v0 demo now includes a local security review layer for package projects:

- `docs/security_model.md` defines the Stage 24 trust, permission, sandbox,
  signed package, reproducibility, audit, and verify model
- `Forgentex.toml` accepts `[security]`, `[permissions]`, `[sandbox]`, and
  `[actions]` sections
- package signatures are mock-verified through `signed_by`,
  `signature_algorithm = "Ed25519"`, and `signature = "mock-sig-*"`
- reproducible source metadata is reported as a deterministic package hash
- `fgx audit [project] [--json]` emits the stable `forgentex.audit.v1` schema
- `fgx verify [project] [--json]` emits the stable `forgentex.verify.v1` schema
- untrusted packages must declare permissions for arbitrary-code package
  actions such as build scripts, codegen, test runners, and agent tools
- untrusted packages must declare permission categories for filesystem,
  network, crypto, I/O, agent, and ledger effects
- `fgx test` and matrix-aware `fgx build` run the supply-chain audit before
  package work and fail on high-severity audit issues

## Stable Stage 25 Performance Tooling Additions

The v0 demo now includes deterministic benchmark and performance review
tooling:

- `docs/performance_tooling.md` defines the Stage 25 benchmark harness,
  performance report schema, and media/DSP benchmark flow
- `fgx bench <file> --entry <function> --json-input <file> --target <triple>`
  runs interpreter correctness and emits deterministic operation-cost evidence
- `fgx bench ... --json` emits the stable `forgentex.bench.v1` schema
- `fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file>`
  compares scalar and optimized sources across x86_64, AArch64, and RISC-V
- `fgx perf-report ... --json` emits the stable
  `forgentex.perf_report.v1` schema
- benchmark reports expose allocation counts, bounds-check counts, selected
  architecture review path, operation counts, estimated cycles, and speedup
- performance patch gates require correctness to pass and attach
  target-specific benchmark evidence
- `docs/token_efficiency_benchmarks.md` records the planned token-efficiency
  corpus, including the butterfly ray-grid benchmark; the corpus and
  `fgx token-report` command are not stable yet

## Stable Stage 26 Mature Release Criteria Additions

The v0 demo now includes a stable release-readiness report:

- `docs/mature_release_criteria.md` defines the Stage 26 maturity gates,
  release labels, release check sequence, and current v1.0 gaps
- `fgx release-check` emits human-readable maturity gate output
- `fgx release-check --json` emits the stable
  `forgentex.release_check.v1` schema
- `fgx release-check --json` includes v0.24 hardening checks and
  hardening pass/fail counts
- `fgx release-check --json` reports the v0.26 conditional borrow-state join
  track and release label
- `fgx release-check --target v1.0 --strict` fails whenever a requested target
  has partial or planned gates (the mechanism stays the CI failure path)
- the default report is `decision=v1_ready`: the ownership gate passes with
  the CFG dataflow borrow checker (`src/borrowck.rs`)

## Stable Stage 27 Borrow Checker v1 Demo Additions

The v0 demo now exposes a focused borrow-checking report:

- `docs/borrow_checker_v1.md` defines the Stage 27 alias-root and move-merge
  additions plus the v0.14 field-sensitive move-path additions and v0.15
  borrow-return provenance summaries plus v0.16 conditional borrow-return joins
  and v0.17 lexical borrow alias locks
- `fgx borrow-report <file>` emits human-readable borrow rule and diagnostic
  output
- `fgx borrow-report <file> --json` emits the stable
  `forgentex.borrow_report.v1` schema
- local borrow aliases preserve their original root for mutable borrow alias
  checks
- moves inside `if` and `for` bodies are conservatively visible to later reads
- field-sensitive moves preserve unmoved sibling fields and reject whole-struct
  use after a partial move until the moved field is assigned again
- direct borrow-return helper calls preserve parameter-origin lifetime evidence
- conditional borrow-return helpers can join multiple proven parameter origins
- live local borrow aliases block indexed writes through another handle to the
  same root
- local reborrow chains and two-phase mutable call activation are tracked by
  the v0.20 borrow rules
- place/projection alias analysis for obvious field and constant-index
  disjointness is tracked by the v0.21 borrow rules
- interprocedural lifetime contracts for returned and mutating borrow parameters
  are tracked by the v0.22 borrow rules
- the built-in borrow conformance suite is tracked by the v0.23 borrow rules
- loop-carried borrow liveness for aliases created before loops is tracked by
  the v0.25 borrow rules
- conditional borrow-state joins for aliases reassigned inside branches are
  tracked by the v0.26 borrow rules
- `fgx release-check` now cites Stage 27, v0.14, v0.15, v0.16, v0.17, v0.18,
  v0.19, v0.20, v0.21, v0.22, v0.23, v0.24, v0.25, and v0.26
  evidence while keeping the v1.0 ownership gate `partial`

## Stable Stage 28 Zig Source Backend Additions

The v0 demo now includes a generated Zig source backend for safe project
experiments:

- `docs/zig_backend_strategy.md` defines the Stage 28 Zig backend scope,
  lowered subset, speed ideas, test gates, and deferred compiler work
- `fgx backends` marks `zig` as available
- `fgx build <project> --backend zig` writes a generated `.zig` file
- generated Zig source includes deterministic trap and bounds-check helpers
- safe exported structs and functions are lowered from manifest projects
- unsafe functions, structured assembly, unsupported generic runtime values,
  and runtime-only effects are rejected by the Zig backend
- Zig backend choice preserves the frontend contract like the C backend
- Zig compiler invocation and `build.zig` package generation remain deferred

## Stable Stage 31 Graph Artifact Additions

The v0 demo now includes deterministic `.fggraph` artifacts for agent and CI
workflows:

- `docs/graph_artifact.md` defines the Stage 31 graph artifact scope
- `fgx graph dump <project|dir|file>` emits `forgentex.graph.v1`
- `fgx graph validate <graph> --json` emits
  `forgentex.graph_validate.v1`
- `fgx graph roundtrip <project|dir|file> --json` emits
  `forgentex.graph_roundtrip.v1`
- `fgx graph context <graph> <symbol> --deps ... --json` emits
  `forgentex.graph_context.v1`
- `fgx graph patch <graph> <patch> --json` emits
  `forgentex.graph_patch.v1`
- `fgx graph diff <before> <after> --json` emits
  `forgentex.graph_diff.v1`
- graph diffs summarize public API, unsafe, capability, target, and test
  consequences for review

## Stable v0.14 Field Move Path Additions

The v0 demo now tracks owned struct field moves more precisely:

- moving an owned field such as `pair.left` records that field path instead of
  moving the whole `pair`
- unrelated sibling fields such as `pair.right` remain available
- using the whole struct after a field move reports `use_after_move` with
  `partially_moved_value`
- assigning the moved field again clears that path
- the borrow report lists `field_move_paths` as an enabled rule

## Stable v0.15 Borrow Return Provenance Additions

The v0 demo now summarizes simple borrow-return helpers:

- a function returning one of its borrow parameters is recognized as
  parameter-origin
- local aliases of borrow parameters preserve that provenance
- callers can safely return `same(xs)` when `same` is proven to return its
  borrow parameter and `xs` is parameter-origin
- unknown calls, field/index returns, and complex control-flow returns remain
  unproven and still report `escaping_reference`
- the borrow report lists `borrow_return_provenance` as an enabled rule

## Stable v0.16 Conditional Borrow Return Join Additions

The v0 demo now joins simple conditional borrow-return summaries:

- a borrow-return summary may include more than one possible returned parameter
- `if` and loop bodies contribute proven return paths to the summary
- callers are accepted only when every possible returned argument is
  parameter-origin
- unproven conditional return paths still report `escaping_reference`
- the borrow report lists `conditional_borrow_return_join` as an enabled rule

## Stable v0.17 Lexical Borrow Alias Lock Additions

The v0 demo now treats local borrow aliases as lexical locks for indexed writes:

- a local borrow alias remains live until it is reassigned or the function ends
- writing through the original root while another alias points at that root
  reports `mutable_borrow_alias`
- writing through the alias itself is accepted
- reassigning the alias to another borrow root releases the old root
- the borrow report lists `lexical_borrow_alias_locks` as an enabled rule

## Stable v0.18 Lifetime Region Graph Additions

The v0 demo now emits lifetime region graph evidence in borrow reports:

- `fgx borrow-report --json` includes a `regions` array
- each region has a function id, root name, mutability flag, loans, and uses
- borrow parameters and local aliases are recorded as loans
- indexed access, calls, returns, and assignments are recorded as use sites
- the borrow report lists `lifetime_region_graph` as an enabled rule

## Stable v0.19 Non-Lexical Borrow Ending Additions

The v0 demo now applies simple last-use checks to local borrow aliases:

- a local alias no longer blocks an indexed root write after its last use
- unused local aliases do not block later indexed root writes
- aliases used in the same assignment or a later statement still block writes
  through another handle to the same root
- the borrow report lists `non_lexical_borrow_endings` as an enabled rule

## Stable v0.20 Reborrow And Two-Phase Access Additions

The v0 demo now models simple local reborrows and mutable call activation:

- local borrow aliases remember their immediate parent and original root
- child reborrows can write before their parent is used again
- parent/root writes and mutable call activations report `mutable_borrow_alias`
  while a child reborrow has a future use
- same-statement parent/child indexed uses remain rejected
- sibling reborrows from the same parent remain conservative
- `&mut [T]` can be passed where `&[T]` is expected as a shared reborrow
- nested non-borrow helper calls such as `set_first(xs, first(xs))` pass
- passing the same root as multiple borrow arguments to one call remains
  rejected
- the borrow report lists `reborrows_and_two_phase_access` as an enabled rule

## Stable v0.21 Place And Projection Alias Additions

The v0 demo now tracks simple borrow places for local alias analysis:

- local borrow aliases remember a root plus field/constant-index projections
- disjoint mutable borrow fields such as `pair.left` and `pair.right` can be
  used in the same local flow or helper call
- overlapping projections such as two aliases of `pair.left` still report
  `mutable_borrow_alias`
- parent/child places, unknown indexes, slice ranges, and unsupported projection
  combinations remain conservative
- the borrow report lists `place_projection_alias_analysis` as an enabled rule

## Stable v0.22 Interprocedural Lifetime Contract Additions

The v0 demo now records function-level lifetime contracts:

- `fgx borrow-report --json` includes `lifetime_contracts`
- contracts list borrow parameters a function may return
- contracts list mutable borrow parameters a function mutates
- mutating contracts propagate through helper calls and wrappers
- read-only helpers that take `&mut [T]` do not activate a mutable call against
  a live child reborrow
- wrappers around mutating helpers still report `mutable_borrow_alias` in that
  case
- the borrow report lists `interprocedural_lifetime_contracts` as an enabled
  rule

## Stable v0.23 Borrow Checker Conformance Additions

The v0 demo now includes a built-in borrow conformance suite:

- `fgx borrow-conformance` emits human-readable suite results
- `fgx borrow-conformance --json` emits the stable
  `forgentex.borrow_conformance.v1` schema
- positive and negative cases cover moves, borrow aliases, reborrows, lifetime
  returns, place/projection aliasing, interprocedural lifetime contracts, and
  raw pointer gates
- contract cases check returned borrow parameters and transitive mutating
  mutable-borrow parameters
- the release check lists `fgx borrow-conformance --json` as ownership and
  borrow evidence

## Stable v0.24 Release Candidate Hardening Additions

The v0 demo now exposes release-candidate hardening in release-check:

- `fgx release-check --json` includes `hardening_checks`, `hardening_pass`, and
  `hardening_fail`
- hardening checks cover strict release gating, gate evidence completeness,
  diagnostic shape freeze, borrow escape-hatch conformance, and target decision
  consistency
- safe raw pointer parameters report `raw_pointer_requires_unsafe`
- `fgx borrow-conformance --json` includes RC cases for raw pointer parameters,
  unproven conditional lifetime arguments, constant-index projections, and
  unknown-index projection overlap
- `v0.24` is marked completed while `v1.0` remains planned until ownership and
  borrowing move from partial to pass

## Stable v0.25 Loop-Carried Borrow Liveness Additions

The v0 demo now keeps outer borrow aliases live across loop back-edges:

- aliases created before a loop remain live when the loop body reads them
- root writes inside that loop body report `mutable_borrow_alias` when they
  would conflict with an outer alias in a later iteration
- aliases declared inside the loop body remain per-iteration and can end before
  a later root write in that same iteration
- the borrow report lists `loop_carried_borrow_liveness` as an enabled rule
- `fgx borrow-conformance --json` includes `loops.loop_carried_alias_rejected`
  and `loops.loop_local_alias_ok`
- `v0.25` is marked completed while `v1.0` remains planned until ownership and
  borrowing move from partial to pass

## Stable v0.26 Conditional Borrow-State Join Additions

The v0 demo now joins possible borrow roots across conditional branch
reassignment:

- aliases that exist before an `if` keep both pre-branch and branch-assigned
  possible roots after the branch
- indexed writes through any possible root report `mutable_borrow_alias` when
  the alias has a later use
- mutable call argument checks compare every joined alias root
- writes after the joined alias's last use remain accepted by the simple
  non-lexical ending rule
- the borrow report lists `conditional_borrow_state_join` as an enabled rule
- `fgx borrow-conformance --json` includes
  `branches.conditional_alias_reassign_rejected`,
  `branches.conditional_alias_call_rejected`, and
  `branches.conditional_alias_last_use_ok`
- `v0.26` is marked completed while `v1.0` remains planned until ownership and
  borrowing move from partial to pass

## Explicitly Unstable

These are not stable yet:

- exact parser internals
- undocumented syntax
- native compilation behavior beyond C/Zig source transpilers
- real assembly lowering
- complete target ABI/layout computation
- full platform ABI classification
- complete instruction encoding validation
- optimized native SIMD lowering
- real wall-clock benchmarking, CPU counters, and profile-guided optimization
- Zig compiler invocation and `build.zig` emission
- Cranelift, LLVM, and MLIR code generation
- generic standard-library APIs
- planned tiny standard-library modules listed in
  `docs/standard_library_v0.md` until they ship with source modules, catalog
  entries, tests, and compatibility notes
- module-qualified function call syntax
- remote package registries, registry dependency resolution, semantic version
  solving, package build script execution, artifact caches, real signed package
  metadata, and OS-level sandbox execution
- full ownership and borrow checking beyond v0.26 local reborrow,
  loop-carried liveness, conditional borrow-state joins, two-phase activation,
  projection, explicit contract, conformance, and RC hardening checks
- real crypto, real networking transports, real ledgers, secure key storage, and wallet APIs
- persistent workbench sessions, cross-file LSP indexing, non-stdio LSP
  transports, marketplace editor packaging, and editor-side source mutation
  after patch approval
- coverage-guided fuzzing, generated C execution, and native backend
  differential tests
- v2 foreign-language capsules, including embedded Python 3.13, JavaScript,
  TypeScript, Java, C#, C, C++, CUDA, Go, Bash, PowerShell, Solidity, and Vyper
  code, pinned GitHub projects, external package-manager dependencies, and
  sandboxed capsule execution

## Compatibility Rule

The v0 examples are the compatibility floor. A future stage may add syntax and
tooling, but it should not break the current demo workflow without a deliberate
documented migration.
