# rt.3 — Warm Workers + Capsule Pipelines (design + slice ladder)

Critical gap #2 (`v3_progress.md`): every live capsule call spawns a fresh
host process — Python ~50ms+, Go recompiles per call. Composition in a loop is
demo-speed only. rt.3 makes composed polyglot programs run at real speed.

Native warm hosts cache compilation only. Every call remains a fresh contained
process with either its declared wall limit or a mandatory 30,000 ms fallback;
first-time compiler/linker trees have a separate hard 60,000 ms ceiling.

## Design (slice 1: warm Python worker)

**Protocol.** One persistent process per capsule, spawned on first call and
reused for the rest of the run. Line-delimited JSON over stdin/stdout:

```text
→ [arg1, arg2, …]\n                 (one request per line)
← {"ok": <result>}\n                (or {"err": "<message>"}\n)
```

The generated driver wraps the capsule body in a read-eval loop; every
response is flushed. Exceptions are caught and returned as `{"err": …}` — a
bad call **must not kill the worker**, so the next call still runs warm.

**Placement.** The worker pool lives on `CapsuleHost` (the live-runtime
capability), keyed by capsule name, dropped at end of run (`Drop` kills the
children). Sandbox approval is checked at spawn; the caller effect allowlist
still gates every call before dispatch, and every call is still audited.

**Default since slice 4.** Warm is the default for `fgx run --live`
(`--cold` opts out). It shipped opt-in first; the flip landed with
auto-respawn, which removes the main new failure mode (a dead worker
failing every later call).

**Verification without timing flakiness.** "Warm" is proven deterministically:
a capsule that returns `os.getpid()` yields the *same* pid across calls in one
run (one process), while the cold path yields different pids. Timings are
*measured and printed* but not asserted.

## Honest limitations (slice 1)

- ~~Python only~~ Python, Node/TS, and Go workers are live; JVM/.NET follow later
  (Node is near-identical; JVM/.NET amortize compile at first call; Go's
  win is the largest since it recompiles per call today).
- One worker per capsule per run — no cross-run daemon, no concurrency.
- ~~No auto-respawn~~ Transport deaths respawn + retry once (slice 4).

## Slice ladder

| # | Slice | Status |
| --- | --- | --- |
| 1 | **Warm Python worker** — protocol, pool on `CapsuleHost`, `--warm` CLI, exception-survival, pid-proof + printed timings | ✅ shipped — 4 tests in `tests/warm_workers.rs`: the pid probe proves one process across warm calls (and two across cold calls) deterministically; a raising call returns `{\"err\": …}` and the same worker serves the next call; warm results equal cold results; a capsule exception traps `capsule_call_failed` with the error audited. **Measured on this machine:** a 10-call loop through `fgx run --live` took **1352ms cold → 157ms warm (8.6×)**, same result; marginal call cost ~130ms → ~5ms. |
| 2 | Warm Node worker (same protocol); measure | ✅ shipped — ESM read-eval driver; pid probe proves one process warm / two cold; exceptions survive per-line |
| 3 | Warm JVM/.NET/Go workers (compile once, loop) | 🟡 **Go shipped** — the biggest win: `go run` compiles once at spawn; typed binds per declared input; `recover()` turns panics into `{\"err\": …}` (proven: div-by-zero panic, then a working call on the same worker). **Measured: a 10-call loop 7534ms cold → 766ms warm (9.8×)** — the per-call compile is gone. JVM/.NET remain (compile-once loop is a bigger driver; .NET unverifiable on this box). |
| 4 | Auto-respawn + health checks; flip warm to default | ✅ shipped — a *transport* failure (broken pipe / EOF: the worker died) replaces the worker and retries the call **once**; a capsule's own `{\"err\": …}` is deterministic and never retried. Proven both ways: a die-once capsule (marker file) is healed invisibly by the retry (caller sees one success), and a deterministic killer fails the call but leaves a fresh worker serving the next one. **Warm is now the DEFAULT for `fgx run --live`** (`--cold` restores per-call spawn; `--warm` accepted as a no-op); library entry points stay explicit (`run_entry_with_capsules` cold / `run_entry_with_warm_capsules` warm) so tests remain deterministic. |
| 5 | Pipeline primitive: compose capsules with control flow at speed (the roadmap's orchestration half) | 🟡 bounded cancellable dependency primitive shipped — `fgx pipeline-run` provides deterministic scalar/struct/artifact and reviewed typed-buffer handoff, canonical byte/hash receipts, one writer, stage audit, linked render/encode receipts, pipeline-wide stage-count/declared-wall/CPU-sum/sequential-peak-memory/owned-byte budgets, derived-DAG scheduling with stable order, fan-out, ordered multi-input fan-in, and cycle rejection, plus external-token and deterministic stage-boundary cancellation. Explicit graph syntax, general control flow, mutable buffers, active-stage interruption/resume, aggregate filesystem/network budgets, caching, and parallel scheduling remain. |
