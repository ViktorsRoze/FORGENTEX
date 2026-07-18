# Forgentex Gap-Closure Plan

**Status:** active implementation sequence, based on the verified v3 baseline.
Native-call v1 (C scalars), v1.2 (scalar slices), v1.3 (explicit-C-ABI C++),
v1.4 (explicit-C-ABI Rust scalars), v1.5 (Rust scalar slices), v1.6
(owned C/Rust `Vec[u8]` outputs), v1.7 (Rust `#[repr(C)]` structs), v1.8
(pinned Zig 0.14 numeric scalars), v1.9 (Zig scalar slices), v1.10
(Zig restricted `extern struct` values), v1.11 (Zig owned `Vec[u8]` outputs),
v1.12 (bounded artifact metadata and content receipts), v1.13 (scoped,
immutable artifact persistence), v1.14 (native process-tree containment), and
v1.15 (native CPU/memory budgets), v1.16 (structured native failure receipts),
v1.17 (explicit sensitive-input audit redaction), v1.18 (host-owned native
runner compilation cache), and v1.19 (owned C/Rust `Vec[i16]` outputs) are
implemented through `fgx run --live`; v1.20 adds governed deterministic preview
render receipts and v1.21 adds linked raw-frame/image-encoder receipts to
`fgx pipeline-run`; v1.22 broadens the reviewed native ownership boundary to
C/Rust/Zig `Vec[i16]` and `Vec[i32]` outputs, and v1.23 adds bounded owned UTF-8
`str` outputs across the same languages.
This document turns
the remaining broad roadmap into the shortest credible path from agent-native
governance tooling to one real application.

## Current baseline

Forgentex already has a solid core: dual human/agent syntax, canonical ASTs,
type/effect/borrow checking, semantic patch review, deterministic FGIR,
interpreter execution, C/Zig source emission, hosted synchronous I/O, and live
managed-language capsules. The C-ABI bridge also compiles, links, and executes
C capsules against fixtures.

`fgx run --live` can now call checked native
**scalar/slice/restricted-struct/owned-vector/owned-string** capsules across C/C++/Rust and
pinned Zig 0.14 through generated child runners. The owned vector set is
deliberately narrow: `u8`, `i16`, and `i32` elements plus owned UTF-8 strings
across C/Rust/Zig.
It returns live values and validated
fixed-length mutable copy-back to the interpreter. C++ functions must export
an explicit `extern "C"` boundary; Rust additionally requires a public
no-mangle export. Zig requires `pub export fn`. Warm hosts reuse compiled
runners while retaining fresh bounded processes. The remaining native boundary
is the important one for applications: broader typed outputs, safely budgeted
persistent processes, and GPU dispatch. Those gaps still block the heavy runtimes
needed for graphics, codecs, physics, and real-time audio even though their
contracts can already be reviewed.

## Priority order

| Priority | Gap | Why it blocks progress | Definition of done |
| --- | --- | --- | --- |
| 1 | Live native capsule calls | C/C++/Rust and pinned Zig 0.14 scalar/slice/restricted-struct calls are live, process-tree-contained, and CPU/memory-bounded; warm hosts reuse compiled runners while keeping fresh per-call processes. | Keep every live call gated, process-isolated, and audited; evaluate persistent native workers without weakening per-call budgets and close platform budget gaps. |
| 2 | Native data boundaries | Scalar slices, restricted C/C++/Rust/Zig structs, and owned C/Rust/Zig `u8`/`i16`/`i32` vectors plus UTF-8 strings are live; media still needs broader typed/artifact outputs. | ABI-approved structs and owned buffers cross a native boundary with recorded layout, ownership, length, and lifetime rules. |
| 3 | Live shared data plane and pipelines | A deterministic linear pipeline now moves scalar, struct, byte-artifact, and reviewed typed-vector values, but lacks explicit graphs, fan-in/out, cancellation, budgets, caching, and scheduling. | Generalize the shipped one-writer audited data plane without weakening deterministic tests or stage isolation. |
| 4 | Native fast path for useful app code | Eligible private direct-call generics, coherent generic/blanket impls with associated items, required/default trait dispatch, monomorphic scalar named/effectful function values and indirect calls, contextually typed non-capturing scalar lambdas from annotated locals and reassignments, direct higher-order arguments, or typed `Fn` returns, a bounded borrowed-String subset, and fresh function-local `Vec[i32]` now lower for C/Zig; `Map`, ordinary owned-string operations, broader Vec ownership/layouts, capturing/remaining-position closures, and wider generic forms remain interpreter-only or native-blocked. Runtime-only effects remain explicit function-level blockers. | Expand the deliberately small dynamic runtime without weakening explicit ownership, allocation bounds, cleanup, or FIR blockers. |
| 5 | Safety hardening | The borrow checker is well-tested but still research-grade for production claims. | Written rules, a larger adversarial corpus, property tests, and an independent reference-model comparison gate releases. |
| 6 | Async/concurrency | Current hosted I/O is synchronous, unsuitable for render queues, responsive tools, servers, or streaming. | Structured tasks, cancellation, bounded queues, and effect-aware async I/O have deterministic test-mode behavior. |
| 7 | Domain runtime: graphics/media | A reviewed GPU capsule is not yet a renderer. | A governed graphics runtime loads a validated scene plan, renders a preview, and emits an auditable artifact. |

