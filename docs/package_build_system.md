# Forgentex Package Manager and Build System

Stage 23 turns manifest projects into small reproducible packages. The goal is
not a public registry yet. The goal is a local package graph that agents and
humans can build, test, lock, review, and target across the same architecture
matrix.

## Scope

Stable Stage 23 behavior:

- `fgx new` creates a package skeleton.
- `Forgentex.toml` declares package metadata, local path dependencies, target
  matrices, and build profiles.
- `fgx lock` emits a deterministic lock report for the local dependency graph.
- `fgx test` checks each package across one target or the full matrix.
- `fgx build` can emit target-specific C backend artifacts.
- `fgx publish --dry-run` validates publish readiness without uploading.

Deferred behavior:

- remote registry resolution
- semantic version solving beyond recorded local dependency versions
- real cryptographic package metadata beyond Stage 24 mock signatures
- build scripts and arbitrary package hooks
- binary artifact caching
- native code generation beyond the C reference backend

## Package Layout

A package root contains a `Forgentex.toml` manifest and a source directory:

```text
my-package/
  Forgentex.toml
  src/
    main.fgx
```

Create this shape with:

```powershell
fgx new examples/packages/my_package --name my-package
```

Generated packages default to a three-target matrix:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `riscv64gc-unknown-linux-gnu`

## Manifest Format

Minimal manifest:

```toml
[package]
name = "codec-app"
version = "0.1.0"
root = "src"
```

Local path dependency:

```toml
[dependencies]
codec_common = { path = "../codec_common", version = "0.1.0" }
```

The short dependency form is also accepted:

```toml
[dependencies]
codec_common = "../codec_common"
```

Target matrix:

```toml
[targets]
matrix = [
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "riscv64gc-unknown-linux-gnu",
]
```

Build profile:

```toml
[profile.dev]
backend = "c"
optimize = "none"
debug = true
```

An executable file-tool package may additionally declare `[application]`,
`[application.limits]`, `[application.artifact.<name>]`, and scoped
`[permissions]`. `fgx app-run` uses those sections as runtime policy rather
than accepting broader CLI grants. See
[the application runtime contract](application_runtime.md).

`max_read_bytes` and `max_write_bytes` bound cumulative file payloads.
Applications using `core_io_list_dir` must also opt into a positive cumulative
`max_directory_entries`; normalized paths returned by a snapshot consume the
read-byte budget.
Artifact declarations add a narrower portable output root, media type,
per-artifact byte ceiling, required flag, and create-new publication mode.

Only local path dependencies are resolved in Stage 23. The dependency version is
recorded for review and lock output, but it is not a solver constraint yet.

## Lockfiles

`fgx lock` walks the manifest and local path dependencies, then emits a stable
`forgentex.lock.v1` package list with package names, versions, paths, manifest
checksums, dependencies, and targets.

```powershell
fgx lock examples/packages/codec_app --json
fgx lock examples/packages/codec_app --out examples/packages/codec_app/Forgentex.lock
```

Stage 23 lock output is a reproducibility report. It is deterministic and
reviewable, but it does not yet pin registry package hashes because remote
packages are intentionally out of scope.

## Tests

Package tests combine project graph validation, type checking, target checking,
and SIMD fallback checking.

```powershell
fgx test --project examples/packages/codec_app --all-targets --json
fgx test --project examples/packages/codec_app --target aarch64-unknown-linux-gnu
```

When no target is supplied, the first manifest target is tested. `--all-targets`
uses the manifest matrix, or the default x86_64/AArch64/RISC-V matrix when the
manifest omits `[targets]`.

## Builds

The C backend remains the reference build backend, and Stage 28 adds Zig source
artifacts as another available backend. Stage 23 adds matrix-aware artifact
directories:

```powershell
fgx build examples/packages/codec_app --backend c --all-targets --out build/stage23 --json
fgx build examples/packages/codec_app --backend c --target riscv64gc-unknown-linux-gnu --profile dev
fgx build examples/packages/codec_app --backend zig --all-targets --out build/stage28-zig --json
```

Artifacts are written below:

```text
<out>/<profile>/<target>/
```

For example:

```text
build/stage23/dev/aarch64-unknown-linux-gnu/
```

Before building, `fgx build` runs the same package test checks for the selected
targets. A failing package graph stops the build.

## Publish Dry Run

Publishing is intentionally review-only in Stage 23:

```powershell
fgx publish examples/packages/codec_app --dry-run --json
```

The dry run emits `forgentex.publish_dry_run.v1` with package name, version,
source files, package-test status, lock graph size, Stage 24 package
verification status, and a note that registry upload is deferred.

## Example Workspace

The repository includes a two-package workspace:

```text
examples/packages/
  codec_common/
  codec_app/
```

`codec_app` depends on `codec_common` through a local path dependency. The app
can be locked, tested, and built across x86_64, AArch64, and RISC-V:

```powershell
fgx lock examples/packages/codec_app --json
fgx test --project examples/packages/codec_app --all-targets --json
fgx build examples/packages/codec_app --backend c --all-targets --out build/stage23 --json
fgx build examples/packages/codec_app --backend zig --all-targets --out build/stage28-zig --json
fgx publish examples/packages/codec_app --dry-run --json
```

## Compatibility Notes

Stage 23 package support is intentionally conservative:

- package dependencies are loaded before standard library modules
- public API summaries skip dependency and standard library modules
- build outputs expose only the root package public symbols
- generated project maps label modules with `package=<name>`
- dependency modules are marked with `source=dependency`
- target-specific artifacts are grouped by profile and target triple

This keeps the first package manager small enough for deterministic tests while
leaving room for signed packages, registries, and native artifact caching later.
