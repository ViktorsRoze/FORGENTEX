# Native and GPU Capsules (v2.6)

v2.6 adds review for the highest-risk non-shell capsule classes: C, C++, Rust,
and Zig (`native`) and CUDA (`gpu`). These cross a C ABI boundary, may pass raw
memory, and — for CUDA — target a specific GPU architecture. The capsule
contract, sandbox, dependency locking, and fixtures from v2.0–v2.5 apply
unchanged; v2.6 adds native/GPU-specific review.

## Risk classes

`risk_class()` already maps these languages:

| Language | Class |
| --- | --- |
| `c`, `cpp`, `c++`, `rust`, `zig` | `native` |
| `cuda` | `gpu` |

Native and GPU capsules are **always at least `review`** — never
auto-approved. C, C++, Rust, Zig, and CUDA capsules with explicit C ABI exports
can execute through the process-isolated live bridge. CUDA execution runs on
Linux hosts with an NVIDIA GPU (§ Live CUDA capsules).

## Metadata

- **Toolchain**: the capsule `version` carries the C/C++ standard
  (`c11`, `c++17`) or the CUDA toolkit (`cuda-12.4`, or a whole major release
  such as `cuda-13`).
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
  boundary, GPU arch `sm_80`, `review`. It holds only a kernel, so it is a
  review fixture, not a runnable capsule.
- `examples/capsule/saxpy_live_cuda.fgx` — a runnable CUDA capsule: an
  `extern "C"` host function that copies borrowed slices to the GPU, launches
  a saxpy kernel, and copies the mutable slice back, targeting `sm_120`
  (GeForce RTX 50 series) with CUDA 13.

## Acceptance gates (v2.6)

- C/C++ capsules classify as `native`; CUDA classifies as `gpu`.
- A CUDA capsule without an `sm_NN`/`compute_NN` target reports
  `gpu_capsule_no_target` and reviews as `reject`.
- A buffer boundary is classified `memory_boundary=true` with an ownership
  review note; a scalar boundary is `value boundary`.
- `fgx native-review --strict` exits non-zero on any rejected capsule.
- Fallback fixtures are tracked and surfaced (a native/GPU capsule with no
  fixtures is noted).

## Live CUDA capsules

A CUDA capsule runs like a C++ capsule: `nvcc` compiles its source and a
generated C++ runner into one executable in a private scratch directory, and
each call starts that runner with the request on stdin and a cleared
environment. The boundary function must have explicit `extern "C"` linkage,
and its parameters follow the C++ capsule mapping: a `&[f32]` input is
`const float *x, size_t x_len`, and a `&mut [f32]` is copied in and back.
Device memory, kernel launches, and error checks belong to the capsule's host
code.

```text
foreign saxpy_gpu cuda version "cuda-13.0" env "cuda-13-sm_120" effects [gpu.compute, alloc]:
    input a: f32
    input x: &[f32]
    input y: &mut [f32]
    output ok: bool
    sandbox "gpu.compute device:0"
    sandbox "limit.mem_mb 512"
    code:
        extern "C" bool saxpy_gpu(float a, const float *x, std::size_t x_len,
                                  float *y, std::size_t y_len) { ... }
```

Run it with `fgx run --live`, or call it from Forgentex code like any other
capsule (`examples/capsule/saxpy_live_cuda.fgx`).

### What a CUDA capsule declares

| Declaration | Meaning | Checked |
| --- | --- | --- |
| `version "cuda-13.0"` | the toolkit the source was written for: 13.0 or any later 13.x (`cuda-13` admits any 13.x). CUDA keeps source compatibility within a major release, not across one | before compiling; CUDA 11 is the oldest supported |
| `env "cuda-13-sm_120"` | the GPU target, passed to nvcc as `-arch`: `sm_NN` (machine code plus PTX), `compute_NN` (PTX only, compiled by the driver at load), `sm_NNa` (this exact GPU generation), or `sm_NNf` (this family) | at `fgx check`, and against the GPU before compiling |
| `effects [gpu.compute]` | the capsule uses the GPU; required | before compiling |
| `sandbox "gpu.compute device:0"` | the devices the runner may see, by `nvidia-smi` index; without `device:N`, all GPUs are visible | before compiling |
| `sandbox "limit.mem_mb 512"` | the host-memory ceiling (default 512 MiB) | applied to the runner |

`gpu.compute` is accepted only for CUDA capsules; a C, C++, Rust, or Zig
capsule requesting it is still refused with
`native_capsule_capability_unsupported`.

### Choosing the toolkit and the GPU

