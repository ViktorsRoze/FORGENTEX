# Deterministic pipeline runtime (canonical v1)

`fgx pipeline-run <file>` executes a bounded shared-value pipeline through the
normal live capsule runtime. It is designed to prove audited application
composition before queues, workers, or a graphics job system are introduced.

## Stage contract

Every participating capsule has one or more scalar, restricted `@repr(C)`
struct, or reviewed owned-vector inputs, exactly one output, one ordered
`read`, `move`, or `update` binding per input, and one target binding:

```fgx
bind first_source read
bind second_source@0 update
bind target_value rw
```

Source bindings map positionally to capsule input declarations. Each source share
type must equal its corresponding capsule input type, except that a reviewed
owned `Vec[u8]`, `Vec[i8]`, `Vec[u16]`, `Vec[i16]`, `Vec[u32]`, `Vec[i32]`, or
`Vec[u64]`, `Vec[i64]`, `Vec[isize]`, `Vec[f32]`, or `Vec[f64]` share may feed
a matching immutable slice input. `u64` remains bounded by the signed
interpreter carrier and `isize` uses the host pointer width. Float buffers must
be finite; `f32` values must round-trip exactly at their declared width. The
target share type must equal the capsule output type. A scalar
`share` default initializes a source value, or a run may replace it with
`--set name=value`. A struct or reviewed vector share may omit a default when
an earlier stage writes it; a stage may not read an uninitialized share. The
static shared-namespace validation continues to reject multiple writers.

`read` snapshots without consumption. `move` is an exclusive transactional
consumer: no other stage may read or move that share, and the move source must
be distinct from the `rw` target. The runtime retains every move source through
native execution and all output, cancellation, artifact, cache-identity, and
budget checks. Only a successful target commit removes the move sources and
inserts the output. Rejection, denial, native failure, cancellation, invalid
output, or budget exhaustion leaves every source unchanged.

`update` is the reviewed fixed-length mutation path. Legacy unversioned syntax
is admitted only for one exclusive consumer. Explicit `name@N` bindings instead
retain version `N` and create `N+1`, allowing one deterministic update producer
and any number of immutable readers per version. All source bindings for that
share must use explicit versions once any does; missing, mixed, duplicate, and
overflowing version producers fail during planning. Every update requires a
reviewed numeric `Vec[T]`, a matching `&mut[T]` capsule input, and a distinct
`rw` target. The native
runner returns its already length/type/range-validated copy-back envelope. The
pipeline then validates before/after buffer receipts, output, cancellation,
artifacts, and cumulative bytes before publishing the next version and writing
the target in one commit. All non-commit paths retain every prior version and
the old target. An
update's copy-back bytes count toward `max_owned_bytes` conservatively.

Stages execute in source-declaration order. The runtime snapshots every source
value in binding order, applies the capsule effect gate, invokes the existing
live host with the complete argument list, validates its output, and only then
writes the target. A missing input or failed call commits nothing. It produces
`forgentex.pipeline_run.v1`, including every stage's input, decision, elapsed
native time, output or error, the final shared values, and a
`forgentex.pipeline_budget.v1` receipt. Stage JSON retains the compatibility
`source` field and adds the exact ordered `sources` array for fan-in evidence.
Each stage also records `move_sources`, `update_sources`, and
`handoff_committed`; a true commit means all requested removals/replacements
and target insertion occurred as one runtime decision. `updates` records source
type, source/target version, element count, bytes, encoding, and before/after
SHA-256 without repeating the payload. Final shared entries carry an additive
numeric `version` field; explicit later versions keep their `name@N` spelling.

This canonical v1 shape includes budget, graph, cancellation, ordered fan-in,
and per-stage evidence. Earlier in-repository report drafts were never public
compatibility contracts and had no external consumers. They are retired
without legacy readers; reports generated before this consolidation are
disposable development artifacts.

## Dependency-graph scheduling

Declaration-order execution remains the default. A run may instead request:

```text
--schedule graph
```

