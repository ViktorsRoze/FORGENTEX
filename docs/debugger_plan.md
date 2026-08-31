# Forgentex Debugger (design + slice ladder)

User-directed priority (2026-07-08): the dev-loop gap moves ahead of `rt.3`.
This is the design record; each slice updates it. Ledger: `v3_progress.md`.

## Thesis fit: an agent-native debugger

Traditional debuggers are interactive UIs for humans. Forgentex's primary
"debugger user" is a **coding agent** that needs machine-readable runtime
evidence to repair a program. So v0 is not a UI — it is a **deterministic
execution trace** whose every step names the **same canonical node id that
semantic patches target** (`@function.f.stmt.2`, `….then.0`, `….body.1`,
`….arm.0.1`). The loop this enables:

```text
fgx debug → failing node id + env at failure → agent emits .fgp patch against
that exact node → fgx patch → fgx debug again
```

Humans get the same trace as readable text; agents consume the JSON.

## v0 surface (`forgentex.debug_trace.v1`)

`fgx debug <file> --entry <fn> [--json-input <in>] [--break <node-id>]
[--max-steps N] [--json]`

- **Step records** — one per executed statement: `seq`, `node` (canonical id),
  `kind`, `text` (readable statement rendering), and `changed` (the env delta:
  bindings created or mutated by that statement, as text values).
- **Call records** — function enter (name, args) / exit (name, return value),
  so the trace shows the dynamic call tree.
- **Run-to-breakpoint** — `--break <node-id>` stops *before* executing that
  node and dumps the full local environment; status `breakpoint`. Non-
  interactive by design (v0): agents re-run cheaply; determinism makes
  "restart + break later" equivalent to interactive stepping.
- **Trap post-mortem** — on any trap, each unwinding frame records the
  function, its locals at failure, and the failing node: a full stack
  snapshot; status `trap`.
- **Step cap** — `--max-steps` (default 10k) turns a runaway loop into status
  `step_limit` with the trace so far, instead of an unbounded dump.

## Design decisions

- **Capability pattern**: the tracer is an `Option<RefCell<DebugTracer>>` on
  the interpreter, exactly like io/net/capsules — `None` costs nothing on
  normal runs (no env cloning, no bookkeeping).
- **AST-path execution**: the debugger runs the interpreter's AST path
  (`fir: None`). Semantics are identical to FIR-backed runs (same tree
  walker underneath); statement-level canonical ids are the AST's native
  vocabulary.
- **Node-id threading**: `run_body`/`exec_stmt` carry an id path mirroring
  the canonical-JSON scheme byte-for-byte, so trace ids == patch targets ==
  `fgx get` targets. This is the whole point.
- **Env deltas by diff**: before/after snapshot per step (tracer-only cost),
  reporting created/changed bindings. Removals (match-arm binding restores)
  are not reported in v0.

## Honest limitations (v0)

- Closure bodies are expressions, so they execute inside a single step —
  no sub-steps inside a lambda (fix: expression-level stepping, later).
- Live capsule calls appear as one step; their transcripts stay in the hosted
  runtime's audit (`capsule_calls`), not duplicated in the trace.
- Non-interactive: no pause/resume process; re-run with a different `--break`
  instead (cheap because runs are deterministic).
- No watch expressions or conditional breakpoints yet (ladder below).

## Production assessment (2026-08-17)

Reproduced against the working tree, not read off the ladder below.