Several CUDA toolkits can be installed side by side; each capsule compiles with
one from the release line it declares. `$NVCC` and then the `PATH` `nvcc` are
used when they satisfy the declaration. Otherwise Forgentex searches
`$CUDA_HOME`, `$CUDA_PATH`, `/usr/local/cuda-*`, `/usr/local/cuda`,
`/opt/cuda`, and `/usr/bin/nvcc` (Debian/Ubuntu's `nvidia-cuda-toolkit`).
It takes the newest satisfying toolkit that can build on this host. Each
candidate is checked once per process with a small compile, because a toolkit
older than the system compiler or C library cannot build anything. `fgx
doctor` lists every toolkit it finds and every GPU `nvidia-smi` reports.

Before compiling, Forgentex refuses a call when:

- no GPU is present (`native_runtime_unavailable`);
- a granted device does not exist (`native_runtime_unavailable`);
- the declared target cannot run on the selected GPU
  (`native_capsule_gpu_target_unsupported`), for example `sm_130` on a compute
  capability 12.0 GPU, or `sm_90a` on anything but compute capability 9.0.

When nvcc fails because the toolkit does not support the host compiler, the C
library's math headers, or the declared target, the error ends with a hint
naming the fix (`NVCC_CCBIN`, a newer toolkit, or `nvcc --list-gpu-arch`).

### The runner's environment and limits

- **Environment.** Cleared, like every native runner, plus three variables:
  `CUDA_VISIBLE_DEVICES` from the device grant; `CUDA_DEVICE_ORDER=PCI_BUS_ID`,
  so CUDA numbers devices as `nvidia-smi` and `fgx doctor` do (CUDA otherwise
  orders them fastest first); and `CUDA_CACHE_DISABLE=1`, so the driver's JIT
  cache is not written into the user's home directory. A `compute_NN` target
  is therefore JIT-compiled on every call.
- **Host memory.** On Linux the native memory ceiling is normally an
  address-space limit (`RLIMIT_AS`). The CUDA driver reserves tens of GB of
  address space when it creates a context — on an RTX 5090 a 12 GiB ceiling was
  still too small — so a CUDA runner gets the same budget as a data-segment
  limit (`RLIMIT_DATA`). That limit counts the heap and private writable
  mappings, so it still bounds host allocations, and the driver starts under
  it (on the RTX 5090 it needed between 64 and 80 MiB).
- **CPU and wall time** are the same as for other native runners. GPU time is
  not separately limited; a kernel that never finishes is ended by the wall
  limit.
- **No isolation on Linux.** As for every native capsule on Linux, the runner
  is not sandboxed ([linux_support.md](linux_support.md) § 2).
  `CUDA_VISIBLE_DEVICES` selects devices for the CUDA runtime; it is not a
  security boundary.

### Receipts

The native receipt's `toolchain` names the nvcc used, `toolchain_semver` its
exact release, and `toolchain_requirement` the declaration it was admitted
under (`13.x`). A CUDA call adds a `gpu` object:

```json
"gpu": {"target": "sm_120", "device_index": 0, "device_name": "NVIDIA GeForce RTX 5090",
        "compute_capability": "12.0", "driver_version": "595.99.02", "visible_devices": "0",
        "host_memory_limit": "data_segment", "jit_cache": "disabled"}
```

`memory_limit_bytes` reports the data-segment ceiling. For every other
language `gpu` is `null`.

### Platforms and verification

Live CUDA runs on Linux. Other hosts refuse CUDA calls with
`native_runtime_unsupported`; they still review CUDA capsules. The bridge was
verified on Ubuntu 26.04 with an RTX 5090 (compute capability 12.0, driver
595.99) and three toolkits side by side. Toolkits used through Ubuntu's
`/usr/bin/nvcc` wrapper still get Ubuntu's own `ptxas`, even when an NVIDIA
toolkit is earlier on `PATH`:

| Toolkit | Source | On Ubuntu 26.04 (gcc 15.2, glibc 2.43) |
| --- | --- | --- |
| 13.2.51 | NVIDIA (`/usr/local/cuda-13.2`) | builds and runs `sm_120` and `compute_75` capsules |
| 12.8.93 | NVIDIA (`/usr/local/cuda-12.8`) | cannot build anything: rejects gcc 15, and with an older g++ its math headers conflict with glibc 2.43's `cospi` declarations |
| 12.4.131 | Ubuntu `nvidia-cuda-toolkit` | builds and runs `compute_75` capsules (Ubuntu patched it for the new gcc and glibc); it predates `sm_120` |

A capsule declaring `cuda-12.4` therefore builds with 12.4 there, skipping
the broken 12.8. GitHub-hosted runners have no GPU, so the GPU tests
(`src/tests/cuda.rs`) return early in CI. The checks that need no GPU run
everywhere: declaration refusals, target and `nvidia-smi` parsing, device
grants, and compile hints.

