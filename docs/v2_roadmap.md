# Forgentex v2 Roadmap

This roadmap captures post-v1 ideas that fit Forgentex's agent-native identity
but should not distract from the v1.0 safety release. v2 begins after the core
language, ownership model, effect system, package flow, and review reports are
stable enough to host less-trusted code deliberately.

## Main Theme: Foreign Code Capsules

v2 should let Forgentex projects contain typed, reviewable blocks of code from
other programming languages. The goal is to let agents reuse strong existing
ecosystems, libraries, and starting projects while keeping the Forgentex
boundary explicit.

Example sketch:

```fgx
foreign python version "3.13" env "cpython-3.13-linux-x86_64" effects [fs.read]:
    input value: i32
    output normalized: i32
    deps [
        "pypi:numpy==2.2.0#sha256:..."
    ]
    code """
    def normalize(value: int) -> int:
        return max(0, min(255, value))
    """
```

The exact syntax is not fixed yet. The contract is more important than the
surface syntax.

## Capsule Contract

Every foreign-language capsule must declare:

- language, language version, runtime, and target environment
- typed inputs and outputs at the Forgentex boundary
- declared effects and capabilities, such as filesystem, network, subprocess,
  GPU, package download, or environment-variable access
- dependency list pinned by version, lockfile entry, commit, checksum, or
  artifact hash
- sandbox policy and allowed host paths or network endpoints
- deterministic test inputs, expected outputs, and error shape
- review metadata for humans and agents

Foreign code is external code, not trusted Forgentex code. It should be easy to
use, but never invisible.

## Priority Foreign Language Catalog

The v2 capsule model should include the languages agents are most likely to use
when strong existing libraries or starting projects already exist. Python 3.13
is the first prototype, but the roadmap should explicitly prepare for this
catalog:

| Language | Capsule role | Extra review requirements |
| --- | --- | --- |
| Python | Data, ML, statistics, scripting, automation, and quick use of existing libraries. | Exact runtime such as Python 3.13, pinned package set, deterministic input/output fixtures. |
| JavaScript | Web tooling, JSON-heavy workflows, automation, and existing Node-style libraries. | Exact runtime, package-lock evidence, network/package-download effects separated from execution effects. |
| TypeScript | Typed web/service code, SDKs, protocol clients, and generated API clients. | Type-check command, emitted JavaScript target, pinned compiler/runtime, generated artifact hash. |
| Java | Enterprise libraries, JVM services, Android-adjacent tooling, and mature ecosystem projects. | JDK/runtime version, build-tool lock evidence, classpath/module boundary, sandboxed process policy. |
| C# | .NET libraries, Windows/server tooling, game/tooling ecosystems, and enterprise integrations. | .NET runtime version, NuGet lock evidence, assembly boundary, platform and permission review. |
| C | System libraries, ABI wrappers, device/toolchain integrations, and existing portable code. | Treat as unsafe foreign code unless fully isolated; require ABI, compiler, target triple, and memory-boundary review. |
| C++ | Performance libraries, simulation engines, graphics, robotics, and existing large projects. | Treat as high-risk native code; require compiler, standard version, ABI, allocator, exception, and ownership boundary review. |
| CUDA | GPU kernels, ML acceleration, simulation, image/media processing, and numeric workloads. | GPU target, CUDA toolkit/driver assumptions, resource limits, deterministic CPU fallback or golden fixtures. |
| Go | Network services, CLI tools, infrastructure agents, and concurrency-heavy libraries. | Go toolchain version, module lock evidence, process/network effects, goroutine/resource limits. |
| Bash | Unix automation and build/deploy glue. | Highest-risk shell capsule; require explicit command allowlist, no implicit env access, no unreviewed globbing, and strict sandbox. |
| PowerShell | Windows automation, admin tooling, and cross-platform operational scripts. | Highest-risk shell capsule; require command allowlist, execution policy, module pins, host-path limits, and strict sandbox. |
| Solidity | EVM smart contracts and crypto protocol integration. | Exact compiler version, EVM target, ABI hash, gas model, chain/network effects, and no secret-key access by default. |
| Vyper | EVM smart contracts where a smaller contract language is preferred. | Exact compiler version, EVM target, ABI hash, gas model, chain/network effects, and no secret-key access by default. |

