# v2 Shared Typed Namespace Across Foreign Capsules

This document specifies the shared-namespace model that lets one named,
typed value be declared in Forgentex and used under the same name inside
foreign-language capsules such as Python and JavaScript. It extends the
capsule contract in `docs/v2_roadmap.md` and must not weaken any rule
there: foreign code stays external, typed, effect-declared, pinned,
sandboxed, and reviewable.

## Goal

```text
Declare a value once in Forgentex.
Bind it by name into a Python capsule and use it as a native Python value.
Bind it by name into a JavaScript capsule and use it as a native JS value.
Keep Forgentex the single source of truth for type, ownership, and review.
```

This enables one project to host backend and frontend code together —
for example a web server whose data processing uses Python libraries and
whose serving/rendering uses JavaScript libraries — with one typed schema
shared by both sides.

## Ownership model

The namespace is Forgentex-owned. Capsules never share memory with each
other or with Forgentex. A binding means:

1. At capsule entry, the current value is materialized into the capsule
   runtime as a native value under the declared name (snapshot-in).
2. The capsule body uses it as ordinary Python/JavaScript data.
3. At successful capsule exit, bindings declared `rw` are read back,
   boundary-checked against the declared Forgentex type, and committed
   (write-back-out).
4. A capsule that exits with an error commits nothing.

Live shared mutable memory across runtimes is explicitly out of scope.
Mutations made by one capsule become visible to other capsules only after
commit, in the program order of capsule execution.

## Syntax sketch

```fgx
share session_count: i64 = 0
share spectrum: [f32]

foreign python version "3.13" effects [alloc]:
    bind session_count mode rw
    bind spectrum mode rw
    deps ["pypi:numpy==2.2.0#sha256:..."]
    code """
    import numpy as np
    session_count += 1
    spectrum = np.abs(np.fft.rfft(signal)).astype(np.float32)
    """

foreign javascript runtime "node-22" effects [network.write]:
    bind session_count mode read
    bind spectrum mode read
    code """
    res.send(render_chart(spectrum, `visitors: ${session_count}`))
    """
```

The exact surface syntax is not fixed; the contract is. Every binding
must declare a name, resolve to a `share` declaration, and declare a mode.

## Binding modes and consistency rules

- `read`: snapshot-in only. Writes inside the capsule do not commit.
- `rw`: snapshot-in plus write-back-out at successful exit.
- One `rw` binding per shared name may be active at a time. Two capsules
  that could run concurrently with `rw` bindings of the same name are a
  compile-time error.
- Commit order follows capsule completion order and is recorded in the
  audit log.
- A type-mismatched write-back (for example a Python float written to an
  `i64` binding) is a boundary diagnostic, and the commit is rejected.

## Value mapping

| Forgentex | Python 3.13 | JavaScript (Node) | Rule |
| --- | --- | --- | --- |
| `bool` | `bool` | `boolean` | direct |
| `i8..i32`, `u8..u32` | `int` | `number` | range-checked at boundary |
| `i64`, `u64`, `usize` | `int` | `BigInt` | mapping to `number` is opt-in and lossy-checked |
| `f32`, `f64` | `float` | `number` | direct |
| `str` | `str` | `string` | UTF-8 |
| `struct` | generated dataclass | generated object/interface | one declaration generates both |
| `[T]` numeric slices | `memoryview`/numpy array | `TypedArray` | single binary layout, no JSON round-trip |
| `Option[T]` | `T \| None` | `T \| null` | direct |
| `Result[T, E]` | raise/return convention | throw/return convention | declared per capsule |

Library-native objects (DataFrames, tensors, server handles, sockets) are
not shareable. They live inside their capsule for the capsule's lifetime.
What crosses the boundary is the typed data they produce or consume.

## Using both ecosystems' libraries

Two supported patterns cover the practical cases:

1. **Pipeline.** A Python capsule uses its pinned libraries to produce a
   typed value into the namespace; a JavaScript capsule consumes it with
   its own libraries. Neither side knows the other's runtime exists.
2. **Long-running capsule.** A capsule that holds stateful library
   objects (an HTTP server, a loaded model) stays alive across calls.
   Other code interacts with it only through shared bindings and typed
   capsule calls. Library state never crosses the boundary.

## Web-server demo (acceptance demo for this feature)

One Forgentex project containing:

- `share` declarations for a session counter and a typed request/response
  struct schema.
- A Python backend capsule (pinned FastAPI or stdlib http) producing
  typed responses.
- A JavaScript frontend capsule (pinned express/static render) consuming
  the same shared schema.
- Generated Python dataclasses and TypeScript interfaces from the single
  Forgentex struct declaration.
- A review report showing every shared name, its type, which capsules
  bind it, in which mode, and every effect each capsule declares.

## Review requirements

Semantic patch review must treat shared-namespace changes as high-signal:

- adding or removing a `share` declaration
- changing a shared type or default
- adding a binding, or escalating `read` to `rw`
- any boundary mapping marked lossy or opt-in

## Non-goals

- Live shared memory or cross-runtime object references.
- Implicit sharing: a capsule sees only names it explicitly binds.
- Bypassing effects, sandbox policy, dependency pinning, or audit.
- More than two foreign languages before Python + JavaScript semantics
  are stable.

## Acceptance gates

- A value declared once in Forgentex is read and mutated by a Python
  capsule and then read by a JavaScript capsule under the same name.
- A failing capsule commits no writes.
- Concurrent `rw` bindings of one name are rejected at check time.
- `i64` values survive Python -> JavaScript round trips without precision
  loss (BigInt path), and the lossy `number` path requires explicit
  opt-in.
- Numeric slice bindings do not serialize through JSON.
- The review report lists every shared name, binding mode, and capsule.
