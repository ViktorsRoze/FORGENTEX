# Forgentex Backend Strategy

Stage 15 defined the first backend boundary for Forgentex; v3.0 hardens that
boundary into a stable `Backend` interface and an explicit dependency policy.
The goal is not to ship an optimizing native compiler yet. The goal is to make
backend choice an explicit toolchain concern while keeping parsing, AST
normalization, checking, effects, semantic patching, target metadata, and
ABI/layout semantics frontend owned.

## Pipeline

The v0 backend pipeline is:

```text
project files
  -> parser
  -> canonical AST
  -> type, effect, target, and ABI checks
  -> FGIR
  -> selected backend
```

FGIR is the Stage 15 backend IR. It is a compact, deterministic lowering of the
checked project into modules, structs, functions, parameters, simple entry
blocks, and review-only assembly markers. It is intentionally close to the typed
AST so that early backends cannot silently reinterpret frontend semantics.

Current schema names:

- backend info catalog: `forgentex.backend_info.v1`
- backend IR: `forgentex.backend_ir.v1`
- frontend contract: `forgentex.frontend_contract.v1`

## CLI

List available and planned backends:

```powershell
fgx backends
fgx backends --json
```

Inspect FGIR:

```powershell
fgx ir .
fgx ir . --json
fgx ir examples/hello.fgx
```

Inspect the frontend contract used to keep backend selection honest:

```powershell
fgx ir . --contract-json
```

Run or build with the current available backends:

```powershell
fgx run examples/media/audio_mix.fgx --entry mix_samples --json-input examples/run/audio_mix.json --backend interp
fgx build . --backend c --out build/c
fgx build . --backend zig --out build/zig
```

Planned backends are registered in the catalog but fail explicitly:

```powershell
fgx build . --backend llvm
```

Those commands currently report that the selected backend is planned for v3.0,
include its feature scaffold and dependency status, and ask the caller to use
`--backend c` or `--backend zig`.

## Backend Catalog

| Backend | Status | Interface | Feature | Default | Dependency status | Target families |
| --- | --- | --- | --- | --- | --- | --- |
| `interp` | available | run | none | yes | zero-dep core | host |
| `c` | available | aot-source | none | yes | zero-dep source emission; host C compiler is external | x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, RISC-V 64-bit |
| `zig` | available | aot-source | none | yes | zero-dep source emission; Zig compiler invocation is deferred | x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, RISC-V 64-bit |
| `cranelift` | planned | jit | `jit-cranelift` | no | feature scaffold only; no Rust dependency linked | x86_64, AArch64 |
| `llvm` | planned | aot-native | `backend-llvm` | no | feature scaffold only; no Rust dependency linked | x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, RISC-V 64-bit |
| `mlir` | planned | dialect | `backend-mlir` | no | feature scaffold only; no Rust dependency linked | x86_64, AArch64, RISC-V 64-bit |

The interpreter is the default run backend. The C backend remains the reference
AOT source path for safe project code and validates the first lowering boundary
before Forgentex commits to native object generation. The Zig backend is a
source backend that can exercise readable native-oriented output, cross-target
compiler behavior, explicit allocation policy, target-feature dispatch, and
SIMD lowering ideas before the project takes on heavier backend dependencies.
Stage 28 emits Zig source only; invoking the Zig compiler and generating
`build.zig` remain deferred.

The default Cargo feature set is empty. Optional native backends have feature
scaffolds (`jit-cranelift`, `backend-llvm`, `backend-mlir`) but no Rust
dependencies linked in this crate. Tests assert that `[dependencies]` remains
empty for the default build.

## FGIR v1 Shape

FGIR v1 records:

- package name and version
- sorted manifest modules
- module path, imports, and source hash
- enum IDs, names, visibility, ordered variants, and optional payload types
- struct IDs, names, visibility, fields, and field types
- function IDs, names, visibility, safety, parameters, return type, effects, and target labels
- a simple `entry` block per function containing linearized statements
- structured assembly as `asm.review_only` instructions
- per-function backend contracts for `interp`, `c`, and `zig`, with
  `ready`/`blocked` status plus deterministic blockers such as runtime-only
  effects, generic runtime types, interpreter-only intrinsics, foreign capsule
  calls, review-only assembly, and backend-specific lowering gaps