SQL and WASM remain useful capsule targets too, but the language catalog above
captures the first famous general-purpose, shell, GPU, native, and crypto
languages that Forgentex should plan around.

Smart-contract capsules for Solidity and Vyper should be treated as crypto
artifacts, not ordinary scripts. They must declare compiler version, EVM target,
chain or local-VM target, ABI hash, storage-layout hash when available, gas
limits, deployment permissions, network effects, and wallet/key access. Wallet
or private-key access should be denied by default and require an explicit
high-risk capability.

## Planned v2 Milestones

| Label | Status | Goal | Exit criteria |
| --- | --- | --- | --- |
| `v2.0` | Done | Foreign capsule specification | Capsule AST (`Item::Capsule`), dual `.fgx`/`.fgai` syntax with verbatim code bodies, dependency pinning rules, effect/sandbox declarations, contract validation in `fgx check`, and the `forgentex.capsule_review.v1` report via `fgx capsule`. See `docs/foreign_capsule_spec.md`. |
| `v2.1` | Done | Python 3.13 capsule prototype | Deterministic mock fixtures (`mock IN -> OUT`) validated against the typed boundary (`capsule_fixture_arity`, `capsule_fixture_type`), plus the `fgx capsule-run` mock executor and `forgentex.capsule_run.v1` report with a single-input execution hook. See `docs/foreign_capsule_spec.md`. |
| `v2.2` | Done | Shared typed namespace across capsules | `share`/`bind` declarations in both syntaxes over one canonical AST, single-writer + unknown-binding + default-type validation in `fgx check`, the `forgentex.shared_namespace.v1` report via `fgx namespace`, and a Python-backend/JS-frontend web-counter demo. Snapshot-in/write-back-out execution deferred. See `docs/v2_shared_namespace.md`. |
| `v2.3` | Done | Capsule dependency and GitHub source locking | Structured dependency parsing (ecosystem/name/version/revision/checksum), the deterministic `forgentex.capsule_lock.v1` lockfile via `fgx capsule-lock`, and the `forgentex.capsule_audit.v1` supply-chain audit via `fgx capsule-audit` grading each dependency low/medium/high with `--strict` CI gating. See `docs/capsule_dependency_locking.md`. |
| `v2.4` | Done | Sandbox-backed capsule execution | Structured sandbox capability model (fs/net/proc/env/chain/gpu grants, `limit.*` resource limits, `.none` denials), effect-to-grant reconciliation (`sandbox_effect_uncovered`), the `forgentex.sandbox_plan.v1` report via `fgx sandbox`, and `fgx capsule-run --sandbox` gating mock execution on approval. OS-level isolation deferred. See `docs/capsule_sandbox_model.md`. |
| `v2.5` | Done | Web and managed runtime capsules | Managed runtime catalog (python/javascript/typescript/java/csharp/go ecosystems + lock-file evidence) via `fgx runtimes`, `capsule_ecosystem_mismatch` reconciliation in `fgx check`, and the one-roof `web_session` demo sharing a typed `Session` struct across a Python backend and a TypeScript frontend. See `docs/managed_runtime_capsules.md`. |
| `v2.6` | Done | Native and GPU capsules | C/C++ (`native`) and CUDA (`gpu`) capsule review: compiler/toolkit metadata, GPU-arch target parsing (`sm_NN`/`compute_NN`), ABI value-vs-memory-boundary classification, fallback-fixture tracking, the `gpu_capsule_no_target` check, and the `forgentex.native_review.v1` report via `fgx native-review`. See `docs/native_gpu_capsules.md`. |
| `v2.7` | Done | Shell capsule hardening | Bash/PowerShell capsules require an explicit `exec.allow` command allowlist (no wildcards) via `shell_capsule_no_allowlist`/`shell_command_wildcard` checks, with host-path/env/network review and the `forgentex.shell_review.v1` report via `fgx shell-review`. See `docs/shell_capsules.md`. |
| `v2.8` | Done | Smart-contract capsules | Solidity/Vyper review: compiler pins, EVM target metadata, required ABI hash (`contract_capsule_no_abi_hash`), gas evidence, chain effects, and a hard secret-key/wallet safety gate (`contract_capsule_secret_key`), with the `forgentex.contract_review.v1` report via `fgx contract-review`. See `docs/smart_contract_capsules.md`. |
| `v2.9` | Done | Agent workflow integration | Compact capsule inventory (`forgentex.capsule_inventory.v1`) and a per-capsule context packet (`forgentex.capsule_context.v1`) that merges contract, sandbox, dependency-audit, and class-specific (native/shell/contract) reviews into one decision plus a human-readable risk summary, via `fgx capsule-context`. See `docs/capsule_agent_workflow.md`. |
| `v2.10` | Done | Capsule compression and compact documentation register | Deterministic, lossless dictionary compression of the agent view (collision-free sigil, `dict=N` header, never-larger guard, round-trip conformance) via `fgx compress`, plus doc-compact specified as a writing register. Token payoff is judged by `fgx token-report`. See `docs/capsule_compression.md` and `docs/token_optimization_v2.md`. |
| `v2.11` | Done | Edit-task token benchmark | `fgx edit-bench` models a multi-edit task and measures whole-file resend vs map+symbol+patch (`forgentex.edit_bench.v1`). Confirms the structural token win: patch strategy wins from the first edit, ~2x on a 2-function file to ~8x on a 12-function file. See `docs/token_efficiency_findings.md`. |