The runtime derives a directed graph from existing `bind <share> read|move|update` and
`bind <share> rw` contracts. A stage that consumes another stage's target depends
on that writer, regardless of source declaration order. Root shares require a
default or run override. The scheduler performs a stable topological sort,
using source order only to break ties between ready stages.

An unversioned `rw` producer creates version zero for graph purposes. A
versioned update edge `bind samples@N update` creates a second dependency output
`samples@(N+1)` in addition to its ordinary distinct target, so later updates
and readers resolve the exact snapshot without declaration-order inference.

Graph mode supports bounded immutable fan-out and ordered fan-in: multiple
stages may read the same share, and one stage may wait for multiple initialized
or produced shares before dispatch. The one-writer rule remains unchanged, and
stages still run sequentially. A cycle, missing root, invalid boundary, or
multiple writer is rejected during planning before native dispatch.

Successful graph planning adds `forgentex.pipeline_graph.v1` to the report. It
records stage nodes, dependency edges with their shared values, initialized
roots, shares with multiple readers, and the exact deterministic execution
order. Its receipt records `origin: derived_bindings` and a null declaration.

## Source-declared graph scheduling

An authored graph is a first-class top-level item in both language views:

```fgx
pipeline preview:
    stage produce_right
    stage produce_left
    stage combine
    edge produce_left -> combine via left
    edge produce_right -> combine via right
```

```fgai
PIPELINE preview
STAGE produce_right
STAGE produce_left
STAGE combine
EDGE produce_left -> combine VIA left
EDGE produce_right -> combine VIA right
ENDPIPELINE
```

Select it with `--schedule source`. This first slice requires exactly one
pipeline declaration in the source. The runtime requires its stage set and
edge set (including each handoff share) to equal the binding-derived graph
exactly. Missing/extra stages, missing/extra/duplicate edges, self-edges,
unknown nodes, missing roots, and cycles are rejected before dispatch. Capsule
bindings therefore remain the typed authority; the source graph is an explicit,
reviewable manifest and cannot invent or hide dataflow.

The authored stage order is the stable tie-breaker among simultaneously ready
nodes. `forgentex.pipeline_graph.v1` records `origin: source_declaration`, the
pipeline declaration name, authored nodes/edges, roots/fan-out, and the actual
execution order. The declaration survives human/agent formatting and canonical
AST serialization.

## Pipeline-wide budgets

Runs may set seven independent limits:

```text
--max-stages <count>
--max-wall-ms <milliseconds>
--max-cpu-ms <milliseconds>
--max-memory-bytes <bytes>
--max-owned-bytes <bytes>
--max-fs-bytes <bytes>
--max-net-bytes <bytes>
```

The stage-count limit is checked before any dispatch. A requested wall budget
requires every participating capsule to declare `limit.wall_ms`; the sum of
those enforced per-stage limits must fit the pipeline maximum before execution
begins. This makes the wall bound reviewable without guessing future runtime
duration.

A requested CPU budget requires every stage to declare an enforceable
`limit.cpu_ms`; the sum of those limits must fit `--max-cpu-ms`. Because stages
execute sequentially, memory admission uses peak rather than sum: every stage
must declare `limit.mem_mb`, and the largest declaration must fit
`--max-memory-bytes`. These are deterministic pre-dispatch admission checks;
the underlying runner enforcement remains aggregate on Windows and inherited
per-process on Unix.

The owned-byte limit covers the cumulative canonical bytes produced in all
eleven reviewed numeric vector layouts. It is checked after a child returns
but before its target share commits. When it is exceeded, the stage event keeps
the buffer receipt as evidence, its output is suppressed, the target remains
unchanged, and the pipeline ends with `budget_exhausted`.

On Windows, `--max-fs-bytes` cumulatively sums the Job Object's read, write,
and other transfer counters for each successful native stage. These counters
conservatively include anonymous runner protocol traffic and executable/runtime
I/O, not only path-based filesystem operations. Each attempted native stage
adds `forgentex.pipeline_stage_io.v1`; overflow preserves that evidence while
suppressing the output, cache identity, and target commit. Pipelines containing
managed stages are rejected before dispatch when this limit is requested.

