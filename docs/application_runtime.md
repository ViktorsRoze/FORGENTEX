# Manifest-backed application runtime (v0.1)

`fgx app-run <project>` is the package-level application shell. It runs either
a bounded hosted file function or a source-declared governed pipeline. The
package manifest, not CLI flags, fixes the entry point, runtime, effect grants,
filesystem roots, cumulative budgets, and published artifacts.

## Manifest contract

```toml
[package]
name = "scoped-copy"
version = "0.1.0"
root = "src"

[application]
forgentex = "0.1"
entry = "copy_binary"
runtime = "hosted_io"
effects = ["fs.read", "fs.write"]

[application.limits]
max_read_bytes = 1048576
max_write_bytes = 1048576
# Required only when core_io_list_dir is used.
max_directory_entries = 1024

[application.artifact.copy]
root = "output"
media_type = "application/octet-stream"
max_bytes = 1048576
required = true
publication = "create_new"

[permissions]
fs.read = ["input"]
fs.write = ["output"]
```

Every application must declare one exact Forgentex compatibility line as
`forgentex = "MAJOR.MINOR"`. Operators, wildcards, leading zeroes, and patch
versions are rejected rather than being interpreted as an implicit semver
policy. `app-run` compares the declaration with the running toolchain and emits
`application_version_incompatible` before project checking, IR generation, or
filesystem dispatch when they differ.

The `hosted_io` runtime maps each repeated `--arg`
positionally to one entry parameter and is decoded from that parameter's
declared type before IR generation or dispatch. Supported CLI types are `bool`,
`str`, portable `Path`, every integer primitive, `f32`, and `f64`. Booleans are
lowercase `true`/`false`; integers are decimal and range-checked; floats must be
finite, with `f32` narrowed before execution. Because the interpreter's integer
wire value is `i64`, `u64` and `usize` CLI values currently stop at
`i64::MAX`. Aggregate, collection, reference, pointer, and user-defined struct
arguments fail with `application_argument_type_unsupported` instead of
guessing a text encoding. Bad scalar/path syntax uses
`application_argument_invalid`; a representable-type overflow uses
`application_argument_out_of_range`.

The entry's declared effects must be included in `[application].effects`. This
slice accepts only `fs.read` and `fs.write`, and each declared effect requires
both a permission root and its corresponding positive byte limit.

Byte limits are capped at 1 GiB each. `max_directory_entries` is capped at
1,000,000. All three limits are cumulative for one run. A copy charges the same
byte count to both read and write budgets. Binary reads return an owned
`Vec[u8]`; their allocation is bounded by the remaining read budget before the
runtime reads the complete file. Applications that do not enumerate
directories do not need to declare an entry limit.

Each `[application.artifact.<name>]` declares one logical published output.
Names start with an ASCII letter and contain at most 64 ASCII letters, digits,
`_`, or `-`. `root` is a portable path that must exist and resolve beneath an
`fs.write` permission root. `media_type` is a lowercase ASCII `type/subtype`
token. `max_bytes` is positive, at most 1 GiB, and no greater than the run-wide
write ceiling. `required` defaults to `true`; `publication` defaults to and
currently accepts only `create_new`.

## Pipeline application contract

The `pipeline` runtime connects a package application directly to the existing
governed native pipeline engine:

```toml
[application]
forgentex = "0.1"
entry = "visualizer"
runtime = "pipeline"
effects = ["alloc", "cpu", "cpu.render", "fs.write", "media.encode"]

[application.limits]
max_write_bytes = 32768

[application.artifact.preview]
root = "output"
media_type = "image/x-portable-pixmap"
max_bytes = 32768
required = true
publication = "create_new"

[application.pipeline]
schedule = "source"
inputs = ["preview_seed"]
output_share = "encoded_preview"
output_path = "output/visualizer.ppm"

[application.pipeline.limits]
max_stages = 3
max_wall_ms = 5000
max_cpu_ms = 2500
max_memory_bytes = 67108864
max_owned_bytes = 65536

[permissions]
fs.write = ["output"]
```

