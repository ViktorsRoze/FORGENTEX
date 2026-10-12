# Numeric Determinism and Precision

Forgentex targets audio, DSP, and other numeric work where wrong or
platform-dependent arithmetic is unacceptable. This document states the
honest position on precision and the plan to guarantee it. It is a
language-core principle, not a capsule feature.

## Policy: fast by default, exact on request

Exact arithmetic costs speed, so it is not the default. Ordinary code uses
IEEE-754 `f32`/`f64` — fast, approximate, and (deliberately) the same
rounding behavior as every other language. When a function must be
perfect, you opt in with a single, cheap token:

```fgx
@exact
fn apply_gain_q15(dst: &mut [i16], gain_q15: i32) -> void:
    ...
```

`@exact` is a **checked contract**: the compiler proves the function uses no
floating point in its signature or body, **nor reaches any through the functions
it calls**, and reports `exact_arithmetic_uses_float` or
`exact_arithmetic_calls_inexact` otherwise. So "make this exact" is one
attribute, and the guarantee is verified rather than trusted. Exact functions use
integer or fixed-point math (the Q15 helpers below), which is bit-exact and
identical on every backend and platform. Everything without `@exact` stays on the
fast float path.

### What the check proves, and what it trusts

The check is a reachability question over the call graph, so the contract holds
for whole call chains rather than for one function body:

- **Transitive.** Every function reachable from an `@exact` function is checked.
  `@exact fn f() -> i32: return g()` is rejected when `g` uses floating point,
  and the diagnostic names both the callee and the function at the far end of
  the chain, so a reader can act on it without tracing the graph by hand.
- **Recursive groups included.** The analysis is a fixpoint over the call graph,
  so self- and mutual recursion are ordinary edges: a recursive group with a
  float anywhere in it is rejected, and one without is accepted.
- **Named types resolved.** A struct or enum that holds an `f32` is floating
  point wherever it appears, however deeply nested, so a parameter cannot smuggle
  a float in behind a type name.
- **Intrinsics trusted, not proven.** A call to a compiler intrinsic is judged by
  the contract it declares, because its implementation is not Forgentex source.
  No intrinsic is float-typed today, so this admits nothing at present — but it
  is a trusted edge, not a verified one.

Two things it deliberately does not claim. It says nothing about **foreign
capsules**, whose bodies are another language entirely. And exactness here means
*the absence of IEEE-754 rounding* — it is not a statement about what integer
arithmetic does at the edges. Overflow, division, and shift behaviour are
specified separately below; an `@exact` function is reproducible on every
backend, which is a different property from being free of surprises.

## The honest baseline: floats are IEEE-754 everywhere

`f32` and `f64` in Forgentex are IEEE-754 binary floating point — the same
representation Python, C, Rust, and every mainstream language use. The
famous "0.1 + 0.2 != 0.3" is a property of binary floating point, not a
bug in any one language. Forgentex **cannot** and does not claim to make
binary floats represent `0.1` exactly; no language can. Anyone who claims
otherwise is wrong.

What Forgentex *can* do — and what actually matters for audio — is remove
the two real sources of numeric surprise and give an exact path where
exactness is required.

## Guarantee 1: exact integer and fixed-point math

Integer arithmetic is bit-exact. The **precise DSP path uses fixed-point
integer math**, not float. The `core.audio` DSP core already demonstrates
this with Q15 fixed-point gain and mix:

```text
apply_gain_q15([20000, -30000, 8000], gain_q15 = 16384)  ->  [10000, -15000, 4000]
```

`gain_q15 = 16384` is `0.5` in Q15 (1.0 = 32768). The result is
`(sample * gain_q15) >> 15` — pure integer multiply and arithmetic shift,
**bit-exact and identical on every backend and platform**, with no
rounding error of any kind. Float (`apply_gain`) remains available as a
convenience path; fixed-point is the precise one.

Planned: a `core.fixed` module with Q15/Q31 helpers, saturating and
rounding variants, and reusable conversions, so any DSP kernel can be
written exactly.

## Guarantee 2: determinism across backends and platforms

When float *is* used, the result must be the same everywhere — the
interpreter, the C backend, the Zig backend, and every CPU must agree.
That means:

- no fast-math / reassociation that changes results,
- no implicit fused-multiply-add contraction unless explicitly requested,
- a defined left-to-right evaluation order,
- differential tests that assert interpreter and C/Zig backends produce
  identical float results for the same kernel and inputs.

Deterministic float is "mathematically correct and reproducible" even
though it is not infinitely precise. This is the standard professional
audio engines rely on.

