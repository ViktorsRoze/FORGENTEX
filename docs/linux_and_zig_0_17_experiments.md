# Linux bring-up and Zig 0.17 upgrade — experiment log

This document records every experiment run while (1) evaluating what Forgentex
needs to build and pass its tests on Ubuntu Linux, (2) upgrading the native
Zig target from 0.14 to 0.17, (3) running several Zig series side by side
(§ 7), and (4) bringing up live CUDA capsules on an RTX 5090 (§ 8). It is a
reproducible record: each finding names the file, command, and observed output
behind it. § 9 covers adding Julia and MoonBit capsules. §§ 7–9 ran on 2026-10-16 on the same machine after its upgrade to
Ubuntu 26.04.1 (kernel 7.0, glibc 2.43, gcc 15.2).

- **Host:** Ubuntu 24.04.5 LTS, kernel 6.17, x86-64, 32 cores, 125 GiB RAM.
- **Date:** 2026-10-13.
- **Method:** all building and testing was done on a throwaway copy of the
  repository so the working tree was never the experiment subject. Toolchains
  were installed into a session-local directory except where noted.

## Headline results

| Question | Answer |
|---|---|
| Does the repo build on Linux as shipped? | **No** — one Windows-only constant is referenced from a runtime branch that is still compiled on Linux. |
| After the compile fix, do the tests pass? | **Almost** — 1,280 / 1,286 with stock tools; the rest are a toolchain mismatch, two test bugs, and a pre-existing parallel race. |
| Is the native capsule sandbox effective on Linux? | **No** — a capsule declared `fs.none` could read and write host files, reach the network, fork, and read the parent's environment. |
| Does Zig 0.17 work after the upgrade? | **Yes** — full suite 1,284 pass / 0 fail / 2 ignored, `--test-threads=1`, and 0.17 fixes a 0.14 miscompile. |
| Can Zig 0.14–0.17 run side by side? | **Yes** — three runner profiles cover the four series. 0.14–0.16 share the by-value struct miscompile, which is refused on those compilers (§ 7). |
| Do Julia and MoonBit capsules run? | **Yes** — Julia 1.10, 1.12, and 1.13 side by side; MoonBit as WebAssembly in a deny-all `moonrun` sandbox, with its imports checked (§ 9). |
| Do CUDA capsules run live on the RTX 5090? | **Yes**, with a data-segment memory limit instead of an address-space one, and with toolkit selection that skips a toolkit unable to build on the host (§ 8). |

## 0. Toolchain inventory

Present at start: `gcc`/`g++` 11.5, `python3` 3.12, `node` 18.19, `go` 1.22,
`java` 21, `git`, `bwrap`, `unshare`. Missing: `rustc`/`cargo`, `zig`, `clang`,
`dotnet`, `pwsh`.

Installed for the experiments: rustup stable **1.99.0** (rustfmt + clippy); Zig
**0.14.1** (official tarball, checksum-verified); Node **22.23.3** (official
tarball, checksum-verified, because Node 18 is too old for the TypeScript path);
later Zig **0.17.0** (installed by the maintainer via `snap --classic --beta`).

Ubuntu's `apt` offers `rustc` 1.75 and `nodejs` 18.19 and packages no Zig — all
below what the project needs (rustc ≥ 1.82 for `unsafe extern`, Node ≥ 22.6 for
`--experimental-strip-types`, Zig pinned exactly). A working Linux setup must use
rustup, a Node tarball/nvm, and a Zig tarball, not apt.

## 1. The build does not compile on Linux

`cargo clippy -- -D warnings` failed to compile the library:

```
error[E0425]: cannot find value `NATIVE_APPCONTAINER_NAME` in module `crate::native_build`
    --> src/ffi.rs:2201
note: found an item that was configured out --> src/native_build.rs:1194  (#[cfg(windows)])
```

`NATIVE_APPCONTAINER_NAME` is `#[cfg(windows)]`, but it is read from a
`cfg!(windows).then(...)` expression — a *runtime* branch that is still compiled
on every platform. `WINDOWS_JOB_UI_RESTRICTIONS` has the same problem in the test
build (`src/tests/ffi.rs`), and `classify_windows_version` (`src/doctor.rs`) is
dead code on Linux, which `-D warnings` rejects.

