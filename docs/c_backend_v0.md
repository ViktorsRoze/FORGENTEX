# Forgentex C Backend v0

Stage 10 adds the first native-oriented backend: a small C11 transpiler for the
safe Forgentex subset. It is intentionally conservative. The backend proves that
the AST can become portable native artifacts before Forgentex has a serious
optimizer or machine-code backend.

Stage 15 wraps this backend behind the shared backend interface. It remains the
reference transpiler used by `fgx build --backend c`, while Zig is available
as a source backend and Cranelift, LLVM, and MLIR are registered as planned
backends in [backend_strategy.md](backend_strategy.md).

## CLI

Build the manifest project:

```powershell
fgx build . --backend c
```

Choose an output directory:

```powershell
fgx build . --backend c --out build/c
```

The command writes:

- `forgentex_demo_media.h`
- `forgentex_demo_media.c`

The package name from `Forgentex.toml` is converted into the C artifact stem.

## Lowered Subset

The v0 C backend lowers safe functions only:

- structs and positional struct constructors
- primitive numeric and `bool` values
- arrays represented as generated C structs
- immutable and mutable slices represented as `{ data, len }`
- `let`, `return`, `require`, `if`, `for`, assignments, expression statements
- arithmetic, comparisons, casts, calls, indexing, field access
- `.len` on slices and arrays
- fresh annotated function-local `Vec[i32]` with explicit `effects [alloc]`,
  bounded push, discarded pop, checked index/len/write, and return-safe cleanup

The backend rejects:

- `unsafe fn`
- structured `asm`
- interpreter-only `Option` / `Result` intrinsics
- generic types that do not have a v0 C representation
- effectful runtime APIs such as filesystem, networking, serialization, or
  streaming; allocation is admitted only for the validated local-`Vec[i32]`
  subset, while direct allocator calls remain FIR-blocked

## Type Mapping

Primitive types map to fixed-width C types:

- `i8`, `i16`, `i32`, `i64` -> `int8_t`, `int16_t`, `int32_t`, `int64_t`
- `u8`, `u16`, `u32`, `u64` -> `uint8_t`, `uint16_t`, `uint32_t`, `uint64_t`
- `usize` -> `size_t`
- `isize` -> `ptrdiff_t`
- `f32`, `f64` -> `float`, `double`
- borrowed `str` -> `const char *`; `str_len`, `.len`, `str_contains`, and
  content `==` / `!=` lower through a null-checked, allocation-free C11 helper
  surface bounded below 16 MiB. Owned/allocating operations (`str_concat`,
  `int_to_str`, `str_slice`) plus Option/Vec-returning search/split remain FIR
  blockers until ownership and deallocation are explicit
- `bool` -> `bool`
- eligible local `Vec[i32]` -> `{ int32_t *data, size_t len, size_t capacity }`;
  growth is capped at 1,048,576 elements and every live return path frees it
- named structs -> `typedef struct Name { ... } Name`

Slices become generated C structs:

```c
typedef struct fgx_const_slice_i16 {
    const int16_t *data;
    size_t len;
} fgx_const_slice_i16;

typedef struct fgx_slice_i16 {
    int16_t *data;
    size_t len;
} fgx_slice_i16;
```

## Runtime Helpers

The header includes small runtime helpers:

- `fgx_trap(code)` aborts deterministically.
- `fgx_bounds_index(index, len)` checks slice/array indexes.
- checked `i64` arithmetic helper functions are emitted for the backend runtime
  contract. The current expression lowerer still emits direct arithmetic for the
  demo subset and will route more operations through helpers in a later stage.

`require` lowers to:

```c
if (!(condition)) fgx_trap("require_failed");
```

Indexing lowers to:

```c
slice.data[fgx_bounds_index((size_t)(i), slice.len)]
```

## Public API

Header prototypes are generated for exported project symbols. Internal safe
functions are emitted as `static` definitions in the `.c` file.

Stage 14 also adds `fgx header <file> --target <triple>` for ABI-focused C
headers from a single file. That command is separate from `fgx build --backend c`
and includes target layout metadata, alignment macros, packed structs, explicit
padding arrays, and C ABI function prototypes.

Project exports are read from Stage 8 module metadata:

```text
export fn mix_samples(...)
export struct AudioBlock:
```

## Compilation

When a C compiler is available, the generated artifacts are intended to compile
as C11:

```powershell
cc -std=c11 -I build/c -c build/c/forgentex_demo_media.c
```

On Windows, use the equivalent compiler command for MSVC, clang, or gcc. This
repository's tests snapshot the generated C output because a C compiler is not
assumed to be installed in every development environment.

## Current Limitations

- No generated executable harness yet.
- No C package manifest or build-system file yet.
- No C lowering for unsafe/assembly paths.
- No namespace mangling beyond simple identifier sanitization.
- No full ABI policy; this is a portable-C proof point, not a final FFI story.
- No Cranelift, LLVM, or MLIR lowering yet.
