# The Python Bridge — Real CPython Behind a Typed Boundary (v2.17)

Agents love generating Python, so Forgentex gives Python a **special
relationship**: it runs for real on the host interpreter, but behind a typed
boundary the agent never has to re-read.

## What it does

A Python capsule's `mock` fixtures become **real execution oracles** (exactly
as the C-ABI bridge did for native code in v2.13): the host Python runs the
body against each fixture's inputs and the output is checked against the
expectation.

```text
$ fgx capsule-run examples/capsule/celsius_python.fgx --python
Forgentex python capsule run
schema=forgentex.python_run.v1 file=... capsule=to_fahrenheit status=ok
  runtime: Python 3.8.5
  entry: to_fahrenheit()
  inputs: [celsius:f64]
  outputs: [fahrenheit:f64]
  effects=[alloc] sandbox=allow deps=0
  tokens: body=29 boundary=28 (agent pays the boundary, not the body)
  pass to_fahrenheit(0.0) -> 32.0 (expected 32.0)
  pass to_fahrenheit(100.0) -> 212.0 (expected 212.0)
  pass to_fahrenheit(-40.0) -> -40.0 (expected -40.0)
  3/3 fixtures passed
```

How it runs: the body is dedented to valid top-level Python, the first
top-level `def` is the entry, inputs are marshaled to JSON, and a generated
driver calls `entry(*inputs)` and prints `json.dumps(result)`. The output is
compared to the typed boundary expectation (numeric with tolerance).

## The token point (the "special relationship")

The agent works from the **boundary card**, not the body. A heavy
`import numpy ...; def f(...)` body costs the agent nothing to *use* — only
its one-line typed signature. The report prints `body_tokens` vs
`boundary_tokens` so the saving is **measured, not assumed**, and ties into
`fgx capsule-tokens`. A verbose data-science routine becomes as cheap to call
as a one-liner, because its body is content-addressed and pulled only when
edited.

## Gating and safety

- The run is gated by the capsule's **sandbox decision**; an unapproved
  capsule reports `denied` and does not execute.
- Effects and pinned dependencies are surfaced in the report.
- `core_io`/`core_socket`-style host access from inside Python is still
  governed by the capsule's declared effects and sandbox.

## Honest scope

- **Real** CPython execution against fixtures, with the interpreter version
  detected and recorded.
- **Determinism:** CPython is not identical across versions, so the version
  is pinned/recorded and the **transcript** (not the interpreter) is the test
  oracle. Tests use a pure-stdlib capsule so they pass anywhere `python` is
  present and degrade to `runtime_unavailable` where it is not.
- **Dependencies:** a body that imports a package needs it installed; without
  it the run honestly reports `error` with the Python stderr (the numpy
  example will fail on a host without a working numpy).
- This generalizes to the other managed runtimes (v2.21–v2.24: TS/Node, Java,
  C#, Go) via the same detect-host-runtime, marshal-boundary, capture-
  transcript pattern.
