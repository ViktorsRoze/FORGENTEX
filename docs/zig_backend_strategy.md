# Zig Backend Strategy

Status: available source-generation v0.

The Zig backend is a source-generation experiment for the checked Forgentex
safe subset. It sits after the C backend and before heavier native backends.
The goal is to learn from Zig's explicit systems-programming model: allocation
is visible, target features are explicit, layout is inspectable, and
specialization can be deterministic.

## Goals

- Emit readable `.zig` source for the same safe subset accepted by the C
  backend.
- Keep C as the reference transpiler while Zig explores native builds and
  cross-target compilation.
- Use Zig-generated artifacts for backend differential tests before using them
  as performance evidence.
- Preserve the frontend contract: backend choice must not alter parsing, AST,
  type checking, effects, target checks, ABI/layout, or patch review semantics.

## Lowered Subset

The Stage 28 Zig backend lowers:

- exported and internal safe functions
- structs, primitive numeric types, `bool`, arrays, slices, and mutable slices
- `let`, `return`, `require`, `if`, `for`, assignments, and expression
  statements
- arithmetic, comparisons, casts, calls, indexing, field access, and `.len`
- deterministic traps and bounds checks matching interpreter/C behavior

The backend should reject:

- `unsafe fn`
- structured assembly
- unsupported generic runtime values
- runtime-only effects such as filesystem, networking, streaming,
  serialization, and implicit allocation

## Zig-Inspired Speed Ideas

Forgentex should borrow these ideas even before Zig generation is complete:

- **Explicit allocators**: allocation must appear in effects, API signatures,
  and performance reports instead of being hidden in helper code.
- **Comptime-like specialization**: hot paths may specialize by element type,
  lane count, target feature, fixed buffer size, or constant policy, but the
  specialization facts must be recorded in FGIR/review output.
- **Target-feature dispatch**: generated code should make scalar, SIMD, and
  target-only paths explicit and keep fallback behavior reviewable.
- **Vector lowering**: portable `vec[T,N]` intrinsics can later map toward Zig
  vector forms after scalar equivalence tests are present.
- **Layout discipline**: ABI/layout reports should confirm that generated Zig
  structs match Forgentex target layout assumptions before public APIs rely on
  them.
- **Bounds and trap policy**: debug/safe builds keep deterministic traps; any
  optimized build that removes or narrows a check needs review evidence.

## CLI

```powershell
fgx build . --backend zig --out build/zig
fgx build examples/packages/codec_app --backend zig --target aarch64-unknown-linux-gnu --json
```

The command emits a generated `.zig` file. It does not invoke the Zig compiler
and does not emit `build.zig` yet.

## Test Gates

- `fgx backends` and `fgx backends --json` list `zig` as available.
- `fgx build --backend zig` writes a deterministic `.zig` artifact.
- Zig backend snapshots match deterministic output.
- Backend differential tests compare interpreter, C, FGIR, and generated Zig
  entry-symbol behavior before Zig artifacts contribute to performance reports.

## Deferred

- invoking the Zig compiler
- `build.zig` package generation
- real wall-clock benchmarks from generated Zig artifacts
- native SIMD instruction selection through Zig output
- real cross-compilation toolchain discovery
