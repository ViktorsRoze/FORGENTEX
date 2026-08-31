# Public 0.1 Execution Matrix

This matrix freezes the execution contract for the interpreter-first Forgentex
0.1 profile. Every designated fixture must parse in human syntax, format and
round-trip through both `.fgx` and `.fgai`, type-check without diagnostics, and
produce the recorded interpreter result.

The primary native subset is **portable C11 source generation** for scalar
functions, structs, payload-free enums, `if`/`else`, `while`, and exhaustive
`match`. The `portable_core.fgx` fixture must emit C, compile, and return the same
result when a host C compiler is available. Compiler absence is reported by the
existing native-build tooling and does not expand the language subset.

Collections and the broader array/slice surface remain part of the stable
interpreter contract. A native backend may support individual cases, but
anything outside the selected C11 subset is experimental. Unsupported constructs
must fail with an explicit FIR blocker instead of emitting weaker code.

| Fixture | Public surface | Required path | Recorded runs |
| --- | --- | --- | --- |
| `examples/public_0_1/portable_core.fgx` | functions, locals, structs, enums, calls, branches, loops, match | dual-view round-trip, check, interpreter, C11 emit/compile/run | `public_score(4) == 19` |
| `examples/public_0_1/arrays_and_borrows.fgx` | fixed arrays, immutable slices, indexing, half-open ranges, borrow provenance | dual-view round-trip, check, interpreter | `public_array_score([4,7,9,12]) == 16`; `public_slice_score([4,7,9,12]) == 16` |
| `examples/public_0_1/collections.fgx` | bounded `Vec[i32]`, `Map[str,i32]`, String construction/length, `Option` fallback, `alloc` effect | dual-view round-trip, check, interpreter; explicit C11 blocker for Map | `public_collection_score() == 14` |

The matrix is enforced by `src/tests/public_profile.rs`. Other public tools are
covered by their dedicated suites: project/module diagnostics, inline tests,
public API reports, diagnostics JSON, canonical AST JSON, workbench reports, and
the borrow conformance gate. Adding a feature to
`docs/public_0_1_profile.md` requires either extending one of these fixtures or
adding a new designated fixture and recorded result here.

## Supported limits

The reproducible evidence report (`fgx release-evidence --json`, schema
`forgentex.public_0_1_evidence.v1`) reruns the frozen suite above and republishes
these supported limits, each of which is a documented property of this matrix:

- portable C11 source generation is the only native subset promised for 0.1
- constructs outside that subset require an explicit FIR blocker, never weaker code
- the interpreter is the stable interpreter contract and reference semantics
- Compiler absence is reported by native-build tooling and never widens the subset

`fgx release-evidence --strict` exits non-zero if any frozen fixture stops
reproducing its recorded result or any supported limit stops being documented.
