# Forgentex Smoke Test Programs

These tiny programs are meant for quick manual checks of the Forgentex demo
language. Each file focuses on one feature so failures are easy to understand.

## Quick Commands

Parse and check every smoke program:

```powershell
fgx parse examples/smoke/arithmetic.fgx --emit-ast
fgx check examples/smoke/arithmetic.fgx --json
fgx fmt examples/smoke/arithmetic.fgx --to fgai
fgx roundtrip examples/smoke/arithmetic.fgx --to fgai
```

Run the valid executable examples:

```powershell
fgx run examples/smoke/arithmetic.fgx --entry add_mul --json-input examples/smoke/run/arithmetic.json
fgx run examples/smoke/branching.fgx --entry clamp_small --json-input examples/smoke/run/branching.json
fgx run examples/smoke/loop_sum.fgx --entry sum_u8 --json-input examples/smoke/run/loop_sum.json
fgx run examples/smoke/slice_write.fgx --entry fill_ramp --json-input examples/smoke/run/slice_write.json
fgx run examples/smoke/struct_point.fgx --entry point_sum --json-input examples/smoke/run/struct_point.json
```

Run focused safety reports:

```powershell
fgx borrow-report examples/smoke/borrow_alias_bad.fgx --json
fgx check examples/smoke/move_if_bad.fgx --json
fgx check examples/smoke/raw_pointer_bad.fgx --json
fgx asm-check examples/smoke/asm_review_only.fgx --target x86_64-pc-windows-msvc --json
```

## Test List

| File | Feature | Expected result |
| --- | --- | --- |
| `arithmetic.fgx` | numeric parameters, arithmetic precedence, return values | run passes with return `14` |
| `branching.fgx` | `if`, comparison, early returns | run passes with return `10` |
| `loop_sum.fgx` | slices, indexing, loop, mutable local assignment | run passes with return `10` |
| `slice_write.fgx` | mutable slices and in-place writes | run passes with `dst = [0, 1, 2, 3]` |
| `struct_point.fgx` | struct constructor and field access | run passes with return `7` |
| `borrow_alias_bad.fgx` | local mutable borrow alias detection | `borrow-report` fails with `mutable_borrow_alias` |
| `move_if_bad.fgx` | conservative move merge from `if` bodies | `check` reports `use_after_move` |
| `raw_pointer_bad.fgx` | raw pointer dereference safety gate | `check` reports `raw_pointer_dereference_requires_unsafe` |
| `asm_review_only.fgx` | structured assembly review metadata | `asm-check` passes for x86_64 target |

## Full Valid Smoke Run

```powershell
fgx check examples/smoke/arithmetic.fgx --json
fgx check examples/smoke/branching.fgx --json
fgx check examples/smoke/loop_sum.fgx --json
fgx check examples/smoke/slice_write.fgx --json
fgx check examples/smoke/struct_point.fgx --json
fgx run examples/smoke/arithmetic.fgx --entry add_mul --json-input examples/smoke/run/arithmetic.json
fgx run examples/smoke/branching.fgx --entry clamp_small --json-input examples/smoke/run/branching.json
fgx run examples/smoke/loop_sum.fgx --entry sum_u8 --json-input examples/smoke/run/loop_sum.json
fgx run examples/smoke/slice_write.fgx --entry fill_ramp --json-input examples/smoke/run/slice_write.json
fgx run examples/smoke/struct_point.fgx --entry point_sum --json-input examples/smoke/run/struct_point.json
```
