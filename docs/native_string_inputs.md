# Native string inputs (v1.26)

A Forgentex `str` input crosses a live C/C++/Rust/Zig capsule boundary as a
borrowed UTF-8 pointer-length pair. A fixed-byte-length mutable borrow uses
`&mut str`; a bounded resizable value uses `&mut String`:

```c
int32_t inspect(const uint8_t *text, size_t text_len);
int32_t edit(uint8_t *text, size_t text_len);
int32_t resize(uint8_t *text, size_t *text_len, size_t text_capacity);
```

Rust uses `*const u8`/`*mut u8`, `usize`/`*mut usize`, and `usize` capacity;
Zig uses `[*]const u8`/`[*]u8`, `usize`/`*usize`, and `usize` capacity. The byte
length is authoritative: the data is not NUL-terminated, and embedded NULs,
newlines, and multibyte characters are ordinary input bytes.

## Lifetime and ownership

- Forgentex owns the language string and the isolated runner owns its decoded
  temporary byte buffer.
- A `str` pointer is read-only. An `&mut str` pointer may update bytes but may
  not change the byte length.
- An `&mut String` pointer may change `*text_len` from zero through the immutable
  `text_capacity`. The capsule must initialize every byte below the returned
  length and must not alter or exceed the capacity.
- Either pointer is valid only until the capsule call returns. The capsule must
  not retain, reallocate, or free it.
- An `&mut str` or `&mut String` argument must be a direct Forgentex variable.
  The runtime does not expose an interpreter-owned pointer; it copies the value
  into runner memory and replaces the variable only after complete response
  validation.
- Empty strings have length zero; callers must not dereference their pointer.
- Each input is limited to 16 MiB of UTF-8 bytes before compiler lookup.
- Every `&mut String` receives a zero-initialized 16 MiB runner buffer. At most
  four such inputs may be declared because their aggregate reserved capacity
  is capped at 64 MiB before compiler lookup.
- The complete encoded request is limited to 64 MiB, so multiple maximum-size
  values fail preflight instead of diverging across runner implementations.

No allocator identity or interpreter pointer crosses the process boundary.
These are borrows, not ownership transfers.

## Private transport

The parent validates the typed value and encodes it as a `str:` envelope with
two lowercase hexadecimal digits per byte in the private stdin request. This
keeps NULs and newlines from changing the line protocol and keeps payloads off
the process command line and scratch filesystem. The C, C++, Rust, and Zig
runners decode the bounded envelope into a temporary buffer; the Rust and Zig
runners also check the decoded UTF-8 before dispatch.

For either mutable form, the child returns the temporary buffer as a numeric
byte envelope. Before any interpreter state changes, the runner requires a
returned length no larger than capacity and the parent validates the complete
buffer as UTF-8. `&mut str` additionally requires the original byte length;
`&mut String` may grow or shrink within the 16 MiB bound. Invalid UTF-8,
out-of-capacity length, forbidden fixed-string resizing, missing mutation
output, extra output, timeout, crash, or any other response error withholds the
copy-back atomically.

When one call has several mutable string and/or scalar-slice inputs, the
interpreter stages the whole mutation set before writing any variable. The set
must cover every declared mutable input exactly once, preserve fixed lengths,
respect resizable capacities, match every declared type, and target distinct
direct variables. A missing, duplicate, aliased, out-of-capacity, or mistyped
later mutation therefore cannot partially commit an earlier valid value.

Successful `forgentex.native_receipt.v1` evidence records the aggregate
`borrowed_input_bytes` for declared string inputs without recording their
contents. The derived pointer-length signature remains covered by the boundary
hash. Static C/C++ fixture execution does not model this temporary lifetime;
use `fgx run --live` for string-input calls.

Examples:

- `examples/capsule/checksum_utf8_c.fgx`
- `examples/capsule/checksum_utf8_cpp.fgx`
- `examples/capsule/checksum_utf8_rust.fgx`
- `examples/capsule/checksum_utf8_zig.fgx`
- `examples/capsule/title_utf8_c.fgx`
- `examples/capsule/title_utf8_cpp.fgx`
- `examples/capsule/title_utf8_rust.fgx`
- `examples/capsule/title_utf8_zig.fgx`
- `examples/capsule/atomic_mutable_c.fgx`
- `examples/capsule/grow_utf8_c.fgx`
- `examples/capsule/grow_utf8_cpp.fgx`
- `examples/capsule/grow_utf8_rust.fgx`
- `examples/capsule/grow_utf8_zig.fgx`

## Deliberate limits

Retained strings, caller-selected allocators, zero-copy shared memory, string
fields inside native structs, nested ownership, and multiple owned outputs
remain blocked. Retention is incompatible with the current fresh-process call
model because every runner-owned pointer becomes invalid when that call exits.
