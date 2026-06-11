# Forgentex Module System v0

Stage 8 introduces a small project layer for multi-file demos. It is not a full
package manager yet; it is a manifest-backed module graph that lets the CLI
parse, check, map, and summarize a group of `.fgx` / `.fgai` files together.

## Manifest

Projects use a root `Forgentex.toml`:

```toml
[package]
name = "forgentex-demo-media"
version = "0.1.0"
root = "examples/project"
```

Supported keys:

- `name`: package name for API and map output.
- `version`: package version for API and map output.
- `root`: source directory, relative to the manifest.

The CLI searches for `Forgentex.toml` from the requested project directory
upward.

## Human Syntax

Human `.fgx` files may start with module metadata:

```text
module media.audio
import media.types

export fn mix_samples(dst: &mut [i16], a: &[i16], b: &[i16], gain: f32) -> void:
    require dst.len == a.len
```

Supported metadata:

- `module <name>` declares the module name.
- `import <name>` adds an import graph edge.
- `export struct ...` marks a struct as public.
- `export fn ...` or `export unsafe fn ...` marks a function as public.
- `pub` is accepted as an alias for `export`.

The core AST still models declarations, statements, and expressions. Module and
visibility metadata live in the project layer for v0, which keeps single-file
roundtrips stable.

## Agent Syntax

The parser also accepts compact markers for future agent views:

```text
M media.audio
I media.types
EXPORT F mix_samples(dst &mut[i16], a &[i16], b &[i16], gain f32) -> void {
}
```

Supported markers:

- `M <name>`
- `I <name>` or `IMPORT <name>`
- `EXPORT` / `PUB` before `S`, `F`, `UF`, or `unsafe F`

## Project Checking

`fgx check --project --json` loads the manifest project, parses each preferred
source view, validates the import graph, then type-checks a combined program.

Project diagnostics include:

- `duplicate_module`
- `unresolved_import`
- `import_cycle`
- `duplicate_symbol`

The v0 type checker still uses a flat symbol table after module graph
validation, so duplicate function or struct names across modules are rejected.
Full namespaced resolution is left for later stages.

## Public API

`fgx api --public` emits exported symbols only:

```text
API forgentex-demo-media 0.1.0
M media.audio examples/project/media/audio.fgx
  fn mix_samples @function.mix_samples (dst:&mut[i16],a:&[i16],b:&[i16],gain:f32) -> void
```

This is the Stage 8 review surface for public API drift. Semantic patch review
reports already expose a `public_api` change category; project API summaries
make that category inspectable at package scale.

## Project Map

`fgx map --project --out examples/project.fgmap` emits modules, imports, export
counts, source hashes, token estimates, functions, structs, unsafe markers,
assembly markers, and effects. `--include-targets` also adds target metadata
labels such as `cpu.avx2` and `target.x86_64`. Source hashes are demo conflict
aids and incremental-parse hints, not cryptographic signatures.

## Limitations

- No package registry or external dependencies.
- No visibility enforcement inside the type checker yet.
- Imports validate module graph structure but do not limit name lookup.
- Namespaced symbol resolution is deferred; names must be unique across a
  project in v0.