Cause: CI (`.github/workflows/ci.yml`) runs only on `windows-latest`, so no
`cfg(unix)` code is ever compiled or linted.

**Fix:** gate the three items with `#[cfg_attr(not(windows), allow(dead_code))]`
instead of `#[cfg(windows)]`, so they compile everywhere but warn nowhere. With
that change `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, and
`cargo build` all pass.

## 2. Test results and failure root causes

| Toolchain used | pass / fail / ignored |
|---|---|
| gcc only (no Zig, no Node 22) | 1,280 / 4 / 2 |
| + Zig 0.14.1 + Node 22 | 1,267 / 17 / 2 |
| + Zig 0.14.1 + Node 22 + `CC=gcc` | 1,268 / 16 / 2 |

Many tests silently skip themselves when a tool is missing (`if
detect_toolchain().is_none() { return; }`), so installing Zig and Node 22 made
hidden failures appear rather than introducing them.

Root causes:

- **`live_c_capsule_call_returns_a_real_result`** asserts the toolchain string
  contains `"gcc"` or `"clang"`. Detection tries `cc` first, and Ubuntu's
  `cc --version` prints `cc (Ubuntu 11.5.0-...)` — neither word. Passes with
  `CC=gcc`. (Test over-specifies the compiler name.)
- **`declared_limits_replace_the_defaults_and_say_so`** unwraps a plan that
  declares `limit.mem_mb`, which Unix refuses by design; the `if cfg!(windows)`
  guard is one line too late. (Test bug.)
- **`typescript_classifier_runs_on_real_node_or_degrades`** needs Node ≥ 22.6
  for `--experimental-strip-types`; Node 18 rejects the flag and the runner
  reports `error` instead of `runtime_unavailable` (it never checks the
  version).
- **14 Zig tests** — the generated Zig does not compile under Zig 0.14 (`var`
  on never-mutated locals, `usize`/`u32` where `i32` is expected, an unused
  `lines` variable). Hidden because no CI runner has Zig.
- **`live_zig_fixed_array_struct_field_round_trips_by_value`** returns wrong
  data — see §3.
- **`renderer_cannot_commit_raw_rgb_dimensions_that_disagree_with_the_scene`**
  fails only under parallelism: five tests write the same path
  `target/render-previews/visualizer-encoded.ppm`. Passes `--test-threads=1`
  (3/3), which is how CI runs.

## 3. Zig 0.14.1 miscompiles a by-value struct (Linux x86-64)

A fixed-size scalar-array field in an `extern struct` passed and returned by
value is miscompiled. Reduced to a standalone program:

```zig
pub const SampleWindow = extern struct { id: i32, samples: [4]i16 };
pub export fn fixed_samples_zig(w: SampleWindow) SampleWindow {
    var a = w; a.samples[0] += 1; a.samples[1] += 2; a.samples[2] += 3; a.samples[3] += 4; return a;
}
// input samples {-10,20,-30,40}; expected {-9,22,-27,44}
```

| Compiler / mode | `samples` result |
|---|---|
| gcc 11 (C equivalent) | `{-9, 22, -27, 44}` ✓ |
| Zig 0.14.1 (default, `-fllvm`) | `{7, 0, -6295, 269}` ✗ |
| Zig 0.14.1 `-O ReleaseSafe` / `ReleaseFast` | `{0, 0, 0, 0}` ✗ |
| Zig 0.14.1 `-fno-llvm` | internal compiler error |
| **Zig 0.17.0** | `{-9, 22, -27, 44}` ✓ |

The bug is in Zig 0.14.1's SysV ABI handling, independent of Forgentex. **Zig
0.17.0 fixes it**, which is one reason the upgrade is a net improvement.

## 4. The native sandbox does not isolate on Linux

The Windows path runs native capsules in a zero-capability AppContainer with a
restricted low-integrity token. The non-Windows path enforces only a private
process group, per-process `RLIMIT_CPU`/`RLIMIT_AS`, a cleared environment, and a
`0700` scratch directory. Probe capsules declared `sandbox "fs.none"` were run
with `fgx run --live`; every probe below **succeeded** and the run still reported
`status: ok`.

| Probe | Declared authority | Result on Linux |
|---|---|---|
| `fopen("/etc/hostname")` | none | read succeeded |
| write a file outside scratch (absolute path) | `fs.none` | file created on host |
| `connect()` to a loopback listener | none | connection established |
| `fork()` a child | none | child ran |
| read `/proc/<getppid()>/environ` | none | read a secret from `fgx`'s env |

The receipt nonetheless reports `filesystem_isolation:
private_working_directory_only`, `runner_environment_policy:
clear_except_toolchain_path`, and `runner_capability_profile:
compute_or_host_artifact_only` — claims the Linux code does not enforce. Two more
timing/limit findings:

- **`setsid` escapes the deadline.** A child that calls `setsid()` survives the
  process-group kill. `fgx` then blocks on the detached child's output pipe: a
  capsule with a 1,000 ms wall limit returned after **12.1 s**, and a
  longer-lived child hangs `fgx` indefinitely.
- **`setrlimit` can fail closed.** The runner sets the hard limit to the
  requested value without reading the inherited one first. Under
  `ulimit -t 20` (hard CPU limit below the 30 s default) every native call
  fails with `native_capsule_launch_failed: Operation not permitted`.

These match what the project's own docs admit is deferred on Linux (no mount or
network namespace, no cgroup accounting); the experiments confirm it concretely.
Closing the gap needs cgroup v2 + namespaces + seccomp/Landlock and is **not**
part of the Zig 0.17 work.

## 5. Packaging and host-detection findings

- **glibc floor.** The release binary's newest non-weak symbol is `GLIBC_2.34`
  (only `pidfd_spawnp`/`pidfd_getpid` reach 2.39, and those are weak), so a
  binary built on 24.04 needs glibc ≥ 2.34. Build release artifacts on the
  oldest supported distro.
- **`fgx doctor` misreports Linux** as `unsupported_host` with host
  `x86_64-linux`: `compile_time_host` reads a `TARGET` env var that Cargo never
  sets and there is no `build.rs`, so it falls back to a non-triple string.
  This was observed while `rustc` was not on `PATH`. `doctor` reads the triple
  from `rustc -vV` first, so with Rust installed it correctly reports
  `x86_64-unknown-linux-gnu`; the fallback only bites hosts without Rust — the
  users of a prebuilt binary.
- **The stdlib path is baked in at build time** via `CARGO_MANIFEST_DIR`
  (`src/stdlib.rs`); a binary copied to a machine without that checkout fails
  project-mode imports (`failed to read standard library module 'core.io'`).
- **Windows filename rules are applied to Linux paths**: a file named
  `2026-10-13T10:00:00.log` makes `list_dir` fail with `io_path_unportable`.
- **Symlinked directories** are followed without a cycle check; a loop surfaces
  as `duplicate_symbol` diagnostics.

## 6. Upgrading the native Zig target from 0.14 to 0.17

Zig 0.17.0 is a real release (2026-10-03), not a nightly, but it is four minor
versions past 0.14 and crosses the 0.15 "Writergate" I/O rewrite. Out of the box
the version gate rejected it (`native_runtime_version_mismatch`), and with the
gate opened the full suite showed 46 failures (36 of them the version mismatch
on mixed-language tests).

### 6.1 Finding the new std API (standalone probes)

`std.io` is gone; `std.fs.cwd` and `std.fs.File` moved. The correct 0.17 idioms
were established by compiling small programs against the installed 0.17 until
each worked, then reading std's own benchmarks:

| 0.14 idiom | 0.17 idiom |
|---|---|
| `pub fn main() !void` | `pub fn main(init: std.process.Init) !void` |
| `std.heap.page_allocator` | `init.gpa` (and `init.io`) |
| `std.process.argsAlloc(a)` | `std.process.Args.Iterator.init(init.minimal.args)` |
| `std.fs.cwd().readFileAlloc(a, p, max)` | `std.Io.Dir.cwd().readFileAlloc(io, p, gpa, .limited(max))` |
| `std.io.getStdIn().reader().readAllAlloc(a, n)` | `std.Io.File.stdin().reader(io, &buf).interface.allocRemaining(gpa, .limited(n))` |
| `std.io.getStdOut().writer()` + `print` | `std.Io.File.stdout().writer(io, &buf).interface` + `print` + **`flush()`** |
| `std.mem.trimRight` | `std.mem.trimEnd` |

### 6.2 Codegen changes

Two separate Zig codegen paths were updated:

- **Live-runner path** (`src/ffi.rs`, `generate_zig_live_runner`): migrated the
  generated runner `main` to the table above, and made the `lines` splitter
  conditional so no-input capsules do not trip Zig's unused-local error.
- **Zig source backend** (`src/zig_backend.rs`), fixing bugs that failed on 0.14
  too and were simply never exercised in CI:
  - a `_ = &name;` binding after each generated local, which suppresses both
    "local is never mutated" and "unused local" without changing behavior;
  - `emit_cast` now routes every integer-target cast through `@intCast`, fixing
    `usize`/`u32` → `i32` (e.g. a `str_len` result whose type could not be
    inferred);
  - reassigned **parameters** (Zig parameters are immutable) are renamed to
    `<name>__arg` in the signature and copied into a mutable `var <name>` shadow
    at the top of the body.

### 6.3 Other changes

Both version gates in `src/ffi.rs` (capsule-declared and host-toolchain) pin
0.17; 23 example capsules and the inline test sources bump `zig version "0.14"`
to `"0.17"`; one platform-late assertion in `declared_limits_replace_the_defaults`
is corrected; and the `zig-0.14` / "Zig 0.14" strings in `doctor`,
`distribution`, the README prerequisite table, and the current-tense docs move to
0.17. Dated changelog rows and the v1.8 milestone-history line are left as
historical record.

### 6.4 Validation

With Zig 0.17.0, `CC=gcc`, Node 22, on the system Rust 1.99.0 toolchain:

| Gate | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test -- --test-threads=1` (CI mode) | **1,284 passed, 0 failed, 2 ignored** |