The ranking is intentional: do not build a full graphics abstraction before
Forgentex can invoke and govern the native renderer that would implement it.

## Progress ledger

Updated as implementation slices land. “Partial” means a bounded, tested
contract exists but the definition of done in the priority table is not yet
met; it must not be read as permission to claim the later capability.

| Gap | Status | Landed evidence | Remaining exit criteria |
| --- | --- | --- | --- |
| Live native calls | Partial | C/C++/Rust and pinned Zig 0.14 scalar/slice/restricted-struct calls; isolated process tree, tree-wide wall timeout/background cleanup, Windows aggregate and Unix inherited CPU/memory limits, crash containment, applied-budget/failure receipts, explicit audit redaction, and host-owned compiled-runner reuse with fresh per-call processes | Persistent native workers, if they can preserve per-call budgets, and aggregate Unix tree budgets |
| Native data boundaries | Partial | Bounded C/C++/Rust/Zig scalar slices and restricted by-value structs; bounded C/Rust/Zig owned `Vec[u8]`, `Vec[i16]`, `Vec[i32]`, and UTF-8 `str` outputs with required typed deallocators and byte-accounted receipts; byte artifacts retain declared media/size/path policy, SHA-256 receipts, and scoped immutable persistence | Nested structs, string inputs, more owned element layouts, and multiple owned outputs |
| Shared data plane/pipelines | Partial | `fgx pipeline-run`: checked scalar/restricted-struct handoff; owned `Vec[u8]`/`Vec[i16]`/`Vec[i32]` values with matching immutable slice consumers and deterministic element/byte/hash receipts; one writer; native artifact plus render/encode receipts; live capsule gate/audit; deterministic single-threaded mode | Explicit graph syntax, fan-in/out, mutable and additional typed buffers, cancellation, pipeline-wide budgets, cache keys, and parallel scheduling |
| Native fast path | Partial | Optimizer, C/Zig source emission, deterministic monomorphization of fully concrete private direct-call generics, coherent bounded generic/blanket impls with methods/constants/types, required/default static trait dispatch, concrete standalone/generic associated-type projections, monomorphic scalar named/effectful function values and indirect calls with runtime-effect contract enforcement, deterministic lifting of contextually typed non-capturing scalar lambdas from annotated locals and reassignments, direct higher-order arguments, and typed `Fn` returns, sub-16-MiB allocation-free borrowed String inspection, and bounded fresh local `Vec[i32]` with explicit cleanup and FIR ownership blockers | Vec signatures/returns/aliases and more element layouts, native Map and owned String operations, exported/ambiguous generics, generic function values, and capturing/remaining-position native closures |
| Safety hardening | Partial | Borrow checks, deterministic traps, 30 curated conformance cases, four persisted minimal regressions, 510 independently modeled move programs, 277 alias/reborrow programs (including constant/dynamic index pairs and conditional joins inside loops), 132 lifetime-provenance programs (including field/index projections and self/mutual recursion), 256 deterministic seeded long-form properties, and a 128-case default runtime-seeded campaign across moves, descendant loans, projections, and deep lifetime wrappers; weekly CI covers four larger seeds, campaign failures persist minimized source/replay metadata, and strict release requires zero per-family mismatches | Extend generated/reference coverage to slice-range projections, richer nested-loop interactions, deeper projection/recursion combinations, dynamic collections, closure captures, and generics; enlarge campaign scheduling and promote newly discovered minimized failures into the persisted corpus |
| Async/concurrency | Partial | Deterministic cooperative `Task[T]` handles; bounded affine `TaskGroup[T]` scopes with ordered join, owned child inputs, and cancellation propagation; bounded non-blocking `Channel[T]` FIFO queues; explicit concurrency effects | Real scheduling, nested structured scopes, suspension/backpressure, timers, effect-aware async I/O, and runtime receipts |
| Graphics/media runtime | Partial | A bounded `ScenePlan` reaches a contained C software renderer; its 16x16 raw RGB result crosses an immutable owned-byte handoff to a separately gated PPM encoder. Linked `forgentex.render_receipt.v1` and `forgentex.encode_receipt.v1` bind scene, input, and output hashes to profiles, tools, effects, and budgets. `core.music` supplies deterministic authored time, versioned cues/conflicts, and bounded scene-action entries | Real text/3D or GPU renderer, action-table manifest and cue-collection schemas, multi-change tempo maps, audio analysis, production codec dispatch, and preview/final profiles |

