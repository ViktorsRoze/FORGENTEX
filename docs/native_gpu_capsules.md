# Native and GPU Capsules (v2.6)

v2.6 adds review for the highest-risk non-shell capsule classes: C and C++
(`native`) and CUDA (`gpu`). These cross a C ABI boundary, may pass raw
memory, and — for CUDA — target a specific GPU architecture. The capsule
contract, sandbox, dependency locking, and fixtures from v2.0–v2.5 apply
unchanged; v2.6 adds native/GPU-specific review.

## Risk classes

`risk_class()` already maps these languages:

| Language | Class |
| --- | --- |
| `c`, `cpp`, `c++` | `native` |
| `cuda` | `gpu` |

Native and GPU capsules are **always at least `review`** — never
auto-approved — and report `not_lowerable` until real native execution
exists.

## Metadata

- **Toolchain**: the capsule `version` carries the C/C++ standard
  (`c11`, `c++17`) or the CUDA toolkit (`cuda-12.4`).
- **Target environment**: `env` carries the target triple
  (`x86_64-pc-windows-msvc`) or, for CUDA, the toolkit plus GPU
  architecture (`cuda-12.4-sm_80`).
- **GPU architecture**: extracted from `env` as an `sm_NN` or
  `compute_NN` token.

## ABI / memory-boundary classification

The typed boundary is classified by whether it passes raw memory:

- **value boundary** — all inputs/outputs are scalars; nothing but values
  crosses the ABI (lower risk).
- **memory boundary** — any input/output is a pointer, slice, or array; a
  raw buffer crosses the C ABI, so ownership, bounds, and lifetime require
  review.

## Validation (blocking at `fgx check`)

- `gpu_capsule_no_target` — a CUDA capsule must declare a GPU architecture
  (`sm_NN` / `compute_NN`) in its `env`. GPU code without a target cannot
  be reviewed or built.

(Native/GPU capsules also remain subject to the v2.0–v2.5 rules: pinned
dependencies, sandbox effect coverage, etc. A CUDA capsule with a
`gpu.compute` effect needs a matching `gpu.*` sandbox grant.)

## Review report

```text
fgx native-review <file> [--json] [--strict]
```

`forgentex.native_review.v1` lists each native/GPU capsule with its
toolchain, target env, GPU arch, memory-boundary classification, fallback
fixture count, effects, and a `review`/`reject` decision. `--strict` exits
non-zero on `reject`. Managed-runtime and other capsules are not included.

## Examples

- `examples/capsule/clamp_c.fgx` — a C capsule with a scalar value
  boundary (`f32 -> i16`) and three deterministic fallback fixtures;
  reviews as `native`, value boundary, `review`.
- `examples/capsule/saxpy_cuda.fgx` — a CUDA capsule with a buffer
  (memory) boundary, `env "cuda-12.4-sm_80"`, and a `gpu.compute` effect
  covered by a `gpu.compute` sandbox grant; reviews as `gpu`, memory
  boundary, GPU arch `sm_80`, `review`.

## Acceptance gates (v2.6)

- C/C++ capsules classify as `native`; CUDA classifies as `gpu`.
- A CUDA capsule without an `sm_NN`/`compute_NN` target reports
  `gpu_capsule_no_target` and reviews as `reject`.
- A buffer boundary is classified `memory_boundary=true` with an ownership
  review note; a scalar boundary is `value boundary`.
- `fgx native-review --strict` exits non-zero on any rejected capsule.
- Fallback fixtures are tracked and surfaced (a native/GPU capsule with no
  fixtures is noted).
