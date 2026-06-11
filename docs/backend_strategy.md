# Forgentex Backend Strategy

Stage 15 defines the first backend boundary for Forgentex. The goal is not to
ship an optimizing native compiler yet. The goal is to make backend choice an
explicit toolchain concern while keeping parsing, AST normalization, checking,
effects, semantic patching, target metadata, and ABI/layout semantics frontend
owned.

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

- backend catalog: `forgentex.backends.v1`
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
```

Inspect the frontend contract used to keep backend selection honest:

```powershell
fgx ir . --contract-json
```

Build with the current source backends:

```powershell
fgx build . --backend c --out build/c
fgx build . --backend zig --out build/zig
```

Planned backends are registered in the catalog but fail explicitly:

```powershell
fgx build . --backend llvm
```

Those commands currently report that the selected backend is planned for Stage
15 and ask the caller to use `--backend c` or `--backend zig`.

## Backend Catalog

| Backend | Status | Role | Target families |
| --- | --- | --- | --- |
| `c` | available | reference transpiler | x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, RISC-V 64-bit |
| `zig` | available | source transpiler and cross-target experiment | x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, RISC-V 64-bit |
| `cranelift` | planned | JIT/native experiment | x86_64, AArch64 |
| `llvm` | planned | optimizing native backend | x86_64, ARMv7/ARM, AArch64, RISC-V 32-bit, RISC-V 64-bit |
| `mlir` | planned | dialect and media/GPU experiments | x86_64, AArch64, RISC-V 64-bit |

The C backend remains the executable reference path for safe project code. It
is the backend used to validate the first lowering boundary before Forgentex
commits to native object generation. The Zig backend is a source backend that
can exercise readable native-oriented output, cross-target compiler behavior,
explicit allocation policy, target-feature dispatch, and SIMD lowering ideas
before the project takes on heavier backend dependencies. Stage 28 emits Zig
source only; invoking the Zig compiler and generating `build.zig` remain
deferred.

## FGIR v1 Shape

FGIR v1 records:

- package name and version
- sorted manifest modules
- module path, imports, and source hash
- struct IDs, names, visibility, fields, and field types
- function IDs, names, visibility, safety, parameters, return type, effects, and target labels
- a simple `entry` block per function containing linearized statements
- structured assembly as `asm.review_only` instructions

FGIR v1 is not SSA, not a register machine, and not an optimization IR. It is a
backend-facing contract that lets C, Zig, Cranelift, LLVM, and MLIR experiments
start from the same checked program.

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
