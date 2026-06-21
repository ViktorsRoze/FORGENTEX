# Prelude: Short Agent Aliases for High-Frequency Builtins

Twelve high-frequency standard builtins carry a **two-letter alias** used in
the `.fgai` agent rendering. The human `.fgx` keeps the readable canonical
name; the agent `.fgai` shows the short one. The AST always stores the
canonical name, so type-checking, effects, and interpretation are unchanged
— the dual syntax does the work.

## The table

| Canonical (`.fgx`) | Agent (`.fgai`) |
| --- | --- |
| `core_slice_len_i16` | `ln` |
| `core_slice_len_mut_i16` | `lm` |
| `core_slice_len_u8` | `lu` |
| `core_slice_copy_i16` | `cp` |
| `core_math_clamp_i16` | `cl` |
| `core_math_saturating_add_i16` | `sa` |
| `core_math_checked_add_i32` | `ca` |
| `core_math_checked_sub_i32` | `cs` |
| `core_math_checked_mul_i32` | `cm` |
| `core_bits_popcount_u32` | `pc` |
| `core_bits_clz_u32` | `cz` |
| `core_bits_ctz_u32` | `tz` |

## How it works

- The agent formatter replaces whole-word canonical builtin names with
  their alias as its final pass (`prelude::apply_aliases`). Canonical names
  are long and distinctive, so whole-word replacement is unambiguous.
- The expression parser expands a two-letter alias back to the canonical
  name when it builds a call, so the AST is identical regardless of which
  form was written.
- The aliases are **reserved**: a user function may not be named like an
  alias (`reserved_prelude_alias` at `fgx check`), so an alias in agent
  source always means the builtin and round-trips are lossless.

```text
.fgx :   R core_math_clamp_i16(x)
.fgai:   R cl(x)
```

## Measured effect

On `examples/media/audio_mix.fgx`, which calls `clamp` and `len` in a loop,
the aliases move agent/human tokens from 0.93 to **0.82** and
best-compressed/human from 0.91 to **0.79** — a real win because the
canonical builtin names are long and the calls are frequent. On code that
does not call builtins, there is no change.

This is the pilot for the dual-named prelude idea: readable for humans,
short for agents, verified lossless, and measured rather than assumed. The
table can grow once a real BPE tokenizer confirms the win at scale; each new
alias must point at a real, type-checked builtin.
