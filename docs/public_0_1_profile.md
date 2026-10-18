# Forgentex Public 0.1 Profile

**Status:** candidate public contract; not release-ready yet.

This profile separates the forthcoming public Forgentex 0.1 release from the
repository's historical internal `v0.x` and `v1.0` development labels. Its
readiness is reported by:

```powershell
fgx release-check --target 0.1 --json
fgx release-check --target 0.1 --strict
```

The non-strict command always emits the evidence report. Strict mode exits
successfully only after every public criterion is complete.

## Release shape

Forgentex 0.1 is an **interpreter-first, agent-native language preview**. Its
public promise is a coherent and reproducible subset, not feature parity among
all experimental backends and capsule runtimes.

The candidate supported core contains:

- human `.fgx` and compact agent `.fgai` source views of one canonical AST
- typed functions, local bindings, structs, enums, pattern matching, ordinary
  control flow, arrays, slices, and the documented interpreter collection APIs
- explicit effects, capability checks, ownership/move checks, and the bounded
  borrow/lifetime rules documented for the release
- formatter, parser, checker, interpreter, project diagnostics, inline tests,
  public API summaries, and machine-readable agent workbench reports
- a small designated standard foundation and end-to-end example suite

Features may be included only when their syntax, checker behavior, execution
path, diagnostics, and limits agree in the public fixture matrix.

## Experimental surfaces

The following may ship beside 0.1 but are not automatically part of its stable
language contract:

- C and Zig features outside the selected primary native subset
- live polyglot capsules whose host runtime or compiler is optional
- native owned `Vec`, `Map`, and String operations beyond the frozen profile
- capturing closures, generic function values, dynamic trait dispatch, JIT,
  LLVM, DWARF, hot reload, and the REPL
- real asynchronous scheduling, parallel pipelines, GPU rendering, production
  codecs, and domain-specific media APIs
- latent/codebook token encodings

Unsupported native constructs must continue to produce explicit blockers; they
must never silently lower with weaker semantics.

## Known limits

What a reader of a 0.1 release should know before relying on it. Each of these
is measured or demonstrated, and each is stated here rather than left for
someone to discover.

**One verified host.** CI builds, checksums, and clean-installs
`x86_64-pc-windows-msvc` only. Linux and macOS are expected to build from the
same dependency-free source but no job has proven it, so 0.1 promises nothing
about them.

**Containment is strongest on Windows, and weaker for managed runtimes.**
A native capsule on Windows runs in a zero-capability AppContainer with network
and IPC denial demonstrated by adversarial tests. Managed capsules (Python,
Node, TypeScript, Go, JVM, .NET) get process containment — deadlines, CPU,
memory, output, and process-tree cleanup — but **not** filesystem or network
denial: a Python capsule declaring `fs.none` can still read an undeclared file,
which a test demonstrates rather than merely admits. On Linux and macOS, native
filesystem and network denial are also not enforced. The per-runtime, per-platform
truth is tabulated in [enforcement_matrix.md](enforcement_matrix.md), which
separates what is *declared* from what is *checked*, *enforced*, and *observed*.

**A limit that cannot be enforced is refused, not ignored.** `limit.fs_bytes`,
`limit.net_bytes`, and `mem_mb` on Unix are rejected before a process starts
unless the operator passes `--trusted`, which records the waived declarations in
a receipt. A capsule cannot grant itself trust.

**The token-efficiency claim is smaller than it looks.** Measured with a real
BPE tokenizer, the dense agent surface is about **1% larger** than the readable
one across the sample — not the 3–14% smaller that character-count heuristics
suggested. `token-report` now labels every heuristic ratio and takes
`--bpe-command` for real counts. See
[token_efficiency_findings.md](token_efficiency_findings.md).

**Semantic patching beats whole-file resend, not a good text patch.** Against a
read-once text-patch baseline, `edit-bench` measures 0.94–1.83×. The larger
published multiples were against resending the whole file.

**Fixed: the Windows native output-loss defect.** A native capsule call
could fail with `missing result envelope (runner exit 0, no output at all)`.
The runner's reply sat in the C runtime's stdout buffer until the exit sequence
wrote it, and with MinGW-Builds' UCRT runtime inside the containment that write
sometimes never reached the pipe — rarely with a w64devkit compiler, but about
ten times per run on GitHub's Windows runners, which use MinGW-Builds. Runners
now flush while `main` is still running: in a concurrent reproducer that went
from 3 failures in 10 to 0, and in the serial native suites from 8 and 5 to 0
and 0. Why the exit-time write is lost is not established, so this is recorded
as fixed by measurement rather than by a proven mechanism.