`entry` must name the project's sole source-authored `pipeline` declaration;
the runtime will not synthesize a schedule from file order. Each positional
argument maps to the corresponding shared value in `inputs`. The final
`output_share` must be an owned `Vec[u8]` produced by a stage with governed
native artifact evidence. Exactly one application artifact is required. Its
media type, byte count, and SHA-256 must match the native receipt before the
host atomically publishes the bytes at `output_path`.

All five pipeline aggregate limits are required. Stage, wall, CPU, memory, and
owned-byte ceilings complement the per-capsule limits and native process
containment. The declared application effects are the stage-effect allow list.
Host publication separately requires `fs.write`, a permission root, an
application write budget, and a create-new artifact declaration.

An encoder or image renderer delegated to application publication declares
`sandbox "artifact.publisher host"` and does not receive `fs.write` authority
or an `artifact.path`. The alternative is native publication through a
governed relative `artifact.path`, matching `fs.write` effect and sandbox
scope. Mixing host publication with a path or capsule filesystem authority is
rejected. This prevents output paths from accidentally resolving against the
shell's current working directory.

## Portable paths

`core.path` exposes the statically distinct, immutable `Path` type:

- `core_path_parse(value: str) -> Path`;
- `core_path_join(base: Path, child: str) -> Path`;
- `core_path_parent(path: Path) -> Option[Path]`;
- `core_path_file_name(path: Path) -> str`;
- `core_path_extension(path: Path) -> str`;
- `core_path_to_str(path: Path) -> str`;
- `core_path_starts_with(path: Path, base: Path) -> bool`.

A portable path is non-empty, relative, uses `/`, and is already normalized.
Empty, `.`, and `..` components, backslashes, control characters, Windows
reserved characters/names, trailing dots/spaces, components over 255 UTF-8
bytes, and complete paths over 4,096 UTF-8 bytes are rejected. Parsing does not
silently rewrite ambiguous input.

## Hosted file operations

The `hosted_io` runtime exposes:

- `core_io_read_file(path) -> str` for bounded UTF-8 text;
- `core_io_read_bytes(path) -> Vec[u8]` for bounded arbitrary bytes;
- `core_io_write_file(path, contents) -> usize` for the original direct text
  write;
- `core_io_write_file_atomic(path, contents) -> usize` for new text files;
- `core_io_write_bytes_atomic(path, bytes) -> usize` for new binary files;
- `core_io_publish_text_artifact(name, path: Path, contents) -> usize`;
- `core_io_publish_bytes_artifact(name, path: Path, bytes) -> usize`;
- `core_io_copy_file(source, destination) -> usize` for the original direct
  copy;
- `core_io_copy_file_atomic(source, destination) -> usize` for a bounded
  no-clobber copy;
- `core_io_list_dir(path: Path) -> Vec[DirEntry]` for one non-recursive,
  collected directory listing with deterministic output order.

### Admitted effects

`hosted_io` admits exactly `fs.read`, `fs.write`, and `alloc`. `alloc` is a
pure in-process memory effect that grants no ambient authority; it is required
for an entry to construct an owned buffer, so without it a hosted application
could only forward bytes it had already read and could never synthesize an
artifact of its own. Native-execution effects such as `cpu` remain rejected at
manifest load, because this runtime cannot govern a native process tree — use
the `pipeline` runtime for native stages.

An owned `Vec[T]` reads as an immutable `&[T]`, so a payload from
`core_io_read_bytes` can be lent to helper functions and to the reviewed
native boundary without being copied or moved, and `bytes[start .. stop]`
projects a bounded window of it. Lending is shared and read-only: passing the
buffer by value still moves it, and `&mut [T]` is not produced from an owned
vector. A helper cannot take `Vec[T]` by mutable reference, so code that grows
a buffer stays inline. See `examples/apps/wav_waveform` for a worked example.

`DirEntry` contains `path: Path`, `kind: str`, and `size: usize`. Kind is
`file`, `directory`, `symlink`, or `other`; size is populated only for regular
files. Results contain full package-relative paths and are sorted by their
portable UTF-8 path representation. The same stable directory state therefore
produces the same result order across hosts.

The checker assigns exact argument and result types to this intrinsic family
and infers `fs.read` and/or `fs.write` from each call.