Ubuntu and macOS currently lack aggregate pipeline process-tree I/O accounting,
so `--max-fs-bytes` fails before dispatch there. `--max-net-bytes` likewise
fails before dispatch on every host because byte-accurate aggregate network
accounting is not yet installed. A requested limit is never silently ignored.

Every report includes requested maxima, planned stages, declared wall/CPU
totals, declared sequential peak memory, attempted stages, observed native
wall time, produced owned bytes, available cumulative filesystem transfers,
and one of
`not_requested`, `within_budget`, `incomplete`, or `exhausted`. Invalid zero or
unreasonably large limits are rejected before dispatch.

## Cancellation

Library callers may pass a cloneable `PipelineCancellationToken`; the runtime
checks it before every stage dispatch and again after a live call returns but
before the target share commits. The CLI exposes a deterministic stage-boundary
control for automation and tests:

```text
--stop-after-stages <count>
```

A cancelled report has status `cancelled` and includes
`forgentex.pipeline_cancellation.v1`, recording the control source, decision,
number of committed stages, and last attempted stage. Previously committed
shared values remain available. A pre-cancelled token dispatches nothing; a
stage-limit cancellation stops before stage N+1.

Cancellation does not terminate a native child that is already executing. Its
existing wall/CPU/memory containment remains authoritative. If an external
token is observed after that child returns, the stage event records the
cancelled error but its output is withheld from shared state.

## Reviewed typed-buffer handoff

A native stage may write one owned `Vec[u8]`, `Vec[i8]`, `Vec[u16]`, `Vec[i16]`,
`Vec[u32]`, `Vec[i32]`, or `Vec[i64]` into a matching shared value. A later
stage may consume it through the matching immutable slice type. The runtime
copies every value out of the producing child, checks its element range and
total byte accounting, owns the resulting vector, and creates a fresh bounded
copy-in view for each consumer. Foreign pointers and allocator ownership never
cross between stages.

Each successful vector producer adds `forgentex.pipeline_buffer.v1` to its
stage event. The receipt records element type, element count, byte count, and a
SHA-256 over the canonical `little_endian_integer_v1` byte encoding. This makes
the typed handoff deterministic and auditable without treating numeric buffers
as file artifacts. Mutable slice consumers remain rejected because pipeline
values are immutable between their one writer and any readers.

## Owned artifact handoff

A native stage may write one governed `Vec[u8]` output into a matching shared
value. A later stage may read that shared value through an immutable `&[u8]`
capsule input; the runtime creates the normal bounded copy-in view, so no
foreign pointer or ownership crosses between children. Mutable `&mut [u8]`
views are rejected because the artifact remains Forgentex-owned and immutable.

The producing stage event embeds its `forgentex.native_artifact.v1` metadata:
media type, byte length, SHA-256, and optional governed persistence path. The
shared-value report contains the owned bytes, while later stages do not acquire
or duplicate the producer's artifact receipt. For `Vec[u8]`, the generic buffer
receipt hashes the same canonical bytes and therefore matches the artifact
SHA-256.

## Governed render profiles

A native image or raw-frame producer opts into one of two render-specific
profiles with:

```fgx
sandbox "render.profile preview"
sandbox "render.seed_field seed"
```

`preview` accepts dimensions through 1920x1080. `final` is a distinct
authorization and receipt identity that accepts dimensions through 3840x2160:

```fgx
sandbox "render.profile final"
sandbox "render.color_space srgb"
sandbox "render.device cpu"
sandbox "render.pass beauty"
sandbox "render.engine forgentex_software_rgb8"
sandbox "render.engine_version 1.0.0"
sandbox "render.sequence_id visualizer_final_v1"
sandbox "render.seed_field seed"
```

