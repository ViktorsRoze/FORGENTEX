# Native + Polyglot Plan (v2.12 and beyond)

This is the plan to turn Forgentex from a language that *reviews and
specifies* foreign code into one that *runs* it — a true full polyglot with
a small, useful native core, real cryptography and I/O for agentic work, and
a special, token-efficient relationship with Python.

## The one decision that makes it all coherent

We do **not** write a custom code generator or pull in heavy Rust
dependencies. We stand on **host toolchains**, the same way the
managed-runtime catalog (v2.5) already detects installed runtimes:

- **Native core** = emit C (the transpiler already exists) and drive the
  host C compiler (`cc`/`clang`) to a real artifact.
- **Borrow C / C++ / Rust / Zig** = compile the capsule's source with its
  host toolchain to an object/static library and link it through the **C
  ABI**. The typed capsule boundary *is* the `extern "C"` signature.
- **Borrow Python** = the host **CPython** interpreter, embedded or as a
  captured subprocess.
- **Borrow JS/TS** = a host JS runtime for the web tier.

Consequences that keep the project honest:

1. The Rust toolchain core stays **dependency-free**. Host compilers and
   interpreters are *external tools*, detected at use time and **gated
   gracefully when absent** (report "unavailable", never crash) — exactly
   like runtime detection today.
2. Everything real is still **deterministic and captured**: every execution
   produces a transcript (inputs, effects exercised, outputs, artifact
   hashes) so runs are replayable and testable without the host tool.
3. The security spine already built — **effects + sandbox grants +
   dependency lock** — moves from *review-time* to *runtime enforcement*.
   Nothing links or runs unless its effects are covered by granted
   capabilities.

## Vertical-slice discipline (unchanged)

Each milestone ships as one slice: AST/representation only if needed, a
`validate(program) -> Vec<Diagnostic>` (or runtime gate), a
`forgentex.*.v1` report with `to_text`/`to_json`, a `fgx` subcommand, at
least one example, tests, docs, and a roadmap update. The v1.0 release check
(`fgx release-check --target v1.0 --strict`) must stay green at every step.

## Dependency graph

```text
v2.12 native build core
   ├── v2.13 C-ABI FFI bridge ──┬── v2.16 core.crypto (FFI to pinned C lib)
   │                            └── v2.18 unified polyglot ABI
   ├── v2.14 core.io (effect-gated)
   └── v2.15 core.net (effect-gated) ── v2.19 agent trust fabric
v2.17 python bridge (CPython) ───────── v2.18 unified polyglot ABI
v2.18 unified polyglot ABI ──────────── v2.20 JS/TS runtime + web one-roof
v2.17 bridge pattern ────────────────── v2.21 TS/Node · v2.22 Java · v2.23 C# · v2.24 Go
v2.16 core.crypto + v2.15 core.net ──── v2.19 agent trust fabric
```

Suggested build order: 2.12 → 2.13 → 2.14 → 2.15 → 2.16 → 2.17 → 2.18 →
2.19 → 2.20. 2.14/2.15 can interleave; 2.17 can start any time after 2.12.

---

## v2.12 — Native build core

**Goal.** Forgentex's own pure/numeric/effectful core becomes a real native
artifact, so "useful things that make sense" (DSP kernels, parsers,
exact-math routines) run at native speed.

**How.** `fgx build --native <pkg>` reuses the existing C transpiler to emit
C, then invokes the detected host C compiler to produce an executable or
static library. `@exact` functions compile with the exact integer path; the
fast float path is the default.

**Deliverables.** `forgentex.native_build.v1` report (toolchain id/version,
compiler flags, source hash, artifact hash + path, list of compiled-in
effects, exact-vs-fast mode per function); graceful "compiler unavailable"
status; an example package that builds to a real binary; a CLI integration
test that skips cleanly when no `cc` is present.

**Honest scope / risks.** No custom codegen and no optimizer — we inherit
the host compiler's. Determinism of the *artifact* depends on the host
toolchain, so the report records the toolchain identity and we test the
*transcript*, not the binary bytes.

## v2.13 — C-ABI FFI bridge

**Goal.** Actually run C/C++/Rust/Zig capsules instead of mocking them.

**How.** The typed boundary (`input`/`output`/`effects`) lowers to an
`extern "C"` signature. `fgx capsule-run --native` compiles the capsule
source with its host toolchain to an object/static library, links it into
the native build, and marshals `CapsuleValue` ↔ C-ABI types across the call.
Linking and execution are **gated**: the capsule's declared effects must be
covered by granted sandbox capabilities, and its dependencies must match the
lockfile, or the run is refused.

**Deliverables.** `forgentex.ffi_plan.v1` (boundary → C signature, value
marshaling table, link units, effect/grant reconciliation, run transcript);
`fgx capsule-run --native`; a C `clamp_c` capsule that returns a real value;
tests with a captured transcript so CI passes without a C compiler.

