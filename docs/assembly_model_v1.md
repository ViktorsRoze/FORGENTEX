# Structured Assembly Model v1

Stage 12 keeps assembly review-only, but makes structured assembly target-aware.
The validator checks assembly dialects, registers, clobbers, inputs, outputs,
unsafe boundaries, and feature-gated instructions before later backend stages
attempt real lowering.

## CLI

```powershell
fgx asm-check examples/arch/x86_64_popcount.fgx --target x86_64-pc-windows-msvc
fgx asm-check examples/arch/aarch64_popcount.fgx --target aarch64-unknown-linux-gnu
fgx asm-check examples/arch/riscv64_add.fgx --target riscv64gc-unknown-linux-gnu
fgx asm-check examples/arch/x86_64_popcount.fgx --target x86_64-pc-windows-msvc --json
```

Text output is a human review summary. JSON output uses
`forgentex.asm_check.v1` and includes review entries plus diagnostics.

## Dialects

The accepted v1 dialect names are:

- `asm x86_64`
- `asm armv7`
- `asm aarch64`
- `asm riscv32`
- `asm riscv64`

Compatibility with the selected target triple is checked. For example,
`asm aarch64` fails under `--target x86_64-pc-windows-msvc`.

## Register Sets

Register validation is deliberately small and explicit:

- x86_64: `rax`, `rbx`, `rcx`, `rdx`, `rsi`, `rdi`, `rsp`, `rbp`, `r8`-`r15`,
  `xmm0`-`xmm31`, `flags`.
- ARMv7: `r0`-`r15`, `sp`, `lr`, `pc`, `s0`-`s31`, `d0`-`d31`, `q0`-`q15`,
  `flags`.
- AArch64: `x0`-`x30`, `w0`-`w30`, `sp`, `v0`-`v31`, and `p0`-`p15` when
  `cpu.sve` or `cpu.sve2` is enabled.
- RISC-V: `x0`-`x31`, `zero`, `ra`, `sp`, `gp`, `tp`, `t0`-`t6`, `s0`-`s11`,
  `a0`-`a7`, `f0`-`f31`, and `v0`-`v31` when `cpu.v` is enabled.

Reserved stack or constant registers cannot be direct outputs or clobbers.
Examples include `rsp`, `sp`, `pc`, `x0`/`zero`, `gp`, and `tp`.

## Features

Feature-specific instructions require either a target default feature or an
explicit function effect such as `effects [cpu.popcnt]`.

The v1 checker recognizes these instruction-to-feature gates:

- x86_64: `popcnt` -> `cpu.popcnt`, `pdep`/`pext` -> `cpu.bmi2`, selected AVX
  mnemonics -> `cpu.avx2`.
- ARMv7: selected NEON mnemonics such as `vcnt`, `vadd`, `vld1`, `vst1` ->
  `cpu.neon`.
- AArch64: selected NEON mnemonics such as `cnt`, `addv`, `ld1`, `st1` ->
  `cpu.neon`; selected SVE mnemonics such as `ptrue`, `whilelt`, `ld1w`,
  `st1w` -> `cpu.sve`.
- RISC-V: `cpop`, `clz`, `ctz` -> `cpu.zbb`; `sh1add`/`sh2add`/`sh3add` ->
  `cpu.zba`; `bset`/`bclr`/`binv`/`bext` -> `cpu.zbs`; vector mnemonics
  starting with `v` -> `cpu.v`.

## Review Summary

Every assembly block contributes a review line with:

- node ID
- assembly target
- changed registers
- clobbers
- required features
- memory effect marker
- ABI risk level

ABI risk is `high` when callee-saved registers are clobbered, `medium` when the
block writes registers or declares memory effects, and `low` for blocks without
visible state changes. Stage 12 does not prove ABI correctness; it only makes
the risk reviewable.

## Examples

Stage 12 adds:

- `examples/arch/x86_64_popcount.fgx`
- `examples/arch/aarch64_popcount.fgx`
- `examples/arch/riscv64_add.fgx`
- `examples/arch/aarch64_copy_neon.fgx`
- `examples/arch/riscv64_vector_add_v.fgx`

The existing `examples/media/popcount_*.fgx` files remain review-only assembly
examples.

## Deferred Work

Stage 12 does not lower assembly, allocate registers, verify instruction
encoding, enforce calling conventions fully, or compute target ABI layouts.
Those are later backend and ABI stages.