Artifact publication is intentionally explicit. The runtime resolves `name`
against the manifest, enforces the artifact's narrower root and byte ceiling,
and permits that logical name to publish once. It then performs the same
no-clobber atomic create-new operation and records the exact path, media type,
byte count, SHA-256, and publication mode. Ordinary write/copy intrinsics remain
available but never produce artifact receipts.

## Serialization available to applications

`core.serialize` provides pure, bounded canonical JSON plus UTF-8 and
little-endian binary helpers. The canonicalizer sorts object keys, emits
compact JSON, normalizes through the current `i64`/`f64` model, and rejects
duplicates, malformed numbers/Unicode, trailing commas, excessive nesting, and
values above 16 MiB. UTF-8 encode/decode, `u16`/`u32`/`i32` little-endian
encode/read, and bounded byte concatenation are sufficient to begin a small
versioned archive header without adding a host codec.

These functions declare no effect because they are deterministic memory
transforms. Only the surrounding file operations need manifest permissions.
This profile is not advertised as RFC 8785/JCS, and applications must still
place their own schema/format version in serialized values.

## Filesystem boundary

Runtime paths must be package-relative, non-empty, normalized paths without
`.` or `..`. Absolute paths are rejected.

Before an operation, Forgentex:

1. canonicalizes the package root and each declared permission root;
2. rejects a permission root that is absolute, missing, or resolves outside the
   package;
3. resolves the requested read path, or the parent of a not-yet-created write
   path;
4. follows filesystem links during canonicalization and rejects a resolved path
   outside its permission roots;
5. checks artifact-specific and cumulative byte/directory-entry budgets before
   returning data, copying, or writing;
6. records the allow, deny, budget-exhausted, or error decision.

Directory listing is all-or-error: it returns no partial vector and consumes no
budget if an entry-count/path-byte limit, unreadable entry, or unportable host
name is encountered. Successfully returned path strings consume the read-byte
budget, and returned entries consume `max_directory_entries`. Symlinks are
reported as symlinks and are not followed while collecting entry metadata.

The atomic writers create and fully write a temporary file in the destination
directory, sync its contents, then publish it with an atomic hard-link
operation. The destination therefore appears complete or remains absent. They
never replace an existing path, including under a concurrent create race, and
remove the temporary name after success or failure. This is atomic no-clobber
publication, not an overwrite transaction and not a power-loss durability
claim for the containing directory.

The application cannot replace these rules with broader command-line grants.
`fgx app-run` also runs the existing package supply-chain audit before loading
or executing code.

## Running the example

The graphics/media demo runs a three-stage source graph—scene planning,
contained CPU rendering, and PPM encoding—and publishes one 64×36 image:

```powershell
fgx app-run examples/apps/media_visualizer --arg 7 --json
start examples\apps\media_visualizer\output\visualizer.ppm
```

Publication is intentionally no-clobber. Remove or rename the generated PPM
before running the same command again.

The hosted binary-copy example remains available:

```powershell
fgx app-run examples/apps/scoped_copy `
  --arg input/hello.txt `
  --arg output/copied.txt `
  --json
```

The checked-in example reads an arbitrary binary file from `input` into a
bounded `Vec[u8]`, parses the requested destination as `Path`, then publishes
the declared `copy` artifact beneath `output`. A source
outside `input`, a destination outside `output`, parent traversal, an absolute
path, a byte-budget overrun, or an existing destination is rejected without
partially changing the destination.

The bounded directory example is:

```powershell
fgx app-run examples/apps/directory_inventory `
  --arg input `
  --json
