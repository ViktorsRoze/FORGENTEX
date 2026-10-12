# Capsule enforcement matrix

What actually stands between a foreign capsule and the machine, per runtime and
per platform. A capsule's declaration is a promise *by the capsule*. This page
says which parts of that promise Forgentex **checks** before running it, which
it **enforces** while it runs, and which it only **records**.

Every cell is backed by code or by a named test, not by intent. If a runner
changes, this page changes in the same commit.

## Four different things, not one

| Level | Question it answers | Where it lives |
| --- | --- | --- |
| **Declared** | What does the capsule say it does? | `effects [...]`, `sandbox "..."`, `limit.*` in source |
| **Checker-approved** | Is the declaration internally consistent and covered by its sandbox? | `sandbox::is_approved`, `fgx check` (`sandbox_effect_uncovered`), native preflight |
| **Enforced** | Would the OS stop the capsule if it did something undeclared? | AppContainer, restricted token, Job Object, rlimits, process groups |
| **Observed** | What evidence of what it actually did is recorded? | `forgentex.native_receipt.v1` for native calls, `forgentex.managed_receipt.v1` for managed ones |

Checker approval is not enforcement. A Python body declared `fs.none` passes the
checker and can still open any file its user account can — that is demonstrated,
not inferred:
`tests::python_run::a_managed_python_capsule_is_checked_but_not_confined_on_the_filesystem`
runs exactly that capsule and requires it to succeed. The same test requires the
capsule *not* to see an ungranted caller variable, so the two halves of that row
cannot drift apart.

## The matrix

✅ enforced · ⚠️ partial, with the caveat stated · ❌ not enforced

Windows cells were checked against tests run on Windows 11 build 26300, and
before that on 26200. The host moved between builds mid-development, so the
evidence was re-recorded rather than carried over: `scripts/windows-containment-check.ps1`
passes 14/14 on 26300 (`forgentex.windows_containment_evidence.v1`, commit
1618071). A containment claim that was only ever checked on a since-upgraded OS
is a claim about a machine nobody has any more. Linux
and macOS cells come from the implementation and from the platform verification
checklist; they have not been re-run on those platforms for this page, and the
checklist is how to do that.

### Native capsules (C, C++, Rust, Zig), live

| Property | Windows | Linux | macOS |
| --- | --- | --- | --- |
| Declared effects checked | ✅ | ✅ | ✅ |
| Undeclarable capabilities refused before compile (fs, net, subprocess, env) | ✅ preflight | ✅ preflight | ✅ preflight |
| Filesystem denial | ⚠️ zero-capability AppContainer and a protected scratch DACL: reading a workspace file is denied (`windows_appcontainer_runner_cannot_read_a_workspace_file`). World-readable system locations granted to all application packages remain readable | ❌ private `0700` scratch and a scrubbed environment only; no mount namespace | ❌ as Linux |
| Network denial | ✅ loopback and host network both denied (`windows_zero_capability_appcontainer_cannot_connect_to_loopback`, `…_to_host_network`) | ❌ no network namespace | ❌ |
| IPC denial (named pipes, events) | ✅ (`windows_appcontainer_cannot_open_an_unrelated_named_pipe`, `windows_appcontainer_cannot_open_or_signal_host_named_event`) | ❌ | ❌ |
| Wall-clock deadline | ✅ 30 s default | ✅ | ✅ |
| CPU budget | ✅ aggregate, Job Object | ⚠️ `RLIMIT_CPU`, per process, whole seconds | ⚠️ as Linux |
| Memory budget | ✅ aggregate committed memory, Job Object | ⚠️ `RLIMIT_AS`, per process, no tree total | ⚠️ as Linux, with host `RLIMIT_AS` caveats |
| Process count | ✅ one active process | ❌ | ❌ |
| Output size bound | ✅ combined stdout+stderr | ✅ | ✅ |
| Process-tree cleanup | ✅ Job kill-on-close | ✅ process group | ✅ process group |
| Environment | ✅ minimal, rebuilt | ✅ ambient home/profile cleared | ✅ as Linux |
| Filesystem byte limit (`limit.fs_bytes`) | ✅ aggregate Job I/O | refused at preflight | refused at preflight |
| Network byte limit (`limit.net_bytes`) | refused at preflight | refused at preflight | refused at preflight |
| Observed evidence | ✅ peak tree memory, peak processes, Job I/O, token and integrity details | ⚠️ no aggregate tree accounting | ⚠️ as Linux |

