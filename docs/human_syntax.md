# Forgentex Human Syntax v0

Human syntax uses `.fgx` files as a review surface. It is intentionally small and
indentation-based for the first demo.

```forgentex
struct AudioBlock:
    samples: &[i16]
    rate: u32
    channels: u8

fn clamp_i16(x: f32) -> i16:
    if x > 32767.0:
        return 32767
    if x < -32768.0:
        return -32768
    return i16(x)
```

Supported declarations:

- `struct Name:`
- `fn name(param: Type) -> Type:`
- `fn name(param: Type) -> Type effects [io.read]:`
- `unsafe fn name(param: Type) -> Type:`

Supported statements:

- `let name: Type = expr`
- `let name = expr` is parsed for compatibility, but Stage 4 checking reports
  `local_type_required`
- `return expr`
- `return`
- `require expr`
- `if expr:`
- `for i in start .. end:`
- `target = expr`
- `asm x86_64:` inside `unsafe fn`

Supported types:

- `bool`
- `u8`, `u16`, `u32`, `u64`, `usize`
- `i8`, `i16`, `i32`, `i64`, `isize`
- `f32`, `f64`
- `str`
- named structs
- named generics such as `Option[u8]` and `Result[u8, ParseError]`
- fixed arrays such as `[u8; 4]`
- `&[T]`
- `&mut [T]`
- `*const T`
- `*mut T`

Assembly is parsed and reviewed, but not executed or lowered in v0.
Stage 5 also treats raw pointer indexing as unsafe behavior; use `unsafe fn`
for pointer dereference examples.
Stage 6 and Stage 17 require effectful functions to declare capabilities such
as `io.read`, `fs.read`, `network.write`, `network.http`, `crypto.sign`,
`alloc`, `cpu.simd`, or `cpu.avx2`.
