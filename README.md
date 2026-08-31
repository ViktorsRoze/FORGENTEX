<div align="center">

<img src="fgai_fgx_logo.png" alt="Forgentex logo" width="620">

# Forgentex

### One language, two views: expressive for people, compact for agents.

[![Stage](https://img.shields.io/badge/stage-v3_demo-7c3aed)](#where-the-project-is-today)
[![Next release](https://img.shields.io/badge/public_release-0.1_coming_soon-16a34a)](#the-road-to-forgentex-01)
[![Tests](https://img.shields.io/badge/tests-1126_passing-2563eb)](#recent-development)
[![Core dependencies](https://img.shields.io/badge/compiler_core-zero_dependencies-0891b2)](#quick-start)
[![License](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-f59e0b)](#license)

```text
Write once. Read your way. Verify everything.
```

</div>

> [!IMPORTANT]
> Forgentex is an advanced demo and research project for a new programming
> language. The repository is currently in its third internal development
> stage, **v3**. That does not mean a public, production-ready `3.0` release:
> the next milestone is the first real language release, **Forgentex 0.1**.

Forgentex explores a simple idea: a programming language can serve humans and
software agents without forcing either one to work through the other's ideal
representation. People get readable `.fgx`; agents get concise `.fgai`. Both
parse into the same canonical AST, use the same type and effect system, and
produce the same program.

The project also asks a broader question: what would a language look like if
agentic editing, narrow context retrieval, verifiable patches, foreign-code
boundaries, and token cost were architectural concerns rather than editor
plugins added later?

## The same program, two views

<table>
<tr>
<th>Human view — <code>.fgx</code></th>
<th>Agent view — <code>.fgai</code></th>
</tr>
<tr>
<td>

```rust
type Point = {
  x: Float,
  y: Float,
}

fn area(width: Float, height: Float) -> Float
  ! pure
{
  width * height
}
```

</td>
<td>

```text
T Point={x:F,y:F}
F area(w:F,h:F)>F!p{w*h}
```

</td>
</tr>
</table>

The compact form is not a separate language and does not bypass safety. A
round trip must preserve meaning:

```text
.fgx ─┐
      ├── parse ──> canonical AST ──> types / effects / borrow checks ──> run or build
.fgai ┘
```

This lets a person review ordinary source while an agent navigates and edits a
smaller, structurally precise representation.

## Core ideas in small examples

The examples below are shortened from programs that live in this repository.
They show the design ideas Forgentex is testing beyond surface-level syntax.

### 1. Effects make hidden behavior visible

```rust
import core.hash

fn digest(message: str) -> str effects [crypto.hash]:
    return core_hash_sha256(message)
```

`digest` does not merely return a `str`; its signature also says that it uses
the `crypto.hash` effect. A caller, reviewer, sandbox, or agent can understand
that requirement without reading the body.

This idea makes several things possible:

- pure functions remain easy to identify;
- sensitive operations can require explicit capabilities;
- unexpected effects can be rejected during checking;
- an agent can search, plan, and review changes by behavior, not only by name.

See the complete [hashing example](examples/crypto/hashing.fgx).

### 2. Borrowed data expresses cost and ownership

```rust
import core.math
import core.slice

fn mix_samples(dst: &mut [i16], left: &[i16], right: &[i16], gain: f32) -> void:
    for i in 0 .. core_slice_len_mut_i16(dst):
        let mixed: f32 = f32(left[i]) * gain + f32(right[i])
        dst[i] = core_math_clamp_i16(mixed)

fn middle(samples: &[i16]) -> &[i16]:
    return samples[1 .. 4]
```

`&[i16]` means borrowed read-only samples; `&mut [i16]` means one mutable
borrow. The signature communicates that the inputs are not consumed and that
the destination is modified in place. The borrow checker then rejects unsafe
aliasing, use-after-move, and invalid lifetime flows in the supported subset.
Bounded half-open ranges (`start .. end`) produce borrowed slice projections. Constant
ranges participate in disjointness proofs, while dynamic bounds remain
conservative; reversed or out-of-bounds ranges trap deterministically.

For media and agent workloads this matters twice: it avoids unnecessary bulk
copies, and it gives automated editors a machine-checkable contract for data
movement. See [audio mixing](examples/media/audio_mix.fgx).

### 3. Foreign code becomes a typed capsule

```rust
foreign to_fahrenheit python version "3.13" env "cpython-3.13" effects [alloc]:
    input celsius: f64
    output fahrenheit: f64
    sandbox "fs.none"
    mock 0.0 -> 32.0
    mock 100.0 -> 212.0
    code:
        def to_fahrenheit(celsius):
            return celsius * 9.0 / 5.0 + 32.0
```

The boundary records the runtime, input and output types, effects, sandbox, and
test oracles. The Python body is still ordinary Python, but it no longer enters
the project as an unstructured string with unlimited authority.

Agents usually need only the compact boundary card—the types, effects,
capabilities, dependency identity, and body hash—rather than spending context
tokens on the entire foreign implementation. Humans can inspect the body when
a deeper review is necessary. See the runnable
[Python capsule](examples/capsule/celsius_python.fgx).

### 4. Structured concurrency is bounded and explicit

```rust
fn score(seed: i32) -> i32:
    return seed * 10

fn scores() -> i32 effects [concurrency.task]:
    let group: TaskGroup[i32] = task_group_new(2)
    let first: bool = task_group_spawn(group, score, 1)
    let second: bool = task_group_spawn(group, score, 2)
    let results: Vec[i32] = task_group_await_all(group)
    return results[0] + results[1]
```

Tasks belong to a bounded group and are joined through explicit operations.
Standalone tasks can also start parked with `task_spawn_parked`; `task_wake`
makes one pending through an explicit, receipt-visible state transition before
`task_await` can execute it.
`WakeToken` extends that state machine to bounded program-driven events:
`task_spawn_on` registers affine tasks on a shared one-shot token and
`wake_token_signal` releases the current waiter set without running it inline.
`AsyncClock` supplies deterministic scheduler time: `timer_after` returns a
wake token and explicit `clock_advance` fires bounded deadlines in stable order.
`timer_cancel` withdraws waiter-free pending deadlines and releases capacity;
`timer_reschedule` moves a pending deadline while preserving token identity and
giving positive-delay ties a fresh stable scheduling position.
`timer_interval` creates a copyable fixed-rate `IntervalTimer` whose
`timer_interval_next` calls yield fresh one-shot tick tokens. Missed cadence
deadlines become immediately ready in order, while `timer_interval_cancel`
closes future issuance without revoking tokens already handed out.
`wake_token_select` consumes a bounded token set, chooses the lowest ready
input index, and cancels losing one-shot arms; losing logical timers release
their queue slots immediately.
`wake_token_wait` adds the deterministic blocking form: it advances the
supplied logical clock only to the nearest selected-arm timer, chooses the
lowest ready input, and traps rather than hanging when no arm can progress.
Borrowed `ReadinessArm` adapters generalize that wait across runnable tasks,
buffered-or-closed channel reads, open channel send capacity, and tokens.
`readiness_arm_channel_send` stays connected to later fill, receive, and close
transitions. `readiness_wait` preserves task and channel handles, advances
timer-backed arms deterministically, and cancels only losing token arms.
Hosted runs also expose `readiness_arm_file_read(path)`: a borrowed, `fs.read`-
gated observation that is ready when the confined path names a regular file.
It charges no content bytes, tracks files created after adapter construction,
and remains a deterministic boundary check rather than an OS-polling reactor.
`readiness_arm_file_create(path)` is the `fs.write` counterpart for no-clobber
atomic publication: an absent confined destination is ready, claiming it makes
the arm pending, and observation neither creates the file nor reserves bytes.
`readiness_wait_timeout` adds a bounded logical deadline and returns `None`
without changing any observed source when the deadline wins. Equal deadlines
favor a ready arm, and an empty arm set provides deterministic logical sleep.
`task_group_await_next` advances one child in stable order and releases a
capacity slot, allowing bounded incremental refill without leaking child handles.
`task_group_spawn_wait` turns that step into explicit FIFO backpressure: once
full, the group advances its oldest child before admitting replacement work.
`task_then` moves a standalone task into a bounded typed continuation chain.
The final affine handle preserves pending, parked, or token-waiting state;
await runs each stage in order, while cancellation prevents the whole chain
from running. This composes deferred call boundaries without claiming that an
arbitrary running function can suspend its statement frame.
`task_poll` adds a nonblocking scheduler boundary: it returns `None` without
mutation for parked or token-waiting work and executes a runnable task once as
`Some(value)`. A terminal poll dynamically closes the handle; pending polls
leave it available for wake, cancellation, or a later retry.
`task_wait_on` moves an open task or continuation chain behind a new one-shot
wake gate. Existing waiter registration transfers atomically, an already-ready
gate admits immediately, and cancellation releases the new registration. This
suspends deferred execution without capturing a running statement frame.
`task_wait` consumes one task and either executes it immediately or advances a
supplied logical clock only to its timer-backed wake deadline. Cross-clock and
source-less waits trap instead of hanging; no host time or thread sleep is
involved.
`task_race` consumes a bounded homogeneous vector of tasks, selects the lowest
ready input or advances to the nearest same-clock timer, executes only that
winner, and cancels every loser. Losing task cancellation releases waiter
registrations without cancelling independently aliased wake tokens.
`task_timeout` consumes one task behind a bounded logical deadline and returns
`Some(value)` when ready work or its same-clock timer wins; otherwise it
advances to the deadline, cancels the task, and returns `None`. Equal deadlines
favor the task, while parked and program-token-only work time out without
hanging.
The concurrency effect is visible in the function contract. V3 currently
implements a deterministic task foundation rather than promising a complete
production async runtime, but the intended model is already clear: agents
should not create invisible background work or leak task handles accidentally.
Successful concurrent entry runs also emit a byte-stable
`forgentex.concurrency_receipt.v1` sequence covering task/group/channel
transitions, parked/woken lifecycle changes, bounded token broadcasts and
logical timer and interval control, homogeneous and heterogeneous selection/wait/cancellation,
typed continuation attachment/resume, task wake-source rebinding, nonblocking
poll, blocking task-wait, consuming task-race, logical task-timeout, and
heterogeneous readiness-timeout outcomes, incremental scheduler and
backpressure steps, capacity refusals, cancellation, and queue depth.

See the [structured task-group example](examples/async/structured_task_group.fgx).

### 5. Agents edit program meaning, not line numbers

```text
VERSION 1
AUTHOR agent.demo
SIGNED_BY agent-public-demo
SIGNATURE_ALGORITHM Ed25519
SIGNATURE mock-sig-stage18-demo
TARGET @function.mix_samples
PRECONDITION NODE_EXISTS @function.mix_samples
OP ADD_EFFECT effect=crypto.verify
```

This semantic patch targets the stable identity of `mix_samples`, verifies that
the node still exists, and describes the requested operation directly. Before
applying it, `fgx patch --check-only` can parse the result and run type, effect,
and borrow checks.

That is safer than asking an agent to reproduce an entire file or hoping a
textual patch still points at the right lines after unrelated formatting. Real
patches may also carry authorship and signature metadata; see the
[signed-patch example](patches/signed_add_crypto_effect.fgp).

## What these ideas provide

| Benefit | How Forgentex approaches it |
|---|---|
| Readable human review | `.fgx` favors familiar names, indentation, and explicit declarations. |
| Efficient agent context | `.fgai`, symbol maps, boundary cards, and typed context packets reduce repeated source loading. |
| Safer automated changes | Stable node identities, semantic preconditions, check-only patches, and review plans make edits verifiable. |
| Visible authority | Effects and capability grants expose filesystem, network, crypto, allocation, concurrency, and foreign-runtime behavior. |
| Predictable data movement | Ownership, borrowing, and budgeted dependency-pipeline values describe who may read, fan out, mutate, move, release, or bound data. |
| Governed polyglot reuse | Capsules retain useful C, Rust, Zig, Python, JavaScript, and other code behind typed and auditable boundaries. |
| Reproducible automation | Canonical ASTs, deterministic graphs, structured diagnostics, and receipts give agents stable interfaces. |
| One shared program | Human and agent views round-trip through the same AST instead of creating a second, agent-only implementation. |

The larger idea is that **representation should fit the reader while semantics
remain shared**. Humans should not have to review compressed machine notation,
and agents should not have to consume an entire repository just to change one
function. Forgentex tries to give both sides an appropriate view while keeping
the compiler's safety and meaning as the common ground.

## Why build a language for agentic work?

Most coding agents still spend much of their context window rediscovering file
structure, reading unrelated code, and emitting fragile text diffs. Forgentex
makes those operations part of the toolchain:

- **Stable program structure.** Definitions have structural node identities,
  so tools can address a function or type without depending only on line
  numbers.
- **Small context packets.** Project maps and typed context queries return the
  requested symbol, its signature, dependencies, effects, and nearby tests
  instead of loading whole files.
- **Semantic patches.** Agents can replace, insert, rename, or delete AST nodes
  and ask the compiler to validate the patch before it touches the source.
- **Machine-readable feedback.** Diagnostics, review plans, receipts, and
  analysis results have deterministic JSON forms.
- **Safety at tool boundaries.** Effects and capabilities describe what code
  may do. Borrow checks constrain aliases and lifetimes. Foreign-language
  capsules are typed, gated, and auditable.
- **One source of truth.** Human and agent syntax converge on one canonical
  representation, avoiding two independently evolving implementations.

A typical agent loop can be narrow and checkable:

```text
project map
    ↓
typed context for one symbol
    ↓
semantic patch against its node identity
    ↓
parse + type + effect + borrow checks
    ↓
review plan and receipt
```

## Token usage: where the savings come from

Dense syntax helps, but syntax alone is not the main win. The larger savings
come from not sending irrelevant code in the first place.

| Mechanism | What the agent receives | Current measured signal |
|---|---|---:|
| `.fgai` agent view | A compact form of the same program | About **3–14% fewer** heuristic tokens than `.fgx` in current samples |
| Lossless compact rendering | The smallest reversible file representation found by the benchmark | Roughly **0.82–0.91×** the human-view token count |
| Map + context + semantic patch | Relevant symbols and a small edit instead of a whole-file rewrite | Around **2× cheaper** on a small two-function sample and **8× cheaper** on a twelve-function sample |
| Capsule boundary cards | Types, effects, capabilities, and hashes without repeatedly loading the foreign body | About **3.1× smaller** than the C body and **6.8× smaller** than the Java body in current toy cases |

These are development measurements, not universal performance claims. The
current reports use a heuristic tokenizer and small repository examples; they
should be rerun with the target model's tokenizer and real workloads before
being used as external benchmarks. The important result so far is structural:
**symbol-level retrieval and semantic edits reduce context much more reliably
than punctuation compression by itself.**

You can reproduce the repository's measurements:

```powershell
cargo run --bin fgx -- token-report examples/hello.fgx --json
cargo run --bin fgx -- edit-bench examples/hello.fgx --edits 8 --json
cargo run --bin fgx -- capsule-tokens examples/capsule/celsius_python.fgx --json
```

See [Token-efficiency findings](docs/token_efficiency_findings.md) for the
method, limitations, and complete tables.

## Where the project is today

The honest maturity label is **advanced demo / early alpha**. V3 already has a
broad executable surface, but it is still a proving ground rather than a
language that should carry production systems.

| Area | V3 status |
|---|---|
| Language core | Functions, records, enums, pattern matching, control flow, effects, and borrow-aware semantics are executable |
| Higher-level features | Useful subsets of generics, traits, function values, and non-capturing closures work; the surface is not yet complete |
| Execution | A broad interpreter is available; C and Zig source/native paths cover narrower subsets |
| Agent workbench | Project maps, node lookup, typed context, semantic patches, review plans, graphs, and JSON diagnostics are implemented |
| Polyglot capsules | Typed and audited bridges cover several host/toolchain-backed languages, with availability depending on installed runtimes |
| Runtime experiments | Warm Python, Node, and Go workers; typed budgeted derived/source-declared dependency pipelines; deterministic task, task-group, and channel foundations |
| Media experiments | Small image, audio, timeline, shader, canvas, and preview proofs exercise the data plane |
| Ecosystem | Standard library, package story, native coverage, documentation, and compatibility guarantees are still maturing |

The repository's internal `v1`, `v2`, and `v3` labels describe development
tracks and gate suites. They are not public semantic versions. In particular,
an internal “v1-ready” gate means that a defined compiler slice passed its
checks; it does not mean Forgentex 1.0 has shipped.

## Recent development

Recent work has focused on turning attractive demos into repeatable,
regression-tested behavior:

- **Borrow and lifetime safety:** projected borrows, recursive lifetimes,
  indexed aliases, loop-joined aliases, associated-type ownership shapes,
  affine `FnOnce` call/transfer and mutable-capture ownership,
  exhaustive two-/three-/four-level nested-loop target joins, shrinkable seeded
  campaigns, and persisted rejection/acceptance regressions now run through a
  strict-release-gated reference-model conformance harness.
- **Slice-range foundations:** `xs[start .. end]` now parses, formats,
  type-checks, executes in the interpreter, preserves borrow provenance, and
  proves disjoint constant ranges; unsupported native lowering is blocked
  explicitly.
- **Recursive borrow carriers:** nested `Vec`/array indexing now preserves
  lifetime provenance through typed aliases, and one-to-three-layer generic
  identity/wrapper chains compose those carriers through concrete calls.
- **Concrete generic ownership:** ownership checks now instantiate generic
  parameter types before alias analysis, so mutable borrows cannot be duplicated
  through direct or wrapped `T`-typed calls.
- **Conformance depth:** the current borrow suite exercises **1,962 programs**;
  **1,922** reference/regression cases report zero model mismatches, alongside
  curated coverage.
- **Language expressiveness:** generics, trait dispatch, native function values,
  and non-capturing closure subsets have expanded across parser, checker,
  interpreter, and selected backends.
- **Agent operations:** structural maps, focused context packets, semantic patch
  validation, review planning, graph queries, and token reports are available
  through the CLI.
- **Foreign execution:** capsules gained typed boundary metadata, effect and
  capability checks, warm-worker paths, audit receipts, and token-cost reports;
  native C/C++/Rust/Zig calls now include bounded call-scoped UTF-8 string
  borrows with payload-free byte accounting plus deadlock-free concurrent
  stdout/stderr capture under a configurable combined ceiling.
- **Typed data movement:** the linear data plane tracks ownership transitions
  and receipts through pipeline stages rather than treating bulk data as
  unstructured process output.
- **Quality gates:** the Rust library and CLI suites currently total **1,126
  passing tests** (1,071 library tests and 55 CLI tests), plus fuzz,
  differential, conformance, and strict release-gate workflows.

For the detailed ledger, see [V3 progress](docs/v3_progress.md) and the
[gap-closure plan](docs/gap_closure_plan.md).

## Installation

The public 0.1 toolchain ships two binaries — `fgx` (the CLI) and
`forgentex-lsp` (the language server) — built from source with a stable Rust
toolchain. `fgx dist-manifest --json` prints the machine-readable distribution
manifest (`forgentex.distribution.v1`): supported hosts, install paths,
backends, prerequisites, and the checksum algorithm.

Prerequisites:

| Prerequisite | Required | Purpose |
| --- | --- | --- |
| `rust-stable` | yes | build the `fgx` and `forgentex-lsp` binaries |
| `host-c-compiler` | optional | run the C backend and native/capsule demonstrations |
| `zig-0.14` | optional | run the Zig backend |

Supported host matrix for 0.1: `x86_64-pc-windows-msvc` (primary, CI-verified;
Windows 10 is supported),
with `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` buildable from the
same dependency-free source. From a repository checkout, install both current
binaries onto your `PATH`:

```powershell
cd C:\path\to\Forgentex
cargo install --path . --bins --force
fgx --version
fgx doctor --strict
forgentex-lsp --probe
```

On Windows the default destination is `%USERPROFILE%\.cargo\bin`; ensure that
directory is in the user `PATH`, then open a new terminal. `fgx doctor --json`
reports the resolved executable, host, OS version, interpreter readiness,
source-build tools, and optional C/Zig backend availability. Strict mode
requires a supported host and the current `fgx` executable to resolve from
`PATH`; an absent optional backend remains a recommendation, not a failure.

Release builds are checksummed (SHA-256) and clean-install-verified by the
`release-artifacts` CI job; downloaded artifacts ship a `CHECKSUMS.txt` you can
verify before putting the binaries on your `PATH`.

To exercise the complete `forgepack` workflow through one copied executable,
run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File `
  .\scripts\forgepack-clean-install.ps1 -FgxPath .\target\release\fgx.exe
```

The script copies only that binary into a fresh temporary install directory and
runs `forgepack evidence --strict --json` there.

## Quick start

Forgentex's compiler core is written in Rust and intentionally has no external
Cargo dependencies. Some capsule and native demonstrations require their host
runtimes or toolchains.

```powershell
cd C:\path\to\Forgentex
cargo build
cargo test
```

Format, inspect, and check a program:

```powershell
cargo run --bin fgx -- fmt examples/hello.fgx
cargo run --bin fgx -- ast examples/hello.fgx --json
cargo run --bin fgx -- check examples/hello.fgx --json
```

Scaffold your own runnable application and run it without editing anything:

```powershell
fgx new my-demo --template app
fgx app-run my-demo --arg "hello from forgentex" --json
type my-demo\output\greeting.txt
```

The `app` template writes the `[application]` contract, a declared create-new
`text/plain` artifact, and the `output/` root it publishes into. The default
`lib` template omits all three, so `fgx app-run` reports
`application_contract_missing` against it by design.

Render the amplitude envelope of a real 16-bit PCM audio loop. The app walks
the RIFF chunk list (the bundled file carries a `JUNK` padding chunk before
`fmt `), decimates the payload into one peak per column, and publishes a PPM:

```powershell
fgx app-run examples/apps/wav_waveform --arg input/loop_8bars_128bpm.wav --json
start examples\apps\wav_waveform\output\waveform.ppm
```

The bundled loop is 8 bars at 128 BPM, so its 32 beats fall across 64 columns
and the on-beat/off-beat grid is visible in the output.

Run the bounded graphics/media application and open its generated PPM on
Windows:

```powershell
fgx app-run examples/apps/media_visualizer --arg 7 --json
start examples\apps\media_visualizer\output\visualizer.ppm
```

The artifact uses create-new publication. Remove or rename it before rerunning
the command.

Ask for agent-sized project context and validate the safety model:

```powershell
cargo run --bin fgx -- map examples --out target/forgentex-map.json
cargo run --bin fgx -- context @function.mix_samples --deps types,effects,tests --json
cargo run --bin fgx -- borrow-conformance --json
cargo run --bin fgx -- release-check --strict --json
cargo run --bin fgx -- release-check --target 0.1 --json
```

Run `cargo run --bin fgx -- help` for the live command surface.

## Complete command reference

After `cargo build`, replace `cargo run --bin fgx --` in the examples with the
path to the built `fgx` executable if desired. Run `fgx help` at any time for
the live command list. Commands that accept `--json` emit deterministic output
intended for scripts and agents.

Argument notation used below:

- `<value>` is required; `[value]` is optional.
- `a|b` means choose one value.
- `...` means an option may be repeated.
- A target `<triple>` is a platform identifier such as
  `x86_64-unknown-linux-gnu`.

### Toolchain and installation

| Command | Purpose |
|---|---|
| `fgx --version` | Print the installed CLI version. |
| `fgx doctor [--json] [--strict]` | Diagnose the resolved executable, supported host, OS, interpreter, source-build tools, and optional native backends. Strict mode fails when the host or `PATH` installation is not ready. |

### Source, syntax, and diagnostics

| Command | Purpose |
|---|---|
| `fgx ast <file> --canonical` | Parse a source file and print its canonical AST representation. |
| `fgx parse <file> --emit-ast` | Parse a file and emit the resulting AST. |
| `fgx diagnose <file> --json` | Parse and analyze a file, returning structured diagnostics. |
| `fgx diagnostics list [--json]` | List the diagnostic codes known to the toolchain. |
| `fgx explain <diagnostic-code> [--json]` | Explain one diagnostic and its likely remedy. |
| `fgx fmt <file> --to fgx\|fgai` | Format or translate source into the human or agent view. Without `--to`, it formats the current view. |
| `fgx roundtrip <file> --to fgx\|fgai` | Convert through the canonical AST and verify the requested source view. |
| `fgx check <file> --json` | Run syntax, type, effect, ownership, and other available checks for one file. |
| `fgx check <file> --target <triple> --json` | Check one file with target-specific rules enabled. |
| `fgx check --project [path] --json` | Check a Forgentex project; the current directory is used when `path` is omitted. |
| `fgx cfg <file> [--json]` | Print the control-flow graph used by analyses such as borrow checking. |
| `fgx effects <file> --json` | Report declared and inferred effects and capability requirements. |
| `fgx ir <project\|file> [--json\|--contract-json]` | Inspect the compiler IR, or emit its machine-readable backend contract. |

### Ownership and safety

| Command | Purpose |
|---|---|
| `fgx borrow-report <file> [--json]` | Explain ownership, move, borrow, alias, and lifetime findings for one file. |
| `fgx borrow-conformance [--json] [--campaign-seed <u64>] [--campaign-cases <1..256>] [--failure-dir <path>]` | Compare the borrow checker with the reference model; optionally run a reproducible generated campaign and persist reduced failures. |
| `fgx fix --plan <file\|project> [--target <triple>] [--json]` | Produce a non-destructive remediation plan for current diagnostics. |
| `fgx release-check [--target v1.0\|0.1] [--json] [--strict]` | Run release gates. Historical internal `v1.0` remains available; `0.1` reports the forthcoming public contract and its explicit blockers. Strict mode fails unless the selected target is ready. |
| `fgx release-evidence [--json] [--strict]` | Rerun the frozen public 0.1 example suite (dual-view round-trip, checker, recorded interpreter results) and republish supported limits as `forgentex.public_0_1_evidence.v1`. Strict mode fails on any unresolved gap. |
| `fgx dist-manifest [--json]` | Print the public 0.1 distribution manifest (`forgentex.distribution.v1`): shipped binaries, install paths, supported host matrix, backends, prerequisites, checksum algorithm, and distribution readiness gaps. |

### Running programs and governed effects

| Command | Purpose |
|---|---|
| `fgx run <file> [--entry <function> --json-input <file> --backend interp]` | Execute a Forgentex function with the interpreter, optionally selecting an entry point and JSON input. |
| `fgx app-run <project> [--arg <v>]... [--json]` | Run a manifest-backed `hosted_io` function or source-declared governed `pipeline` after compatibility and supply-chain checks. Hosted apps provide scoped bounded file operations; pipeline apps add aggregate compute/memory limits, native artifact verification, and host-controlled no-clobber publication. Reports use the runtime-specific versioned schema; pre-dispatch JSON failures use `forgentex.application_error.v1`. See [the application runtime contract](docs/application_runtime.md). |
| `fgx forgepack build <spec> <output> [--json]` | Build a byte-deterministic v1 archive from bounded, spec-relative sources and publish it atomically without replacing an existing file. |
| `fgx forgepack extract <archive> <directory> [--json]` | Verify the complete archive, preflight every destination, and atomically create files beneath an existing explicit output root without clobbering. |
| `fgx forgepack evidence [--json] [--strict]` | Run a self-contained build/rebuild/inspect/verify/extract workflow plus no-clobber and corruption denials, and emit `forgentex.forgepack_evidence.v1`. |
| `fgx forgepack inspect <archive> [--json]` | Validate and report the complete archive structure and declared entry metadata without claiming payload hashes were recomputed. |
| `fgx forgepack verify <archive> [--json]` | Strictly validate a v1 archive and recompute every entry SHA-256. See [the frozen format contract](docs/forgepack_v1.md). |
| `fgx io-run <file> --entry <fn> [--grant fs.read] [--grant fs.write] [--arg <v>]... [--json]` | Run an entry point with explicit filesystem capability grants and repeated arguments. |
| `fgx net-run <file> --entry <fn> [--allow <host:port>]... [--arg <v>]... [--json]` | Run an entry point with an explicit network allowlist. |
| `fgx crypto-selftest [--json]` | Execute the deterministic core cryptography self-test suite. |
| `fgx pipeline-run <file> [--set share=value] [--allow-effects a,b] [--schedule declaration\|graph\|source] [--cache-dir dir] [--cache-key-file file] [--stop-after-stages n] [--max-stages n] [--max-wall-ms n] [--max-cpu-ms n] [--max-memory-bytes n] [--max-owned-bytes n] [--max-fs-bytes n] [--max-net-bytes n] [--json]` | Execute a typed pipeline in declaration order, a binding-derived dependency graph, or an exactly validated source-declared graph, with supplied values, an effect allowlist, cancellation, and receipted pipeline-wide budgets. An existing non-symlink `--cache-dir` grants trusted-root integrity-checked reuse; a separate 32–4096 byte `--cache-key-file` outside that directory upgrades records to HMAC-SHA256 group authentication. Filesystem transfer accounting is currently enforced through Windows Job Objects; unavailable filesystem/network accounting fails closed. |
| `fgx trust-session [--capability <cap>] [--payload <p>] [--json]` | Demonstrate a capability-scoped, receipted trust session. |

### Foreign capsules and polyglot execution

| Command | Purpose |
|---|---|
| `fgx capsule <file> [--json] [--strict]` | Inspect and validate the foreign capsules declared in a source file. |
| `fgx capsule-run <file> [--capsule <name>] [--input <values>] [--sandbox] [--json] [--strict]` | Run a selected capsule through its governed host-runtime bridge. |
| `fgx capsule-run <file> --native [--capsule <name>] [--out <dir>] [--json]` | Build or run a native capsule path and optionally retain its artifacts. |
| `fgx capsule-lock <file> [--json]` | Resolve capsule dependencies into deterministic lock information. |
| `fgx capsule-audit <file> [--json] [--strict]` | Audit capsule types, effects, capabilities, hashes, dependencies, and policy findings. |
| `fgx namespace <file> [--json] [--strict]` | Review shared names and cross-language namespace collisions. |
| `fgx sandbox <file> [--json] [--strict]` | Review the sandbox policy required by a capsule file. |
| `fgx runtimes [--json]` | Report discovered host runtimes and bridge availability. |
| `fgx native-review <file> [--json] [--strict]` | Review native-language capsule declarations and native policy requirements. |
| `fgx shell-review <file> [--json] [--strict]` | Review shell capsules and their command/capability surface. |
| `fgx contract-review <file> [--json] [--strict]` | Review smart-contract capsules and contract-specific policy. |
| `fgx capsule-context <file> [--capsule <name>] [--json]` | Produce a compact boundary card and review context for one capsule. |
| `fgx polyglot-run <file> [--out <dir>] [--json]` | Run the coordinated polyglot demonstration and optionally retain generated output. |

### Native builds, targets, and ABI inspection

| Command | Purpose |
|---|---|
| `fgx build <project> --native [--emit obj\|shared] [--out <dir>] [--json]` | Build the supported native subset, optionally emitting an object or shared library. |
| `fgx build <project> --backend c\|zig [--out <dir>]` | Lower a project through the C or Zig source backend. |
| `fgx build <project> --backend c\|zig --target <triple>\|--all-targets [--profile dev] [--json]` | Build for one supported target or every configured target. |
| `fgx asm-check <file> --target <triple> [--json]` | Validate governed assembly blocks for a specific target. |
| `fgx abi <file> --target <triple> [--json]` | Report target-specific ABI and layout decisions. |
| `fgx header <file> --target <triple>` | Generate a foreign-call header for the selected target. |
| `fgx simd-review <file> --target <triple> [--json]` | Review SIMD usage and target compatibility. |
| `fgx backends [--json]` | List compiler backends and their current capabilities. |
| `fgx targets [--json]` | List known compilation targets and support status. |
| `fgx stdlib [--json]` | Report standard-library modules and implementation status. |

### Projects, packages, tests, and public API

| Command | Purpose |
|---|---|
| `fgx new <path> [--name <package>] [--template lib\|app]` | Scaffold a new Forgentex project. The default `lib` template creates a checkable library; `app` adds the `[application]` contract, a declared create-new artifact, and its `output/` root so `fgx app-run` works with no further edits. |
| `fgx lock [project] [--out <file>] [--json]` | Generate deterministic project lock data. |
| `fgx test <file> [--filter <name>] [--target <triple>] [--json]` | Run inline tests from one file, optionally filtering by name or target. |
| `fgx test [--project <path>] [--target <triple>\|--all-targets] [--json]` | Run a project's tests for the host, one target, or every target. |
| `fgx publish <project> --dry-run [--json]` | Validate and preview a package publication without publishing it. |
| `fgx audit [project] [--json]` | Audit project metadata, dependencies, and policy-relevant configuration. |
| `fgx verify [project] [--json]` | Run the project's reproducibility and integrity verification flow. |
| `fgx api --public [--project <path>]` | Print the project's public API surface. |

### Agent context, semantic editing, and token measurement

| Command | Purpose |
|---|---|
| `fgx compress <file> [--emit] [--json]` | Analyze compact representations and optionally emit the selected lossless form. |
| `fgx token-report <file> [--json]` | Compare heuristic token counts for human, agent, and compact source views. |
| `fgx edit-bench <file> [--edits <n>] [--json]` | Compare whole-file editing cost with map/context/semantic-patch cost. |
| `fgx capsule-tokens <file> [--json]` | Compare foreign capsule bodies with their typed boundary-card context. |
| `fgx codebook build\|check\|measure\|stats\|encode\|decode [project\|input] [--codebook <file>] [--previous <file>] [--out <file>] [--budget <n>] [--cryptic] [--force-codebook] [--json]` | Build, evolve, and audit a semantic project tier containing identifiers, naming morphology, type/effect shapes, exact AST patterns, and one-/two-slot atomic expression/statement templates. Stable lineage and never-larger exchange rules apply. |
| `fgx map <dir> --out <file> [--include-targets]` | Build a structural symbol map for a directory. |
| `fgx map --project [path] --out <file> [--include-targets] [--codebook <file>] [--force-codebook]` | Build a structural project map. A current codebook is applied only when the complete `#FGCB` payload is strictly smaller; the write receipt reports both candidate costs. Force mode exists for diagnostics/round-trip tests. |
| `fgx get <symbol> --from <dir> --deps types` | Retrieve one symbol and its required type context. |
| `fgx context <symbol> --deps types,effects,tests [--max-tokens <n>] [--since <context.json>] [--codebook <file>] [--force-codebook] [--out <context.json>] [--project <path>] [--json]` | Build or persist focused context. A current codebook is selected only when final JSON is strictly smaller; otherwise the exact ordinary plain packet is returned. `codebook_ref` presence identifies application. Effects remain explicit. |
| `fgx patch <source> <patch> --review\|--check-only\|--emit-review-json [--project <path>] [--codebook <file>]` | Review or validate a plain or artifact-bound codebook patch. Encoded transport is decoded only after current-corpus and header identity checks, then follows the ordinary semantic patch path. |
| `fgx review <patch> [--source <file>] [--project <path>] [--codebook <file>] [--json]` | Produce an agent review for a plain or codebook patch, binding the artifact hash and reporting transport versus decoded semantic-patch tokens. |

### Graph artifacts

| Command | Purpose |
|---|---|
| `fgx graph dump <project\|dir\|file> [--out <file>]` | Materialize a deterministic `.fggraph` program graph. |
| `fgx graph validate <graph> [--json]` | Validate graph structure, references, and invariants. |
| `fgx graph view <graph> --to fgx\|fgai` | Render a graph artifact as human or agent source. |
| `fgx graph patch <graph> <patch> [--out <graph>] [--json]` | Apply a semantic patch to a graph artifact. |
| `fgx graph diff <before> <after> [--json]` | Compare two program graphs structurally. |
| `fgx graph roundtrip <project\|dir\|file> [--json]` | Verify source-to-graph-to-source stability. |
| `fgx graph context <graph> <symbol> --deps types,effects,tests [--json]` | Extract focused symbol context directly from a graph artifact. |

### Fuzzing, differential testing, and performance

| Command | Purpose |
|---|---|
| `fgx fuzz [--iterations <n>] [--json]` | Run deterministic parser/formatter and semantic fuzz campaigns. |
| `fgx diff-test [--project <path>] [--source <file>] --entry <function> --json-input <file> [--json]` | Compare an entry point across available execution paths using the same JSON input. |
| `fgx bench <file> --entry <function> --json-input <file> --target <triple> [--iterations <n>] [--json]` | Benchmark a function for a target with reproducible input and iteration count. |
| `fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]` | Compare scalar and optimized implementations and report the measured delta. |

The supported language subset varies by interpreter, capsule bridge, and native
backend while v3 remains in active development. Treat `fgx help`, structured
diagnostics, and the backend reports as authoritative for the checked-out
revision.

## The road to Forgentex 0.1

The next release is meant to be the first coherent public version of the real
language—not another internal stage label. The 0.1 effort is about choosing and
stabilizing a trustworthy subset from the v3 laboratory:

1. Freeze a documented core grammar and canonical AST contract.
2. Make interpreter, checker, formatter, and primary build path agree on that
   subset.
3. Freeze explicit compatibility, diagnostic-code, and versioned JSON-schema
   expectations.
4. Ship a small, useful standard foundation and end-to-end examples.
5. Keep agent context, semantic patching, effects, borrowing, and auditability
   as first-class language-toolchain behavior.
6. Publish reproducible tests and benchmarks with clearly stated limits.

The candidate contract and machine-checked blockers are defined in the
[public 0.1 profile](docs/public_0_1_profile.md). Run
`fgx release-check --target 0.1 --json` for the current evidence report;
`--strict` intentionally fails while its decision is
`public_0_1_not_ready`.

Until those promises are written down and enforced, v3 should be approached as
a sophisticated preview: excellent for experiments, language design work, and
agent-tooling research, but not yet for production commitments.

## Documentation

- [Public 0.1 profile](docs/public_0_1_profile.md)
- [Public 0.1 execution matrix](docs/public_0_1_execution_matrix.md)
- [Public 0.1 compatibility contract](docs/public_0_1_compatibility.md)
- [V3 progress ledger](docs/v3_progress.md)
- [Gap-closure plan](docs/gap_closure_plan.md)
- [Human grammar](docs/grammar_human_fgx.md) and [agent grammar](docs/grammar_agent_fgai.md)
- [Agent workbench protocol](docs/agent_workbench_protocol.md)
- [Token-efficiency findings](docs/token_efficiency_findings.md)
- [Borrow checker](docs/borrow_checker_v1.md)
- [Foreign-capsule specification](docs/foreign_capsule_spec.md)
- [Native borrowed string boundary](docs/native_string_inputs.md)
- [Testing strategy](docs/testing_strategy.md)

## Contributing

Forgentex is a demo becoming a language, so clear counterexamples are as useful
as feature contributions. Good contributions include minimal parser or checker
reproductions, conformance cases, agent-workflow measurements, documentation,
and small examples that reveal where two execution paths disagree.

Please open an issue before a large design change so the intended semantics can
be agreed before implementation work begins.

## License

Forgentex is dual-licensed under your choice of MIT or Apache-2.0.

<div align="center">

Built by **Viktor Roze** as an experiment in programming languages that humans
can trust and agents can use efficiently.

</div>
