# Target Architecture Model

Stage 11 makes architecture support explicit without turning Forgentex into a
native compiler yet. The frontend can now name targets, attach target metadata
to AST/project-map output, validate target-specific calls, and report
architecture-specific review notes.

## Canonical Targets

The v0 target catalog is:

| Triple | Family | Default features |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | `x86_64` | `sse2` |
| `x86_64-unknown-linux-gnu` | `x86_64` | `sse2` |
| `aarch64-apple-darwin` | `aarch64` | `neon` |
| `aarch64-unknown-linux-gnu` | `aarch64` | `neon` |
| `aarch64-pc-windows-msvc` | `aarch64` | `neon` |
| `armv7-unknown-linux-gnueabihf` | `arm` | `vfpv3`, `thumb2` |
| `riscv64gc-unknown-linux-gnu` | `riscv64` | `i`, `m`, `a`, `f`, `d`, `c` |
| `riscv32imac-unknown-none-elf` | `riscv32` | `i`, `m`, `a`, `c` |

Architecture families are `x86_64`, `arm`, `aarch64`, `riscv32`, and
`riscv64`.

## Feature Names

Feature probes use the stable spelling `cpu_has_<feature>()`.

| Family | Features |
| --- | --- |
| x86_64 | `sse2`, `avx2`, `avx512f`, `bmi2`, `popcnt` |
| ARM 32-bit | `neon`, `vfpv3`, `thumb2` |
| AArch64 | `neon`, `sve`, `sve2`, `crc`, `crypto` |
| RISC-V | `i`, `m`, `a`, `f`, `d`, `c`, `v`, `zbb`, `zba`, `zbs`, `crypto` |

`cpu.simd` remains a generic effect from Stage 6. It is not treated as a
specific feature guard.

## Metadata

Target metadata is derived from:

- declared effects such as `cpu.avx2` or `target.aarch64`
- structured assembly blocks such as `asm x86_64:`
- semantic patches that add fast paths or assembly stubs

`fgx ast <file> --canonical` and `fgx parse <file> --emit-ast` include
`target_metadata` for functions. `fgx map --include-targets` includes compact
`targets=...` labels in project maps.

## Target Checks

Run:

```powershell
fgx targets
fgx targets --json
fgx check examples/media/popcount_x86.fgx --target x86_64-pc-windows-msvc --json
fgx asm-check examples/arch/x86_64_popcount.fgx --target x86_64-pc-windows-msvc
fgx simd-review examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu
fgx abi examples/abi/layout_demo.fgx --target x86_64-unknown-linux-gnu
fgx map examples --include-targets
```

Review-only assembly examples are available for x86_64, AArch64, and RISC-V in
`examples/media/popcount_x86.fgx`, `examples/media/popcount_aarch64.fgx`, and
`examples/media/popcount_riscv64.fgx`.

`fgx check <file> --target <triple>` first runs the normal checker, then adds
target diagnostics. Stage 11 checks:

- unknown target triples
- unknown target families
- unknown CPU features
- assembly blocks whose family does not match the selected target
- calls to feature-specific functions without a matching `cpu_has_<feature>()`
  guard when the feature is not a default feature
- calls to architecture-specific functions from an incompatible target family

Example:

```text
fn fast(x: u32) -> u32 effects [cpu.avx2]:
    return x

fn caller(x: u32) -> u32 effects [cpu.avx2]:
    if cpu_has_avx2():
        return fast(x)
    return x
```

The guarded call is accepted for `x86_64-*`. Without `cpu_has_avx2()`, target
checking reports `target_guard_required`.

Stage 13 adds portable SIMD review on top of this target model. `fgx check
<file> --target <triple>` also enforces that SIMD functions keep a scalar
fallback unless explicitly marked `target.only`. `fgx simd-review` reports
target lowering plans for x86_64 SSE/AVX, ARMv7 NEON, AArch64 NEON/SVE, RISC-V
Vector Extension, and scalar fallback paths.

Stage 14 adds ABI/layout reports on top of the same target catalog. `fgx abi`
uses the selected triple to choose pointer size, pointer alignment, calling
convention, integer argument registers, and return registers.

## Review Reports

Semantic patch review reports now include target notes. Fast-path patches record
the target feature set and scalar fallback. Assembly patches record the target
family and that the path is review-only in v0.

Stage 11 does not lower assembly, emit native code, or emit native SIMD
instructions. ABI/layout reporting begins in Stage 14, but full platform ABI
classification remains future work.
