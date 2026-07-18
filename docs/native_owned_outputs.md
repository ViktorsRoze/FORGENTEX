# Native owned outputs (v1.23)

Forgentex native capsules may return one newly allocated `Vec[u8]` through a
reviewed descriptor. This first owned-output boundary targets encoded
artifacts, frame bytes, and codec output whose length is not known in advance.
C, Rust, and Zig capsules may additionally return `Vec[i16]`, the first typed
owned buffer for audio/sample data, or `Vec[i32]`, a broader integer layout
that also matches the native useful-language collection subset. The same
three languages may return one owned `str`; Forgentex validates its bytes as
UTF-8 before constructing the language value.

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

typedef struct FgxOwnedStr {
    uint8_t *ptr;
    size_t len;
    size_t capacity;
} FgxOwnedStr;
```

A byte capsule named `render` must also export
`void render_free(uint8_t *ptr, size_t len, size_t capacity)`; typed capsules
use the same signature with `int16_t *ptr` or `int32_t *ptr`. Rust uses the
equivalent public `#[repr(C)]` type and public no-mangle free export. Zig 0.14
uses the exact public `extern struct` with the matching nullable pointer type
and a matching `pub export fn render_free`. Allocation and deallocation must
stay in the same capsule/toolchain domain.

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
- Every serialized element is range-checked before creating the corresponding
  Forgentex-owned vector.
- Every owned string is validated as UTF-8 after copy and deallocation but
  before interpreter state or mutable copy-back can change.
- No foreign pointer, capacity, or allocator identity reaches the interpreter.
- Static fixtures do not execute owned allocations; use `fgx run --live`.

Successful calls record the actual allocation footprint in bytes as
`owned_output_bytes` in
`forgentex.native_receipt.v1`. The boundary hash covers the return and required
deallocator signatures.

## Artifact policy (v1.2)

An owned byte-vector output may opt into a reviewed artifact contract. Typed
vectors and owned strings do not implicitly become byte artifacts and cannot
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

Only one owned return is supported: `Vec[u8]`, `Vec[i16]`, `Vec[i32]`, or
UTF-8 `str` in C/Rust/Zig. String inputs, other vector element types, multiple
owned values, nested ownership, caller-selected allocators, streaming,
zero-copy shared memory, retained pointers, mutable artifact replacement, and
artifact deletion remain blocked.
