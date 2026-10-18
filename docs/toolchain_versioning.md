# Toolchain versioning

How Forgentex should let a native or managed capsule target a specific
toolchain *version* and *environment*, resolve it on the host, and record what
it used — uniformly across Zig, Rust, C/C++, Python, Node, Go, JVM, and .NET.

Phase 1 of this model is implemented (`src/toolchain_version.rs`); the rest is a
staged plan. The design exists because the single-version Zig pin proved
brittle: when the host's Zig differed from the one blessed string, every Zig
capsule failed, and the only recourse was a full source migration
([docs/linux_and_zig_0_17_experiments.md](linux_and_zig_0_17_experiments.md)).

## Three axes, currently conflated

"Version" means three orthogonal things. The bridges mix them today:

| Axis | What it controls | Where it lives now |
|---|---|---|
| **Language level** | the syntax/semantics the source is written in | Rust `edition "2021"`, C `version "c11"` |
| **Toolchain version** | the compiler/runtime binary | Zig `version "0.17"` — an exact pin |
| **Environment** | which interpreter instance / library set | Python `env "conda:… / venv:… / path:…"` (`src/capsule_env.rs`) |

Python feels flexible because it has the **environment** axis and its toolchain
version barely matters — the generated driver is plain text that runs on any
Python 3. Zig is the opposite: one hard-pinned version, and the *generated
scaffolding itself* is version-specific. The cost of supporting a version range
is entirely about whether Forgentex's generated code is version-sensitive:

| Language | Generated-code version sensitivity | Cost of a range |
|---|---|---|
| Python, Go, JVM, .NET, C/C++ | low (driver/ABI stable) | cheap — resolve + loosen the gate |
| Node | medium (`--strip-types` needs ≥ 22.6) | moderate |
| Rust | medium (editions, MSRV of emitted code) | moderate |
| Zig | high (`std.io` → `std.Io`, `main` signature) | expensive — a codegen profile per family |

## The model

### 1. Version *constraints*, not exact pins

A capsule declares a constraint, parsed by `toolchain_version::VersionConstraint`:

| Declaration | Meaning |
|---|---|
| `0.17` | minor pin — any `0.17.x` (`[0.17.0, 0.18.0)`) |
| `0.17.1` | exact |
| `>=1.82` | open lower bound (floor / MSRV) |
| `^0.17` | semver caret (pins the most significant non-zero component) |

`Version::extract` recovers a comparable version from a noisy `--version`
banner (`rustc 1.99.0 (…)` → `1.99.0`, `cc (Ubuntu 11.5.0-…) 11.5.0` → `11.5.0`).

### 2. A reviewed set per language

"Support multiple versions" does **not** mean *any* version — it means any
version in the set Forgentex has a **reviewed codegen profile** for that also
satisfies the capsule's constraint. This preserves the audit stance the security
model relies on. The reviewed sets are data:

- Zig: `[0.14.0, 0.18.0)` — the 0.14, 0.15, 0.16, and 0.17 series. Zig makes
  no cross-version source promise, so every series in the set has a reviewed
  runner profile (§4, § Phase 3); admitting 0.18 means reviewing a profile for
  it first.
- Rust: `>=1.82` — a floor, because Rust is backward compatible. 1.82 is the
  MSRV of the emitted runners (`unsafe extern`, the v4 `Cargo.lock`).

### 3. The resolver pipeline

Replace the scattered `detect_*` functions with one pipeline per language:

```
ToolchainRequirement { lang, version: Constraint, env: Option<Selector> }
        │
        ▼  Resolver (per-language adapter)
1. explicit override      → env "path:…" / $ZIG / $RUSTC / [toolchains] in Forgentex.toml
2. query version managers → rustup toolchains, pyenv/conda, nvm/fnm, SDKMAN/JAVA_HOMEs,
                            snap + /opt for zig, multiple cc/clang
3. probe --version, build the candidate set
4. intersect: (satisfies constraint) ∩ (reviewed set)
5. pick highest, or fail: "need Zig 0.17.x; host has 0.14.1, 0.17.0; …"
        │
        ▼
ResolvedToolchain { exact_version, invocation, env, profile_id }  → recorded in the receipt
```

### 4. Codegen profiles (the expensive part)

For version-sensitive languages, carry a profile keyed by version family and
select it from the resolved version:

```
match resolved.profile_id {
    ZigProfile::V0_17 => generate_zig_runner_v0_17(...),  // std.Io, main(init)
    ZigProfile::V0_14 => generate_zig_runner_v0_14(...),  // std.io, old main
}
```