## Native + Polyglot Phase (v2.12+)

The capsule track (v2.0–v2.11) built the *review and specification* layer:
a typed boundary, declared effects, a sandbox capability model, dependency
locking, and per-class risk reviews — but foreign code is still **mocked**,
not executed, and Forgentex itself has no native artifact. This phase makes
the polyglot real. The strategy: **stand on host toolchains** (emit C, drive
the host C compiler; that same C ABI is the FFI spine for C/C++/Rust/Zig;
CPython for Python; a JS runtime for the web), exactly as the managed-runtime
catalog already detects runtimes — so the Rust core stays dependency-free and
every real backend degrades gracefully when its host tool is absent. Each
milestone keeps the vertical-slice discipline (AST/representation as needed,
`validate`, a `forgentex.*.v1` report, a `fgx` command, an example, tests,
docs) and must never regress the v1.0 release check. Full design in
`docs/native_polyglot_plan.md`.

| Label | Status | Goal | Exit criteria |
| --- | --- | --- | --- |
| `v2.12` | Done | Native build core | `fgx build --native <pkg> [--emit obj\|shared]` lowers the existing C-transpiler output through the host C toolchain (`$CC`/`cc`/`clang`/`gcc`, detected and gated gracefully to `compiler_unavailable` when absent; a path-form `$CC` gets its directory added to the child `PATH` so sibling `as`/`ld` resolve) to a real object/shared library; deterministic `forgentex.native_build.v1` report (toolchain id/version, command, C-source hash, artifact hash/bytes, compiled-in effects, `@exact` honored). Verified end-to-end with GCC 15.2.0 producing a linkable object with visible `export`ed symbols. No new Rust dependencies. See `docs/native_build.md`. |
| `v2.13` | Done | C-ABI FFI bridge | `fgx capsule-run --native` derives a real `extern "C"` signature from the typed boundary, compiles a C capsule body to `capsule.o`, compiles a generated harness against the boundary header, links them, and runs each fixture as a real execution oracle — marshaling `CapsuleValue` ↔ C-ABI scalar types. Linking/execution is gated on `check` (effects covered by grants) + pinned deps. `forgentex.ffi_plan.v1`. Verified with GCC 15.2.0: `clamp_c` executes 3/3 fixtures (40000→32767, −40000→−32768, 100→100). First slice: C + scalar boundary; C++/Rust/Zig (same C ABI) and slices/structs follow. See `docs/ffi_bridge.md`. |
| `v2.14` | Done | `core.io` — real, effect-gated I/O | `core_io_read_file`/`write_file`/`copy_file` perform real disk I/O via `fgx io-run`, each gated at runtime by a granted capability (`fs.read`/`fs.write`); a denied op is recorded and **fails closed** (nothing written), and every op (allow/deny/error) is captured in a deterministic transcript with an FNV hash. `forgentex.io_run.v1`. The `real_copy` example copies for real under an explicit grant and is denied without it; `core_io_*` are unavailable outside `io-run`. See `docs/core_io.md`. |
| `v2.15` | Done | `core.net` — real, effect-gated networking | `core_socket_tcp_request`/`tcp_send` open real TCP sockets via `fgx net-run`, gated by a host:port allowlist — an unlisted endpoint is denied and the socket is never opened (**fail closed**). Every op (allow/deny/error) is captured in a deterministic transcript (op/endpoint/bytes-out/in) with an FNV hash that excludes timing. `forgentex.net_run.v1`. Verified with a real loopback round-trip (`ping-forgentex` echoed, 14 bytes out/in); tests use an in-process echo server so CI needs no network. Real sibling of the mock `core.net`; UDP/HTTP follow. See `docs/core_net.md`. |
| `v2.16` | Done (hashing/MAC) | `core.crypto` — real cryptography | `core.hash` provides real SHA-256 (FIPS 180-4), HMAC-SHA256 (RFC 2104), and constant-time compare — implemented to spec, **zero-dependency**, and **verified against official NIST/RFC 4231 known-answer vectors** (`fgx crypto-selftest`, `forgentex.crypto_selftest.v1`); `SHA-256("abc")` matches CPython's hashlib byte-for-byte. The symmetric trust backbone (content-addressing, shared-key auth, safe tag verification). Asymmetric Ed25519 sign/verify, AEAD, KDF, and secure key generation are deliberately deferred to an audited host crypto library over the FFI bridge (never hand-rolled). See `docs/core_crypto.md`. |
| `v2.17` | Done | Python bridge (the special relationship) | `fgx capsule-run --python` runs a Python capsule for real on the host CPython (`$PYTHON`/`python3`/`python`, gated gracefully to `runtime_unavailable` when absent): the body is dedented, the first top-level `def` is the entry, inputs are marshaled to JSON, and the output is checked against the capsule's `mock` fixtures (now real execution oracles). Gated by the sandbox decision; the report carries the typed boundary, effects, deps, and — the token point — `body_tokens` vs `boundary_tokens` so the agent pays for the signature, not the body. `forgentex.python_run.v1`. Verified on Python 3.8.5 (celsius capsule, 3/3). Replaces the v2.1 mock. See `docs/python_bridge.md`. |
| `v2.18` | Done | Unified polyglot execution ABI | `fgx polyglot-run` dispatches every capsule through one calling convention and one gate: C/C++/Rust/Zig via the FFI bridge (v2.13), Python via the CPython bridge (v2.17), others marked `not_wired` for later. The shared typed namespace is surfaced as the cross-language data plane — a value declared once with `share` flows to capsules in different languages by name (`gain : i32 -> scale[c:read], label[python:read]`). `forgentex.polyglot_abi.v1`. Verified end-to-end: one run executed a C capsule via gcc (2/2) and a Python capsule via host CPython (1/1) under one schema. Honest scope: live cross-runtime shared mutation stays orchestration-level (v2.2 deferral). See `docs/polyglot_abi.md`. |
| `v2.19` | Done | Agent trust fabric | `fgx trust-session` runs the end-to-end agent-to-agent loop on real primitives: an HMAC-SHA256 capability token (with expiry), a signed message envelope transmitted over the real loopback TCP transport (v2.15), constant-time signature verification, nonce+timestamp freshness, a content-addressed (SHA-256) audit receipt, and explicit **replay** and **tamper** rejection — 10 steps, all verified. `forgentex.trust_session.v1`, deterministic audit hashes; the secret key is never logged (only a `key_id`). Honest scope: shared-key (symmetric) trust; public-key Ed25519 identity arrives with the audited host crypto library over FFI. Replaces the mock agent-economy demos. See `docs/agent_trust_fabric.md`. |
| `v2.20` | Done | JS/TS runtime + web one-roof | `fgx capsule-run --node` runs JavaScript (and best-effort TypeScript via `node --experimental-strip-types`) capsules for real on host Node, fixture-verified behind the typed boundary, mirroring the Python bridge; `forgentex.web_run.v1` with `body_tokens` vs `boundary_tokens`. JS/TS route through the unified ABI (`polyglot-run`) via the `node` backend. Verified end-to-end on Node v24.17.0: the `one_roof` example runs a Python backend (2/2) + a JS frontend (2/2) = 4/4 under one schema, sharing `total : i64 -> accumulate[python:rw], render[javascript:read]` by name — backend and frontend under one Forgentex roof. Missing node degrades to `runtime_unavailable`. See `docs/web_one_roof.md`. |
| `v2.21` | Done | TypeScript / Node runtime bridge | TypeScript capsules with **real type annotations** run for real on host Node via `node --experimental-strip-types`, fixture-verified behind the typed boundary, general-purpose (not just web). Verified on Node v24.17.0: the `classify` tool capsule runs 3/3 (95→high, 50→mid, 10→low). npm deps flow through the v2.3 supply-chain audit. Shares the `node` backend and `forgentex.web_run.v1` report with v2.20; routes through `polyglot-run`. See `docs/web_one_roof.md`. |
| `v2.22` | Done | JVM / Java runtime bridge | `fgx capsule-run --java` runs Java capsules for real on the host JDK: Forgentex parses the class + first `public static` method, generates a typed driver, compiles both with `javac`, and runs each fixture with `java` (arg/`println` marshaling, no JSON dep). Gated by sandbox; Maven/Gradle deps flow through the v2.3 audit. `forgentex.jvm_run.v1` with `body_tokens` vs `boundary_tokens`; routes through `polyglot-run` via the `jvm` backend. Verified on OpenJDK 21.0.11 (tax capsule 3/3: 10000→10825, 0→0, 50000→54125). Missing JDK degrades to `runtime_unavailable`. See `docs/jvm_bridge.md`. |
| `v2.23` | Done | .NET / C# runtime bridge | `fgx capsule-run --csharp` runs C# capsules for real on the host .NET SDK: Forgentex scaffolds a console project (driver + class), detects the target framework from the installed SDK, builds with `dotnet build`, and runs the assembly per fixture (arg/`Console.WriteLine` marshaling, no JSON dep). Gated by sandbox; NuGet deps flow through the v2.3 audit. `forgentex.dotnet_run.v1` with `body_tokens` vs `boundary_tokens`; routes through `polyglot-run` via the `dotnet` backend. Verified on .NET Core SDK 2.1.801 (discount capsule 3/3: 10000→9000, 0→0, 25000→22500). Missing .NET degrades to `runtime_unavailable`. See `docs/dotnet_bridge.md`. |
| `v2.24` | Done | Go runtime bridge | `fgx capsule-run --go` runs Go capsules for real on the host Go toolchain: Forgentex assembles a `package main` program (body + a generated `main` that parses args, calls the first `func`, and prints the result, emitting only the imports it uses), and runs it per fixture with `go run`. Gated by sandbox; `go.mod`/`go.sum` deps flow through the v2.3 audit; Go can also expose a C-ABI surface (`-buildmode=c-shared`) for the v2.13 FFI path. `forgentex.go_run.v1` with `body_tokens` vs `boundary_tokens`; routes through `polyglot-run` via the `go` backend. Verified on go1.26.4 (scale capsule 3/3: 21→42, 0→0, -5→-10). Missing Go degrades to `runtime_unavailable`. See `docs/go_bridge.md`. |