| Gap | Evidence | Severity |
| --- | --- | --- |
| Blind to the governed surface | The debug interpreter is built with `io: None`, `net: None`, `capsules: None`. Debugging a `foreign` call traps with `capsule_call_not_permitted`; the same program returns `43` under `run --live`. None of the four `examples/apps` can be traced. | blocking |
| Quadratic trace overhead | `DebugTracer::snapshot` renders every binding to text on every step. Overhead 71 -> 159 -> 600 ms as step count doubles, while a plain run stays flat at 42 ms. Unusable on real payloads. | blocking |
| Enums carry no source position | `Item::Enum` was skipped by the source-map builder, so no enum or variant had a span in either view. Statement, struct, trait, impl, and function spans were already correct — **including duplicate identical statements**, because the builder advances a scan cursor. An earlier draft of this plan claimed duplicates were indistinguishable; testing disproved it. | foundation |
| No conditional breaks or watches | Slice 2, unstarted. | missing |
| No expression-level stepping | Closure bodies execute inside one step. Slice 3, unstarted. | missing |
| No interactive session | Re-running with a different `--break` is the only control. Slice 4, unstarted. | missing |
| No editor debugging | No DAP adapter; no breakpoints, variables, or call stack in VS Code. Slice 5, unstarted. | missing |
| Async is invisible | The `Task`/`TaskGroup` runtime emits byte-stable transition receipts, but the debugger exposes no task surface. | missing |

What v0 gets right and must be preserved: canonical node ids on every step (a
trace line is directly addressable by a semantic patch), per-iteration loop
bodies, env deltas, the dynamic call tree, run-to-breakpoint with full locals,
and innermost-first trap stacks with per-frame locals.

## Production ladder (phases 1-6)

Dependency order, not a schedule. Phases 1 and 2 gate the usefulness of
everything after them; phase 3 gates phase 5.

**1. Debug what Forgentex actually runs.** *(partially shipped — see below)* Give the debug interpreter the same
`io`/`net`/`capsules` wiring as the live runtime, behind unchanged grant and
effect gates. Add a capsule step record carrying arguments, toolchain, sandbox
decision, duration, and the `native_receipt` hash. Add `--live`,
`--allow-effects`, and `--app <project>`. *Done when all four shipped example
applications trace end to end with capsule calls and file operations visible as
steps carrying their receipts.*

Also make `env` executable. `Capsule::env` is already parsed from
`env "..."`, carried into the AST, and emitted in capsule JSON — but nothing
consults it: managed capsules resolve their interpreter purely from `$PYTHON`
(then `python3`, then `python`). Give it a resolution order of declared `env`,
then `$PYTHON`, then the probe chain, with schemes `conda:<name>`,
`venv:<path>`, and `path:<abs>`. Resolve to an absolute interpreter at
preflight, record its path and version in the receipt, and fail closed when the
environment is absent. This is worth doing on its own account, independent of
debugging: today a capsule's interpreter is whatever the host's `$PYTHON`
happens to point at, so managed capsules are not reproducible across machines.

**2. Make tracing affordable.** *(shipped)* Stop stringifying eagerly; snapshot structurally
and render only touched bindings. Summarise large values by default
(`Vec[u8] len=2649628 sha=...`) with `--values summary|full`. Detect unchanged
bindings by identity or version counter rather than rendered text. Add a
ring-buffer `--tail N`. *Done when overhead is linear in executed steps and
independent of heap size, with a regression guard.*

**3. Source positions.** *(shipped — see below)* Cover the node kinds the
source-map builder skipped, and project spans onto the debug trace so a step can
be opened in an editor and DAP has line numbers to report.

The original plan called for capturing spans during lexing and parsing, on the
grounds that the post-parse reconstruction was fragile guesswork. Testing did
not support that: the reconstruction resolves duplicate identical statements,
odd interior spacing, and recovery after each item correctly, because it
advances a scan cursor rather than searching globally. A parser rewrite was
therefore not justified by evidence, and the phase narrowed to the one real hole
plus the projection the debugger needs. Capturing spans at parse time remains
the better long-term design, but it is now an optimisation rather than a
prerequisite.

**4. Interactive sessions.** *(session core shipped; transport and asynchronous
pause remain)* A long-lived session with `step`, `next`, `step-out`, `continue`,
`pause`, and `evaluate`. Conditional breakpoints and
hit counts (slice 2). Watch expressions in frame scope. Trap and effect-denial
breakpoints. Expression-level stepping inside closures (slice 3). *Done when a
session can pause mid-run, evaluate in the paused frame, and resume.*

Replay-instead-of-stepping was an honest v0 substitute while runs were pure. It
stops being honest once a run touches the filesystem and native processes.

