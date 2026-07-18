# C-ABI FFI Bridge (v2.13)

Native-class capsules now **execute for real** instead of mock. The typed
capsule boundary becomes a real `extern "C"` signature, the foreign code is
compiled and linked through the host toolchain, and each fixture runs as a
real execution oracle.

## How it works

```text
typed boundary  ->  extern "C" signature  ->  boundary.h
capsule body    ->  capsule.c  ->  cc -c  ->  capsule.o
generated harness (calls per fixture, against boundary.h)  ->  runner.o
link capsule.o + runner.o  ->  run  ->  compare stdout to fixture outputs
```

Because the harness compiles against the derived `boundary.h` and links
against the separately compiled `capsule.o`, the **C compiler itself enforces
the boundary**: if the body's signature does not match the typed boundary,
the link fails. The fixtures are no longer just type checks — they are run
for real and compared.

## Usage

```text
fgx capsule-run <file> --native [--capsule <name>] [--out <dir>] [--json]
```

## Live interpreter invocation (native-call v1.23)

Scalar C boundaries, C++ boundaries explicitly exported as `extern "C"`, and
Forgentex-owned scalar slices are callable by a running Forgentex program.
Rust scalar, scalar-slice, and restricted struct boundaries are live through a
public, no-mangle `extern "C"` function. Numeric scalar, scalar-slice, and
restricted struct Zig 0.14 boundaries are live through an explicit
`pub export fn`:

```text
fgx run examples/capsule/live_clamp_c.fgx --entry clamp_live --json-input examples/run/live_clamp_c.json --live
fgx run examples/capsule/sum_i16_c.fgx --entry sum_live --json-input examples/run/sum_i16_c.json --live
fgx run examples/capsule/add_gain_i16_c.fgx --entry boost_then_first --json-input examples/run/add_gain_i16_c.json --live
fgx run examples/capsule/add_gain_i16_cpp.fgx --entry boost_cpp_then_first --json-input examples/run/add_gain_i16_cpp.json --live
fgx run examples/capsule/render_params_c.fgx --entry render_param_score --json-input examples/run/render_params_c.json --live
fgx run examples/capsule/clamp_rust.fgx --entry clamp_with_rust --json-input examples/run/clamp_rust.json --live
fgx run examples/capsule/sum_i16_rust.fgx --entry sum_with_rust --json-input examples/run/sum_i16_rust.json --live
fgx run examples/capsule/add_gain_i16_rust.fgx --entry boost_rust_then_first --json-input examples/run/add_gain_i16_rust.json --live
fgx run examples/capsule/render_params_rust.fgx --entry adjust_render_params --json-input examples/run/render_params_rust.json --live
fgx run examples/capsule/clamp_zig.fgx --entry clamp_with_zig --json-input examples/run/clamp_zig.json --live
fgx run examples/capsule/sum_i16_zig.fgx --entry sum_with_zig --json-input examples/run/sum_i16_zig.json --live
fgx run examples/capsule/add_gain_i16_zig.fgx --entry boost_zig_then_first --json-input examples/run/add_gain_i16_zig.json --live
fgx run examples/capsule/render_params_zig.fgx --entry adjust_render_params_with_zig --json-input examples/run/render_params_zig.json --live
fgx run examples/capsule/make_bytes_c.fgx --entry bytes_from_c --json-input examples/run/make_bytes_c.json --live
fgx run examples/capsule/make_bytes_rust.fgx --entry bytes_from_rust --json-input examples/run/make_bytes_rust.json --live
fgx run examples/capsule/make_bytes_zig.fgx --entry bytes_from_zig --json-input examples/run/make_bytes_zig.json --live
fgx run examples/capsule/make_samples_c.fgx --entry samples_from_c --json-input examples/run/make_samples_c.json --live
fgx run examples/capsule/make_samples_rust.fgx --entry samples_from_rust --json-input examples/run/make_samples_rust.json --live
fgx run examples/capsule/make_samples_zig.fgx --entry samples_from_zig --json-input examples/run/make_samples_zig.json --live
fgx run examples/capsule/make_values_c.fgx --entry values_from_c --json-input examples/run/make_values_c.json --live
fgx run examples/capsule/make_values_rust.fgx --entry values_from_rust --json-input examples/run/make_values_rust.json --live
fgx run examples/capsule/make_values_zig.fgx --entry values_from_zig --json-input examples/run/make_values_zig.json --live
fgx run examples/capsule/make_greeting_c.fgx --entry greeting_from_c --json-input examples/run/make_greeting_c.json --live
fgx run examples/capsule/make_greeting_rust.fgx --entry greeting_from_rust --json-input examples/run/make_greeting_rust.json --live
fgx run examples/capsule/make_greeting_zig.fgx --entry greeting_from_zig --json-input examples/run/make_greeting_zig.json --live
fgx run examples/capsule/persist_bytes_c.fgx --entry persist_bytes --json-input examples/run/persist_bytes_c.json --live
```