Qualitative prioritization estimates as of 2026-07-17 (not test-coverage
percentages): live native calls **82%**, native data boundaries **80%**, shared
data plane/pipelines **60%**, and native useful-language fast path **72%**.

The active struct-value slice is complete: a `@repr(C)` named value with scalar
fields passes by value to and from C/C++ through a generated private type
header. Returned fields serialize through the child response and validate as a
complete typed value before reaching the interpreter. It does not authorize
nested structs, pointer fields, string fields, arrays, or owned buffers.

The first pipeline slice is also complete. `fgx pipeline-run` executes each
capsule with one scalar or restricted-struct `bind <source> read` and matching
`bind <target> rw` in source-declaration order. It reads the source from a
Forgentex-owned shared store, calls the normal live capsule gate, validates the
output type, and then writes the target. A struct value may be uninitialized
until an earlier stage writes it; an attempted read before that is rejected.
Every attempted stage is recorded. This is deliberately not a general DAG,
queue, artifact job, or parallel execution system.

The owned-artifact pipeline slice is complete. A producer may write governed
`Vec[u8]` bytes to a shared value, and a later capsule may consume an immutable
`&[u8]` view through the existing bounded native slice boundary. The producer's
artifact metadata remains attached to its stage audit. This does not authorize
mutable artifact views, fan-out ownership, cache lookup, or parallel access.

The reviewed typed-buffer pipeline slice is complete. Producers may write
owned `Vec[i16]` or `Vec[i32]` values and later stages may consume matching
immutable slices. Each producer records a `forgentex.pipeline_buffer.v1`
receipt with element count, byte count, and a deterministic hash over canonical
little-endian integer bytes. Runtime byte accounting must agree before the
shared target commits. Mutable views, other element layouts, fan-out ownership,
and parallel access remain blocked.

The first two async/concurrency slices are complete in the deterministic interpreter.
`task_spawn` captures one typed callable and moves one input into an affine
`Task[T]`; `task_await` runs it once, while `task_cancel` prevents pending work
from running. `Channel[T]` is a bounded FIFO with immediate send/receive results
instead of hidden blocking. `TaskGroup[T]` adds a bounded structured scope whose
children share one result type, cannot escape, join in insertion order, and
receive cancellation as a group. The `concurrency.task` and
`concurrency.channel` effects keep scheduling and communication explicit. This
is a reference-mode contract, not parallel execution or asynchronous I/O.

## Milestone A: native-call v1 (first slice shipped)

The first release slice supported **C plus scalar inputs and outputs only**.
It reused the C-ABI fixture bridge's signature derivation, toolchain invocation,
linking, and result checking rather than designing a second native integration.

### Design

- `fgx run --live` dispatches a native C capsule through a per-call host
  process, not by loading untrusted code into the compiler process.
