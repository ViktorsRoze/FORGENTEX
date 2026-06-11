# ABI and Layout Model

Stage 14 adds target-aware ABI/layout reporting. It is still frontend tooling,
not native compilation. The goal is to make memory layout and C ABI boundaries
visible before Forgentex emits real object files.

## Attributes

Attributes are ordinary lines before declarations or fields in `.fgx`:

```forgentex
@repr(C)
@aligned(16)
struct AbiPacket:
    @endian(little)
    version: u32
    flags: u8
    _pad0: padding[3]
    data: *const u8
    len: usize

@extern(C)
unsafe fn inspect_packet(packet: *const AbiPacket) -> usize:
    return 0
```

Agent `.fgai` uses compact attribute lines before declarations and inline field
attributes:

```text
A repr(C)
A aligned(16)
S AbiPacket { version u32 @endian(little); flags u8; _pad0 padding[3]; data *constu8; len usize }
```

Supported Stage 14 attributes:

| Attribute | Applies to | Meaning |
| --- | --- | --- |
| `@repr(C)` | struct | Use C-compatible field ordering and padding rules. |
| `@packed` | struct | Use 1-byte field alignment. |
| `@aligned(N)` | struct | Raise final struct alignment to `N`, where `N` is a power of two. |
| `@endian(little\|big\|native)` | integer field | Mark byte order for serialized integer fields. |
| `@extern(C)` | function | Mark a C ABI function boundary for reports and headers. |

Explicit padding uses `padding[N]`, which becomes `N` bytes in ABI reports and
`uint8_t name[N]` in generated headers.

## Commands

```powershell
fgx abi examples/abi/layout_demo.fgx --target x86_64-unknown-linux-gnu
fgx abi examples/abi/layout_demo.fgx --target armv7-unknown-linux-gnueabihf --json
fgx header examples/abi/layout_demo.fgx --target riscv64gc-unknown-linux-gnu
```

`fgx abi` emits target family, pointer size/alignment, endian, calling
convention, integer argument and return registers, struct size/alignment, field
offsets, padding, endian markers, and C ABI function summaries.

`fgx header` emits a C header with target metadata macros, alignment/extern
macros, `typedef struct` declarations, explicit padding arrays, packed pragmas,
and C ABI function prototypes.

## Target Model

Stage 14 derives ABI metadata from the Stage 11 target catalog:

| Family | Pointer | Calling convention | Integer args |
| --- | --- | --- | --- |
| x86_64 Windows | 8 bytes | `win64` | `rcx`, `rdx`, `r8`, `r9` |
| x86_64 Unix | 8 bytes | `sysv64` | `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9` |
| ARMv7 | 4 bytes | `aapcs32` | `r0`-`r3` |
| AArch64 | 8 bytes | `aapcs64` or `aapcs64-windows` | `x0`-`x7` |
| RISC-V 32/64 | 4 or 8 bytes | `riscv-elf-psabi` | `a0`-`a7` |

All current catalog targets are little-endian. The same struct can report
different sizes across targets when it contains pointer-sized fields.

## Diagnostics

| Code | Meaning |
| --- | --- |
| `abi_repr_required` | Struct lacks `@repr(C)` for stable C ABI layout. |
| `abi_invalid_alignment` | `@aligned(N)` is missing a power-of-two alignment. |
| `abi_invalid_endian` | Endian annotation is not `little`, `big`, or `native`. |
| `abi_endian_non_integer` | Endian annotation appears on a non-integer field. |
| `abi_invalid_padding` | `padding[N]` does not contain an integer byte count. |
| `abi_recursive_type` | Struct embeds itself recursively by value. |
| `abi_unknown_type` | Layout references an unknown named type. |
| `abi_unsupported_type` | Type has no stable ABI representation in Stage 14. |

## Review Integration

Semantic patch reports already expose `ABI or layout changed`. Adding/removing
structs, struct fields, functions, and function signatures marks that category.
Stage 14 makes the consequence inspectable through `fgx abi`.

## Deferred

Stage 14 does not yet implement full platform ABI classification, bitfields,
unions, variadic functions, dynamic linking metadata, import filtering, or
native code generation.
