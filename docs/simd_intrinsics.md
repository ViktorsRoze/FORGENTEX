# Portable SIMD Intrinsics

Stage 13 adds a small portable SIMD layer for media/DSP demos. The goal is to
let an agent propose vectorized code that remains reviewable, target-aware, and
safe to fall back to scalar behavior. This is still a frontend and interpreter
feature; Forgentex does not emit native SIMD instructions yet.

## Type Forms

Vector and mask types are named generics:

```text
vec[i16, 8]
vec[f32, 4]
mask[8]
```

Canonical `.fgai` spelling removes whitespace, for example `vec[i16,8]`.

`vec[T, N]` represents `N` lanes of scalar element type `T`. `mask[N]`
represents a lane mask for comparisons and selection. The checker requires
matching element types and lane counts for binary vector operations.

## Intrinsic Surface

Typed suffixes encode element type and lane count:

| Operation | Example | Type rule |
| --- | --- | --- |
| Vector constructor | `simd_vec_i16x8(a,b,c,d,e,f,g,h)` | `i16... -> vec[i16, 8]` |
| Splat | `simd_splat_i32x4(x)` | `i32 -> vec[i32, 4]` |
| Load | `simd_load_i16x8(samples, offset)` | `&[i16], usize -> vec[i16, 8]` |
| Store | `simd_store_i16x8(dst, offset, value)` | `&mut [i16], usize, vec[i16, 8] -> void` |
| Widen | `simd_widen_i16x8_i32x8(value)` | `vec[i16, 8] -> vec[i32, 8]` |
| Narrow | `simd_narrow_i32x8_i16x8(value)` | `vec[i32, 8] -> vec[i16, 8]` |
| Add | `simd_add(a, b)` | matching vectors -> same vector |
| Multiply | `simd_mul(a, b)` | matching vectors -> same vector |
| Saturating add | `simd_saturating_add(a, b)` | matching integer vectors -> same vector |
| Compare equal | `simd_eq(a, b)` | matching vectors -> `mask[N]` |
| Select | `simd_select(mask, a, b)` | `mask[N], vec[T,N], vec[T,N] -> vec[T,N]` |
| Reduction add | `simd_reduce_add(value)` | `vec[T,N] -> T` |
| Reduction multiply | `simd_reduce_mul(value)` | `vec[T,N] -> T` |
| Shuffle | `simd_shuffle(value, i0, i1, ...)` | vector plus lane indices -> vector |

All `simd_*` calls infer the `cpu.simd` effect. Stage 16 also exposes
standard-library aliases named `core_simd_*`; they share the same checker,
interpreter, and review behavior. Functions using them should
declare `effects [cpu.simd]`.

## Target Review

Use:

```powershell
fgx simd-review examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu
fgx simd-review examples/media/audio_mix_simd.fgx --target riscv64gc-unknown-linux-gnu --json
```

The review report lists:

- SIMD functions and node IDs.
- Intrinsics used by each function.
- Vector and mask types found in the body.
- Whether a scalar fallback exists.
- Whether a scalar/vector equivalence test marker exists.
- A target lowering plan.

Current lowering plans are review metadata:

| Target family | Review lowering plan |
| --- | --- |
| `x86_64` | `x86_64.sse2`, `x86_64.avx2-if-available`, `scalar-fallback` |
| `arm` | `armv7.neon-if-available`, `scalar-fallback` |
| `aarch64` | `aarch64.neon`, `aarch64.sve-if-available`, `scalar-fallback` |
| `riscv32` | `riscv32.scalar-default`, `riscv32.v-if-cpu.v`, `scalar-fallback` |
| `riscv64` | `riscv64.scalar-default`, `riscv64.v-if-cpu.v`, `scalar-fallback` |

## Fallback Rule

`fgx check <file> --target <triple>` now enforces the SIMD fallback rule:

- A function using `simd_*` must have a scalar fallback.
- Fallbacks are detected by sibling names such as `mix_samples_scalar` for
  `mix_samples_simd`, or by scalar work after a guarded SIMD block.
- A function may opt out with `effects [cpu.simd, target.only]`.

Target-only SIMD is explicit because it removes portability. Review output
surfaces that marker through `target_only=yes`.

## Interpreter Semantics

The interpreter executes the safe portable SIMD subset as arrays of lane values.
This is intentionally simple and deterministic. It is useful for golden
correctness tests, not performance measurement.

`cpu_has_<feature>()` probes currently return `false` in the interpreter, so
portable examples should keep scalar fallback paths runnable.

## Example

`examples/media/audio_mix_simd.fgx` demonstrates:

- `vec[i16, 8]` and `mask[8]`
- vector load, saturating add, equality mask, select, and store
- scalar fallback through `mix_samples_scalar`
- an equivalence test marker through `test_mix_samples_simd_equivalence`

Useful commands:

```powershell
fgx check examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu --json
fgx check examples/media/audio_mix_simd.fgx --target riscv64gc-unknown-linux-gnu --json
fgx run examples/media/audio_mix_simd.fgx --entry mix_samples_simd --json-input examples/run/audio_mix_simd.json
```

## Stable Diagnostics

SIMD diagnostics use the same JSON shape as type and target diagnostics.

| Code | Meaning |
| --- | --- |
| `simd_unknown_intrinsic` | Unknown `simd_*` call. |
| `simd_intrinsic_arity` | Wrong intrinsic argument count. |
| `simd_type_mismatch` | Element type, lane count, source, or destination does not match. |
| `simd_requires_vector` | Intrinsic expected a vector argument. |
| `simd_lane_mismatch` | Mask and vector lane counts do not agree. |
| `simd_requires_mutable_slice` | Store destination is not mutable. |
| `simd_fallback_required` | Target check found SIMD without scalar fallback or `target.only`. |
| `simd_equivalence_test_missing` | Review found no equivalence test marker for a SIMD function. |

## Deferred

The current implementation does not yet perform native instruction selection,
autovectorization, ABI-specific vector layout, CPU dispatch generation, real
wall-clock benchmarking, CPU counters, or profile-guided optimization. Stage 25
adds deterministic performance estimates for review, while real profiling
belongs with later native backend work.