FGIR v1 is not SSA, not a register machine, and not an optimization IR. It is a
backend-facing contract that lets C, Zig, Cranelift, LLVM, and MLIR experiments
start from the same checked program, including the sum-type information needed
for tagged-union and `match` lowering. `fgx build --backend c|zig` now
preflights this contract and refuses artifact emission when the selected
backend is blocked. The interpreter backend also lowers the in-memory program to
FGIR, checks the selected entry's `interp` contract, and executes the selected
entry and its Forgentex callees from FIR-backed bodies for
`fgx run --backend interp`, including the default demo run and hosted
`fgx run --live` runs, so review-only entries are stopped at the backend
boundary. The compatibility runtime helpers `fgx io-run` and `fgx net-run`
also generate FGIR before executing their gated I/O and network hosts. The
public in-memory interpreter helpers lower to FGIR by default as well, while
the explicit `*_with_ir` helpers keep CLI and tests able to provide a
precomputed lowering.
Successful C/Zig wrapper builds also write
`forgentex.fgir.json` and `forgentex.backend_build.json` beside the generated
source artifacts; the receipt records the selected backend and FIR hash so every
source build can be traced back to the exact lowering contract. C enum/struct
declarations, C header prototypes, C function definitions, Zig struct
declarations, and Zig function headers now derive their declaration shape,
visibility, parameters, and return types from FIR. C/Zig bodies now consume FIR
for straight-line statements plus structured `if`/`else`, `for`, `while`,
`break`, and `continue` markers. C and Zig both consume FIR
`match`/`case`/`endmatch` markers for plain enums and payload-carrying tagged
unions; Zig emits data-less enums as `enum` declarations and payload enums as
`union(enum)`. Build preflight also scans FIR declarations for source-backend
type gaps, so generic struct fields or enum payloads are reported before source
emission. C/Zig body emission now fails visibly if FIR emission cannot complete
instead of silently re-walking AST statements, and the error includes the FIR
cursor reason (for example an unsupported op or unexpected terminator).
v3.1 is complete only when future native entrypoints consume the same FIR
contract directly.

## Frontend Contract

Backend choice must not change the frontend view of the program. The contract
JSON includes:

- package name and version
- canonical AST hash
- type diagnostics
- effect analysis output

Tests compare this contract before and after invoking the C backend. Planned
native backends fail before generating artifacts, so they cannot alter frontend
state or source files.

## Target Modularity

Target knowledge stays behind explicit target and backend boundaries:

- Stage 11 target triples define canonical x86_64, ARMv7/ARM, AArch64, RISC-V
  32-bit, and RISC-V 64-bit targets.
- Stage 12 structured assembly validates architecture-specific register and
  feature metadata before any real machine-code lowering.
- Stage 13 portable SIMD reports lowering plans for x86_64, ARM/AArch64, and
  RISC-V while preserving scalar fallback checks.
- Stage 14 ABI/layout reports target pointer widths, alignment, and C ABI
  summaries.
- Stage 15 FGIR carries target labels and review-only assembly markers to
  backend experiments.

Future target lowerers should be independent modules. A backend can support a
target family only when its lowering, ABI, runtime, tests, and diagnostics are
all represented.

## Planned Backend Sequence

1. Keep the C backend as the reference transpiler for safe code.
2. Exercise FGIR with more examples and differential tests against the interpreter.
3. Use the Zig source backend for the same safe subset as C, with generated
   `.zig` artifacts.
4. Use the Zig track to test explicit allocator effects, target-feature
   dispatch, `@Vector`-style SIMD lowering, exact layout mapping, and
   cross-target compile diagnostics.
5. Prototype a Cranelift path for fast native/JIT experiments on x86_64 and AArch64.
6. Prototype an LLVM path for broad native support, including ARM and RISC-V.
7. Explore MLIR only after the language has enough high-level media or dataflow
   structure to justify dialect work.

## Zig Backend Track

The Zig backend is a narrow source-generation track, not a replacement for C.
Its Stage 28 milestone lowers the same safe project subset as the C backend and
fails on the same unsafe, assembly, generic, and runtime-only effect cases.

Zig-specific experiments should stay reviewable:

- allocator-taking APIs and allocation effects should be explicit in FGIR,
  diagnostics, and performance reports
- comptime-like specialization should be represented as deterministic metadata,
  such as lane count, element type, target feature, or fixed buffer size
- portable SIMD should have scalar fallback and equivalence evidence before
  lowering toward Zig vector forms
- safe/debug builds should preserve traps and bounds checks, while optimized
  modes may only remove or narrow checks when review evidence explains why
- generated Zig artifacts should become eligible for backend differential tests
  against the interpreter and C output before being used for performance claims

## Deferred

- native object files and executable linking
- Zig compiler invocation and `build.zig` emission
- real LLVM, Cranelift, or MLIR dependencies
- real assembly lowering
- optimizer passes
- register allocation
- full platform ABI classification
- cross-compilation toolchain discovery
- production runtime packaging