**Honest scope / risks.** C ABI only at first (Rust/Zig via their C export);
value types limited to the marshalable `CapsuleValue` set (scalars, slices,
structs with known layout). Memory ownership across the boundary is
explicit and reviewed; no implicit allocation crossing the line.

## v2.14 — `core.io`: real, effect-gated I/O

**Goal.** Agentic work is I/O-heavy; give it real file/filesystem I/O that
is safe by construction.

**How.** Native `core_io_*` builtins, each annotated with its effect
(`fs.read`/`fs.write`). At runtime every call is checked against the sandbox
grant set; denied calls fail with a diagnostic, not silently. Each operation
is appended to a deterministic I/O transcript in the run report.

**Deliverables.** `forgentex.io_run.v1` (operations, paths touched, bytes,
grant decisions, transcript hash); a `file_copy` example that copies under
an explicit `fs.read`+`fs.write` grant and is refused without it; tests
using a temp dir.

## v2.15 — `core.net`: real, effect-gated networking

**Goal.** The transport substrate for agent-to-agent communication.

**How.** Native `core_net_*` for TCP/UDP and a minimal HTTP client/server,
effect-typed (`network.read`/`network.write`) and gated by a host/port
allowlist taken from the sandbox plan. Deterministic transcripts capture the
request/response envelope (not wall-clock-dependent fields) for replay.

**Deliverables.** `forgentex.net_run.v1`; a loopback request/response
example; tests against an in-process mock transport so CI needs no network.

## v2.16 — `core.crypto`: real cryptography

**Goal.** The trust backbone for the agent economy — sign work, verify
peers, encrypt channels — with misuse-resistant, typed APIs.

**How.** Real primitives (hash, MAC, Ed25519 sign/verify, AEAD, KDF,
secure-random) via FFI (v2.13) to a **pinned, audited** C crypto library
(libsodium-class). The typed API already specified (`SecretKey`,
`PublicKey`, `Signature`) gets **runtime-enforced** secret handling:
zeroization on drop, constant-time comparison, and a hard rule against
logging/serializing secret-typed values. Real signing/verifying of semantic
patches and `AgentMessage`s.

**Deliverables.** `forgentex.crypto_run.v1`; `crypto.sign`/`crypto.verify`
effects gated like any other; a signed-then-verified semantic-patch example
that fails closed on tamper; constant-time and zeroization tests.

**Honest scope / risks.** We *borrow* vetted crypto, never roll our own
primitives. The pin + checksum + audit (v2.3) is the supply-chain control;
the report names the exact library and version.

## v2.17 — Python bridge (the special relationship)

**Goal.** Agents love generating Python — make Python both **powerful** (its
whole ecosystem) and **cheap** (tiny token surface) under Forgentex.

**How.** Real CPython execution of Python capsules via the host interpreter,
with typed-boundary marshaling and deterministic output capture. The
token-efficient calling convention: the boundary signature is declared
**once**; thereafter the agent calls the capsule **by name through the
shared namespace** and never re-sends the Python glue or import boilerplate.
Heavy libraries (numpy, pandas, scikit, …) live behind a typed,
effect-gated, sandboxed capsule whose *entire* agent-visible surface is the
boundary signature — so using a 50-line numpy routine costs the tokens of
its one-line typed signature, not the body.

**Deliverables.** `forgentex.python_run.v1`; a real `stats_python` /
`normalize_python` capsule executed against its fixtures; a token-report
comparison showing the boundary-only call surface vs inlining the Python.
Replaces the v2.1 mock executor.

**Honest scope / risks.** CPython is not deterministic across versions, so
the version is pinned and recorded; the transcript (not the interpreter) is
the test oracle. Sandbox/effects still gate file/net/proc access from inside
Python.

## v2.18 — Unified polyglot execution ABI (the capstone)

**Goal.** "Become a true full polyglot": one model for calling *any*
language, with one data plane and one safety gate.

**How.** A single `CapsuleValue` marshaling layer and calling convention
spanning C/C++/Rust/Zig (C ABI), Python (CPython), and the managed runtimes.
The **shared typed namespace** (v2.2) becomes the cross-language data plane:
a value declared once in Forgentex flows across every language by name —
declare `gain: f32` once, read it in C, mutate it in Python, render it in
JS, all type-checked at the boundary. One effect/sandbox gate governs every
backend identically.

**Deliverables.** `fgx polyglot-run`; `forgentex.polyglot_abi.v1` (the value
marshaling matrix per language, the namespace data-plane bindings, the
unified effect/grant decision); an example mixing a C kernel + a Python
post-step sharing one typed buffer.

## v2.19 — Agent trust fabric

**Goal.** Close the agent-economy loop end-to-end and for real.

**How.** On top of crypto (v2.16) and net (v2.15): signed capability tokens,
agent identity/key management, replay protection (nonce + timestamp), signed
semantic-patch exchange between two agents over the real transport, and
audit receipts for every exchange.

