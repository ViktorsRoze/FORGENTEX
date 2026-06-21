<p align="center">
  <img src="fgai_fgx_logo.png" alt="Forgentex logo — .fgai and .fgx" width="320">
</p>

<h1 align="center">Forgentex</h1>

<p align="center">
  <b>The agent-native language that brings tools, agents, and other programming languages together — typed, gated, audited, token-efficient.</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/v1.0-released-blue" alt="v1.0">
  <img src="https://img.shields.io/badge/v2%20polyglot-complete-blue" alt="v2 polyglot complete">
  <img src="https://img.shields.io/badge/dependencies-zero-brightgreen" alt="zero dependencies">
  <img src="https://img.shields.io/badge/tests-377%20passing-brightgreen" alt="377 tests">
  <img src="https://img.shields.io/badge/languages-7%20runtimes-brightgreen" alt="7 language runtimes">
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-lightgrey" alt="license">
</p>

```text
Humans design and audit.
Agents build and optimize.
Compiler enforces truth.
```

---

## What is this?

Most programming languages are designed for human fingers. Forgentex is
designed for a world where **AI agents write most of the code** and humans
act as architects, reviewers, and safety auditors.

The same program always exists in two forms — two lossless views of one AST:

<table>
<tr>
<th><code>.fgx</code> — what humans review</th>
<th><code>.fgai</code> — what agents read &amp; write</th>
</tr>
<tr>
<td>

```python
struct Point:
    x: i32
    y: i32

fn area(width: i32, height: i32) -> i32:
    return width * height
```

</td>
<td>

```text
S Point { x i32; y i32 }

F area(width i32, height i32) -> i32 {
  R width * height
}
```

</td>
</tr>
</table>

Agents don't rewrite files — they send **semantic patches** the compiler
validates before anything touches disk, and the compiler answers with a
review report a human can act on in seconds: *what changed, which
unsafe/assembly/ABI/effect surfaces it touched, and which tests prove it.*

## Now it runs for real — the v2 polyglot arc (v2.0 → v2.24)

v1 made the language. **v2 made it execute — across seven language families,
all under one typed boundary, one capability gate, one audit, with a
dependency-free core.** The strategy: never reimplement a runtime — *stand on
host toolchains* and govern them.

Every bridge was **verified on a real host runtime** this development cycle:

| Language | Backend | Verified on |
| --- | --- | --- |
| C / C++ / Rust / Zig | native object + C-ABI FFI | **gcc 15.2.0** |
| Python | host CPython | **CPython 3.8.5** |
| JavaScript / TypeScript | host Node | **Node 24.17.0** |
| Java | host JDK (`javac` + `java`) | **OpenJDK 21.0.11** |
| C# | host .NET SDK | **.NET SDK 2.1.801** |
| Go | host `go run` | **go 1.26.4** |

One command runs them all through **one calling convention and one gate**, and
the shared typed namespace becomes a **cross-language data plane** — a value
declared once flows across languages *by name*:

```text
$ fgx polyglot-run examples/web/one_roof.fgx
schema=forgentex.polyglot_abi.v1 status=ok languages=[javascript, python]
  @accumulate   python      backend=python  gate=allow exec=ok  2/2
  @render       javascript  backend=node    gate=allow exec=ok  2/2
  data plane (declare once, flow by name):
    total : i64 -> accumulate[python:rw], render[javascript:read]
  wired fixtures: 4/4 passed
```

Alongside the bridges, v2 shipped the rest of a real runtime — all gated and
transcribed: **native build** (`fgx build --native` → host `cc` → a linkable
object), **effect-gated I/O** (`core.io`), **effect-gated TCP** (`core.net`),
**real crypto hashing** (`core.hash` — SHA-256/HMAC), and an **agent trust
fabric** (signed capability tokens, replay/tamper rejection, audit receipts).

## Experiment results

Forgentex's culture is *measure, don't assume*. The numbers below come from
the toolchain's own measurement commands.

**Token efficiency**

- 🩹 **Edit-task benchmark** (`fgx edit-bench`): an agent that edits via
  *map + per-symbol read + semantic patch* beats *whole-file resend* **from the
  very first edit** — ~**2×** cheaper on a 2-function file, ~**8×** on a
  12-function file.
