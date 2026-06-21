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

- **Language: C.** C++/Rust/Zig reduce to the same C ABI (`extern "C"`,
  `#[no_mangle] pub extern "C"`, `export fn`) and join in later slices; for a
  non-C native capsule the bridge produces the plan and reports
  `execution_unsupported_in_slice`.
- **Boundary: scalars** (int/float/bool). Slices and structs with known
  layout join when their marshaling lands.
- **No host compiler** → `compiler_unavailable`, gracefully (CI needs no
  compiler; tests assert the deterministic plan and accept either outcome).
- This builds directly on v2.12: the same host-toolchain detection, and the
  `capsule.o` is the kind of unit the native build links.
