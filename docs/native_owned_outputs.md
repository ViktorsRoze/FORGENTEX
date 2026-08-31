# Native owned outputs (current pre-0.1 contract)

Forgentex C, C++, Rust, and pinned Zig 0.14 capsules may return one newly allocated
numeric or pointer-free record vector through a reviewed descriptor. Numeric layouts
are `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `isize`, `usize`,
`f32`, and `f64`. A record element must be a named `@repr(C)` struct accepted by
the existing recursively composed scalar-leaf layout review. The same four languages may return one owned `str`; Forgentex validates
its bytes as UTF-8 before constructing the language value.

## ABI contract

```c
typedef struct FgxOwnedU8 {
    uint8_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedU8;

typedef struct FgxOwnedI16 {
    int16_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedI16;

typedef struct FgxOwnedI32 {
    int32_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedI32;

typedef struct FgxOwnedU64 {
    uint64_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedU64;

typedef struct FgxOwnedIsize {
    ptrdiff_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedIsize;

typedef struct FgxOwnedUsize {
    size_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedUsize;

typedef struct FgxOwnedF32 {
    float *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedF32;

typedef struct FgxOwnedF64 {
    double *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedF64;

typedef struct FgxOwnedStr {
    uint8_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedStr;

/* For `Vec[NativeRecord]`; emitted after the reviewed NativeRecord definition. */
typedef struct FgxOwnedNativeRecord {
    NativeRecord *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedNativeRecord;
```

A byte capsule named `render` must also export
`void render_free(uint8_t *ptr, size_t len, size_t capacity)`; typed capsules
use the corresponding exact element pointer, including `float *` or
`double *`. C++ must give both the return function and `<capsule>_free`
explicit `extern "C"` linkage; preflight tokenizes the source and does not
accept claims hidden in comments or string literals. Rust uses the equivalent
public `#[repr(C)]` type and public
no-mangle free export. Zig 0.14 uses the exact public `extern struct` with the
matching nullable pointer type and a matching `pub export fn render_free`.
Allocation and deallocation must stay in the same capsule/toolchain domain.
Record vectors use `FgxOwned<StructName>` and a matching
`<capsule>_free(StructName *ptr, size_t len, size_t capacity)`. Rust must provide
the exact public `#[repr(C)]` descriptor with `*mut StructName`; Zig must provide
the exact public `extern struct` descriptor with `?[*]StructName`. The element
struct itself is source-validated exactly as a by-value native struct: public
field order, names, scalar types, nested records, and fixed-array dimensions
must match the Forgentex declaration. Packed/custom-aligned records, pointer or
string fields, recursive ownership, empty/recursive structs, deeper-than-16
array nesting, and more than 1,024 scalar leaves are rejected.
`FgxOwnedIsize` and `FgxOwnedUsize` are host-ABI-sized: the reviewed native
host matrix is 64-bit, so their elements are eight bytes. The C element types
are `ptrdiff_t` and `size_t`, respectively. `FgxOwnedU64` and 64-bit
`FgxOwnedUsize` retain their complete unsigned ranges through the interpreter's
exact `u64` carrier. Values at or below `INT64_MAX` retain the ordinary signed
carrier; larger values remain unsigned through JSON, native scalar calls,
owned-vector copy-back, arithmetic, comparisons, and pipeline receipts.

Float runners use a round-trip-safe private decimal wire. C and C++ emit nine
significant digits for `f32` and seventeen for `f64`; Rust and pinned Zig use
their shortest round-trip representation. The parent parses `f32` at 32-bit
width before widening it into the interpreter carrier, so all four languages
produce the same canonical value. NaN and infinities are rejected before the
vector reaches interpreter state.

`FgxOwnedStr` length and capacity are byte counts. Its runner copies the bytes
before deallocation and serializes them as numeric bytes over the private wire,
so commas, newlines, NULs, and multibyte text cannot alter the envelope. The
parent rejects invalid UTF-8 before exposing a `str` to the interpreter.

## Runtime rules

- Maximum returned allocation: 64 MiB by element byte size.
- Length and capacity are element counts, not byte counts.
- A null pointer is valid only for zero length.
- Capacity must be at least length.
- The child copies exactly `len` typed elements before invoking the deallocator.
- The deallocator runs exactly once on normal and descriptor-validation paths.
- Every serialized integer is range-checked and every float is narrowed to its
  declared width and checked for finiteness before creating the corresponding
  Forgentex-owned vector.
- Every owned string is validated as UTF-8 after copy and deallocation but
  before interpreter state or mutable copy-back can change.
- Record-vector limits use the reviewed host ABI `sizeof(Record)`, not the
  flattened wire size. The child copies complete records, frees the foreign
  allocation, and emits scalar leaves in declaration/index order. The parent
  requires an exact multiple of the record leaf count, validates every leaf,
  reconstructs nested arrays/records atomically, and restores declared record
  tags recursively.
- No foreign pointer, capacity, or allocator identity reaches the interpreter.
- Static fixtures do not execute owned allocations; use `fgx run --live`.

Successful calls record the actual allocation footprint in bytes as
`owned_output_bytes` in
`forgentex.native_receipt.v1`. The boundary hash covers the return and required
deallocator signatures.

## Artifact policy (v1.2)

An owned byte-vector output may opt into a reviewed artifact contract. Other typed
vectors, record vectors, and owned strings do not implicitly become byte artifacts and cannot
declare this policy:

```text
sandbox "artifact.media_type image/png"
sandbox "artifact.max_bytes 8388608"
```

Both rules are required together, may appear only once, and apply only to a
capsule with exactly one `Vec[u8]` output. The media type must be a valid
`type/subtype` token. The declared maximum must be positive and cannot exceed
the hard 64 MiB native-output ceiling.

The isolated child validates the declared ceiling before copying the foreign
allocation and still calls the deallocator on rejection. The parent validates
the envelope again before exposing it. A successful call adds
`forgentex.native_artifact.v1` metadata to the native receipt with `media_type`,
`bytes`, and a SHA-256 content hash. The native boundary hash covers these
policy values, so changing the artifact contract changes the receipt identity.
Without the optional persistence rule below, this identifies and bounds an
in-memory artifact and grants no filesystem access.

## Governed persistence (v1.3)

An artifact policy may add a destination owned by the Forgentex parent:

```text
effects [alloc, fs.write]
sandbox "fs.write build/artifacts"
sandbox "artifact.media_type image/png"
sandbox "artifact.max_bytes 8388608"
sandbox "artifact.path build/artifacts/preview.png"
```

The path must be relative, contain no parent traversal, and remain within a
relative `fs.write` scope after filesystem resolution. The capsule must declare
the `fs.write` effect, but the foreign child never receives the path and never
writes the artifact itself.

After the owned descriptor, declared ceiling, copied bytes, and SHA-256 have
validated, the parent writes a synced temporary file and publishes it with a
create-only atomic filesystem operation. Re-publishing identical bytes is
idempotent. Existing different content is never overwritten. The artifact
receipt records the relative persisted `path`, and the native boundary hash
covers that path policy.

## Deliberate limits

Only one owned return is supported: the twelve reviewed numeric vector layouts,
one reviewed scalar-leaf record vector, or UTF-8 `str` in C/C++/Rust/Zig.
Record-vector witnesses are `make_records_c`, `make_records_cpp`,
`make_records_rust`, and `make_records_zig`. Immutable borrowed record-vector
inputs use a separate runner-owned `&[Record]` contract demonstrated by
`record_slice_c`, `record_slice_cpp`, `record_slice_rust`, and
`record_slice_zig`. Mutable fixed-count record inputs use the transactional
copy-back contract demonstrated by `mutate_records_c`, `mutate_records_cpp`,
`mutate_records_rust`, and `mutate_records_zig`. Read-only string inputs use the
separate bounded borrow contract in `docs/native_string_inputs.md`.
String-vector inputs, pipeline aggregate handoff, string vector elements, multiple owned values, nested
ownership, caller-selected allocators, streaming, zero-copy shared memory,
retained pointers, mutable artifact replacement, and artifact deletion remain
blocked.