- 🧩 **Polyglot boundary equalizer** (`fgx capsule-tokens`): agents work from a
  one-line **boundary card**, not the foreign body. Body-vs-boundary tokens
  run **3.1× (C)** to **6.8× (Java, ~85% saved)** — verbose languages cost no
  more to *use* than terse ones, because the body is content-addressed and
  pulled only when edited.
- 🔤 **Dual-name prelude**: high-frequency builtins get two-letter `.fgai`
  aliases, moving `audio_mix` agent/human tokens **0.93 → 0.82**, lossless.

**Real execution, verified**

- 🔧 **Native**: `fgx build --native` produced a real object via gcc with
  linkable exported symbols (`nm` shows `T clamp_i32`, …).
- 🔐 **Crypto**: `fgx crypto-selftest` passes **6/6** official NIST/RFC 4231
  vectors; `SHA-256("abc")` matches CPython's `hashlib` byte-for-byte.
- 📁 **I/O fails closed**: a copy is **denied without an `fs` grant** and the
  destination file is *never created*; allowed with the grant, it copies for
  real — every op in a deterministic transcript.
- 🌐 **Networking fails closed**: a TCP request to an **unlisted endpoint never
  opens a socket**; an allowlisted one does a real loopback round-trip.
- 🤝 **Agent trust**: an HMAC capability token + signed message crosses a real
  TCP socket; **replay and tampering are rejected**; the secret key is never
  logged (only a `key_id`).

Reproduce any of these:

```bash
fgx edit-bench examples/audio/src/dsp.fgx          # token benchmark
fgx capsule-tokens examples/capsule/order_total_java.fgx
fgx crypto-selftest                                # known-answer crypto vectors
fgx trust-session                                  # end-to-end agent trust loop
fgx polyglot-run examples/web/one_roof.fgx         # backend + frontend, one roof
```

## Why would I want this?

When an LLM edits a normal codebase it burns tokens on formatting, reads whole
files to change one function, breaks things with fragile text diffs, and gets
compiler errors written for humans. Forgentex treats the program as a **typed
semantic database** and the surrounding tools/agents/languages as a **governed
fabric**:

- 🗺️ **Project maps** (`.fgmap`) — navigate by symbol, load only the nodes you
  need, with token-cost estimates.
- 🩹 **Semantic patches** (`.fgp`) — small, validated, signable, conflict-checked
  by node hash.
- 🤖 **Machine-readable diagnostics** — stable codes, node IDs, expected/found
  types, and *suggested repair patches* (`fgx fix --plan`).
- 🛡️ **Safety you can audit** — ownership/borrow checking, an effect system
  (`fs.read`, `network.write`, `crypto.sign`, `gpu`, …) now *enforced at
  runtime*, explicit `unsafe`, structured assembly, capability sandboxing.
- 🧬 **Foreign code, governed** — Python/JS/TS/Java/C#/Go/C/… run behind a
  typed boundary, sandbox, pinned + audited dependencies, and a deterministic
  fixture transcript.

## Try it in two minutes