**5. Editor cockpit (DAP).** *(shipped — see below)* A `forgentex-dap` adapter mapping DAP onto the
phase 4 session, using phase 3 spans for node-id translation. Launch/attach
configurations for `run`, `app-run`, and `pipeline-run`. Breakpoints, stepping,
call stack, scopes, variables, watch, hover-evaluate. Render capsule steps as
native frames so the boundary is visible in the stack rather than a gap in it.
*Done when a gutter breakpoint on the media application stops and shows locals
without a terminal.*

**6. Step into foreign code (delegate, do not reimplement).** Rather than
building native debuggers, attach the language's own debugger to the capsule
subprocess in its declared `env`. This follows the same "polyglot, not
from-scratch" stance the native and audio tracks already take.

The leverage: `debugpy` and `delve` **already speak DAP**. Once phase 5 exposes
a Forgentex DAP adapter, foreign debugging becomes DAP-to-DAP delegation — the
Forgentex adapter multiplexes, forwarding breakpoint, stack, scope, and evaluate
requests to the child adapter whenever execution is inside a capsule, and
splicing the foreign frames beneath the Forgentex frame in one unified stack.
That is far cheaper than per-language debugger integrations.

- *Stage A — managed languages.* `python` via `debugpy --listen --wait-for-client`,
  `node`/`typescript` via `--inspect-brk`, `go` via `dlv dap`. These are already
  plain subprocesses spawned by `CapsuleWorker` with no AppContainer, job object,
  or token restriction, so the containment cost is small.
- *Stage B — compiled native.* `gdb`/`lldb` against the generated runner. This
  conflicts far more sharply with containment and should follow stage A.

Three tensions have to be handled explicitly rather than discovered later:

1. **Debug attach needs a loopback socket**, and "an adversarial capsule cannot
   connect to loopback" is currently an adversarial test the runtime passes.
   Debug mode must therefore be an explicit, opt-in relaxation, recorded in the
   receipt, so a debug run can never be mistaken for a governed one.
2. **Pausing blows the mandatory wall/CPU deadlines.** Budget enforcement has to
   be suspended for a debugged capsule, and that suspension recorded.
3. **Interactive runs are not deterministic**, so a debug run must be excluded
   from cache identity and marked non-reproducible.

*Done when a breakpoint set inside a Python capsule body stops in that capsule's
declared conda environment, with Python frames and Forgentex frames in one
stack.*

**7. Production depth.** *(concurrency visibility shipped; the rest remains)* Async/concurrency (task tree with park/wake causality,
deadlock detection, per-task stacks — largely a presentation layer over the
existing transition receipts). Time-travel reverse-step by deterministic replay,
which is near-free in a language whose runs are already reproducible. Data
watchpoints on shares and buffers, attributed to the writing stage. Pipeline
stage-boundary stepping with buffer receipts and budget attribution. A
`gdb`/`lldb` handoff for faults below the native boundary.

### Cross-cutting commitments

- Debugging never widens authority: effect grants, sandbox policy, and
  `audit.redact` apply unchanged to debug runs.
- Every surface is versioned: `debug_trace.v1` grows additively or becomes `v2`;
  session and adapter schemas follow the same compatibility rules.
- Determinism is a feature, not an accident — it is what makes replay,
  time-travel, and breakpoint re-runs sound. Introducing nondeterminism into a
  debug run is a defect.
- Agents and humans read the same evidence: JSON is the contract, the text
  render and DAP view are projections.

### Deliberately out of scope

- Attaching to an already-running deployed process (needs an in-runtime debug
  agent and a remote protocol; the containment model needs review first).
- Debugging optimised C/Zig output (needs DWARF emission from the backends).
- A bespoke debugger UI: DAP buys VS Code and every other DAP client for less.

### Shipped from phase 1 (2026-08-17)

`fgx debug` reaches the governed surface. Runtime access is opt-in, so the
default stays pure and every prior test passes unchanged:

