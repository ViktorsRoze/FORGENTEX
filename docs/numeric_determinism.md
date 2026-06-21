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

`@exact` is a **checked contract**: the compiler proves the function uses
no floating point anywhere in its signature or body and reports
`exact_arithmetic_uses_float` otherwise. So "make this exact" is one
attribute, and the guarantee is verified rather than trusted. Exact
functions use integer or fixed-point math (the Q15 helpers below), which is
bit-exact and identical on every backend and platform. Everything without
`@exact` stays on the fast float path.

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

## Guarantee 4: checked arithmetic

Integer overflow is a defined behavior (trap or saturate), never a silent
wrap, on the checked arithmetic path. The accumulator width is explicit
(`i32`/`i64`) so fixed-point math cannot silently overflow mid-kernel.

## Plan

| Phase | Status | Scope |
| --- | --- | --- |
| `numeric.1` | Done | `@exact` checked opt-in (`exact_arithmetic_uses_float`); Q15 exact gain/mix in `core.audio`; `clamp_i16_from_i32` saturating accumulator. |
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