Linux and macOS gaps, and the work that closes them, are itemised in
[native_containment_platform_verification.md](native_containment_platform_verification.md).

### Managed capsules (Python, Node, TypeScript, Go, JVM, .NET), cold path

Managed runtimes run inside the same process containment as native runners
(`src/managed_exec.rs`), without the AppContainer. Every property below is
exercised by a probe capsule written to break it
(`tests::managed_limits::probe_*`); with enforcement removed, all five limit
probes fail, which was checked rather than assumed.

| Property | Windows | Linux / macOS |
| --- | --- | --- |
| Declared effects checked | ✅ `sandbox::is_approved` before launch | ✅ |
| Filesystem denial | ❌ ordinary user authority — **demonstrated** for Python | ❌ |
| Network denial | ❌ | ❌ |
| Wall-clock deadline | ✅ 60 s default, `limit.wall_ms` when declared | ✅ |
| CPU budget | ✅ aggregate, Job Object; 60 s default | ⚠️ `RLIMIT_CPU` per process |
| Memory ceiling (`limit.mem_mb`) | ✅ aggregate committed memory, Job Object; no default | refused — `RLIMIT_AS` caps address space, which V8, Go, and the JVM over-reserve, so it would break the runtime rather than bound it |
| Output size bound | ✅ | ✅ |
| Process count | ✅ 64 per call, Job Object (`go run` compiles in the same tree) | ❌ |
| Process-tree cleanup | ✅ Job kill-on-close | ✅ process group |
| Environment | ✅ explicit allowlist: what the runtime needs to start, plus the caller variables the capsule declared with `env.read` — **demonstrated** for Python, Node, Go, and the JVM | ✅ |
| `limit.fs_bytes`, `limit.net_bytes` | refused | refused |
| Per-call deadline on a warm worker | ✅ 60 s, matching the managed default | ✅ |
| Guest output separated from the result | ✅ tagged result/diagnostic lines, and the body's print facility rebound — **demonstrated** for Python and Node | ✅ |
| Guest output reported | ✅ warm calls carry it in `forgentex.managed_receipt.v1`; a cold call reports it when the capsule fails | ⚠️ cold success path does not carry it back yet |
| Observed evidence | ✅ `forgentex.managed_receipt.v1` per call: profile, every enforced limit and its source, every waived declaration | ✅ |

**Refused, unless trusted.** A capsule declaring a limit its runtime cannot
enforce is refused before any process starts, with the reason and the way out
in the message. The operator can pass `--trusted` (`fgx run --live --trusted`)
to run it anyway: every enforceable limit still applies, the unenforceable
declarations are waived, and the call's receipt records `"profile": "trusted"`
and the waived list. Nothing a capsule declares can grant this — trust comes
from whoever runs the program.

### Warm workers (Python, Node, TypeScript, Go)

A warm worker is one long-lived process shared by every call to a capsule, so it
cannot apply per-call limits. A capsule that declares any `limit.*` therefore
does not use a warm worker, even when warm workers are enabled (they are the
default for live runs); it runs on the contained cold path above, and its
receipt says `"warm": false`. Capsules that declare no limits still use warm
workers, which have no per-call deadline: a worker that stops responding blocks
the caller. A worker's environment is the same explicit allowlist a cold call
gets, fixed at spawn — otherwise a process that outlives many calls would hold
the caller's secrets for the whole run.