- `--live` / `--allow-effects a,b` run `foreign` capsules for real instead of
  trapping with `capsule_call_not_permitted`.
- `--grant-fs a,b` / `--grant-net host,...` open scoped hosted filesystem and
  network access, spelled exactly as `fgx run` spells them.
- Capsule transcripts ride along in `capsule_calls` on the report, in both JSON
  and text, because a capsule call is a single opaque step and the trace would
  otherwise show that a boundary was crossed with no evidence of what happened
  across it.
- `fgx debug` is listed in `--help`; it previously existed but was
  undiscoverable.

Still open in phase 1: `--app <project>`. Debugging a bare `.fgx` file does not
load the project manifest, so an application entry that publishes a declared
artifact traps with `artifact_undeclared` at that final step — the trace itself
is complete and correct up to it.

`DebugOptions::io_policy` is in place to carry a manifest-derived `IoPolicy`,
which is the piece the interpreter needs. What remains is extracting the
120-line prologue of `application::run` — security enforcement, manifest and
compatibility checks, effect scoping, argument typing, policy construction — so
the debugger can reuse it rather than reimplement it. Reimplementing it is not
an option: a debug run that applies different security checks than a real run
diverges exactly where it matters. That extraction touches the most sensitive
path in the project and is deliberately left as its own slice.

Debugging real programs immediately made phase 2 urgent, which is the ordering
the plan predicted. Tracing `render_waveform` over the bundled 2.6 MB WAV
produced **11.3 MB of output for 40 steps**, because each step rendered every
binding in full. Values are now elided to their first 8 elements
(`[82, 73, 70, 70, … +2649620 more]`), taking the same trace to **1,580 bytes**;
`--values full` opts back in. Three tests cover the refusal without `--live`,
the live capsule step plus transcript, and the elision.

### Shipped from phase 2 (2026-08-30)

The per-step environment clone is gone. `exec_stmt` no longer snapshots the
whole environment and diffs it; it reports the bindings the statement *writes*,
computed from the statement itself:

- the `let` name, the assignment root (`v`, `v[i]`, and `v.f` all root at `v`),
  and bare-variable call arguments — the only shape a `&mut` copy-back can
  target, since a `&mut` parameter requires a direct variable rather than a
  projection.
- This is sound because a leaf statement can only write a binding it names:
  closures capture free variables **by value**, so no aliasing path exists from
  a statement to a binding it does not mention. Container statements
  (`if`/`for`/`while`/`match`) already recorded pre-order with no delta and
  never cloned.
- A call that only reads a bare-variable argument is over-reported rather than
  missed. For a debugger that is the safe direction, and it keeps the analysis
  free of callee-signature lookup.

Overhead is now linear: 9 / 9 / 21 / 38 ms as the traced buffer doubles from
1k to 8k elements, against 71 / 159 / 600 ms before at a quarter the size. The
full `render_waveform` trace over the bundled 2.6 MB WAV — **353,966 steps** —
now completes in **6.8 s**; it was previously not completable. A regression
guard traces a 30,000-element build and fails past 20 s; with the clone restored
it takes 44.9 s against 0.45 s without.

`--tail N` closes this phase. `--max-steps` bounds a run by keeping the *first*
N steps, which is the wrong end when the failure is at the finish; `--tail`
keeps the last N as a sliding window. The full `render_waveform` trace goes from
28 MB to **1,526 bytes** at `--tail 6` while still showing the failing publish.

Two details that matter for reading one correctly: the step cap counts
*executed* steps rather than retained ones, so a runaway loop is still bounded
rather than spinning forever inside a full window; and the report states
`showing the last N steps; M earlier steps dropped` in text and `dropped_steps`
in JSON, because a tail read as a complete trace makes its first line look like
the start of the run.

### Shipped from phase 3 (2026-08-30)

- Enums and their variants now carry spans in **both** views: `enum Mode:` maps
  the header and each indented variant to its own line, and the agent view's
  `EN Mode { Fast; Careful }` maps header and variants to the shared line. The
  scan cursor advances correctly, so the item after an enum still resolves.
