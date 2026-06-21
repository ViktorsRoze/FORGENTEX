# JS/TS Runtime + Web One-Roof (v2.20)

The web tier of the polyglot. JavaScript (and best-effort TypeScript) capsules
run for real on the host **Node** runtime, behind the same typed boundary as
every other backend — so a Python backend and a JavaScript frontend run under
one Forgentex roof, sharing one typed value by name.

## Real JS execution

```text
$ fgx capsule-run examples/web/one_roof.fgx --node --capsule render
Forgentex web capsule run
schema=forgentex.web_run.v1 capsule=render language=javascript status=ok
  runtime: node v24.17.0
  entry: render()
  pass render(5) -> "<b>5</b>" (expected "<b>5</b>")
  pass render(12) -> "<b>12</b>" (expected "<b>12</b>")
  2/2 fixtures passed
```

Mirrors the Python bridge (v2.17): the body is dedented, the first named
`function` is the entry, inputs are marshaled as JSON, a generated `.mjs`
driver calls `entry(...args)` and prints `JSON.stringify(result)`, and the
output is checked against the capsule's `mock` fixtures.

## One roof: backend + frontend together

```text
$ fgx polyglot-run examples/web/one_roof.fgx
schema=forgentex.polyglot_abi.v1 status=ok languages=[javascript, python]
  @accumulate   python      backend=python  gate=allow exec=ok  2/2
  @render       javascript  backend=node    gate=allow exec=ok  2/2
  data plane (declare once, flow by name):
    total : i64 -> accumulate[python:rw], render[javascript:read]
  wired fixtures: 4/4 passed
```

`total` is declared once with `share total: i64`. The Python backend
accumulates it (`rw`); the JavaScript frontend renders it (`read`). One run
executes the Python capsule on host CPython and the JS capsule on host Node —
the original "backend and frontend under one Forgentex roof" vision, now
executable, with the shared typed value flowing across both languages by name.

## Usage

```text
fgx capsule-run <file> --node [--capsule <name>] [--json]
```

JS/TS capsules also route automatically through `fgx polyglot-run` (the v2.18
unified ABI) — the `javascript`/`typescript` languages map to the `node`
backend with no change to the calling convention.

## TypeScript as a general-purpose runtime (v2.21)

TypeScript is not just for web pages — it is what agents and LLM frontends
generate constantly. A `typescript` capsule with **real type annotations** runs
on host Node via `node --experimental-strip-types`:

```text
$ fgx capsule-run examples/ts/classify.fgx --node
schema=forgentex.web_run.v1 capsule=classify language=typescript status=ok
  runtime: node v24.17.0
  entry: classify()
  pass classify(95) -> "high" (expected "high")
  pass classify(50) -> "mid" (expected "mid")
  pass classify(10) -> "low" (expected "low")
  3/3 fixtures passed
```

The body `export function classify(score: number): string { const band: string = … }`
keeps genuine TS types; Forgentex strips and runs it, verified on Node
v24.17.0. npm dependencies declared on a TS/JS capsule flow through the v2.3
supply-chain audit like any other ecosystem.

## Honest scope

- **JavaScript** runs via `node`. **TypeScript** runs best-effort via
  `node --experimental-strip-types` (Node 22.6+); without a capable runtime a
  TS capsule reports `runtime_unavailable`.
- Missing `node` degrades gracefully to `runtime_unavailable` (verified), so
  CI needs no Node; with Node present the fixtures are real oracles (verified
  on Node v24.17.0).
- The entry is the first named `function`; arrow-only modules are a follow-on.
- The token discipline carries over: the report shows `body_tokens` vs
  `boundary_tokens`, so the agent pays for the boundary, not the body.