**Polyglot capsules need their host runtimes.** A capsule's language toolchain
(CPython, Node, Go, a JDK, .NET, Zig, a C compiler) is optional and resolved on
the host; `fgx doctor` reports what is available. An absent toolchain skips a
demonstration rather than failing a build.

**Interpreter-first.** The interpreter is the reference semantics for 0.1. The C
backend covers a bounded subset and refuses what it cannot lower with the same
meaning; the Zig backend is for safe experiments.

## Standard foundation

The public 0.1 release freezes a small, deterministic, allocation-free subset of
the standard library as its guaranteed foundation. These modules run identically
in the interpreter and lower cleanly into the portable C11 subset, so they carry
a patch-line stability promise:

| Module | Purpose |
| --- | --- |
| `core.mem` | allocation-free memory/alignment predicates |
| `core.slice` | slice length, bounds, and precondition helpers |
| `core.math` | checked arithmetic and DSP-friendly numeric helpers |
| `core.bits` | bit extraction, popcount, leading/trailing-zero, and rotation |
| `core.option` | allocation-free `Option` constructors and predicates |
| `core.result` | allocation-free `Result` constructors and predicates |

The full standard library (`fgx stdlib --json`) also ships host-effect,
feature-probe, SIMD, crypto, agent-economy, and media modules. Those remain
available but are **not** part of the frozen minimal foundation and carry no 0.1
patch-line promise until explicitly promoted here. The frozen inventory is
encoded in `PUBLIC_STANDARD_FOUNDATION` (`src/public_compat.rs`); the
`public_0_1_standard_foundation` gate fails if any frozen module or function is
removed or renamed.

## Compatibility promise

The exact diagnostic and JSON inventory is frozen in
[`public_0_1_compatibility.md`](public_0_1_compatibility.md). The public rules
are:

1. Every accepted public example continues to parse, format, type-check, and
   execute throughout the 0.1 patch line.
2. `.fgx` and `.fgai` round-trip to the same canonical AST.
3. Public diagnostic codes and required structured fields remain stable within
   the 0.1 patch line; wording may improve without changing their meaning.
4. Public JSON schemas are versioned. Removing a field or changing its type
   requires a new schema identifier.
5. Newly unsupported behavior receives a diagnostic and migration note before
   removal from a later minor release.
6. Experimental features are clearly labeled and carry no patch-line
   compatibility promise unless promoted into this profile.

## Machine-checked criteria

`fgx release-check --target 0.1` adds these gates to the already-green internal
compiler and safety gates:

| Gate | Current status | Exit condition |
| --- | --- | --- |
| `public_0_1_profile` | Pass | This bounded profile is present and cited by release tooling |
| `public_0_1_execution_contract` | Pass | The checked matrix in `docs/public_0_1_execution_matrix.md` agrees across format, check, interpreter, and the bounded C11 subset |
| `public_0_1_compatibility` | Pass | Diagnostic and JSON-schema guarantees are frozen and tested |
| `public_0_1_standard_foundation` | Pass | The frozen public core-module/API inventory is designated and drift-checked |
| `public_0_1_distribution` | Partial | A frozen manifest, install path, prerequisites, and a build/checksum/clean-install CI job exist; the final proof is a green `release-artifacts` run on the verified Windows host, recorded in `docs/public_0_1_release_proof.md` |
| `public_0_1_examples_evidence` | Pass | A designated example suite and one reproducible evidence report cover every public claim |

The decision remains `public_0_1_not_ready` while any criterion is partial or
planned. Distribution is the single remaining blocker, and its machinery is now
in place: `src/distribution.rs` freezes the artifact/host/backend/prerequisite
manifest (`fgx dist-manifest --json`, schema `forgentex.distribution.v1`), the
README documents the install path and prerequisites, and the `release-artifacts`
CI job builds release binaries, computes SHA-256 checksums, and clean-install
verifies them.

The one residual is a real release run, and it is now *recorded* rather than
merely deferred: `docs/public_0_1_release_proof.md` holds the run link, the
commit it built, the host, and the published digests, and `distribution_gaps()`
parses that file and names whichever field is still `pending`. So the gate
closes on evidence a reader can open, not on an assertion that the work was
done.

0.1 verifies one host, `x86_64-pc-windows-msvc`. Linux and macOS stay in the
manifest as `source-build-unverified`, because the source is portable but no CI
job has built it there; declaring them supported would be exactly the promise
this gate exists to prevent. Verifying them is the first post-0.1 distribution
task. Every other public criterion is machine-checked and green. This is intentional: internal maturity gates are
evidence for the public release, not a substitute for its contract.

The reproducible evidence report is published by:

```powershell
fgx release-evidence --json
fgx release-evidence --strict
```
