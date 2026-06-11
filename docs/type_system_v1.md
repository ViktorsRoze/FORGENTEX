# Forgentex Type System v1

Stage 4 makes the demo language meaningfully statically typed while keeping the
surface small enough for the first compiler/tooling loop.

## Type Environments

The checker builds explicit environments before checking function bodies:

- modules: currently a single `root` module placeholder.
- imports: reserved for Stage 8 module imports.
- structs: names mapped to field declarations.
- functions: names mapped to parameter and return signatures.
- locals: parameters, loop variables, assembly outputs, and typed `let` bindings.

Function parameters and returns are always explicit. Local `let` bindings also
require explicit types in v1; literal inference is used only to validate whether
a literal can flow into that explicit destination.

## Type Forms

Canonical agent spelling is the source of truth for structural equality.

```text
bool
u8 u16 u32 u64 usize
i8 i16 i32 i64 isize
f32 f64
str
void
Name
Name[T]
Name[T,E]
vec[T,N]
mask[N]
[T;N]
&[T]
&mut[T]
*constT
*mutT
```

`Option[T]`, `Result[T,E]`, `vec[T,N]`, `mask[N]`, `Hash[Blake3]`,
`Signature[Ed25519]`, `SecretKey[Ed25519]`, `PublicKey[Ed25519]`, and
`TokenAmount[MockLedger]` are represented as named generic types. The v1
checker can parse, format, compare, pass, return, and store them, but does not
yet implement pattern matching or enum constructors.

Stage 13 adds special type rules for portable `simd_*` intrinsics so vector
element types, lane counts, masks, load/store sources, and mutable store
destinations can be checked statically.

Stage 17 adds `str` for deterministic I/O and networking demos. The type is
first-class for parameters, returns, literals, JSON inputs, and `Result`
payloads, but the standard string operation surface remains intentionally tiny.

Stage 18 adds secret-aware rules for `SecretKey[...]`: safe code can pass a
secret to approved signing and public-key derivation APIs, but logging,
serialization/export, and copying produce `secret_leak` diagnostics unless the
function is explicitly unsafe.

Arrays have fixed length syntax `[T; N]` in `.fgx` and `[T;N]` in `.fgai`.
Indexing arrays, slices, and pointers returns their element type.

## Literal Rules

Integer literals start as `$int` until checked against an explicit destination
type. They may flow into integer and floating numeric destinations when the
destination accepts the value.

Floating literals start as `$float` and may flow only into `f32` or `f64`
destinations without an explicit cast.

Integer literals are range checked for:

```text
u8 u16 u32 u64 usize
i8 i16 i32 i64 isize
```

Unsigned destinations reject negative literals. For `usize`, v1 uses a portable
unsigned 64-bit range until target-aware pointer widths arrive in Stage 11.

## Cast and Arithmetic Rules

Numeric casts are explicit expressions such as `u8(value)` or `f32(sample)`.
Arithmetic between two concrete numeric types requires matching types. Mixed
concrete arithmetic such as `i16 + i32` is rejected until one side is explicitly
cast.

Literals are still flexible:

```text
i32 + 1    // ok
f32 + 1    // ok
i32 + 1.0  // rejected without a cast
```

## Struct Constructors and Fields

Struct construction uses positional arguments in v1:

```text
struct Byte:
    value: u8

fn make() -> Byte:
    return Byte(255)
```

The checker validates constructor arity and each argument against the matching
field type. Field access validates that the base is a known struct and that the
requested field exists.

Named field initialization is intentionally deferred; the checker documents this
by treating positional constructor order as the v1 initialization rule.

## Diagnostics

All type-system diagnostics include a stable code, node ID, message, suggested
patch kind, and expected/found types when they are useful for repair.

Machine-readable output is available through:

```powershell
fgx check examples/media/audio_mix.fgx --json
```

Stable diagnostic codes are listed in
[diagnostics.md](diagnostics.md).