## Guarantee 3: no implicit coercion, explicit rounding

Python's other surprise is silent coercion (int/float mixing, truncation).
Forgentex requires explicit types and explicit casts, so a conversion that
can lose information is visible in the source. At the conversion boundary
that matters for audio (`f32 -> i16`), rounding is explicit:

- named rounding modes (truncate, round-half-to-even, round-half-away),
- optional dither for `f32 -> i16` quantization,
- saturation (`clamp_i16` / `clamp_i16_from_i32`) instead of silent wrap.

## Guarantee 4: checked arithmetic, specified per width and operation

Integer arithmetic is checked at the **declared width**. `i32 + i32` is a
32-bit addition: it traps if the sum leaves `i32`, even though the same sum
would fit an `i64`. The accumulator width is therefore a real constraint rather
than a naming convention, which is what fixed-point math needs — a Q15 kernel
that overflows its `i32` accumulator is told so at the operation that did it.

Every integer operation, at every width:

| Operation | Result | Trap code |
| --- | --- | --- |
| `a + b`, `a - b`, `a * b` | The exact mathematical result, if it fits the declared type | `integer_overflow` when it does not |
| `a / b`, `a % b` | Truncating division and its remainder (quotient rounds toward zero; the remainder takes the sign of `a`) | `division_by_zero` when `b == 0`; `integer_overflow` for `MIN / -1` and `MIN % -1` on a signed type |
| `a << b`, `a >> b` | The value shifted as a bit pattern; bits leaving the top are discarded, and `>>` on a signed type is arithmetic (the sign bit is replicated) | `invalid_shift` when `b` is outside `[0, width)` |
| `a & b`, `a \| b`, `a ^ b` | Bitwise, per bit, with no overflow possible | — |

The widths are `i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`, `u64`,
and `usize`. `isize`/`usize` are the target's pointer width.

Three details worth stating plainly, because they are the ones that differ
between languages:

- **A shift is not an overflow.** Shifting bits off the top is what a shift
  does, so `0x4000_0000i32 << 2` is `0`, not a trap. Only the *amount* is
  checked. This is the same rule Rust uses.
- **`MIN / -1` traps** because the quotient is one past the maximum. `MIN % -1`
  is mathematically `0`, but it traps too, so the two forms of the same
  operation answer alike — again matching Rust, and matching what C leaves
  undefined.
- **An integer literal has no width of its own.** It takes the width of the
  other operand, or of the binding, parameter, or return type it flows into.
  `let x: i8 = 100 + 27` is an `i8` addition.

### Both backends implement this, and are tested against each other

The interpreter checks each operation at the width the type checker resolved for
it. The C backend lowers each one to an explicit guard — `fgx_checked_i32_add`
and its siblings, one set per width — rather than emitting C's own operators.

That last point is the substantive change. C leaves signed overflow, division by
zero, `MIN / -1`, and a shift amount at or beyond the width **undefined**, so
emitting `a + b` and relying on what the compiler happens to do is not a
semantics at all; it is a different answer per compiler, optimisation level, and
target. Wrapping in particular is a GCC *option* (`-fwrapv`), not a property of
C, and the generated code cannot assume the flags it will be built with.

`fgx diff-test` compiles the generated C and compares its results against the
interpreter, so the two staying in agreement is a test rather than an intention.

## Plan

| Phase | Status | Scope |
| --- | --- | --- |
| `numeric.1` | Done | `@exact` checked opt-in, transitive over the call graph including recursive groups and resolved named types (`exact_arithmetic_uses_float`, `exact_arithmetic_calls_inexact`); Q15 exact gain/mix in `core.audio`; `clamp_i16_from_i32` saturating accumulator. |
| `numeric.2` | Planned | `core.fixed` module: Q15/Q31 types, saturating/rounding multiply, conversions. |
| `numeric.3` | Planned | Documented float determinism guarantee + interpreter-vs-backend differential tests for float kernels. |
| `numeric.4` | Planned | Named rounding modes and dither at the `f32 -> i16` boundary. |
| `numeric.5` | Planned | Optional exact `rational` / `decimal` types for non-realtime "must be exact" math. |

## Summary

Forgentex does not pretend floats are exact. It gives you an **exact
fixed-point integer path** for the math that must be perfect (the audio
DSP core uses it), **deterministic float** for the rest, **explicit casts
and rounding** so nothing is lost silently, and **checked arithmetic** so
overflow is never a silent wrap. That is a stronger and more honest
position than "trust the float," and it directly answers the rounding and
basic-math surprises that frustrate users in other languages.