- `DebugReport::resolve_positions` projects the source map onto the trace,
  filling `line` on every step and frame. The interpreter stays free of span
  concerns: node ids remain the tracer's vocabulary and positions are attached
  afterwards, so a node with no recorded span keeps `null` rather than being
  given a misleading position.
- `fgx debug` renders the line beside each step (`2  L5  @function.outer.stmt.0
  let z = 0`) and on each stack frame (`fn inner (L1)`), and `line` appears in
  the JSON for both. This is the input DAP needs in phase 5.
- Nested statement bodies now map too. The span builder covered `if.then` and
  `for` bodies only, so `while` bodies, `else` bodies, and match arms had no
  position — a debugger step inside a loop had no line to open. It also never
  consumed the `else:` and `case ...:` headers, which silently handed the
  header's line to the *following* statement. Both are fixed and asserted: a
  program with a loop, a branch, and a match now has a correct line on every
  step and none left unresolved.

### Shipped from phase 4 (2026-08-30)

Conditional breakpoints and watches shipped first:

- `--break-if <node>=<expr>` stops at a node only when the expression holds in
  the environment as it stands there, with the same full-locals dump as an
  unconditional breakpoint.
- `--watch <expr>` is repeatable and evaluates after every step, recording
  `(source, value)` pairs on the step in JSON and text.

Two failure modes are handled deliberately, because a debugger that changes
whether the program completes is worse than no debugger:

- A watch that cannot be evaluated — a name not yet in scope — records its error
  (`<unknown_symbol: …>`) and the run continues.
- A condition that fails to evaluate, or is not a boolean, does **not** stop.
  A broken condition must not masquerade as a hit.

Evaluating a condition or watch can call a function, which re-enters the
statement executor. A `evaluating` flag suspends tracing for the duration, so
those statements are neither recorded as program steps nor allowed to re-enter
the tracer's own `RefCell` borrow. A test asserts that adding a
function-calling watch changes the step count by zero.

**The suspendable session is now shipped too.** The interpreter runs on a
scoped thread and *blocks on a command channel* at each stop — a tree-walking
evaluator has no continuation to resume, so the pause has to be a genuinely
parked thread. `Program` is `Send + Sync` and the AST has no interior
mutability, so only a shared reference crosses the boundary.

- `debug_session(program, entry, input, options, driver)` starts paused at the
  first statement and hands the driver a `Stopped` event carrying node,
  function, depth, statement text, and locals. The CLI projects the node through
  the source map to render its line.
- `StepIn`, `Next`, `StepOut`, `Continue`, `Evaluate`, `Quit`. `next` and
  `step-out` are depth comparisons against the depth the command was issued at,
  so a call made from the stopped frame runs to completion instead of being
  descended into. Depth is maintained alongside the existing enter/exit call
  records.
- `Evaluate` runs in the **paused frame** and leaves the session paused, so a
  frame can be interrogated repeatedly. Reusing the phase-4 `evaluating` guard
  keeps those statements and their synthetic call records out of the trace.
- Unconditional and conditional breakpoints suspend the session and resume the
  same statement after `Continue`; they do not fall through to the one-shot
  breakpoint path and terminate the run.
- `Quit` finishes with `status: "cancelled"` and no return value.
- A driver that hangs up does not strand the worker: the session falls back to
  `Continue` and runs to completion rather than blocking forever.

`fgx debug --interactive` drives it from stdin — `step`/`s`, `next`/`n`,
`out`/`o`, `continue`/`c`, `locals`/`l`, `print <expr>`/`p`, `quit`/`q`, with
end-of-input finishing the run. A CLI test spawns the binary, steps into a
callee, evaluates in its frame, steps out, and asserts the process terminates,
which also covers the session not deadlocking.

Still open in phase 4: a versioned JSON-RPC transport, an asynchronous `Pause`
command that can interrupt a running `Continue`, hit counts, trap/effect-denial
breakpoints, and expression-level stepping. The callback session is sufficient
for a DAP adapter to drive stops and resumes, but those missing controls must not
be described as shipped yet.