```

It returns `2` for the checked-in input and reports two directory entries plus
the exact normalized-path bytes charged to its read budget.

## Report

Every completed `hosted_io` dispatch produces
`forgentex.application_run.v1` with:

- package, version, entry, and runtime;
- required and running Forgentex versions plus `compatible` confirmation;
- declared effects and relative permission roots;
- requested read/write byte ceilings and directory-entry ceiling;
- artifact declarations with logical name, root, media type, maximum bytes,
  required flag, and publication mode;
- operation transcript and decisions;
- successfully consumed read/write bytes and directory entries;
- successfully published artifact path, exact size, SHA-256, media type, and
  publication mode;
- exactly one of a typed `result` object or structured `error` object;
- SHA-256 of the exact `Forgentex.toml`;
- final `ok`, `denied`, `budget_exhausted`, or `error` status.

Successful results have the stable shape `{"type": "<declared type>",
"value": <JSON scalar or null>}`. Supported application result types are
`bool`, `str`, `Path`, every interpreter-supported integer primitive, finite
`f32`/`f64`, and `void`. Aggregate and other unsupported return types are
rejected before dispatch instead of being debug-formatted into an unstable
string.

Errors have the stable shape `{"code": "...", "phase": "...",
"message": "..."}`. Completed dispatches use this object inside the v1 report
and set `result` to `null`; successful reports set `error` to `null`.
Pre-dispatch failures cannot truthfully produce a run receipt, so JSON mode
emits `forgentex.application_error.v1` with the same error object and exits
non-zero. Phases distinguish security, manifest, compatibility, project,
arguments, contract, policy, IR, runtime, I/O, result conversion, and
postflight failures.

This canonical v1 shape includes compatibility evidence, typed results,
structured errors, artifact declarations, and exact artifact receipts.
Earlier in-repository report drafts were never public compatibility contracts
and had no external consumers. They are retired without legacy readers;
reports generated before this consolidation are disposable development
artifacts.

Pipeline applications instead produce
`forgentex.pipeline_application_run.v1`. It records the package and
compatibility identity, source-declared graph and aggregate-budget receipt,
input/output shares, effect allow list, native stage/render/encode evidence,
host publication operation, exact artifact receipt, manifest SHA-256, and
structured application error. A failed pipeline or denied publication exits
non-zero while retaining the completed pipeline evidence.

A required artifact that was not published changes an otherwise successful run
to `error` with `artifact_required_missing`. An undeclared name, a path outside
the artifact root, a second publication under the same name, or an unsupported
publication mode is rejected. A per-artifact size failure happens before
filesystem publication and before write-budget commitment. Artifact receipts
are retained if a later operation traps; application runs are not whole-run
filesystem transactions.

## Deliberate limits

This is an application-shell foothold, not the completed real-app platform:

- only synchronous `hosted_io` and source-declared `pipeline` runtimes are
  accepted;
- entry arguments are positional, type-directed scalar/path values; aggregate
  CLI encodings and named options are not present;
- entry results are stable scalar/path/void values; aggregate result transport
  is not present;
- current `core.io` operations cover bounded text/binary reads, direct legacy
  writes/copies, no-clobber atomic text/binary publication, and non-recursive
  directory snapshots; recursive walking, general metadata queries, streaming,
  atomic replacement, and directory durability are not present;
- directory listings are sorted collections, not filesystem snapshot
  isolation; a directory concurrently mutated by another process may produce
  an error or reflect the entries observed during that enumeration;
- containment is canonical-path checked before each operation, but the host
  does not yet use descriptor-relative `openat`/reparse-point traversal; a
  hostile process concurrently replacing path components remains a sandbox
  hardening gap;
- atomic publication requires a host filesystem that supports same-directory
  hard links;
- pipeline applications dispatch existing governed native capsules; managed
  worker and network application adapters are not present;
- writes are individually governed but a whole application run is not a
  filesystem transaction;
- artifact declarations cover create-new files only; there are no artifact
  groups, directory artifacts, replacement/update modes, signing, install
  bundle, configuration, secret store, migration system, or updater;
- persistent cache selection, resume, async I/O, active cancellation, and
  parallel application scheduling remain separate gaps.

The `forgepack` v1 header/manifest contract is frozen in
`docs/forgepack_v1.md` and backed by a strict reference codec, byte-identical
golden archive, and package-level deterministic `build`, structured `inspect`,
strict `verify`, and confined `extract` commands. Build source/output paths are
confined to the build-spec directory and output publication is atomic
create-new. Inspection exposes all validated metadata without claiming entry
hashes were recomputed. Extraction verifies first, treats an existing explicit
directory as its output grant, preflights all collisions/symlinks, and
atomically creates each file without replacement. `fgx forgepack evidence
--strict --json` now exercises the complete success path, reproducible rebuild,
no-clobber denial, and corruption denial from self-materialized fixtures. The
clean-install script and release-artifact CI job invoke that evidence through a
copied `fgx` binary; only a published green CI artifact remains an external
operational fact.