- The generated host exposes the derived `extern "C"` boundary, receives one
  typed value per line from a private per-call request file, calls the compiled
  capsule, and returns a typed response. Values do not consume command-line
  capacity.
- Existing static checks, pinned-dependency requirements, sandbox decision,
  and runtime effect allowlist are evaluated before a host process is started.
  A declared `limit.wall_ms` now bounds and terminates the live process tree.
  Every call also removes surviving descendants when its direct runner exits.
  CPU and memory budgets are installed before execution. Windows accounts
  across the Job Object; Unix enforcement is inherited per-process and uses
  whole-second CPU granularity.
- Every call uses the existing `forgentex.capsule_call.v1` transcript with the
  capsule, language, input/output text, and decision. Successful native calls
  now attach `forgentex.native_receipt.v1` with a derived-boundary hash,
  toolchain fingerprint, elapsed native duration, and applied wall limit.
  `audit.redact <input-name>` replaces selected call and pipeline audit values
  without altering the foreign request; invalid names fail checking. Failed
  native attempts attach a
  `forgentex.native_failure.v1` receipt with a stable kind, failure phase,
  elapsed time, declared budgets, and truthful limit/containment activation;
  raw arguments, foreign output, and stderr are excluded from that receipt.
- If no C compiler is available, execution returns a structured
  `native_runtime_unavailable` result; it must never silently fall back to a
  mock result.

### Implemented acceptance tests

1. A `.fgx` function calls `clamp_sample(40000.0)` during `fgx run --live` and
   receives the real `i16` result from the compiled C capsule.
2. An ungranted capsule is denied before compilation or process spawn, and the
   denial appears in the audit transcript.
3. Compile failures, child launch/exit failures, and malformed or non-finite
   response envelopes are deterministic and cannot corrupt the interpreter
   process or partially copy back a mutable buffer.
4. Existing fixture-only FFI behavior remains unchanged.

Aggregate Unix tree budgets, persistent native workers, and broader Zig
dispatch remain follow-up slices; they must
preserve the same gate and transcript semantics.

This is the most valuable next code slice because it makes every governed C
library potentially usable by a program without weakening the trusted core.

## Milestone B: native buffers and structs (v1.2 scalar slices shipped)

Media cannot be built on a scalar boundary. Add one explicit ownership model
instead of trying to infer it from a C signature.

| Boundary | Initial rule |
| --- | --- |
| `&[T]` input | **Implemented:** Forgentex-owned, read-only scalar buffer; host receives `const T *` plus `size_t len` through a bounded temporary allocation; no retained pointer is allowed. |
| `&mut [T]` input | **Implemented:** copy-in/copy-back scalar buffer for the process-hosted slice; modified length is fixed, every returned scalar is range-checked, the full response validates before copy-back, and the caller must pass a direct variable. |
| `Struct` | **Implemented, by value:** a named `@repr(C)` value with unannotated scalar primitive fields may pass as an input or the one return value in C/C++/Rust. A generated private C header or exact public Rust `#[repr(C)]` declaration fixes layout; a returned value is fully parsed before it is exposed. Nested structs, pointers, strings, arrays, packed/custom-aligned layouts, and field endian annotations remain blocked. |
| Owned output buffer | **Implemented for three reviewed element layouts:** C/Rust/Zig returns `FgxOwnedU8`, `FgxOwnedI16`, or `FgxOwnedI32` `{ ptr, len, capacity }`. Each exports a matching typed `<capsule>_free`, stays under 64 MiB by byte size, and is copied before exactly-once deallocation. Other element types and multiple buffers remain blocked. |
| Owned string | **Implemented for UTF-8 output:** C/Rust/Zig returns `FgxOwnedStr { ptr, len, capacity }` with byte counts and a matching `<capsule>_free`. The child copies before deallocation; the parent rejects invalid UTF-8 before constructing `str`. String inputs and multiple owned values remain blocked. |