`fgx run --live` derives the same `extern "C"` declaration, compiles the
capsule and a generated one-call runner, then launches that runner as a child
process with a private line-delimited request file. C++ uses `$CXX` (then
`c++`, `clang++`, `g++`) for compilation and linking; its exported function
must use explicit `extern "C"` linkage. Rust uses `$RUSTC` (then `rustc`) and
requires edition `2018`, `2021`, or `2024` plus
`#[unsafe(no_mangle)] pub extern "C" fn` (the older `#[no_mangle]` spelling is
also accepted). This avoids OS command-line limits for
bounded slices. Its JSON scalar result returns to the
interpreter through the normal capsule-call path and appears in the
`capsule_calls` audit transcript. A missing language toolchain fails explicitly
with `native_runtime_unavailable` and never substitutes a mock result.

Zig live calls use `$ZIG` and then `zig`, require the capsule and detected host
toolchain to match the reviewed 0.14 line, and compile a private one-call
`ReleaseSafe` executable. Zig accepts numeric scalar inputs, read-only or
mutable numeric scalar slices as pointer/length pairs, and one numeric scalar,
restricted struct, reviewed owned `Vec[u8]`/`Vec[i16]`/`Vec[i32]`, or owned
UTF-8 `str` return.
Mutable slices use the same bounded temporary allocation and atomic
copy-back envelope as C and Rust. A named struct must be `@repr(C)` in
Forgentex and have an exact public `extern struct` declaration in Zig with the
same field order, names, and scalar types. Booleans, nested structs, arbitrary
pointers, and other owned element layouts remain blocked before compiler lookup.

Each successful native call attaches `forgentex.native_receipt.v1` inside its
`capsule_calls` audit entry. The receipt records the FNV-1a hash of the derived
boundary signature, compiler name/version, elapsed native duration, and any
applied `limit.wall_ms`; it deliberately excludes the request payload.
The receipt records `containment: process_tree`. Windows uses a private Job
Object with kill-on-close; Unix uses a private process group. Descendants are
terminated when the runner exits or times out, so background work cannot
outlive a native call.

Declared `limit.cpu_ms` and `limit.mem_mb` budgets are installed before the
runner begins foreign work and are recorded as `cpu_limit_ms` and
`memory_limit_bytes`. Windows applies aggregate job CPU and committed-memory
limits. Unix descendants inherit `RLIMIT_CPU` and `RLIMIT_AS`; CPU time is
rounded up to the platform's whole-second granularity and the limits apply per
process rather than as an aggregate tree quota.
Budget exhaustion terminates or constrains the runner and currently surfaces
as `native_capsule_failed`.

Failed live native dispatches attach a `forgentex.native_failure.v1` receipt to
the attempted capsule-call audit entry. It contains a stable failure `kind`,
the `preflight`, `compile`, `launch`, `execute`, `response`, or `persist`
phase, elapsed time, declared wall/CPU/memory limits, and whether those limits
and process-tree containment were active. The structured receipt deliberately
omits arguments, foreign output, and stderr; the caller-facing error remains a
separate diagnostic. A preflight or compile failure therefore cannot claim
that runtime containment was applied, while timeouts and crashes record it.

An input declared with `sandbox "audit.redact <input-name>"` still crosses the
checked native request boundary unchanged, but its capsule-call transcript
slot becomes `{"redacted":true}` and the audit entry names the binding in
`redacted_inputs`. Redaction happens before both effect denial and dispatch, so
denied, failed, and successful attempts follow one rule. Invalid or unknown
input names fail `fgx check`. Pipeline reports also mask a redacted stage input
and its shared source value instead of reintroducing the value through the
composition audit. Failed redacted calls retain their stable failure kind but
replace foreign stderr in both the audit entry and caller diagnostic.

