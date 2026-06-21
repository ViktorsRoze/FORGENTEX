# Unified Polyglot ABI (v2.18) — the Capstone

One calling convention, one gate, one report across every backend. The C-ABI
bridge (v2.13) and the Python bridge (v2.17) — and the managed runtimes to
come (v2.20–v2.24) — are dispatched through a single command, and the shared
typed namespace (v2.2) is surfaced as the **cross-language data plane**.

## One command, every backend

```text
$ fgx polyglot-run examples/polyglot/pipeline.fgx
Forgentex unified polyglot ABI
schema=forgentex.polyglot_abi.v1 file=... status=ok languages=[c, python]
  @scale            c          backend=ffi       gate=allow exec=executed   2/2
  @label            python     backend=python    gate=allow exec=ok         1/1
  data plane (declare once, flow by name):
    gain : i32 -> scale[c:read], label[python:read]
  wired fixtures: 3/3 passed
```

A single run dispatched a **C capsule through the FFI bridge (host `cc`)** and
a **Python capsule through the host CPython**, each through the same gate,
folded into one schema. The agent sees one convention, not one per language.

## The cross-language data plane

`gain` is declared once with `share gain: i32`. Both the C capsule and the
Python capsule `bind gain` — so the report shows it flowing across languages
**by name**:

```text
gain : i32 -> scale[c:read], label[python:read]
```

This is the "true full polyglot" payoff: a value declared once in Forgentex is
the same typed value in every language that binds it. Today the report
captures the **typed wiring** (who binds what, in which language and mode);
live in-process mutation of the shared value across runtimes stays
snapshot/orchestration-level (the v2.2 deferral).

## How dispatch works

| Capsule language | Backend | Executor |
| --- | --- | --- |
| `c`, `cpp`, `rust`, `zig` | `ffi` | C-ABI bridge (v2.13) |
| `python` | `python` | host CPython (v2.17) |
| others | `not_wired` | later milestones (v2.20–v2.24, contract/shell) |

Each backend reports a uniform `{gate_decision, execution, passed, total}`,
and its own graceful-degradation rule is respected: a missing host compiler or
interpreter is **not** a failure (the capsule shows `compiler_unavailable` /
`runtime_unavailable` and the overall run stays `ok`). A real mismatch, error,
or sandbox denial makes the run `partial`.

## The report: `forgentex.polyglot_abi.v1`

- `languages` — every language present.
- `capsules[]` — `{name, language, backend, gate, execution, passed, total, ok}`.
- `data_plane[]` — each shared value with its `{capsule, language, mode}`
  binders.
- `passed` / `total` — wired-backend fixture tally.

## Scope and honesty

- Real execution of the C and Python backends; not-yet-wired languages are
  marked, not faked.
- One gate (sandbox + deps) applied uniformly; one marshaling notion
  (`CapsuleValue`) shared across backends.
- The data plane is the typed binding wiring; cross-runtime shared-memory
  mutation is future orchestration work.
- When the managed runtimes land (v2.20–v2.24), they slot into the same
  router and report with no change to the convention.