The implemented demonstrations are `sum_i16_c`, which receives `&[i16]` and
returns a scalar sum; `add_gain_i16_c`, which changes an `&mut [i16]`
audio-style buffer in place through the validated copy-back protocol; and
`scene_metrics_c`, which returns a restricted multi-field `SceneMetrics` value
by value. `add_gain_i16_cpp` proves that the same mutable boundary reaches a
C++17 capsule only through an explicit `extern "C"` export. `make_bytes_c`,
`make_bytes_rust`, and `make_bytes_zig` prove the first explicit owned-output contract without
exposing an unmanaged pointer. `render_params_rust` proves checked struct
input and output declarations across Rust. Their declared artifact policy now
bounds and identifies the returned bytes without granting filesystem writes.
`make_samples_c`, `make_samples_rust`, and `make_samples_zig` prove the first
non-byte owned vector: four `i16` elements return as a typed vector while the
receipt accounts eight bytes and no byte-artifact policy is inferred.
`make_values_c`, `make_values_rust`, and `make_values_zig` extend the same
contract to `i32`; four elements account sixteen bytes while preserving the
same copy-before-free and range-validation rules.
`make_greeting_c`, `make_greeting_rust`, and `make_greeting_zig` prove the owned
string contract with embedded comma/newline and multibyte UTF-8. The receipt
accounts fourteen bytes; invalid UTF-8 is rejected only after the foreign
allocation has been copied and freed exactly once.
`persist_bytes_c` proves parent-owned, scoped, immutable publication without
revealing the destination to foreign code. The next composition work is moving
owned artifacts through the audited pipeline. `clamp_zig` proves the first pinned,
process-isolated Zig scalar call path; `sum_i16_zig` and `add_gain_i16_zig`
prove bounded pointer/length input and atomic copy-back; `render_params_zig`
proves exact by-value struct input and output declarations.

## Milestone C: application composition

The first runtime primitive is now `fgx pipeline-run`. It uses existing
`share` and `bind` declarations rather than adding implicit global state. A
stage must have exactly one scalar, restricted `@repr(C)` struct, or reviewed
owned-vector input and output, one `read` source binding, and one `rw` target binding. Source
declaration order is the fixed, single-threaded schedule. `--set name=value`
overrides a declared scalar share default for one run, and `--allow-effects`
applies the same pre-dispatch effect gate used by live function calls.

```text
read asset -> analyze audio -> plan scene -> render frame -> encode artifact
```

Each stage receives a typed value, declares its effects, and emits one audit
event. The runtime permits only one writer for a shared value. A pipeline must
have a deterministic single-threaded execution mode for tests before it gains
parallel scheduling.

The included `examples/pipeline/music_preview.fgx` demonstrates the narrow
vertical handoff: fixed music energy -> native `SceneMetrics` analysis ->
native preview score. It is a composition proof, not a renderer or artifact
encoder.

`examples/pipeline/artifact_preview.fgx` extends that proof with a native owned
byte producer and an immutable native checksum consumer. It demonstrates
artifact data-plane handoff and receipt preservation, not a renderer or codec.

`examples/pipeline/typed_buffers.fgx` chains isolated native producers and
immutable consumers across both reviewed typed layouts. `Vec[i16]` sample data
feeds an `&[i16]` reducer, whose scalar result seeds a `Vec[i32]` producer and
an `&[i32]` reducer. Repeated runs prove stable values and buffer hashes;
effect denial proves the last committed shared state remains atomic.

`examples/pipeline/render_preview.fgx` is the first graphics-runtime foothold:
a scalar seed becomes a checked `ScenePlan`, then a contained C software
renderer produces and immutably persists a deterministic 16x16 PPM. Its render
receipt binds the scene and output hashes to the seed, dimensions, profile,
renderer/toolchain identity, declared effects, and applied resource budgets.
It is deliberately a pixel proof rather than text geometry, 3D, GPU dispatch,
animation, or video encoding.

`examples/pipeline/render_encode_preview.fgx` separates that proof at the
correct trust boundary. The renderer publishes a bounded in-memory raw-RGB
artifact, and a distinct `media.encode` capsule consumes its immutable bytes
and persists the PPM. The encode receipt records the exact input hash, so it
must match the render artifact hash, as well as the final image hash and
applied encoder budgets. This proves governed encoder dispatch without
claiming a production image/video codec.

