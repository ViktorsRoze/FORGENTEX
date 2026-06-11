# Diagnostic Catalog and Repair Plans

Stage 29 turns Forgentex diagnostics into compiler-native next actions for
agents and human reviewers.

## Commands

```powershell
fgx diagnostics list [--json]
fgx explain <diagnostic-code> [--json]
fgx fix --plan <file|project> [--json]
fgx fix --plan <file|project> --target <triple> [--json]
```

## Schemas

- `forgentex.diagnostic_catalog.v1` lists normalized diagnostic families,
  canonical compiler codes, repair safety, suggested semantic patch kinds,
  review gates, target consequences, and related diagnostics.
- `forgentex.diagnostic_explain.v1` explains one diagnostic code. It accepts
  both normalized codes such as `TYP001` and compiler codes such as
  `return_type_mismatch`.
- `forgentex.fix_plan.v1` reports source diagnostics, candidate semantic patch
  previews, diagnostics that are not safely plannable, and target impact.

## v0.24 Shape Freeze

Release-candidate hardening treats the diagnostic catalog shape as frozen for
v1.0 planning. `fgx release-check --json` includes the
`diagnostic_shape_freeze` hardening check, which expects catalog entries to keep
stable machine-readable fields for code, canonical code, family, rule, summary,
safety, suggested patch kinds, review gates, target consequences, and related
diagnostics.

## Families

| Family | Area |
| --- | --- |
| `PAR` | parser |
| `NAM` | names and imports |
| `TYP` | type system |
| `BOR` | borrow and ownership |
| `EFF` | effects and capabilities |
| `ASM` | structured assembly |
| `ABI` | layout and headers |
| `TAR` | target architecture |
| `SEC` | security and sandboxing |
| `PERF` | performance budgets |

## Repair Policy

Repair plans are semantic `.fgp` previews, not direct text edits. A plan says
which diagnostic it fixes, which operation it proposes, what safety label it
carries, and which review gates must pass before a human or agent applies it.

Safe local plans include type annotations, casts, literal replacement, index
repairs, and effect declarations. Project plans include import-name or module
file candidates. Unsafe, assembly, target-specific, and borrow-alias plans are
kept behind human-review gates.

For example, assembly in a safe function can produce `ASM001`/`unsafe_required`.
The planner may propose:

```text
VERSION 1
AUTHOR fgx.fix
TARGET @function.example
PRECONDITION NODE_EXISTS @function.example
OP MARK_FUNCTION_UNSAFE
REVIEW generated_by=stage29_fix_plan
```

That plan is deliberately labelled `human-review`; the compiler does not pretend
that marking code unsafe is a routine local edit.
