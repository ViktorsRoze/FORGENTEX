# Linux support

Where Forgentex stands on Linux, what is still missing, and the order in which
the gaps are being closed. Every finding here was reproduced on a Linux host;
the experiments behind them are recorded in
[linux_and_zig_0_17_experiments.md](linux_and_zig_0_17_experiments.md).

- **Reference host:** Ubuntu 26.04.1 LTS, x86-64 (kernel 7.0, glibc 2.43).
  Earlier checks ran on Ubuntu 24.04.5 LTS (kernel 6.17, glibc 2.39).
- **Toolchains checked on 26.04:** Rust 1.99.0 (rustup) and 1.93.1 (the 26.04
  package version), Zig 0.17.0, gcc 15.2, Node 22.22 (Ubuntu apt) and 22.23
  (official build), Python 3.14 (system) and 3.12, Go 1.26, OpenJDK 25,
  .NET SDK 10.0. On 24.04: gcc 11.5, Node 18.19, Go 1.22, OpenJDK 21, .NET 8.
- **Last checked:** 2026-10-16.

Linux is still listed as `source-build-unverified` in the distribution manifest
(`src/distribution.rs`); only `x86_64-pc-windows-msvc` is verified.

## What works

- On a default Ubuntu 26.04 or 24.04 (`CC` unset, Ubuntu's packaged Node),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` all pass: 1,297 passed, 0 failed, 2 ignored, both in cargo's
  default parallel run and in CI's serial mode. TypeScript capsules report
  `runtime_unavailable` with Ubuntu's Node (too old on 24.04, built without
  TypeScript support on 26.04); with an official Node 22 build, they run.
- The bash helper scripts (`scripts/check.sh`, `demo.sh`, `fuzz.sh`,
  `forgepack-clean-install.sh`) all pass, and CI runs the same gate on Ubuntu
  in the `rust-ubuntu` job.
- A release build passes the CI clean-install checks (`dist-manifest`,
  `release-evidence --strict`, `forgepack evidence --strict`).
- The project's Ubuntu containment checklist
  ([native_containment_platform_verification.md](native_containment_platform_verification.md))
  passes. That checklist asserts the weaker Linux receipt values; it does not
  test isolation (see § 2 below).
- The VS Code extension's test scripts pass on Node 18.
- Zig 0.14, 0.15, 0.16, and 0.17 can be installed side by side. Each Zig
  capsule runs on a compiler from the series it declares (`zig version "0.15"`
  runs on 0.15.x), with a runner generated for that series' standard-library
  API. On x86-64, Zig 0.14–0.16 miscompile one by-value struct shape, so a
  capsule passing that shape is refused on those compilers and asked to use
  0.17. `fgx doctor` lists every Zig and Rust toolchain it finds
  ([toolchain_versioning.md](toolchain_versioning.md), Phase 3).
- CUDA capsules run live on Linux with an NVIDIA GPU: nvcc builds the capsule
  and a C++ runner, the runner sees only the granted GPU, and its host memory
  is bounded by a data-segment limit, because the CUDA driver's address-space
  reservations rule out the usual `RLIMIT_AS`. Several CUDA toolkits can sit
  side by side, and `fgx doctor` lists them and the GPUs
  ([native_gpu_capsules.md](native_gpu_capsules.md) § Live CUDA capsules).
- Julia capsules run on the host Julia (1.10, 1.12, and 1.13 verified, side by
  side). MoonBit capsules run as WebAssembly in `moonrun`'s deny-all sandbox,
  which makes them the one capsule kind that *is* isolated on Linux
  ([julia_and_moonbit_capsules.md](julia_and_moonbit_capsules.md)).

## Ubuntu 26.04