`stdlib/src/core/music.fgx` adds the first authored-timeline contract used by
the vertical slice. Bars and beats are one-based, ticks are explicit, integer
milliseconds round down, cue-grid half ties move forward, and the first
`TempoMap` supports one validated change between two constant BPM segments.
The module and `music.timeline` example are type-checked as ordinary source
modules and remain C/Zig lowerable; interpreter conformance covers the 16-bar
120 BPM duration, marker conversion, quantization boundaries, a tempo change,
invalid positions/maps, and zero-divisor traps. It is not a general multi-
change map, audio decoding, beat detection, or pipeline scheduling.

The next authored-time slice adds `MusicCue` version 1 without introducing a
dynamic collection. Each cue carries a stable ID, an approved numeric action
reference, track, quantized half-open interval, and priority. Validation
rejects unknown versions, zero IDs/actions/durations, invalid tracks, and
timeline overflow. Same-track overlaps resolve by higher priority and then
lower cue ID; a combined winner operation preserves affine cue ownership. The
`music.timeline` consumer and conformance tests cover construction, rejection,
end-touching, cross-track independence, priority, and stable ties. Scene-action
tables, cue collections/sections, audio-derived markers, and render-receipt
binding remain future slices.

The action-link slice replaces an otherwise ungoverned numeric cue reference
with `SceneAction` version 1. One entry declares a stable ID, one of four
bounded action kinds, an approved target ID, a deterministic Q16 parameter,
seed, and maximum instance count. Emitters require a nonzero seed; non-emitter
actions affect one target; all entries obey a caller-supplied instance limit.
The combined validator proves cue/action ID equality and consumes each owned
value once. The `music.timeline` example defines the bounded metal-rain
emitter. A table/manifest, collections, actual scene mutation, capabilities,
and receipt binding remain out of scope.

This is also the correct place to add cancellation, resource budgets, and
artifact/cache keys. They should be runtime capabilities, not implicit global
state.

## Milestone D: complete the useful language fast path

Run this track alongside Milestones A-C, but do not allow it to delay the first
application slice.

- **Shipped (`2ac5c56`):** effect-annotated `Fn[P…, R, effects[E…]]`
  contracts, effect-subset assignment, and conservative indirect-call/task
  propagation close the static capability-hiding gap.
- **Foundation shipped (`154f8e5`):** `T: Eq` bounded generics with static
  operator and call-site enforcement, wrapper propagation, canonical syntax,
  and preserved C/Zig specialization. Extend interfaces and dispatch
  incrementally; avoid a large trait system prematurely.
- **Marker interfaces shipped (`30fb96b`):** user `trait` declarations,
  explicit `impl Trait for Type` conformance, conjunctive bounds, coherence
  diagnostics, canonical tooling visibility, and zero-cost native erasure.
  Associated items and broader impl forms remain incremental follow-ups.
- **Required method contracts shipped (`ff7e766`):** traits declare pure
  `Self`-typed signatures and impls map them completely to exact checked
  top-level functions.
- **Trait-qualified dispatch shipped (`3ccfc7e`):** `Trait::method(...)` infers
  and checks `Self`, accepts bounded generics or concrete impls, and selects the
  validated backing function in the interpreter.
- **Native trait-call specialization shipped (`8ffdc0d`):** uniquely resolvable
  calls in concrete functions and generated generic clones rewrite to validated
  backing functions for C/Zig. Ambiguous or unresolved calls retain the explicit
  `op:trait_dispatch` blocker.
- **Associated constants shipped (`633ac99`):** typed trait contracts and
  complete literal impl values use explicit `Trait::constant[Type]` selection,
  substitute `Self`, execute in the interpreter, and inline for C/Zig.
  Associated types and defaults arrived in subsequent slices; broader impl
  forms remain.
- **Concrete associated types shipped (`fe1ffc9`):** traits require named types,
  exact impls bind them completely, and `Self::Item` resolves in required method
  and constant contracts for validation, interpreter dispatch, and C/Zig static
  specialization. Standalone/generic projections arrived in slice 5k.
- **Generic marker impls shipped (`01a81ac`):** bounded blanket and structural
  targets recursively match concrete types, reject overlap/unconstrained
  parameters, and erase through eligible C/Zig generic specialization.