### Shipped from phase 5 (2026-08-30)

`fgx dap` serves one Debug Adapter Protocol session on stdio, using the same
`Content-Length` framing as the LSP bridge. It is a translation layer, not a
second debugger: stepping and evaluation are the phase-4 session, and the lines
DAP speaks in are the phase-3 source map. Implemented as a subcommand rather
than a third binary, so the frozen distribution manifest is untouched.

- `initialize` / `launch` / `configurationDone` / `disconnect`, `threads`,
  `stackTrace`, `scopes`, `variables`, `evaluate`, `continue`, `next`,
  `stepIn`, `stepOut`, and `setBreakpoints`, plus `stopped`, `output`,
  `terminated`, and `exited` events.
- **Line to node.** A gutter click is a line; the tracer speaks canonical node
  ids. Sub-expression ids share a line with their statement, so they are
  excluded and the outermost statement node wins — a breakpoint must land on
  something the tracer actually steps through. A line with no statement is
  reported back `verified: false` rather than silently accepted and never hit.
- **Breakpoints are editable mid-run** through a shared `Arc<Mutex<BTreeSet>>`
  the interpreter consults, so a client can move one while paused.
- Breakpoints set before `launch` are held and resolved once the source map
  exists, because clients routinely configure before naming the program.
- Capabilities advertise only what is wired up. Conditional and function
  breakpoints and step-back are declared **false**: a client that trusts a
  false capability gives a worse experience than one that knows the limits.

The VS Code extension contributes a `forgentex` debug type with launch
configuration and snippets. `adapter.js` is a shim that spawns `fgx dap` and
connects the pipes, and the extension passes `forgentex.fgxPath` through to it,
so the editor debugs with the same binary it checks with.

`tools/dap/session_check.py` is a scripted DAP client that drives the real
binary through initialize, launch, breakpoint resolution, two step-ins into a
callee, variable inspection, running to a breakpoint, and termination. It runs
from `cargo test`; hosts without Python skip.

### Shipped from phase 6 (2026-08-30): declared environments

`env` is now executable — the prerequisite for attaching a language's own
debugger to a capsule in the environment it declares.

```
foreign analyse python version "3" env "conda:tensorpack":
```

Three schemes resolve, once, before execution, to an **absolute interpreter
path** that is then used directly: `conda:<name>`, `venv:<path>`, and
`path:<file>`. Resolving up front keeps the per-call cost identical to the
ambient path and makes the choice recordable.

Two things this deliberately does **not** do:

- It does not fall back. A declared environment that cannot be resolved is an
  error, because the point of naming one is that the capsule does not run
  somewhere else.
- It does not change existing capsules. `env` has carried descriptive runtime
  labels since v2.0 — `cpython-3.13-linux-x86_64`, `temurin-21`,
  `cuda-12.4-sm_80` — and those stay metadata. Only a scheme-prefixed spec
  selects a runtime. A colon-shaped spec with an unknown scheme *is* an error,
  so `konda:sound` is caught rather than silently ignored.

Conda environments are located by directory layout rather than through
`conda run`. That was not the first design: `conda run` looked obvious, but the
conda launcher on this host prints `failed to create process.` for every
invocation *and exits 0*, which resolved to an empty path. Layout needs no
subprocess, is stable, and cannot be defeated by a broken launcher. Roots come
from `$CONDA_PREFIX`, `$CONDA_EXE`, and the standard install locations, and a
Windows conda env keeps its interpreter at the env root rather than in
`Scripts`.

This is worth having on its own account, before any debugger work: until now
the Python a capsule ran on was whatever `$PYTHON` happened to point at, so a
managed capsule was not reproducible across machines.

### Foreign-debugger admission

The three tensions are now decided rather than deferred, in
`src/foreign_debug.rs`. Admission happens **before** any debugger is launched:

- **Opt-in only.** Without an explicit request the answer is always no. The
  relaxations must never be applied because a debugger happened to be present.