Every previously failing Zig test passes, including the by-value struct that 0.14
miscompiled.

## 7. Several Zig series side by side

Goal: let one machine hold Zig 0.14, 0.15, 0.16, and 0.17 and run each Zig
capsule on a compiler of the series it declares.

### 7.1 Runner templates per series

The generated runner reads the request from stdin and prints the result. That
I/O code was probed standalone on each official release (0.14.1, 0.15.2,
0.16.0, 0.17.0). Each candidate template was built and run on all four:

| Template | Entry point and I/O | Builds and runs on |
|---|---|---|
| Classic | `main()`, `std.io.getStdIn().reader().readAllAlloc`, `trimRight`, unbuffered `getStdOut().writer()` | 0.14 only |
| Buffered | `main()`, `std.fs.File.stdin().reader(&buf)` + `.interface.allocRemaining`, `trimEnd`, buffered writer with `flush` | 0.15 only |
| Io | `main(init: std.process.Init)`, `std.Io.File.stdin().reader(io, &buf)`, `init.gpa`, `flush` | 0.16 and 0.17 |

So three profiles cover the four series (`ffi::ZigRunnerProfile`).

### 7.2 The by-value struct miscompile spans 0.14–0.16

The § 3 bug is not specific to 0.14.1. A matrix of `extern struct` shapes,
each passed and returned by value in its own file (a combined file crashed the
0.14 and 0.15 compilers), was built with `-O ReleaseSafe` on every series:

- **Wrong on 0.14, 0.15, and 0.16, correct on 0.17:** structs larger than 8
  and smaller than 16 bytes that hold a scalar field and an array crossing
  byte 8 — for example `{ id: i32, samples: [4]i16 }` (12 bytes).
- **Correct on every series:** exactly 8 or 16 bytes (the 16-byte `PointBatch`
  fixture included), array-only structs, and structs whose array sits wholly
  on one side of byte 8.

A first guard that refused every mixed struct of 16 bytes or less wrongly
refused `PointBatch`, which 0.14.1 handles. The shipped guard
(`ffi::refuse_zig_struct_abi_hazard`) refuses exactly the failing shape, only
on x86-64 non-Windows hosts and only for compilers below 0.17.

### 7.3 Validation

- One program with four capsules declaring `0.14`, `0.15`, `0.16`, and
  `0.17` ran each capsule on its own series and returned the expected `62`.
- The full Zig test suite, retargeted at each older series: 0.14 passed 94,
  0.15 passed 93, 0.16 passed 94. Every remaining failure was a test-harness
  artifact of retargeting: the guard refusing the `fixed_samples` fixture, a
  provenance assertion expecting `0.17.x`, and one 0.15 test that pins no
  series.
- `fgx doctor` listed all four compilers as reviewed. The default-environment
  suite passed 1,302 / 0, and the new CI step ran verbatim with all three
  checksums verified.