- **Default trait methods shipped (`23f4509`):** safe pure top-level defaults
  are checked per concrete `Self`, permit explicit overrides, execute through
  interpreter fallback, and specialize generic defaults for C/Zig from the
  unique selected impl. Generic mutable-slice defaults remain blocked.
- **Generic impl items shipped (`326b7c5`):** coherent bounded and blanket impls
  may bind associated types, literal constants, and safe pure methods. Captured
  target parameters substitute symbolically during checking, concrete runtime
  selection, and unique C/Zig backing-function specialization.
- **Associated-type projections shipped (`502447d`):** standalone
  `Trait::Item[Type]` types validate and resolve through concrete or generic
  impls, remain symbolic behind matching generic bounds, recursively expose
  `Self::Item` from generic trait calls, and normalize before concrete C/Zig FIR
  lowering.
- **Generic function values shipped (`be14b0f`):** a leading `forall[...]`
  marker preserves generic parameters and trait bounds through assignment and
  alpha-renamed higher-order arguments. Indirect interpreter calls infer fresh
  checked instantiations; C/Zig retain the explicit `generic_type:Fn` blocker.
- **Native named function values shipped (`9e7fcd2`):** pure, binder-free
  `Fn` signatures over the shared scalar type set lower to deterministic C
  function-pointer typedefs and Zig `*const fn` types. Locals, higher-order
  parameters, reassignment, and indirect calls are covered by generated-source
  checks plus a host-compiled C behavior test.
- **Native effectful function values shipped (`7333f9a`):** monomorphic scalar
  `Fn` effect metadata is erased from the C/Zig machine signature and pure and
  effectful equivalents share one C typedef. Function-level backend contracts
  still reject runtime-only effects before emission; generic `Fn` values retain
  `generic_type:Fn`.
- **Native non-capturing closure subset shipped (`f140f71`):** annotated local
  monomorphic scalar lambdas with no outer-local references lift into stable
  private functions and reuse the named-function C/Zig ABI. The pass runs for
  source generation, normal builds, and optimized builds. Capturing closures
  remain unchanged and retain `op:lambda`.
- **Inline non-capturing closure extension shipped (`5e3c320`):** direct calls
  to statically known functions supply their parameter contracts to inline
  lambda arguments. Eligible scalar lambdas lift through the same stable ABI;
  lexical shadowing is respected, captured arguments stay blocked, and no
  general closure environment or dynamic expected-type inference is implied.
- **Returned non-capturing closure extension shipped (`c2b15c3`):** a typed
  scalar `Fn` return contract supplies the expected lambda signature, so an
  eligible returned lambda lifts to a private helper and C/Zig return its
  function pointer. Returned captures stay behind `op:lambda`; no environment
  representation or exported generic function-value ABI is introduced.
- **Reassigned non-capturing closure extension shipped (`5a988fb`):** lexical
  bindings retain their declared types during source-backend preparation, so a
  lambda assigned to an existing scalar `Fn` local receives the same expected
  contract and lifts normally. Captured assignment values remain `op:lambda`;
  the pass does not infer unannotated or dynamic target types.
- **Shipped (`dcb0aef`):** monomorphize eligible private, fully concrete
  direct-call generic functions for C/Zig output with deterministic names and
  nested-call discovery; retain FIR blockers for every unresolved form.
- Extend specialization only when exported generic ABI and generic/closure
  ownership, environment-layout, and effect rules are explicit.
- **Borrowed String subset shipped (`85525c0`):** C/Zig lower bounded byte
  length, containment, and content equality without allocation; owned,
  Option-returning, and Vec-returning operations stay FIR-blocked.
- **Bounded owned local Vec subset shipped (`b89f7e4`):** C/Zig lower fresh,
  annotated function-local `Vec[i32]` with push, discarded pop, checked
  index/len/write, explicit `alloc`, a 1,048,576-element cap, and return-safe cleanup. Aliases,
  escapes, signatures/returns, nested declarations, other elements, and
  value-returning pop remain FIR-blocked.
- Continue from the local `Vec[i32]` foothold to broader source-lowered Vec
  ownership/layouts, `Map`, and ordinary owned-string operations through a small, pinned runtime library with
  explicit allocation/deallocation; preserve the interpreter as reference.