Each profile is independently reviewed, preserving the guarantee. The goal is to
keep scaffolding as version-agnostic as possible and fork only where the API
genuinely diverges.

### 5. Generalize the environment selector

Extend Python's `env` vocabulary (`conda:`, `venv:`, `path:`) across languages
rather than inventing a mechanism per bridge: add `rustup:<toolchain>`,
`nvm:<ver>` / `node:<path>`, `jdk:<path>` / `sdkman:<ver>`, `zig:<path>`.
`ResolvedEnv` already models "a located toolchain root + interpreter".

### 6. A toolchain lockfile

The moment capsules declare ranges, a build resolves differently on different
hosts — which breaks `reproducible = true`. Ranges need a companion lockfile
recording the exact resolved version per capsule, so a reproducible build pins
what a flexible declaration resolved to.

## Phasing

1. **Constraint + reviewed range + provenance** — *implemented* (§ Phase 1).
2. **Multi-toolchain discovery** — *partially implemented* (§ Phase 2): native
   Zig and Rust are discovered beyond `PATH`, the matching version is selected,
   and `fgx doctor` lists every one found. Still to do: nvm/SDKMAN for the
   managed runtimes and a `[toolchains]` registry in `Forgentex.toml`.
3. **Codegen profiles** — *implemented for Zig* (§ Phase 3): the 0.14–0.17
   series run side by side through three runner profiles. Still to do: Node
   TS-vs-classic, Rust edition/MSRV profiles.
4. **Lockfile** — pin resolved versions for reproducibility.

## Phase 1 (implemented)

`src/toolchain_version.rs` adds `Version`, `VersionConstraint`
(`Exact` / `AtLeast` / `Range`), parsing, banner extraction, `matches`,
`is_within`, and the reviewed sets `zig_reviewed()` and `rust_min_supported()`.
It is dependency-free and unit-tested.

Wired into the native bridge:

- **Declared-version gate** (`native_standard`): a Zig capsule may declare any
  constraint inside the reviewed series (`0.17` or a specific `0.17.1`), not
  just one exact string.
- **Host-version gate** (`validate_native_toolchain_version`): the host Zig must
  land in the reviewed series *and* satisfy the capsule's declaration, with
  errors that name both the requirement and what was found. Rust now enforces
  the MSRV floor — a readable rustc below 1.82 is refused with an actionable
  message (an unreadable banner is not rejected, so exotic toolchains are not
  regressed).
- **Receipt provenance**: `native_receipt.v1` gains `toolchain_semver` (the
  exact host version resolved) and `toolchain_requirement` (the series/floor the
  call was admitted under, e.g. `0.17.x` or `>=1.82`).

## Phase 2 (implemented: native discovery)

Native Zig and Rust are now resolved by constraint rather than by first-on-`PATH`
(`native_build::resolve_zig_toolchain` / `resolve_rust_toolchain`, wired through
`ffi::detect_native_toolchain`):

- An explicit `$ZIG` / `$RUSTC` is honored as given — the deterministic,
  reproducible choice.
- Otherwise a `PATH` toolchain that already satisfies the request is used with
  no further scanning (the common case stays fast).
- Only a missing or non-satisfying `PATH` toolchain triggers a search of
  well-known locations: for Zig the snap wrapper and `zig-*` directories under
  `/opt`, `~/.local/opt`, and `~`; for Rust the rustup-managed toolchains under
  `~/.rustup/toolchains` and `~/.cargo/bin`. The highest version satisfying the
  request wins (`toolchain_version::select_highest`); if none match, the first
  toolchain found is returned so validation reports `found X, need Y`.

So a host whose default Zig is 0.14 but which also has a 0.17 under `/opt`, or
whose default `rustc` is below the MSRV but which has a newer rustup toolchain,
now resolves the right one automatically.

`fgx doctor` surfaces the whole discovery surface: its report gains a
`discovered_toolchains` list (`native_build::discover_zig_toolchains` /
`discover_rust_toolchains`), each entry tagged with whether it falls in the
language's reviewed set — so a user can see every Zig and Rust the host offers
and which ones a capsule may use.

Still to do in Phase 2: discovery for the managed runtimes (pyenv/conda already
exists for Python via `capsule_env.rs`; add nvm/fnm for Node, SDKMAN/`JAVA_HOME`
for the JVM) and a `[toolchains]` registry in `Forgentex.toml` for explicit
per-project pins.

## Phase 3 (implemented: multiple Zig series)

Forgentex now accepts, resolves, and runs every Zig series from 0.14 to 0.17,
and several can be installed and used side by side.