## 8. CUDA on an RTX 5090

Goal: run CUDA capsules live, on the machine's NVIDIA GeForce RTX 5090
(compute capability 12.0, 32 GiB, driver 595.99.02). Three toolkits were
installed: NVIDIA's 13.2.51 (`/usr/local/cuda-13.2`, also `/usr/local/cuda`
and first on `PATH`), NVIDIA's 12.8.93 (`/usr/local/cuda-12.8`), and Ubuntu's
`nvidia-cuda-toolkit` 12.4.131 (`/usr/lib/nvidia-cuda-toolkit`, wrapped by
`/usr/bin/nvcc`).

### 8.1 Which toolkit builds on Ubuntu 26.04

A standalone saxpy (`-std=c++17 -O2 -arch=sm_120`) was built with each NVIDIA
toolkit and each installed host compiler:

| Toolkit | gcc 15 (default) | g++-13 | g++-11 |
|---|---|---|---|
| 13.2 | builds, runs | builds, runs | builds, runs |
| 12.8 | `#error -- unsupported GNU version! gcc versions later than 14` | `mathcalls.h(83): error: exception specification is incompatible with that of previous function "cospi"` | same as g++-13 |

CUDA 12.8 cannot build anything on glibc 2.43: the C library now declares
`cospi` and related functions, which 12.8's math headers redeclare
incompatibly. Ubuntu's 12.4 package builds with gcc 15, because Ubuntu patched
it. It cannot target `sm_120`, which arrived in 12.8.

### 8.2 Ubuntu's toolkit picked up the wrong `ptxas`

Compiling with Ubuntu's 12.4 failed with
`ptxas fatal : Value 'sm_52' is not defined for option 'gpu-name'`. Its real
nvcc lives in `/usr/lib/nvidia-cuda-toolkit/bin`, which holds no `ptxas`.
Ubuntu installs `ptxas`, `fatbinary`, `nvlink`, and `cudafe++` in `/usr/bin`,
but `/usr/local/cuda/bin` came first on `PATH`. So nvcc 12.4 ran CUDA 13.2's
`ptxas`, which no longer accepts 12.4's default `sm_52`. NVIDIA's toolkits are
self-contained: their `nvcc.profile` puts their own `bin` first. Forgentex
therefore uses Ubuntu's toolkit through `/usr/bin/nvcc` and compiles with
`/usr/bin` first on `PATH`. After that, a `cuda-12.4` capsule built with 12.4
and ran on the RTX 5090 through PTX JIT (`compute_75`).

### 8.3 Memory limits and the CUDA driver

The same binary was run under shell `ulimit`s in a cleared environment
(`env -i`):

| Limit | Result |
|---|---|
| none, cleared environment | runs |
| `RLIMIT_AS` 256 MiB – 12 GiB | `out of memory` at context creation |
| `RLIMIT_AS` 16 GiB and above | runs |
| `RLIMIT_DATA` 64 MiB | `CUDA-capable device(s) is/are busy or unavailable` |
| `RLIMIT_DATA` 80 MiB and above | runs |

The CUDA driver reserves address space roughly in proportion to the GPU's
memory, so an address-space ceiling cannot bound a CUDA runner. A
data-segment ceiling still bounds real host allocations: under
`RLIMIT_DATA` 256 MiB a 1 GiB `malloc` fails, while CUDA starts normally.
CUDA runners therefore get the native memory budget (default 512 MiB) as
`RLIMIT_DATA`.

The first version of the test for this passed a 1 GiB allocation under the
256 MiB limit. The compiler had removed the `malloc` / `memset` / `free`
sequence, because nothing observed the memory, so no allocation happened. The
test touches its memory through a `volatile` pointer now.

### 8.4 Validation

With the bridge in place (`src/cuda.rs`, `src/tests/cuda.rs`), on the RTX 5090:

- `examples/capsule/saxpy_live_cuda.fgx` computes `y = 2x + y` on the GPU, in
  about 1 s per cold call including the nvcc build. A warm call reuses the
  cached runner. Called from Forgentex code, it updates the caller's slice.
- A `compute_75` (PTX-only) build runs through the driver's JIT with its cache
  disabled.
- With `gpu.compute device:0`, `cudaGetDeviceCount` reports 1.
- An `sm_130` target and a grant for a nonexistent GPU are refused before
  anything is compiled.
- With all three toolkits installed, `cuda-12.4` builds with 12.4 (skipping the
  broken 12.8), `cuda-13.0` builds with 13.2, and `cuda-12.8` fails with a hint
  naming the fix.
- The full suite passed 1,318 / 0 with fmt and clippy clean.

## 9. Julia and MoonBit capsules

Toolchains were installed into a session-local directory: Julia 1.10.12,
1.12.7, and 1.13.2 from the official tarballs (checksums verified against
`versions.json`), and MoonBit from `cli.moonbitlang.com` (moon 0.1.20261012,
moonc 0.10.15), mirroring its install script's steps. The script was read, not
run.

### 9.1 Julia

- **No JSON in the standard library.** Julia 1.13's stdlib has TOML but no
  JSON, so the driver carries its own small codec. It was checked on all three
  versions with escapes, surrogate pairs, nested arrays, and objects.
- **Start-up.** `julia --startup-file=no -e 'println(1+1)'` took 164 ms
  (1.10), 146 ms (1.12), and 126 ms (1.13). A full call through the codec
  took 250–310 ms. That is why Julia gets a warm worker.
- **Typed dispatch.** JSON `100` decodes as an `Int64`, which a method typed
  `f(x::Float64)` rejects. The driver converts each argument to its declared
  type before the call.
- **Validation.** The Julia tests passed with each version as the `PATH`
  Julia. With 1.13 on `PATH` and 1.10 and 1.12 under `~/.local/opt`, `fgx
  doctor` listed all three, and each `~X.Y` capsule ran on its own series.

### 9.2 MoonBit

- **The manifest format changed.** This `moon` rejects `moon.mod.json`
  ("no longer supported … migrate to `moon.mod`"). `moon new` now writes
  `moon.mod` and per-package `moon.pkg` files. The bridge sidesteps both:
  `moon run <file>.mbt --build-only` builds a standalone file with no project.
- **No stdin in the core library**, which has argv (`@env.args`) and JSON
  but no input API. Two routes worked:
  - the native target, with `extern "C" fn getchar`: a 0.5 s build and a
    3 ms run;
  - the `wasm-gc` target, with `fn read_char() -> Int = "spectest"
    "read_char"`. This one reads Unicode code points from stdin under
    `moonrun`.
- **`moonrun --policy` is a real sandbox.** With an empty policy, the program
  saw no environment variables (147 without it), `current_dir` failed with
  `Sandbox policy blocked file read: "."`, and stdin still worked. One call
  took about 7 ms.
- **But not for WASI.** `moonrun --help` says "WASI is not covered", and the
  binary implements `path_open`, `fd_read`, and other WASI calls. Ordinary
  compute (math, maps, sorting, string formatting, JSON) compiled to modules
  importing only `spectest.read_char` and `spectest.print_char`. Code using
  `@env` imported `__moonbit_fs_unstable.*`. So the bridge reads each
  module's import section and allows only those two console functions.
- **64-bit integers.** MoonBit's `Int64::to_json` emits a string
  (`"9007199254740993"`), and a JSON number decoded as `Double` would lose
  precision. The wrapper decodes integers from `Json::Number`'s source text
  (`repr`) and encodes results itself.
- **Validation.** All MoonBit tests passed, including exact round-trips of
  `9007199254740993` and `18446744073709551615` and a refusal for a
  hand-declared `wasi_snapshot_preview1.proc_exit` import. The full suite
  passed 1,342 / 0, both with the toolchains on `PATH` and without them (the
  Julia and MoonBit tests then return early).

## 10. Still open

The open Linux gaps, their current status, and the plan to close them are
tracked in [linux_support.md](linux_support.md), which is kept up to date as
items close. This log records the experiments as they were run.