- Keep every unsupported construct explicitly blocked by FIR contracts. A clear
  refusal is safer than partial source emission.

## Milestone E: safety and concurrency hardening

The safety work should be a release gate rather than a feature wishlist.

- Publish the borrow rules that the checker implements.
- Expand the conformance corpus around loops, joins, reborrows, projections,
  enum payloads, dynamic collections, closures, and generic instantiations.
- **First generated/reference slice shipped (`fff5733`):** an independent move
  availability model exhaustively generates 510 owned root/field programs over
  straight-line flow, branch joins, and two-iteration loops. The production
  checker must agree on every `use_after_move` result in conformance and strict
  release checks.
- **Alias/reborrow reference slices:** a separate place/liveness model
  exhaustively generates 39 root/parent/child write sequences, 56 two-alias
  field-projection sequences, 16 conditional target-set join programs, 126
  nested-field projection sequences, 8 loop-carried alias programs, 16
  constant/dynamic index-projection pairs, and 16 conditional target joins
  inside loops.
  Conformance and strict release require zero `mutable_borrow_alias` mismatches
  across all 277 cases. Extend next to slice-range projections and richer
  nested-loop interactions.
- **Lifetime-provenance reference slice:** an independent parameter/unproven
  origin lattice generates 132 direct, conditional, match, loop/while,
  helper-call, wrapper, field/index projection, and self/mutual recursion return
  programs, including local aliases and selective returned-argument contracts.
  Strict release requires zero `escaping_reference` mismatches. Extend next to
  deeper projection/recursion combinations.
- **Seeded soundness-property slice:** four fixed-seed generators add 64 long
  move sequences, 64 descendant-loan sequences, 64 projected-alias sequences,
  and 64 depth-4-to-8 lifetime wrapper chains. All 256 cases have stable replay
  IDs and a separate strict-release mismatch count.
- **Runtime campaign slice:** the default release run adds 32 cases per family,
  while CLI callers may supply a bounded seed and case count. Versioned replay
  commands identify every case; on mismatch, model-equivalent subsequences or
  shallower wrappers are tested and the smallest reproducer is emitted in the
  JSON report. Four minimal persisted regressions now replay on every run;
  `--failure-dir` writes minimized source plus versioned metadata, and weekly CI
  exercises four seeds at 64 cases per family while retaining failure artifacts.
  Extend next by promoting newly discovered failures and enlarging scheduled
  seed rotation.
- Extend the shipped bounded task groups and channels only as the borrow rules
  cover nested task ownership, cancellation, suspension, and scheduler joins;
  do not introduce arbitrary threads as a shortcut.

## The 3D-text-music-visualizar vertical slice

Do not start with an LLM or a 4K master. Start with a deterministic creative
pipeline that proves the architecture.

1. Load a fixed 16-bar `MusicBrief` and validated `ScenePlan`.
2. Use supplied text and symbols only; no generated assets yet.
3. Invoke one native renderer capsule to produce a short 720p or 1080p preview
   with a fixed camera path and deterministic seed.
4. Invoke one encoder capsule to write the approved output artifact.
5. Store the scene hash, asset hashes, renderer/toolchain fingerprint, seed,
   effects, capability grants, and output hash in a render receipt.
6. Add constrained LLM planning only after its output is schema-validated and
   can be compared against the deterministic plan.
7. Add cascades/physics, translucency, motion blur, advanced lighting, and 4K
   only as independently tested renderer capabilities.

The first render can use a simple pinned Rust/wgpu, Blender-compatible, or C
renderer capsule. Forgentex should own the scene contract, capability gates,
reproducibility, and review; the capsule should own GPU API details and shader
implementation.

## Work discipline

For every milestone, preserve the existing Forgentex standard:

1. Define the `.fgx`, `.fgai`, canonical AST, and JSON audit shape.
2. Add parser, formatter, type/effect checks, runtime gate, and deterministic
   failure modes together.
3. Add positive, denial, crash/timeout, and roundtrip tests.
4. Update the corresponding roadmap and run the full gate: `cargo test`,
   `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and
   `fgx release-check --target v1.0 --strict`.

This keeps Forgentex's differentiator intact while closing the gap that matters
most: turning reviewed integration contracts into safe, live, useful software.