Checked on Ubuntu 26.04.1 LTS on 2026-10-16. In a default environment the
documented commands pass — `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, and `cargo test` (1,297 passed,
0 failed) — as do `scripts/check.sh` (the serial suite, fuzzing, the C-backend
differential tests, and the CLI demo), the release-gate commands from a clean
install directory, and the project's Ubuntu containment checklist. What the
upgrade changed, and how Forgentex copes:

| Area | On Ubuntu 26.04 | Result |
|---|---|---|
| Rust | apt ships 1.93 (24.04 shipped 1.75) | Builds, passes clippy, and passes the full suite (checked with upstream 1.93.1), so rustup is no longer required on 26.04 |
| C compiler | gcc 15.2, whose default language standard is C23 | C capsules (compiled with `-std=c11`), the C backend, and the differential tests pass |
| Node | apt ships 22.22, built **without TypeScript support** (`process.features.typescript` is `false`; Debian/Ubuntu omit the bundled type stripper) | **Fixed on 2026-10-16.** The version check alone let TypeScript capsules start and fail with `ERR_NO_TYPESCRIPT`. They now also ask Node for the feature, so Ubuntu's Node reports `runtime_unavailable` with the reason; an official Node 22 build runs them |
| Python | system `python3` is 3.14 | The Python bridge passes (41 tests) |
| Java | `default-jdk` is JDK 25; this upgrade left only `default-jre` installed | The JVM bridge passes with JDK 25. With only a JRE there is no `javac`, and JVM capsules report `runtime_unavailable` |
| .NET | apt offers `dotnet-sdk-10.0` | The C# bridge passes on .NET 10 (`net10.0` target) |
| Go | 1.26 | The Go bridge passes |
| coreutils | uutils (Rust) coreutils 0.10 replaces GNU coreutils | `scripts/*.sh` and the CI Zig step (`realpath`, `mktemp -d`, `sha256sum -c`) work unchanged |
| Kernel | 7.0, cgroup v2, Landlock enabled, AppArmor restricting unprivileged user namespaces | The containment checklist passes. For step 6: Landlock is usable without privileges, while namespace-based isolation would need an AppArmor profile |
| Release binary | built against glibc 2.43 | Still needs only glibc 2.34 (its 2.39 symbols are weak), so it runs on 22.04 and later |
| CUDA | gcc 15.2 and glibc 2.43 are newer than older toolkits support | NVIDIA's CUDA 13.2 and Ubuntu's packaged 12.4 (`nvidia-cuda-toolkit`) work, and CUDA capsules ran on an RTX 5090. NVIDIA's 12.8 cannot build anything: it rejects gcc 15, and with an older g++ its math headers conflict with glibc 2.43. Forgentex skips a toolkit that cannot build on the host when another one satisfies the capsule |
| Zig | not packaged; the snap's `latest/beta` is 0.17.0, but `latest/edge` is already 0.18-dev | Works today, and the official 0.14.1, 0.15.2, and 0.16.0 tarballs run alongside it. When 0.18 is released the beta channel moves and snap refreshes it automatically. Forgentex has not reviewed 0.18, so it would skip that compiler and the snap would no longer supply 0.17. Hold the snap (`sudo snap refresh --hold zig`) or use the tarballs |

GitHub-hosted `ubuntu-26.04` runners are available; the CI job still targets
`ubuntu-24.04`.

## What is missing

### 1. The developer workflow on a default Ubuntu machine

**Fixed in step 1 (2026-10-14).** Before it, on a fresh Ubuntu with no special
environment, `cargo clippy -- -D warnings` failed and `cargo test` reported
1,294 passed, 3 failed. Now all three documented commands pass there. What
changed:

- **Clippy on current stable.** Four sites tripped two lints that are new in
  Rust 1.99 (`chunks_exact_to_as_chunks`, `byte_char_slices`) — a failure on
  every OS, since Windows CI also installs the latest stable. They now use
  `as_chunks` (stable since Rust 1.88) and a byte-string literal. The SHA-256
  block loop in `src/crypto.rs` was rechecked against the NIST test vectors and
  against coreutils `sha256sum` on multi-block inputs.
- **Compiler-name test.** `live_c_capsule_call_returns_a_real_result` required
  "gcc" or "clang" in the receipt's compiler name, but detection tries `cc`
  first and Ubuntu's `cc --version` prints `cc (Ubuntu …)`. The test now checks
  that the receipt names the compiler detection actually chose and carries a
  parsed version.
- **Node version.** TypeScript capsules run through
  `node --experimental-strip-types`, which needs Node 22.6+; Ubuntu ships 18,
  and nothing checked. The required version is now
  `toolchain_version::node_typescript_min`, checked before Node starts in the
  fixture runner, live calls, and warm workers. An older Node yields
  `runtime_unavailable` with the reason instead of a raw Node error.
- **Render-test race.** The render-pipeline tests share fixed output files
  declared inside the fixtures in `examples/pipeline/`. The tests already had a
  lock for this (`render_artifact_guard`), but only 7 of the 18 render tests
  took it; all 18 now do.

**Kept passing by step 2 (2026-10-14).** The helper scripts now have bash
counterparts: `scripts/check.sh`, `demo.sh`, `fuzz.sh`, and
`forgepack-clean-install.sh`. They stop at the first failing command (the
PowerShell versions never check `$LASTEXITCODE`). `windows-containment-check.ps1`
has no Linux counterpart, because Linux has no containment to verify yet
(step 6).

CI now has a `rust-ubuntu` job on `ubuntu-24.04`: the same format, clippy, and
serial test gate as the Windows job, plus the fuzz and CLI-demo scripts. It
installs Zig 0.17 on `PATH` and 0.14.1, 0.15.2, and 0.16.0 under
`~/.local/opt` (all checksum-verified), plus Node 22 and JDK 21. It takes gcc,
Python, Go, and .NET from the runner image. Its full step sequence was
reproduced on a clean copy of the repository with the same toolchains — 1,297
tests passed, along with the fuzz and demo scripts — but its first run on
GitHub is the real confirmation.

### 2. The native capsule sandbox does not isolate on Linux

On Windows, native capsules run in a zero-capability AppContainer under a
restricted token. On Linux the runner gets a private process group,
per-process `RLIMIT_CPU`/`RLIMIT_AS`, a cleared environment, and a `0700`
scratch directory — nothing more. Reproduced with probe capsules:

- A capsule declaring `sandbox "fs.none"` read a host file, wrote outside its
  scratch directory, connected to a loopback listener, and forked a child. The
  run reported `status: ok`.
- A capsule can read `fgx`'s own environment through `/proc/<ppid>/environ`, so
  clearing the runner's environment does not hide the parent's secrets.
- A child that calls `setsid()` survives the process-group kill, and `fgx`
  blocks on its output pipe: a capsule with a 1,000 ms wall limit returned after
  12.1 s, and a longer-lived child would hang `fgx` indefinitely.
- Under an inherited CPU hard limit below the 30 s default (common under
  systemd and in containers), every native capsule fails with
  `native_capsule_launch_failed: Operation not permitted`. The runner sets the
  hard limit to the requested value instead of clamping to the inherited one.

From reading the code: there is no CPU or memory accounting across a capsule's
process tree, no process-count limit, no cleanup of children when `fgx` itself
dies, core dumps stay enabled, and the compile step runs unconfined. The managed
runtimes (Python, Node, Go, JVM, .NET) write and execute driver scripts in
predictable `/tmp/forgentex_<pid>_<n>_*` directories, which another local user
could pre-create.

The project already plans the fix — a cgroup v2 subtree per call,
user/mount/network namespaces, and seccomp — in
[native_containment_platform_verification.md](native_containment_platform_verification.md).
Windows has 16 tests proving its denials; Linux has none yet.

### 3. No way to ship Linux binaries

- CI's `release-artifacts` job builds only Windows `.exe` files and hard-codes
  the `x86_64-pc-windows-msvc` host. Marking Linux verified also needs code
  changes: `VERIFIED_HOST` in `src/distribution.rs`, the
  `only_windows_is_declared_verified_for_0_1` test, and the release-proof host
  check.
- The standard library's location is fixed at build time through
  `CARGO_MANIFEST_DIR` (`src/stdlib.rs`), so a prebuilt binary on another
  machine cannot import `core.*` modules.
- `fgx doctor` reads the host triple from `rustc -vV`. When Rust is not
  installed — the usual case for someone using a prebuilt binary — it falls
  back to `x86_64-linux`, which matches no manifest triple, and reports
  `unsupported_host`.
- The README's install steps and the miniaudio download command in
  `THIRD-PARTY.md` are Windows/PowerShell only. A binary built on Ubuntu 24.04
  needs glibc ≥ 2.34, so it runs on 22.04 and later.

### 4. Linux behavior gaps

Found by reading the code unless noted.

- **Filenames.** Windows filename rules are enforced on Linux paths: a name
  containing `:` (an ISO timestamp, for example), `<>"|?*`, a trailing dot or
  space, or a reserved name such as `aux.c` makes `list_dir` fail for the whole
  directory with `io_path_unportable` (`src/interp.rs`). The `:` case was
  reproduced.
- **Compilers outside the system paths.** The runner's cleared environment drops
  `LD_LIBRARY_PATH`, so compilers from conda or `/opt` can produce runners that
  fail to start.
- **`CC` with arguments.** `$CC`, `$CXX`, `$RUSTC`, and `$ZIG` are treated as a
  single program name, so `CC="ccache gcc"` silently falls back to `cc`.
- **Managed-runtime environment.** The scrubbed environment drops `LC_CTYPE`
  (non-ASCII JVM text becomes `?`) and does not set
  `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT` (.NET fails on images without ICU).
- **Runtime discovery.** Go and .NET are only looked for in Windows install
  locations beyond `PATH`, and a host with only a Java runtime (no `javac`)
  skips the JVM bridge.
- **Smaller issues.** `forgepack` does not preserve the executable bit; a `/tmp`
  mounted `noexec` breaks native runners and `go run`; running `fgx` as PID 1
  in a container leaves zombie processes.

### Not Linux-specific, tracked elsewhere

- Native debugging through gdb/lldb is not implemented on any OS.
- Audio playback through miniaudio is blocked on every OS; its planned capsule
  contract (`audio_dsp_track.md`) is written Windows-first
  (`env "windows-x86_64"`).
- The rest of the toolchain-versioning roadmap: Node/JVM version discovery,
  `[toolchains]` pins in `Forgentex.toml`, Node and Rust codegen profiles, and
  a lockfile ([toolchain_versioning.md](toolchain_versioning.md)).

## Plan

| Step | Scope | Size | Status |
|---|---|---|---|
| 1 | Make the documented commands pass on a default Ubuntu: fix the four clippy sites; fix the three failing tests (compiler-name assertion, a Node version check that degrades cleanly, and serializing the render tests) | small | **done** (2026-10-14) |
| 2 | Keep them passing: an Ubuntu CI job and bash versions of the scripts | small–medium | **done** (2026-10-14; awaiting its first GitHub run) |
| 3 | Cheap sandbox hardening: clamp `setrlimit` to the inherited hard limit, set `RLIMIT_CORE` to 0, kill detached children and stop blocking on their pipes, make `fgx` non-dumpable so `/proc/<pid>/environ` is unreadable | medium | open |
| 4 | Distribution: Linux release artifacts, a relocatable standard library, a `doctor` host fallback that works without `rustc`, and Linux install docs | medium | open |
| 5 | The behavior gaps in § 4 | medium | open |
| 6 | Full isolation: user/mount/network namespaces, cgroup v2 accounting, seccomp or Landlock, and Linux equivalents of the 16 Windows containment tests | large | open |

## Setting up Ubuntu for development

What to take from Ubuntu depends on the release:

| | Ubuntu 24.04 | Ubuntu 26.04 |
|---|---|---|
| Rust | apt's 1.75 is too old: use rustup | apt's 1.93 works (`cargo rustc rustfmt rust-clippy`), or use rustup |
| Node for TypeScript capsules | apt's 18 is too old | apt's 22 is built without TypeScript support |
| JDK | `default-jdk` (21) | `default-jdk` (25) — check `javac` exists after an upgrade |
| .NET SDK (optional, one test) | Microsoft's packages | `dotnet-sdk-10.0` |
| Zig 0.17 | official tarball | official tarball, or the snap held at 0.17 |
| Older Zig series (optional) | official tarballs under `~/.local/opt` | official tarballs under `~/.local/opt` |
| Julia (optional) | official tarballs or juliaup | official tarballs or juliaup |
| MoonBit (optional) | `cli.moonbitlang.com` installer (`~/.moon`) | `cli.moonbitlang.com` installer (`~/.moon`) |
| CUDA toolkit (optional, NVIDIA GPU) | NVIDIA's packages or `nvidia-cuda-toolkit` | NVIDIA's CUDA 13, or `nvidia-cuda-toolkit` (12.4); not NVIDIA's 12.8 |

```sh
# C/C++ compilers and the managed-runtime bridges (Python, Go, JVM)
sudo apt install build-essential git python3 golang-go default-jdk

# Rust on 26.04 can come from apt:
sudo apt install cargo rustc rustfmt rust-clippy
# ...on 24.04 (or for the latest stable anywhere) use rustup instead:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Zig 0.17: the official tarball, checksum-verified, then on PATH
mkdir -p ~/.local/opt && cd ~/.local/opt
curl -LO https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz
echo "1cbe9df9f27e6b78d14ccbca43b6703a404ef79ef1c463de901d7f088d4e2026  zig-x86_64-linux-0.17.0.tar.xz" | sha256sum -c -
tar -xf zig-x86_64-linux-0.17.0.tar.xz
echo 'export PATH="$HOME/.local/opt/zig-x86_64-linux-0.17.0:$PATH"' >> ~/.bashrc

# Optional: older reviewed Zig series side by side (no PATH change needed)
for pair in 0.14.1:24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c \
            0.15.2:02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239 \
            0.16.0:70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00; do
  v=${pair%%:*}; f=zig-x86_64-linux-$v.tar.xz
  curl -LO https://ziglang.org/download/$v/$f
  echo "${pair#*:}  $f" | sha256sum -c - && tar -xf $f
done
```

Put Zig 0.17 on `PATH` (or set `$ZIG`) rather than relying on discovery alone.
Capsule execution also finds `zig-*` directories under `~/.local/opt`, `~`,
and `/opt`, plus `/snap/bin/zig`. But the test suite's availability checks and
`fgx doctor`'s `zig_ready` look only at `PATH` and `$ZIG`, so without a Zig
there the Zig tests skip silently. Any other series found in those directories
is used for capsules that declare it.
`live_zig_runs_on_every_installed_reviewed_series` runs one capsule on each.

CUDA capsules need the NVIDIA driver (for `nvidia-smi` and the GPU) and a
toolkit. NVIDIA's installers put each one in `/usr/local/cuda-X.Y`, where
Forgentex finds it without a `PATH` change; `fgx doctor` lists the toolkits and
GPUs it sees.

TypeScript capsules need an official Node 22.6+ build (nodejs.org, NodeSource,
or nvm) on either release; with Ubuntu's packaged Node they report
`runtime_unavailable` with the reason. A .NET SDK is optional and covers one
test.

Then, from the repository root:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run --bin fgx -- doctor   # reports readiness and lists the Zig and Rust toolchains found
```

`scripts/check.sh` runs the full local gate in one go — the checks above (with
CI's serial test mode), then the fuzz and CLI-demo scripts. The demo rewrites
`examples/project.fgmap`, as `demo.ps1` does.

CI runs the suite with `--test-threads=1`, which keeps native-capsule timing
stable on Windows runners. On Linux the default parallel run passes as well.
