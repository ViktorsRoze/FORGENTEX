# Deterministic pipeline runtime (v1.4)

`fgx pipeline-run <file>` executes a bounded shared-value pipeline through the
normal live capsule runtime. It is designed to prove audited application
composition before queues, workers, or a graphics job system are introduced.

## Stage contract

Every participating capsule has exactly one scalar, restricted `@repr(C)`
struct, or reviewed owned-vector boundary plus these two bindings:

```fgx
bind source_value read
bind target_value rw
```

The source share type must equal the capsule input type, except that a reviewed
owned `Vec[u8]`, `Vec[i16]`, or `Vec[i32]` share may feed a matching immutable
slice input. The target share type must equal the capsule output type. A scalar
`share` default initializes a source value, or a run may replace it with
`--set name=value`. A struct or reviewed vector share may omit a default when
an earlier stage writes it; a stage may not read an uninitialized share. The
static shared-namespace validation continues to reject multiple writers.

Stages execute in source-declaration order. The runtime reads the source value,
applies the capsule effect gate, invokes the existing live host, validates its
output, and only then writes the target. It produces
`forgentex.pipeline_run.v1`, including every stage's input, decision, output or
error, and the final shared values.

## Reviewed typed-buffer handoff

A native stage may write one owned `Vec[u8]`, `Vec[i16]`, or `Vec[i32]` into a
matching shared value. A later stage may consume it through `&[u8]`, `&[i16]`,
or `&[i32]`. The runtime copies every value out of the producing child, checks
its element range and total byte accounting, owns the resulting vector, and
creates a fresh bounded copy-in view for each consumer. Foreign pointers and
allocator ownership never cross between stages.

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

## Governed preview renders

A native image or raw-frame producer may opt into the first render-specific
contract with:

```fgx
sandbox "render.profile preview"
sandbox "render.seed_field seed"
```

The renderer must receive one named scene struct containing integer `width`,
`height`, and the named seed field; return one governed `Vec[u8]`; declare
`cpu.render` or `gpu.render`; and provide wall, CPU, and memory limits. An
`image/*` result must be persisted through `artifact.path`. The exact
`application/x-forgentex-rgb` media type is the bounded in-memory handoff to a
separate encoder. The preview profile rejects non-positive or larger-than-
1920x1080 dimensions before renderer dispatch.

A successful stage adds `forgentex.render_receipt.v1`. It binds the canonical
scene hash and disclosed deterministic seed to the profile, dimensions,
renderer boundary hash, toolchain, effects, applied budgets, and native image
artifact hash/path. Failed, denied, generic-artifact, and redacted stages never
receive a render receipt.

## Governed image encoding

A native PPM encoder opts into the first encoding-specific contract with:

```fgx
sandbox "encode.profile ppm"
```

It must consume exactly one immutable `&[u8]` input from a preceding producer
whose governed artifact media type is `application/x-forgentex-rgb`, return one
owned `Vec[u8]` image, declare `media.encode`, persist beneath an approved
`artifact.path`, and provide wall, CPU, and memory limits. This statically keeps
an arbitrary byte producer from being relabeled as a frame at the encoder edge.

A successful stage adds `forgentex.encode_receipt.v1`. It binds the exact input
byte count and SHA-256 to the encoder profile, boundary hash, toolchain,
effects, applied budgets, and final image artifact. The input hash therefore
equals the preceding raw render artifact hash. Failed or denied stages never
receive an encode receipt or publish the final path.

Example:

```text
fgx pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --json
fgx pipeline-run examples/pipeline/artifact_preview.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/typed_buffers.fgx --allow-effects alloc,cpu --json
fgx pipeline-run examples/pipeline/render_preview.fgx --allow-effects alloc,cpu,cpu.render,fs.write --json
fgx pipeline-run examples/pipeline/render_encode_preview.fgx --allow-effects alloc,cpu,cpu.render,media.encode,fs.write --json
```

## Deliberate limits

This version is single-threaded and permits scalar values, restricted by-value
scalar-field `@repr(C)` structs, and reviewed `u8`/`i16`/`i32` owned vectors. It
has no explicit graph syntax, fan-in/out, mutable buffers, additional element
layouts, cancellation, pipeline-wide resource budgets, cache keys, retries, or
parallel scheduling. The PPM preview is a deterministic software-render proof, not a 3D
or GPU renderer. The encoder is a contained PPM contract proof, not a general
image/video codec runtime. Native scene buffers, production codecs, timelines,
and render queues remain to be added.
