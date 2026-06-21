# Go Bridge — Real Go Capsules (v2.24)

Cloud & network-services support. Go has the simplest model of the managed
runtimes — `go run` compiles and runs a single source file in one step.

## Real Go execution

```text
$ fgx capsule-run examples/go/scale.fgx --go
Forgentex Go capsule run
schema=forgentex.go_run.v1 capsule=scale status=ok
  runtime: go version go1.26.4 windows/amd64
  entry: scale()
  pass (21) -> 42 (expected 42)
  pass (0) -> 0 (expected 0)
  pass (-5) -> -10 (expected -10)
  3/3 fixtures passed
```

How it runs:

1. Forgentex parses the first `func` of the body as the entry.
2. It assembles a `package main` program: the body, plus a generated `main`
   that parses process args to the boundary types, calls the entry, and
   `fmt.Println`s the result. Imports are emitted **only when used** (Go treats
   unused imports as compile errors).
3. `go run` compiles and runs it per fixture; the output is checked against the
   expectation. Stdlib-only bodies need no `go.mod`.

## Usage

```text
fgx capsule-run <file> --go [--capsule <name>] [--json]
```

Go capsules also route through `fgx polyglot-run` (the v2.18 unified ABI) via
the `go` backend. Go can additionally expose a C-ABI surface
(`go build -buildmode=c-shared`) to link through the v2.13 FFI bridge when a
native boundary is wanted — the subprocess path here is the default.

## Honest scope

- Scalar typed boundary: `int64`/`float64`/`bool`/`string`.
- The entry is the first `func`.
- `go.mod`/`go.sum` dependencies declared on the capsule flow through the v2.3
  supply-chain audit; the demo uses only the standard library.
- Missing Go degrades gracefully to `runtime_unavailable` (so CI needs no Go);
  verified on go1.26.4.
- The token discipline carries over: `body_tokens` vs `boundary_tokens`.