All you need is [Rust](https://rustup.rs). The toolchain has **zero
dependencies** — `cargo build` and you have the whole thing. (The language
*bridges* use whatever host runtimes you have installed, and degrade
gracefully to `runtime_unavailable` when one is absent.)

```bash
git clone <this-repo> && cd Forgentex
cargo build --release

# the same program in both syntaxes
cargo run --bin fgx -- fmt examples/hello.fgx --to fgai

# type-check with agent-readable diagnostics
cargo run --bin fgx -- check examples/media/audio_mix.fgx --json

# run a Python capsule for real, behind a typed boundary
cargo run --bin fgx -- capsule-run examples/capsule/celsius_python.fgx --python

# build Forgentex's own code to a native object via the host C compiler
cargo run --bin fgx -- build examples/native --native

# ask the toolchain to grade itself
cargo run --bin fgx -- release-check --target v1.0 --strict
```

That last command exits zero: **all fifteen v1.0 maturity gates still pass.**

## The project in numbers

| | |
| --- | --- |
| 🧰 CLI commands (`fgx`) | **61** — parse, check, patch, run, build, capsule-run, polyglot-run, trust-session, edit-bench, capsule-tokens… |
| 📚 Standard library modules | **17** (`core.math`, `core.simd`, `core.io`, `core.net`/`core.socket`, `core.hash`, …) |
| 🌉 Language runtimes | **7 families** (C/C++/Rust/Zig via FFI, Python, JS, TS, Java, C#, Go) |
| 🏗️ Compiler | **133** Rust files, **~41,900 lines** of dependency-free core |
| ✅ Tests | **377** (unit + integration) plus a 30-case borrow conformance suite |
| 📊 Report schemas | **60+** stable `forgentex.*.v1` JSON reports (one per subsystem) |
| 🖥️ CPU architectures | **5** (x86_64, ARMv7, AArch64, RISC-V 32/64) |
| ⚙️ Backends | AST interpreter + **C11**/**Zig** source + **native object** (host `cc`) |
| 📦 Examples | **66** — media/DSP, capsules in 7 languages, I/O, networking, trust, polyglot |
| 📖 Design docs | **80** in [`docs/`](docs/), one per subsystem with acceptance gates |
| 🏁 Milestones | v0.1 → **v1.0**, then the full **v2.0–v2.24** polyglot arc |

## The road through v1.0

The hardest v1.0 gate was ownership and borrow checking, built in 13 staged
increments and unified by a **CFG dataflow borrow checker**: moved paths,
alias places, and liveness as fixpoint analyses over the control-flow graph,
so every control-flow shape is checked by one algorithm. A conformance suite
(`fgx borrow-conformance`) pins all of it. Every other v1.0 gate shipped along
the way — dual grammar + AST schema, the semantic patch protocol, project-wide
checking, the effect system, structured assembly for five architectures,
portable SIMD with enforced fallbacks, ABI/layout reports, a package manager
with lockfiles and audits, fuzzing, and differential tests.

## What's next (v3)

Planned and designed (see [`docs/v3_backend_plan.md`](docs/v3_backend_plan.md)):

- ⚙️ **Backend maturity** — a stable Forgentex IR, dependency-free IR
  optimization, then an **optional, feature-flagged** Cranelift JIT (REPL +
  hot function reload) and LLVM path. The trusted core stays zero-dependency;
  backends are opt-in. (Why: C-emission already gives a world-class optimizing
  backend for free; we only add a dependency for JIT/IR needs.)
- 🪙 **Project codebook** — an opt-in, per-project *learned* token optimizer
  that compresses a project's own vocabulary into a committable codebook agents
  reference ([`docs/project_codebook.md`](docs/project_codebook.md)).
- 🧠 **Latent agent-message protocol** — the typed, signed, model-versioned
  *envelope and audit* for latent multi-agent collaboration (inspired by
  LatentMAS): Forgentex governs the exchange and wraps the model runtime as a
  capsule; it never computes latents itself
  ([`docs/latent_agent_protocol.md`](docs/latent_agent_protocol.md)).

## Dig deeper

- [commands.md](commands.md) — full command reference and scope
- [docs/v2_roadmap.md](docs/v2_roadmap.md) — the v2.0–v2.24 polyglot arc, milestone by milestone
- [docs/native_polyglot_plan.md](docs/native_polyglot_plan.md) — the "stand on host toolchains" architecture
- [docs/polyglot_abi.md](docs/polyglot_abi.md) — one calling convention across every backend
- [docs/agent_trust_fabric.md](docs/agent_trust_fabric.md) — signed, replay-protected agent messages
- [docs/token_efficiency_findings.md](docs/token_efficiency_findings.md) — the measured token results
- [docs/semantic_patch_protocol_v1.md](docs/semantic_patch_protocol_v1.md) / [docs/borrow_checker_v1.md](docs/borrow_checker_v1.md) — how agents edit, and the safety model

## Status & contributing

Forgentex is research-grade: the language surface is intentionally small, the
workflow is real, and everything is verified by `cargo test`,
`cargo clippy -- -D warnings`, fuzzing, differential tests, and the
toolchain's own `fgx release-check`. The language *bridges* run on whatever
host runtimes you have and degrade gracefully when one is missing. If an
agent-native, human-auditable, polyglot systems language excites you — issues,
ideas, and experiments are very welcome.

**License:** MIT OR Apache-2.0
**Viktors Roze**