`render.color_space` is optional for legacy renderers. The first exact color
contract admits only `srgb`; unsupported names fail capsule validation. For a
raw-RGB handoff, the encoder must either preserve the legacy absence or declare
the same color space, so one side cannot silently add or drop color meaning.
This equality is checked before encoder dispatch.

`render.device` is independently optional and admits `cpu` or `gpu`. When it
is present, the capsule must declare exactly the corresponding `cpu.render` or
`gpu.render` effect and must not also declare the other render effect. This
prevents a receipt from claiming a selected device that disagrees with the
reviewed capability boundary. The contained executable fixture selects `cpu`;
the native capability profile still rejects live GPU access before compiler
lookup, so accepting a structurally valid `gpu` manifest is not a GPU-runtime
claim.

The optional engine identity is another atomic pair. `render.engine` is a
lowercase artifact token of at most 64 characters, and
`render.engine_version` is an exact numeric `major.minor.patch` core version
with no leading-zero or overflow ambiguity. Both declarations are included in
the reviewed sandbox policy and therefore in the native boundary hash; both
also appear in the render receipt. This is deterministic declared
implementation identity; the separately recorded executable hash below proves
the bytes launched, while vendor signing and hardware attestation remain open.

`render.pass` is also optional and selects one bounded RGB8 visualization:
`beauty`, `depth`, `normal`, or `object_id`. A raw-frame encoder must declare
the same value through `encode.input_pass`, or both sides must preserve the
legacy absence. Unknown and one-sided values fail before encoder dispatch.
This is a single display-oriented RGB8 pass contract; it does not claim
multi-pass output, floating-point depth/normals, arbitrary AOVs, or lossless
object-ID transport.

A renderer may additionally opt into a complete selected-frame manifest:

```fgx
sandbox "render.frame_index_field frame_index"
sandbox "render.frame_count_field frame_count"
sandbox "render.frames_per_second 30"
```

The three rules are all-or-nothing. Both named scene fields must be distinct
integers separate from width, height, and seed. Frame indices are zero-based,
must be below a positive frame count capped at 1,000,000, and FPS is limited to
1 through 240. These checks happen before native dispatch. Renderers without
the manifest retain the canonical single-frame identity `frame_index: 0`,
`frame_count: 1`, and `frames_per_second: null`.

A renderer with that complete frame manifest may also declare a bounded
lowercase sequence token:

```fgx
sandbox "render.sequence_id visualizer_final_v1"
```

`render.sequence_id` is optional, limited to the same 1–64 character artifact
token grammar as engine names, and invalid without the selected-frame
manifest. Each successful declared-sequence render receives a deterministic
`frame_cache_key_sha256`. The versioned key binds the sequence and renderer,
canonical scene type/hash, seed, exact frame index/count/FPS, dimensions,
profile, quality/adaptive settings, color/device/pass, engine/version, effects,
native boundary hash, toolchain, compiled runner SHA-256, and raw artifact media
type/path contract. Identical execution identities produce the same key; a
frame-identity change produces a different key. This is lookup identity only:
the runtime does not store, retrieve, skip, schedule, resume, or reuse frames.

A second optional all-or-nothing pair binds authored render quality:

```fgx
sandbox "render.samples_field samples_per_pixel"
sandbox "render.shutter_q16_field shutter_q16"
```

Both named fields must be distinct integers separate from every identity field.
Preview permits 1–64 samples per pixel and final permits 1–4096. Shutter is a
Q16 fraction from 0 through 65536 inclusive, representing closed through one
full frame. These values are resolved from the scene and checked before native
dispatch. Renderers without this pair report both values as `null`.

An adaptive-intent pair may extend that quality manifest:

```fgx
sandbox "render.adaptive_min_samples_field adaptive_min_samples"
sandbox "render.adaptive_noise_q16_field adaptive_noise_q16"
```

The pair is atomic, requires the samples/shutter quality pair, and names two
additional distinct integer scene fields. Minimum samples must be from 1
through the authored samples-per-pixel ceiling. The noise threshold is a Q16
fraction from 1 through 65536 inclusive. Both checks happen before native
dispatch; undeclared adaptive intent remains `null / null`. These fields govern
and audit renderer inputs but do not prove that a foreign renderer implements a
particular convergence algorithm.