- **A resolvable environment is required.** Without one there is nothing
  definite to attach to, and guessing would debug a different runtime than the
  one that actually runs. A descriptive label does not count.
- **Only DAP-speaking debuggers are admitted**: `debugpy`, `node --inspect-brk`,
  `dlv dap`. That is what makes the approach cheap — the adapter multiplexes two
  DAP conversations rather than learning a second protocol. Compiled capsules
  are refused here and routed to a future `gdb`/`lldb` bridge.
- **Every relaxation is named and recorded**, admitted or not, under
  `forgentex.foreign_debug.v1`: `loopback_listener`, `deadlines_suspended`,
  `nondeterministic`. Each contradicts something the runtime asserts elsewhere —
  the adversarial loopback test, the mandatory per-call wall/CPU budget, and the
  reproducibility that pipeline cache identity depends on — so a debug run can
  never be mistaken for a governed one.

Still open in phase 6: the attach itself — launching the capsule under its
debugger and multiplexing that DAP conversation beneath the phase-5 adapter.
That step needs `debugpy` (or `delve`) present in the target environment; none
is installed on this host, so it is deliberately not written blind.

### Shipped from phase 7 (2026-08-30): concurrency is visible

A concurrent program's statements do not show why it is waiting — a park, a
wake, a refused send is a transition, not a statement. The runtime already
recorded these and `fgx run` emitted them, but the debugger surfaced none of
them, so a trace showed a task created and later finished with nothing between.

`DebugReport::concurrency` now carries the transitions, as the plan predicted:
largely a presentation layer over evidence the runtime already produces.

- **Text groups by operation and decision**, so the shape of a stall reads at a
  glance rather than as a long repetitive list:

  ```text
  concurrency: 10 transitions
    clock.advance advanced x2
    task.spawn_on parked x1
    timer.fire signaled x1
    wake_token.cancel select_loser x1
  ```

- **JSON keeps every transition in order**, because an agent follows causality
  through the sequence rather than a summary.
- A run with no concurrency reports none, so the section never appears for an
  ordinary program.

Combined with the env deltas already on each step — `task(pending)` becoming
`task(consumed)`, a channel's length changing — this is the raw material for the
task tree and park/wake causality the phase calls for.

Still open in phase 7: a derived task tree rather than a flat transition list,
deadlock and starvation detection, time-travel replay, data watchpoints on
shares and buffers, pipeline stage-boundary stepping, and the `gdb`/`lldb`
handoff for faults below the native boundary.

## Slice ladder (original v0 track)

Superseded for ordering by the production ladder above, which re-sequences the
unstarted slices behind the two blocking gaps. The mapping is: slice 2 and the
closure half of slice 3 land in phase 4; the capsule-detail half of slice 3
moves forward into phase 1; slice 4 becomes phase 4; slice 5 becomes phase 5.
Slice 1 remains shipped and unchanged.

| # | Slice | Status |
| --- | --- | --- |
| 1 | **Trace + breakpoint + post-mortem** (this doc's v0 surface, CLI, JSON schema, tests) | ✅ shipped — 6 tests in `src/tests/debugger.rs`: canonical node ids on every step with per-iteration loop-body repeats and env deltas; call tree enter/exit with arg/return values; breakpoint stops *before* the node with full locals (and the node provably never executes); trap post-mortem stacks innermost-first with per-frame locals; step cap yields `step_limit` with a bounded trace; JSON schema + human text render. CLI verified live: a div-by-zero trap reports failing node `@function.inner.stmt.1` with the full stack, and `--break` dumps locals at the target node. |
| 2 | Conditional breakpoints (`--break-if <node> <expr>`) + watch expressions evaluated per step | ⬜ re-sequenced into phase 4 |
| 3 | Expression-level stepping (inside closures/complex exprs); capsule-call step detail | ⬜ |
| 4 | Interactive session (`fgx debug --interactive`: step/next/continue/print) on the same tracer | shipped core; async pause and JSON-RPC transport remain in production phase 4 |
| 5 | DAP bridge for VS Code (the workbench cockpit) | ⬜ |
