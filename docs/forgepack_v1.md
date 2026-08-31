# `forgepack` archive format v1

**Status:** package-level v1 workflow shipped, 2026-07-25. The frozen codec,
golden archive, deterministic builder, structured inspector, strict verifier,
confined extractor, and versioned command receipts are implemented.

`forgepack` is a deterministic, bounded archive for portable logical paths and
opaque file payloads. Version 1 has no compression, encryption, signatures,
symlinks, streaming records, sparse ranges, or update mode.

## File layout

All integers are unsigned little-endian. An archive is exactly:

```text
offset  bytes  field
0       8      magic: 46 47 58 50 41 43 4b 00 ("FGXPACK\0")
8       2      format_version: 1
10      2      flags: 0
12      4      entry_count
16      4      manifest_bytes
20      4      payload_bytes
24      M      canonical UTF-8 manifest
24+M    P      concatenated entry payloads
```

The file must end after the declared payload. Unknown versions or flag bits,
short data, trailing data, and arithmetic overflow are errors. The fixed
header is 24 bytes.

## Exact bounds

- entries: at most 4,096;
- each portable logical path: at most 4,096 UTF-8 bytes, with components at
  most 255 UTF-8 bytes;
- all logical paths combined: at most 1,048,576 UTF-8 bytes;
- manifest: at most 4,194,304 bytes;
- payload: at most 1,073,741,824 bytes;
- media type: at most 127 ASCII bytes.

Paths use the same strict `core.path` contract as the application runtime:
relative, non-empty, slash-separated, and already normalized. Empty, `.`, and
`..` components, backslashes, control characters, Windows-reserved characters
or names, trailing dots/spaces, and absolute paths are rejected.

## Manifest

The manifest is canonical JSON under `core.serialize`. Its exact schema name is
`forgentex.forgepack_manifest.v1`:

```json
{"entries":[{"length":23,"media_type":"text/plain","offset":0,"path":"docs/readme.txt","sha256":"..."}],"schema":"forgentex.forgepack_manifest.v1"}
```

The root contains exactly `entries` and `schema`. Each entry contains exactly
`length`, `media_type`, `offset`, `path`, and `sha256`. Unknown or missing
fields are errors.

Entries are in ascending order of their path UTF-8 bytes. Paths are unique.
Offsets are zero-based within the payload and must describe a contiguous,
gap-free layout in manifest order: the first offset is zero, each next offset
equals the previous offset plus length, and the last range ends exactly at the
declared payload length. Empty files are allowed.

`media_type` is a lowercase ASCII `type/subtype` token. `sha256` is exactly 64
lowercase hexadecimal digits and hashes only that entry's payload bytes.

## Validation order and trust

Inspection validates the header, total length, UTF-8, canonical JSON, exact
schema, field types, bounds, path/media/hash syntax, ordering, uniqueness, and
payload ranges. Verification runs the same inspection parser and additionally
recomputes every entry SHA-256. Consumers must not trust paths, ranges, or
hashes before these checks.

File paths may not be ancestors of other file paths: an archive cannot contain
both `folder` and `folder/file.txt`. This is rejected during both build and
inspection because it cannot map to a portable file tree.

## Build specification and commands

`fgx forgepack build <spec> <output> [--json]` consumes
`forgentex.forgepack_build_spec.v1`:

```json
{"entries":[{"media_type":"text/plain","path":"docs/readme.txt","source":"input/readme.txt"}],"schema":"forgentex.forgepack_build_spec.v1"}
```

The root and entry objects are exact: unknown or missing fields are errors.
`source` and the output argument are strict portable paths relative to the
build-spec directory. Existing sources and the output parent are canonicalized
through the host filesystem and must resolve beneath that directory, including
through symlinks. Each source is size-admitted before and after its bounded
read. The output is published atomically in create-new mode and is never
replaced.

Input entry order is irrelevant. The builder sorts by logical `path`, rejects
duplicates, creates the canonical archive manifest, and emits
`forgentex.forgepack_build.v1` with entry/manifest/payload/archive byte counts
and the exact archive SHA-256. Repeated builds from stable inputs are
byte-identical.

`fgx forgepack verify <archive> [--json]` performs a bounded read, runs the
shared strict parser, recomputes every entry hash, and emits
`forgentex.forgepack_verify.v1` with the same byte counts, archive SHA-256, and
the number of verified entry hashes. A verification failure exits non-zero and
does not modify the archive.

`fgx forgepack inspect <archive> [--json]` performs the same bounded structural
parse and emits `forgentex.forgepack_inspect.v1`. The report includes version,
flags, every path/media type/offset/length/declared hash, byte counts, and the
whole-archive SHA-256. It explicitly emits `structure_valid=true` and
`hashes_verified=false`: inspection validates hash syntax and payload ranges
but does not recompute entry hashes. Payload-only corruption can therefore be
inspected for diagnosis while `verify` rejects it. Header, manifest, ordering,
or layout corruption is rejected by both commands.

`fgx forgepack extract <archive> <directory> [--json]` treats the explicitly
named, existing directory as the output grant. It verifies the entire archive
and every payload hash before inspecting or changing that root. It then
preflights every destination: existing files, prefix conflicts, non-directory
parents, and symbolic-link components reject the whole request before the
first file is published.

Missing ordinary directories are created beneath the canonical output root.
Immediately before each file publication, the resolved parent is checked
again, and the file uses atomic create-new publication. The command emits
`forgentex.forgepack_extract.v1` with the archive hash and every output
path/size/hash. `publication` is `create_new` and `transactional` is explicitly
`false`: each successful file is complete and never replaces an existing path,
but a later host I/O failure can leave earlier files and newly created
directories in place. As with application I/O, descriptor-relative traversal
is not yet used, so a hostile concurrent process replacing path components
remains a filesystem-hardening gap.

## Workflow evidence and clean installation

`fgx forgepack evidence [--json] [--strict]` creates a unique temporary
workspace and exercises the public command implementation without depending on
the source tree or checked-in fixtures. It materializes text and binary inputs,
builds the same archive twice, requires identical bytes and hashes, inspects
and strictly verifies it, extracts and compares every output byte, then proves
create-new denial and payload-corruption denial. The workspace-cleanup result
is also reported.

The machine report uses `forgentex.forgepack_evidence.v1` and contains ten
named pass/fail checks. `--strict` exits non-zero if any check fails. This is
operational evidence for the experimental `forgepack` workflow; it is not one
of the frozen public 0.1 compatibility schemas.

From a release build, the repository-level clean-install proof is:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File `
  .\scripts\forgepack-clean-install.ps1 -FgxPath .\target\release\fgx.exe
```

The script copies only the selected `fgx` executable into a fresh temporary
directory and invokes the strict evidence command through that copy. The
`release-artifacts` CI job performs the same invocation against each packaged
binary before artifact publication.

## Golden archive

[`examples/apps/forgepack/golden/two_entries.fpk.hex`](../examples/apps/forgepack/golden/two_entries.fpk.hex)
is the lowercase hexadecimal encoding of the canonical two-entry archive:

- `docs/readme.txt` contains `Forgentex forgepack v1\n`;
- `images/pixel.bin` contains bytes `00 01 02 03 ff`.

Tests decode that checked-in byte fixture, require a byte-identical rebuild,
verify both entries, and reject magic, flags, count, canonical-JSON, offset, and
payload corruption.