The renderer must receive one named scene struct containing integer `width`,
`height`, and the named seed field; return one governed `Vec[u8]`; declare
`cpu.render` or `gpu.render`; and provide wall, CPU, and memory limits. An
`image/*` result must be persisted through `artifact.path` or explicitly
delegated to an application host with `artifact.publisher host`. The exact
`application/x-forgentex-rgb` media type is the bounded in-memory handoff to a
separate encoder. Both profiles reject non-positive or over-profile dimensions
before renderer dispatch. The final profile does not imply GPU execution or a
production video codec; those remain separately governed capabilities.

The raw-RGB wire contract is explicit: an eight-byte header contains
little-endian `u32` width and height, followed by exactly `width * height * 3`
row-major RGB8 bytes. After native return and before shared-state commit, the
host validates the header, checked payload size, and equality with the scene
dimensions. A malformed or mismatched renderer output commits no handoff and
receives no render receipt.

A successful stage adds `forgentex.render_receipt.v1`. It binds the canonical
scene hash and disclosed deterministic seed to the profile, dimensions,
selected frame index/count/FPS, samples per pixel, Q16 shutter, renderer
adaptive minimum/noise settings, color space, selected device, and render pass
plus declared engine name/version (each optional field or pair is `null` when
undeclared), optional sequence ID and frame-cache key, boundary hash, toolchain,
effects, applied budgets, and native image artifact hash/path. Failed, denied,
generic-artifact, and redacted stages never receive a render receipt. This
establishes frame identity, bounded authored/adaptive intent, one exact
transfer/display contract, explicit device intent, one selected RGB8 pass,
declared engine identity, the exact pre-launch-verified renderer executable
SHA-256, and an independently auditable cache lookup key for independently
dispatched work. The `renderer_binary_sha256` is copied from native execution's
verified runner identity and is present for legacy and declared-engine
renderers alike. It is not a publisher signature or hardware/device proof. The
runtime is not yet a sequence scheduler, cache storage/reuse system,
multi-pass/AOV system, color-conversion engine, live GPU path, verified adaptive
renderer, physically based renderer, or animation runtime.

## Governed image encoding

Native PPM and PNG encoders opt into exact encoding-specific contracts with:

```fgx
sandbox "encode.profile ppm"
# artifact.media_type must be image/x-portable-pixmap

sandbox "encode.profile png"
sandbox "encode.color_space srgb"
sandbox "encode.input_pass beauty"
sandbox "encode.engine forgentex_png_rgb8"
sandbox "encode.engine_version 1.0.0"
sandbox "encode.compression stored_deflate"
sandbox "encode.compression_level 0"
sandbox "encode.pixel_format rgb8"
sandbox "encode.interlace none"
sandbox "encode.scanline_filter none"
sandbox "encode.chunk_checksum crc32"
sandbox "encode.stream_checksum adler32"
sandbox "encode.chunk_order ihdr_srgb_idat_iend"
# artifact.media_type must be image/png
```

It must consume exactly one immutable `&[u8]` input from a preceding producer
whose governed artifact media type is `application/x-forgentex-rgb`, return one
owned `Vec[u8]` image, declare `media.encode`, choose an approved
`artifact.path` or explicit `artifact.publisher host`, and provide wall, CPU,
and memory limits. Host publication cannot be combined with `artifact.path`,
an `fs.write` effect, or an `fs.write` sandbox scope. This statically keeps
an arbitrary byte producer from being relabeled as a frame at the encoder edge.
Profile and artifact media type must agree before dispatch.

`encode.engine` and `encode.engine_version` form an optional atomic semantic
identity, parallel to the renderer contract. The engine is a bounded lowercase
artifact token and the version is an exact numeric `major.minor.patch` core;
one-sided declarations, leading-zero ambiguity, and integer overflow fail
capsule validation. Legacy encoders retain `null / null`.