The managed-runtime bridges (v2.21–v2.24) generalize the Python bridge
(v2.17) into a uniform pattern for every host-runtime language — web/tooling
(TypeScript/Node), business (Java, C#), and cloud/network services (Go).
Each is "basic support": typed boundary + host-runtime execution +
effects/sandbox gating + deterministic capture + ecosystem dependency
pinning. They all plug into the unified polyglot ABI (v2.18) and share the
typed namespace, so a value declared once flows across all of them by name.
See `docs/native_polyglot_plan.md`.

## Review Requirements

Capsule reports should make these facts visible:

- which language and runtime are introduced or changed
- whether the runtime version is exact, for example Python 3.13
- which packages, GitHub repositories, commits, and hashes are trusted
- which effects and permissions are requested
- which typed Forgentex functions call into the capsule
- which tests prove the boundary behavior
- whether the capsule can run on each configured target environment
- whether native backends reject, stub, or bridge the capsule
- whether the capsule belongs to a high-risk class such as shell, native,
  GPU, or smart-contract code

Semantic patch review must treat foreign code changes as high-signal review
events. A one-line change inside a foreign block should still report dependency,
effect, sandbox, target, and typed-boundary consequences.

## Non-Goals For v2 Start

- treating embedded foreign code as safe Forgentex code
- allowing unpinned package installs or floating GitHub branches
- hiding package downloads or subprocess execution behind ordinary function calls
- promising native C/Zig/LLVM backend lowering for foreign capsules
- supporting arbitrary shell snippets as the first capsule language
- letting Bash or PowerShell capsules run without command allowlists and strict
  host/environment limits
- letting C, C++, CUDA, Solidity, or Vyper capsules bypass unsafe/native,
  target, gas, crypto, or ABI review
- allowing capsules to bypass the effect, package, audit, or review systems

## First Demo Ideas

- Python 3.13 data-cleaning capsule using a pinned dependency and typed
  Forgentex inputs/outputs.
- Python 3.13 statistics capsule compared with planned `core.stats` for cases
  where an existing library is a better starting point.
- GitHub-sourced parser or simulation project imported at a pinned commit, then
  wrapped by a small Forgentex typed boundary.
- TypeScript SDK capsule that wraps a pinned generated API client.
- Java or C# library capsule that exposes one typed service boundary.
- C or C++ numeric/library wrapper with an explicit ABI and memory boundary.
- CUDA kernel capsule with deterministic CPU fallback fixtures.
- Bash and PowerShell automation capsules that demonstrate command allowlists
  and sandbox rejection paths.
- Solidity and Vyper contract capsules with pinned compiler versions, ABI
  hashes, local EVM test fixtures, and no secret-key access.
- WASM plugin capsule for deterministic sandboxed computation.
- SQL query capsule with typed rows, explicit database effects, and fixed schema
  hashes.

## Audio Application Track

A standing application track that exercises the capsule system toward a
real goal: complex audio products (a Sound Forge–class editor). The
strategy is polyglot — reuse existing native libraries (miniaudio first)
through capsules for OS audio, the realtime thread, and codecs, while
building the pure signal-processing core natively in Forgentex. The seam
is miniaudio's realtime callback: miniaudio owns the device and the
thread; Forgentex owns the DSP that runs inside the callback.

The native DSP core is buildable and runnable today (interpreter + C
source backend). The miniaudio capsule is a reviewable contract now and
advances v2.6 native capsules; live playback is blocked on the native
backend, C FFI, and real capsule execution.

| Phase | Status | Scope |
| --- | --- | --- |
| `audio.1` | Done | Native DSP core v0 (`examples/audio`): conversion, interleave, gain, mix, peak/energy, plus exact Q15 fixed-point gain/mix — interpreter-tested, inline-tested, C-emitted. |
| `audio.2` | Planned | `audio.*` effect category + `cpu.realtime` capability + realtime-safety checking (no alloc/io on the callback path). |
| `audio.3` | Planned | Filters and buffering: biquad EQ, one-pole smoother, fixed-capacity ring buffer. |
| `audio.4` | Planned | miniaudio C capsule contract: typed device-config shared schema, `audio.output` effect, device sandbox grant, miniaudio pinned by commit+sha (review-only, native risk class). |
| `audio.5` | Planned | Format/channel breadth (i24/u8/f64, channel mapping) and resampling (linear then sinc) matching miniaudio conventions. |
| `audio.6` | Blocked | Live playback: miniaudio's callback invokes the C emitted from the native DSP core. Depends on native backend + C FFI + real capsule execution. |

Full architecture, the realtime-safety-as-effects design, the
`core.audio` surface, the capsule contract shape, and per-phase acceptance
gates are in [`docs/audio_dsp_track.md`](audio_dsp_track.md). Build the
native DSP core first: it is the part truthfully in scope today, it is what
a future miniaudio callback will call, and it lets the effect system prove
realtime-safety before any OS code is involved.

## Relationship To v1

v1.0 should remain focused on stable language safety. v2 foreign capsules depend
on v1-grade ownership, effects, package locking, audit reports, target
environment modeling, graph artifacts, and agent review packets. Without those
foundations, embedded foreign-language code would be powerful but too opaque.