Warm live hosts now cache the compiled C/C++/Rust/Zig runner by a SHA-256 key
covering the checked program, capsule contract/body, toolchain, ABI signature,
language standard, and platform. The second call skips compilation and records
`runner_reused: true`. Runtime inputs are never part of the key or retained
cache: the private request file is deleted after every attempt, and the host
removes its runner directories when it is dropped. Each call still starts a
fresh process under the normal containment and wall/CPU/memory limits; this is
compile reuse, not a persistent foreign process. `--cold` keeps the original
compile-per-call behavior.

A read-only `&[T]` input with a scalar element type lowers to two C parameters:
`const T *name, size_t name_len`. A mutable `&mut [T]` lowers to
`T *name, size_t name_len`. The child runner validates every element, rebuilds
a bounded temporary buffer, calls the capsule, emits a typed result envelope,
then emits one validated copy-back envelope per mutable slice before freeing
the buffer. The scalar result and every copy-back envelope validate as one
response; copy-back must have the original length and scalar element type.

Rust uses the same ownership contract with an explicit export signature:
read-only slices receive `*const T, usize`, mutable slices receive
`*mut T, usize`, and the capsule may construct a Rust slice only for the call's
duration. The generated child owns the temporary `Vec<T>`. A mutable vector is
serialized only after the call, then Forgentex validates the complete result
and every fixed-length mutation before replacing interpreter state. Retaining
either pointer is forbidden.

An owned `Vec[u8]` output lowers to `FgxOwnedU8 { ptr, len, capacity }` in C,
Rust, or Zig. All three backends may also return `Vec[i16]` or `Vec[i32]`
through the parallel typed descriptors. Every layout requires a typed
`<capsule>_free(ptr, len, capacity)` export in the same allocator domain. The
descriptor length and capacity count elements; the child rejects null
non-empty descriptors, capacities smaller than length, and allocations whose
byte size exceeds 64 MiB. It copies the typed elements before calling the
capsule deallocator exactly once, range-checks the response, and exposes an
interpreter-owned vector. The receipt records the actual byte footprint in
`owned_output_bytes`; its boundary hash covers both exports. See
`docs/native_owned_outputs.md`.

An owned `str` output uses `FgxOwnedStr { ptr, len, capacity }`, where length
and capacity count bytes. The isolated child copies and frees the allocation,
then the parent validates UTF-8 before constructing the language string. Bytes
cross the child envelope numerically, so embedded commas, newlines, NULs, and
multibyte characters cannot change its structure. Owned strings record their
byte footprint but are not artifacts and cannot opt into artifact policy.

An owned byte output may additionally declare `artifact.media_type` and
`artifact.max_bytes` sandbox rules. They are validated before compiler lookup,
the declared ceiling is enforced before the value reaches the interpreter,
and successful calls attach `forgentex.native_artifact.v1` metadata containing
the media type, byte length, and SHA-256 content hash. The boundary hash also
covers this policy. This is in-memory artifact identity and policy, not file
persistence.

Adding `artifact.path` turns that identity into governed persistence. The path
must be relative and contained by a declared relative `fs.write` scope, and the
capsule/caller effect chain must include `fs.write`. Forgentex publishes the
validated bytes atomically from the parent; the foreign child never sees the
path. Identical content is idempotent, while an existing different artifact
fails closed. The receipt records the persisted path.

A `@repr(C)` named struct can cross **by value** as an input or the one return
value when every field is an unannotated scalar primitive. Forgentex generates
`types.h` for the private C/C++ child build; that capsule body must include it
before defining the matching function. Rust capsule code must declare the same
public `#[repr(C)]` struct with exactly matching public fields, declaration
order, and scalar types; the contract is rejected before compiler lookup when
the declarations differ. The child serializes every returned field and
validates the complete value before the interpreter retags it as the declared
struct. Nested structs,
pointers, strings, arrays, packed/custom-aligned layouts, field endian
annotations, and owned buffers remain deliberately blocked.