The first compression contract is another optional atomic pair, admitted only
for PNG: `encode.compression stored_deflate` with the exact integer
`encode.compression_level 0`. One-sided declarations, another mode, a nonzero
or ambiguously formatted level, and PPM use fail validation. This pair records
the encoder behavior already verified by independent PNG conformance, which
requires every emitted DEFLATE block to use the stored-block type. It does not
offer fixed/dynamic Huffman compression or general levels 1–9.

PNG output layout is likewise an optional atomic pair:
`encode.pixel_format rgb8` plus `encode.interlace none`. It is PNG-only;
one-sided declarations, `rgba8`, and `adam7` fail validation. Independent IHDR
conformance verifies bit depth 8, truecolor type 2, standard compression/filter
methods, and interlace method 0, so the receipt describes the emitted container
rather than only the raw input. Palettes, alpha, grayscale, higher bit depths,
and Adam7 remain unsupported.

PNG scanline selection is a separate optional PNG-only contract:
`encode.scanline_filter none`. Unsupported strategies such as `adaptive`,
duplicate declarations, and PPM use fail validation. Independent conformance
inflates the image data and verifies that every reconstructed row begins with
filter type 0; this is distinct from IHDR's standard filter-method byte.
Sub, Up, Average, Paeth, and adaptive per-row selection remain unsupported.

PNG integrity is an optional atomic pair: `encode.chunk_checksum crc32` and
`encode.stream_checksum adler32`. Missing halves, alternate algorithms,
duplicate declarations, and PPM use fail validation. Independent conformance
recomputes CRC-32 over each chunk type plus payload and Adler-32 over the full
inflated scanline stream, binding the declarations to emitted bytes. This is
container error detection, not artifact signing or publisher provenance.

The optional PNG-only `encode.chunk_order ihdr_srgb_idat_iend` contract fixes
the complete fixture layout to one IHDR, one sRGB, one IDAT, and one IEND in
that exact order. Alternate values, duplicates, and PPM use fail validation.
Independent parsing now compares the full encountered chunk sequence, so extra
ancillary chunks, split IDAT payloads, reordering, or trailing chunks fail this
fixture's conformance even where a broader PNG decoder might accept them.

`render_encode_png.fgx` is the first standards-based image codec fixture. Its
contained C encoder emits the PNG signature, RGB8 IHDR, CRC-protected chunks, a
`sRGB` chunk with perceptual rendering intent, a zlib stream with deterministic
stored DEFLATE blocks and Adler-32, and IEND. Conformance
independently parses every chunk, verifies both checksum families and the exact
color chunk, inflates the stored blocks, and compares every decoded pixel with
the raw renderer output. This is a deterministic bounded sRGB/RGB8 PNG path,
not a general compression-tuning, color-conversion, wide-gamut, ICC-profile,
animation, or video API.

A successful stage adds `forgentex.encode_receipt.v1`. It binds the exact input
byte count and SHA-256 plus validated `input_width`, `input_height`, and
`input_format: "rgb8"`, color space (or `null`), input pass (or `null`),
declared encoder engine/version (or `null / null`), declared compression
mode/level (or `null / null`), and declared output pixel format/interlace mode
(or `null / null`), scanline filter (or `null`), declared chunk/stream
checksums (or `null / null`), and chunk order (or `null`) alongside the encoder
profile, boundary hash, toolchain, exact
pre-launch-verified `encoder_binary_sha256`, effects, applied budgets, and final
image artifact.
The host repeats raw-frame layout validation before encoder dispatch, so a
generic producer cannot gain trust merely by declaring the raw-RGB media type.
The input hash therefore equals the preceding raw render artifact hash. Failed
or denied stages never receive an encode receipt or publish the final path.

## Deterministic cache identity

Every successful, non-sensitive native stage receives a
`forgentex.pipeline_cache_identity.v1` receipt after output, buffer, artifact,
cancellation, and pipeline-budget checks succeed but before shared-state
commit. Its final SHA-256 binds five independently disclosed component hashes:

- ordered source names, capsule input names/types, and canonical input values;
- native boundary, exact runner artifact, language/version/environment, and
  toolchain identity;
- declared effects, sandbox/dependencies, and effective containment policy;
- native plus pipeline-wide execution limits; and
- target share plus output name/type.

Length-prefixed field encoding prevents delimiter ambiguity. Repeated work on
the same host/toolchain receives the same key even when native duration or
compiled-runner cache status changes. Changing any bound component changes the
key. A stage with an `audit.redact` input publishes `cache_identity: null`,
because an unsalted content hash could confirm guesses about a secret.

The input identity includes each source binding's `read`, `move`, or `update`
mode, and the output contract includes every updated source, preventing reuse
from confusing read-only, consuming, and mutating restoration semantics.

The library API provides a deliberately narrow reuse path through
`PipelineCache` and `run_with_cache_control`. Its bounded memory tier defaults
to 256 entries and 64 MiB; explicit construction remains hard-capped at 1,024
entries and 1 GiB. A lookup request
binds the canonical complete program, exact detected toolchain, ordered source
values, effect grants, pipeline limits, and target contract. On a hit, the
runtime revalidates all six full identity hashes, the output type, exact
canonical output byte count, and SHA-256 before the normal cancellation,
owned-byte budget, and atomic target-commit checks. It emits
`forgentex.pipeline_cache_restore.v1`, sets stage decision `cache_hit`, and
records zero native wall time. First execution records `stored_process_local`;
restoration records `verified_process_local_hit`.

`PipelineCache::persistent` adds a cross-process tier, and the CLI exposes it
as `--cache-dir <dir>`. The path is an explicit grant and must name an existing
non-symlink directory. Each eligible value is stored in a versioned binary
format containing the request hash, six component hashes, output hash, bounded
toolchain identity, strict canonical JSON output, and a SHA-256 digest over the
complete preceding record. Filenames bind the request and record digest.
Outputs are capped at 16 MiB, toolchain identity at 4 KiB, matching-directory
scans at 1,024 entries, and loaded values still pass the same live type, byte,
hash, cancellation, and budget checks as memory hits.

Publication uses a create-new temporary file plus a no-clobber hard link.
Already valid identical content is reused. A corrupt or colliding destination
is never overwritten: it is ignored on lookup and a freshly executed result is
published under a new immutable sibling name. Successful disk restoration
records `verified_integrity_checked_file_hit` and
`storage: integrity_checked_content_addressed_file`; first publication records
`stored_process_local_and_integrity_checked_file`. A directory write failure
does not fail a successful stage—it leaves only the bounded memory entry.

This default format is an integrity boundary, not origin authentication.
SHA-256 detects corruption and binds content inside a caller-trusted cache
root, but a writer that controls the directory can replace a record and
recompute every hash. Callers using this mode must keep the directory private
and trusted.

`PipelineCache::authenticated_persistent` upgrades the directory to a distinct
version-2 HMAC-SHA256 format. The CLI selects it with
`--cache-key-file <file>`. The key file must be a non-symlink regular file
outside the cache directory and contain 32 through 4,096 raw bytes. The HMAC
covers the complete version-2 content plus its SHA-256 digest and is compared
in constant time. Key material appears in neither filenames, serialized
records, receipts, nor cache debug output. A wrong key or modified tag becomes
a miss; authenticated mode refuses version-1 integrity-only records, so key
selection cannot silently downgrade verification.

HMAC provides symmetric group origin authentication: only a key holder can
produce a record accepted by that group, but every verifier holding the key can
also write one. It does not encrypt cached values, identify an individual
signer, or provide read-only public verification. Generate and distribute the
key through an external secure secret-management path, keep it outside the
cache root, and rotate it after any holder is compromised. Asymmetric signed
records remain a future boundary if distinct producer identity or verifier-only
access is required.