- **Declarations.** A capsule declares the series its source is written for —
  `zig version "0.15"`, an exact `"0.16.0"`, or a range such as `">=0.16"`.
  The declaration is accepted when it overlaps the reviewed set.
- **Resolution.** Host resolution searches for the declaration intersected
  with the reviewed set (`VersionConstraint::intersection`). `$ZIG` and then
  the `PATH` `zig` are preferred when they satisfy it; otherwise the
  well-known locations (the snap, `zig-*` directories under `/opt`,
  `~/.local/opt`, and `~`) are searched for the newest match. One program can
  therefore mix capsules written for different series, each running on a
  compiler of its own series. `$ZIG` no longer forces every capsule onto one
  compiler; it is simply the first candidate.
- **Runner profiles.** Zig's standard-library I/O changed twice in this span,
  so the generated runner comes in three families (`ffi::ZigRunnerProfile`),
  each verified to build and run on exactly its own series:

  | Profile | Series | Entry point | Request / result I/O |
  |---|---|---|---|
  | `Classic` | 0.14 | `main()` | `std.io.getStdIn()` / unbuffered `getStdOut()`, `trimRight` |
  | `Buffered` | 0.15 | `main()` | buffered `std.fs.File` reader and writer, `flush`, `trimEnd` |
  | `Io` | 0.16, 0.17 | `main(init: std.process.Init)` | `std.Io.File` with the `io` interface, `flush`, `trimEnd` |

  Argument parsing, the capsule call, and result printing are shared. The Zig
  source backend's output compiles unchanged on every series.
- **A compiler bug the profiles must respect.** Zig 0.14, 0.15, and 0.16
  miscompile some by-value `extern struct`s on x86-64 System V: those larger
  than 8 and smaller than 16 bytes that hold a scalar field and an array
  crossing byte 8 (for example `{ id: i32, samples: [4]i16 }`). The call
  crashes or returns wrong values in `-O ReleaseSafe`; 0.17 is correct. A
  16-byte struct, an array-only struct, or one whose array sits wholly on one
  side of byte 8 is unaffected. On those series Forgentex refuses such a
  boundary with `native_capsule_boundary_unsupported` instead of running it
  (`ffi::refuse_zig_struct_abi_hazard`).
- **Provenance.** `toolchain_requirement` in the receipt records the series the
  call was admitted under (for example `0.15.x`), and `toolchain_semver` the
  exact compiler used.

To install several series, unpack each official tarball into its own
`zig-x86_64-linux-<version>` directory under `~/.local/opt` (or `/opt`) and
keep one of them on `PATH`; `fgx doctor` lists every Zig it finds and whether
it is in the reviewed set.

## Current status by language

| Language | Level axis | Version axis (Phase 1) | Environment axis |
|---|---|---|---|
| Zig | — | reviewed series `0.14.x–0.17.x`, constraint-resolved, side by side | `$ZIG`, PATH, snap + `/opt` + `~/.local/opt` |
| Rust | edition 2018/2021/2024 | MSRV `>=1.82`, constraint-resolved | `$RUSTC`, PATH, rustup toolchains |
| CUDA | GPU target `sm_NN` / `compute_NN` (in `env`) | `cuda-X.Y` admits X.Y and later X.x, floor `>=11.0`; constraint-resolved, side by side, preferring a toolkit that builds on the host | `$NVCC`, PATH, `$CUDA_HOME` / `$CUDA_PATH`, `/usr/local/cuda-*`, `/opt/cuda`, `/usr/bin/nvcc` |
| C / C++ | `c11` / `c++17…` | none (any `cc`/`clang`) | `$CC` / `$CXX` |
| Python | — | implicit | `conda:` / `venv:` / `path:` |
| Node | — | TypeScript needs ≥ 22.6 and Node's built-in type stripping, both checked before a TS capsule runs | `$NODE` only |
| Julia | — | Julia `[compat]` rules (`1.10`, `~1.10`, `=1.10.12`), floor `>=1.10`; resolved side by side | `$JULIA`, PATH, juliaup, `julia-*` under `~/.local/opt`, `~`, `/opt` |
| MoonBit | — | moonc version (`0.10` = 0.10.x), floor `>=0.10` | `$MOON`, PATH, `$MOON_HOME`, `~/.moon` |
| Go / JVM / .NET | — | implicit | `$GO` / `$JAVA_HOME` / `$DOTNET` |

The remaining Phase 2 work fills the environment column for the managed runtimes
(Node, JVM, .NET) and the version column for Node. Phase 3's codegen profiles
are in place for Zig; Node and Rust profiles remain.