For mutable inputs, the Forgentex call argument must be a direct variable. The
runtime replaces that variable only after the child response validates, so an
array literal, indexed expression, or retained foreign pointer cannot become a
hidden write path. The capsule never receives an interpreter-owned pointer and
must not retain the temporary pointer after the call.

This is intentionally C/C++/Rust scalar values, scalar slices, and restricted
by-value scalar-field structs, plus one owned `Vec[u8]`, `Vec[i16]`,
`Vec[i32]`, or UTF-8 `str` output in C, Rust, or Zig. Persistent native workers,
string inputs, additional element layouts, multiple owned outputs, GPU dispatch,
Aggregate Unix tree budgets, filesystem/network byte budgets, and broader
OS-level sandbox enforcement remain later slices. A declared `limit.wall_ms`
terminates the native process tree and reports
`native_capsule_timeout`. The child process
provides interpreter-process isolation; it is not a substitute for an OS
sandbox. A non-zero child exit (including an abort/crash) is contained and
reported as `native_capsule_failed`; its failure receipt records the stable
class and containment evidence without copying the raw diagnostic.

```text
$ fgx capsule-run examples/capsule/clamp_c.fgx --native
Forgentex C-ABI FFI bridge
schema=forgentex.ffi_plan.v1 file=examples/capsule/clamp_c.fgx capsule=@capsule.clamp_sample lang=c class=native
  extern "C": int16_t clamp_sample(float x)
    in   f32          -> float      x
    out  i16          -> int16_t    y
  gate: allow effects=[cpu.avx2] grants=[fs.none] deps_pinned=true
  toolchain: cc (cc (GCC) 15.2.0)
  link: build/ffi/capsule.o + build/ffi/runner.o
  execution: executed (3/3 fixtures)
    [0] (40000.0) -> 32767 expected 32767 ok
    [1] (-40000.0) -> -32768 expected -32768 ok
    [2] (100.0) -> 100 expected 100 ok
```

## The gate (real, not review)

Linking and execution are refused unless:

- the capsule passes `fgx check` — its effects are covered by sandbox grants
  and its contract is valid, and
- its dependencies are pinned (the v2.3 supply-chain rule).

A denied capsule reports `gate_decision=deny` with reasons and never compiles
or runs. This moves the effects/sandbox/lock spine from review-time to
**runtime enforcement**.

## The report: `forgentex.ffi_plan.v1`

| Field | Meaning |
| --- | --- |
| `extern_c_signature` | the C-ABI signature derived from the typed boundary |
| `marshaling` | per binding: Forgentex type → C type, in/out |
| `gate_decision` / `gate_reasons` | allow/deny + why (effects, grants, deps) |
| `link_units` | the objects linked (`capsule.o`, `runner.o`) |
| `execution_status` | `executed`, `mismatch`, `denied`, `compiler_unavailable`, `compile_failed`, `unsupported_boundary`, `execution_unsupported_in_slice` |
| `transcript` | per fixture: inputs → output vs expected, pass |

## Scope and honesty (first slice)

- **Language:** C directly, C++ with an explicit `extern "C"` export, Rust
  scalar/slice/struct live calls with an explicit public no-mangle C ABI export,
  and pinned Zig 0.14 numeric scalar/slice/struct/owned-vector calls through `pub export fn`.
  Another native capsule still produces the plan and reports
  `execution_unsupported_in_slice`.
- **Boundary:** C/C++ scalar inputs/outputs, scalar-element `&[T]`/`&mut [T]`
  inputs, and restricted `@repr(C)` scalar-field structs by value as inputs or
  the one return value. Rust shares that restricted struct contract and permits
  one scalar, struct, owned `Vec[u8]`, `Vec[i16]`, or `Vec[i32]` return. Mutable
  inputs use fixed-length copy-in/copy-back; owned vectors use the explicit
  typed descriptor/free contract. Zig
  supports numeric scalars, scalar slices, restricted structs, and the same
  reviewed owned-vector returns. Governed artifact metadata and scoped
  immutable persistence remain byte-vector-only.
- **No host compiler** → `compiler_unavailable`, gracefully (CI needs no
  compiler; tests assert the deterministic plan and accept either outcome).
- This builds directly on v2.12: the same host-toolchain detection, and the
  `capsule.o` is the kind of unit the native build links.