### Foreign debugging (debugpy attach)

Admission deliberately relaxes three properties and records each one under
`forgentex.foreign_debug.v1`: a loopback listener, suspended deadlines, and
non-reproducibility. See [debugger_plan.md](debugger_plan.md).

## Open problems, most important first

1. ~~Declared limits on managed capsules were silently ignored.~~ **Closed
   2026-09-18**: enforced where the runtime can enforce them, refused where it
   cannot, and waived only under an explicit `--trusted`, with the waiver
   recorded.
2. ~~An explicit environment for managed runtimes.~~ **Closed 2026-09-20**: a
   managed child is given an explicit environment — what its runtime needs to
   start, plus the caller variables the capsule declared — instead of inheriting
   the caller's. `PYTHONPATH`, `PYTHONHOME`, and `NODE_OPTIONS` are deliberately
   never inherited, because they redirect what the runtime loads.
3. **Filesystem and network denial** for managed runtimes everywhere, and for
   native runners on Linux and macOS.
4. **Warm workers without a per-call deadline.** Only capsules that declare no
   limits use them, but those can still block their caller.
5. **Diagnostic output on its own channel.** Managed runners read the response
   from stdout, so a capsule that prints can corrupt its own reply.

## The managed boundary protocol

Until 2026-09-28 a managed capsule's *entire* standard output was its result, so
one `print()` in the body — a deprecation warning, a progress bar, a debug line a
library ships with — turned a working capsule into `worker returned invalid
JSON`. A probe capsule containing a single print reproduced it on both the warm
and cold paths.

The boundary now separates the two channels twice over:

- Each line the host reads is tagged with a per-call nonce and a kind:
  `<nonce> R <json>` is the result, `<nonce> D <text>` is one line the capsule
  printed. An untagged line is diagnostics, so output the language never routed
  — a C extension, a subprocess — still cannot be mistaken for an answer.
- Where a language allows its print facility to be replaced (Python, Node), it is
  rebound to emit tagged diagnostic lines on the same stream as the result, which
  keeps a print *ordered* against the answer it accompanies. That matters for a
  warm worker, where a print on a second pipe would race the result. Go, the JVM,
  and .NET redirect the body's output to the error stream instead.

Two properties came with it:

- **Arguments travel on stdin, not argv.** On argv a payload above roughly 32 KB
  failed on Windows with an opaque `The filename or extension is too long`. Since
  a capsule that declares any `limit.*` is not eligible for a warm worker,
  declaring a limit silently capped its input — containment cost capacity. A
  100 KB payload with a declared limit now succeeds, and 1 MB crosses on both
  paths.
- **A warm worker keeps one pipe, not two.** The first version of this protocol
  piped a worker's error stream and drained it on a thread, so a warm capsule's
  printing could be reported whatever language it was written in. Measured, that
  cost too much: with the extra pipe and thread per worker, the native runners'
  output-capture flake went from **0 failures in 3 serial suite runs to 2 in 3**,
  and removing it returned the suite to 3 in 3 clean. Python and Node capsules
  send their printing down the result pipe as tagged `D` lines, so they lose
  nothing. A warm **Go** capsule redirects its body's output to the error stream,
  which is discarded, so its printing is not reported — the cold path reports it.
  That the flake responds to how many pipes the host holds is also evidence about
  its mechanism, which the original investigation did not have.
- **A warm call has a deadline** (60 s, the managed default). A warm worker
  applies no per-call limits, which is why a capsule declaring any runs cold, but
  "no limits" must not mean "may hang the run forever": a worker that stops
  answering now fails its call, reporting whatever it printed, instead of
  blocking the caller. Untagged output is also bounded per call.

Not yet done: a cold call reports the capsule's output when it fails, but does
not carry it back on success — the runners return the result alone. The JVM and
.NET paths still pass typed scalar arguments on argv, so a very long string
argument to those two remains bounded by the command line.