Eligibility is intentionally strict: only native stages with immutable `read`
inputs, `cpu`/`alloc` effects, no render/encode policy, no path-bearing artifact
policy, no filesystem or network budget, and no direct or transitive sensitive
input may be stored. A pathless governed `Vec[u8]` artifact is eligible because
its complete immutable payload is already the cached output. On restoration,
the runtime revalidates the vector type/range and current artifact policy, then
rebuilds `forgentex.native_artifact.v1` from the exact byte count, media type,
and SHA-256 before cancellation, owned-byte budget, and target commit. Changing
the media/size policy changes the request and forces native execution.

Effect denial is checked before lookup. Move/update stages, managed runtimes,
render/encode work, `artifact.path` publication, and sensitive derivations
still execute or fail normally and never enter this cache. Corrupt entries
become misses and are replaced only after a fresh successful stage commit.

There is no implicit global cache, public/asymmetrically verified cache import,
filesystem artifact-publication replay, transactional mutation replay, retry,
or whole-run resume yet. Those side-effectful features require explicit replay
rules.

Example:

```text
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --json
mkdir pipeline-cache
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --cache-dir pipeline-cache --json
# cache.key: 32–4096 securely generated raw bytes, stored outside pipeline-cache
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --cache-dir pipeline-cache --cache-key-file cache.key --json
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --schedule graph --json
fgx pipeline-run examples/pipeline/source_graph.fgx --allow-effects cpu --schedule source --json
fgx pipeline-run examples/pipeline/move_handoff.fgx --allow-effects alloc,cpu --schedule graph --json
fgx pipeline-run examples/pipeline/update_handoff.fgx --allow-effects alloc,cpu --schedule graph --json
fgx pipeline-run examples/pipeline/versioned_updates.fgx --allow-effects alloc,cpu --schedule graph --json
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --stop-after-stages 1 --json
fgx pipeline-run examples/pipeline/fan_in.fgx --allow-effects cpu --schedule graph --max-stages 3 --max-wall-ms 3000 --max-cpu-ms 3000 --max-memory-bytes 268435456 --json
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --max-stages 2 --max-wall-ms 2000 --max-owned-bytes 1024 --json
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --max-fs-bytes 1073741824 --json
fgx pipeline-run examples/pipeline/artifact_preview.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/typed_buffers.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/extended_integer_buffers.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/float_buffers.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/wide_integer_buffers.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/render_preview.fgx --allow-effects alloc,cpu,cpu.render,fs.write --json
fgx pipeline-run examples/pipeline/render_encode_preview.fgx --allow-effects alloc,cpu,cpu.render,media.encode,fs.write --json
fgx pipeline-run examples/pipeline/render_encode_final.fgx --allow-effects alloc,cpu,cpu.render,media.encode,fs.write --json
fgx pipeline-run examples/pipeline/render_encode_png.fgx --allow-effects alloc,cpu,cpu.render,media.encode,fs.write --json
```

## Deliberate limits

This version is single-threaded and permits scalar values, restricted by-value
scalar-leaf `@repr(C)` structs, and nine reviewed integer plus two reviewed
floating-point owned-vector layouts. Integer receipts use canonical
little-endian bytes; float receipts use exact little-endian IEEE-754 bits,
including signed zero. `u64` remains carrier-bounded and `isize` remains
host-width. Transactional exclusive move, fixed-length reviewed-vector update,
and explicit sequential `@N` share-version graphs are supported. Multiple
writers of one next version, aggregate element layouts,
active-stage
interruption/resume, asymmetric verifier-only cache or filesystem artifact replay, retries, or
parallel scheduling remain unsupported. Pipeline
budgets currently cover stage count, declared native wall/CPU time, declared
sequential peak memory, reviewed owned-vector bytes, and cumulative Windows Job
filesystem-transfer accounting. Portable filesystem and all network-byte
accounting remain unavailable and fail closed. The PPM preview is a
deterministic software-render proof, not a 3D
or GPU renderer. The encoder is a contained PPM contract proof, not a general
image/video codec runtime. Native scene buffers, production codecs, timelines,
and render queues remain to be added.