**Deliverables.** `forgentex.trust_session.v1`; a two-agent demo that
proposes → signs → sends → verifies → reviews → receipts a patch; replay and
tamper tests that fail closed.

## v2.20 — JS/TS runtime + web one-roof

**Goal.** The original v2 dream: a web server with backend and frontend
under one Forgentex roof, executable.

**How.** Real JS/TS execution for web capsules via a host JS runtime,
sharing the typed namespace with the Python/native backend so a `Session`
struct declared once is the same type on the server and in the browser.

**Deliverables.** `forgentex.web_run.v1`; the `web_session`/`web_counter`
demos run live; a typed-namespace round-trip test across backend and
frontend.

---

## Cross-cutting principles

- **Dependency-free Rust core.** All real execution stands on *host* tools,
  detected and gated; the toolchain itself adds no Rust dependencies.
- **Fail closed.** No effect runs without a covering grant; no foreign
  artifact links without a matching lock entry; secret-typed values cannot
  be logged or serialized.
- **Deterministic transcripts.** Every run emits a replayable transcript so
  tests and CI never require the host tool to be installed.
- **Token discipline.** Foreign power is exposed through the *boundary
  signature*, not its body — the agent pays for the interface, not the
  implementation. Measured with `fgx token-report` / `fgx edit-bench`.
- **Never regress v1.0.** The release check stays green across the phase.

## Managed runtime bridges (v2.21–v2.24)

The Python bridge (v2.17) establishes one pattern; the business, web, and
cloud languages reuse it. None of these go through the C-ABI bridge — they
are host *runtimes*, detected and gated exactly like Python, each running its
capsule across the typed boundary with effects/sandbox gating, deterministic
capture, and ecosystem dependency pinning through the v2.3 audit. This is
"basic support": run real code in the language, typed at the boundary,
safe and captured — not deep zero-copy interop or full library type-mapping.

| Milestone | Language | Host tool(s) | Dependency manifest | Use case |
| --- | --- | --- | --- | --- |
| v2.21 | TypeScript / JS | `node`, `tsc`/esbuild | `package.json` / `package-lock.json` | web + LLM frontends, tooling |
| v2.22 | Java | `javac` / `java` (JDK) | Maven `pom.xml` / Gradle + lockfile | business |
| v2.23 | C# | `dotnet` (.NET SDK) | `*.csproj` / `packages.lock.json` | business |
| v2.24 | Go | `go` | `go.mod` / `go.sum` | cloud & network services |

Each emits `forgentex.<lang>_run.v1` and is selected by the capsule's
declared language. Go is special: besides subprocess execution it can expose
a C-ABI surface via `go build -buildmode=c-shared`, so it can *also* link
through the v2.13 FFI bridge when a native boundary is preferred. All four
plug into the unified polyglot ABI (v2.18) and share the typed namespace, so
a value declared once in Forgentex flows across every language by name. The
v2.5 managed-runtime catalog already recognizes these ecosystems; these
milestones make them execute.

## Backend dependency policy

The "dependency-free" rule is a **boundary, not a ban**. Two kinds of
dependency are not the same thing:

- **Host-tool dependencies** (`cc`, `gcc`, `python`, a JS runtime) — invoked
  at runtime, detected and gated. This whole phase already relies on them,
  by design. They are *not* what the rule protects.
- **Rust crate dependencies** (`Cargo.toml`) — what actually determines
  whether the trusted core stays auditable.

The policy:

1. **The trusted core stays zero-dependency, permanently.** Parser, AST,
   typechecker, formatter, semantic-patch engine, diagnostics, capsule
   review. For a language whose pitch is agent trust and auditability, a
   zero-dependency core is a feature, not just discipline.
2. **The backend is a pluggable, optional layer.** Any future Rust
   dependency (a JIT or optimizing backend) lives in a separate backend
   crate behind a `cargo` feature flag, **off by default**, so a default
   build stays zero-dep and auditable.
3. **C-emission stays the always-available fallback.** No backend is ever
   mandatory; you opt *into* a heavier one, never get stuck with it.
4. **Dogfood the supply chain.** Any backend dependency we take goes through
   the same pin + checksum + supply-chain audit machinery we built for
   capsules (v2.3).

We do **not** need to break this rule for size, optimization, or target
coverage — emitting C already borrows a world-class optimizing backend (the
host compiler) for free. We break it only for capabilities C-emission cannot
express: JIT/REPL, IR-level control, and dev-loop compile speed. That is the
subject of the v3 plan (`docs/v3_backend_plan.md`).

## Parallel tracks (not renumbered here)

The audio track (`audio.2`–`audio.6`: realtime-safety effects, biquad EQ +
ring buffer, miniaudio capsule, resampling, live playback) and the numeric
track (`numeric.2`–`numeric.5`: `core.fixed` Q15/Q31, float-determinism
differential tests, rounding modes, rational/decimal) continue alongside
this phase and benefit directly from the native core (v2.12) and the FFI
bridge (v2.13).
