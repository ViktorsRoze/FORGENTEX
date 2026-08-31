# Forgentex Gap-Closure Plan

**Status (2026-08-16):** active implementation sequence, based on the verified
v3 working baseline. Forgentex is an advanced demo/early-alpha language with a
credible governed-native and deterministic-pipeline core. It is not yet a
general application platform.
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
`str` outputs. The current pre-0.1 numeric slices add finite width-aware
`Vec[f32]`/`Vec[f64]`, host-width signed `Vec[isize]`, and full-range
`Vec[u64]`/64-bit-host-range `Vec[usize]` outputs across C/Rust/Zig without introducing a new
public compatibility version. C++ now has live parity for all twelve reviewed
numeric owned outputs and owned UTF-8 `str` under explicit C linkage for both
the return and deallocator symbols. Fixed-byte-length mutable UTF-8 `&mut str`
inputs now copy back atomically across all four native languages after exact
length and complete UTF-8 validation. Mixed mutable string/slice calls now
stage exact, non-aliased, type- and length-checked replacement sets before the
first interpreter state write. Bounded resizable `&mut String` inputs now use a
runner-owned pointer/mutable-length/immutable-capacity ABI across the same four
languages, with 16 MiB per-input and 64 MiB aggregate reserved-capacity gates.
One owned `Vec[Record]` output is now live across C/C++/Rust/Zig when `Record`
is a reviewed pointer-free scalar-leaf `@repr(C)` layout; its typed descriptor,
ABI-sized byte ceiling, copy-before-free path, complete leaf grouping, and
atomic recursive reconstruction are enforced.
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
**scalar/slice/restricted-struct/owned-vector/owned-record-vector/owned-string** capsules across
C/C++/Rust and pinned Zig 0.14 through generated child runners. The reviewed
owned-vector output set is `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, full-range
`u64`, `i64`, `isize`, host-range `usize`, `f32`, and `f64`, plus owned UTF-8
strings across C/C++/Rust/Zig. Named `@repr(C)` values may contain
other reviewed named structs recursively when every leaf remains a supported
scalar primitive, plus non-empty fixed-size arrays of scalars or reviewed
structs bounded by the same total leaf limit.
Exactly one owned vector may also use such a named pointer-free struct as its
element. Mutable aggregate inputs now use fixed-count transactional copy-back
across C/C++/Rust/Zig. Native pipeline handoff of record vectors and multiple
owned returns remain blocked.
Immutable `&[Record]` inputs now use the same reviewed layout as a call-scoped
runner-owned typed array across C/C++/Rust/Zig. Exact record/leaf validation,
one-million-record and 64-MiB ABI-size limits, aggregate reservation accounting,
and payload-free borrowed-byte receipts apply. Mutable `&mut [Record]` adds a
non-const typed pointer, exact-count copy-back, complete range/shape validation,
recursive tag restoration, and all-or-nothing commit with every other mutable
input. Pipeline aggregate handoff remains blocked.
It returns live values and validated fixed-length or bounded resizable mutable
copy-back to the interpreter. C++ functions must export
an explicit `extern "C"` boundary; Rust additionally requires a public
no-mangle export. Zig requires `pub export fn`. Warm hosts reuse compiled
runners only after verifying the cached executable SHA-256 at lookup and again
immediately before launch; changed entries are evicted and rebuilt. Every call
still uses a fresh bounded process. Bounded request values travel through
anonymous stdin rather than process arguments or a scratch file. Scratch itself
excludes inherited peer access before compiler or runner creation: a protected
owner/System/Restricted-Code DACL with explicit AppContainer read/execute
access on Windows and verified mode `0700` on Ubuntu/macOS. Windows live
runners start suspended inside the per-user `Forgentex.NativeRunner.0_1`
AppContainer with zero capabilities, layered over the kernel-verified
restricted primary token at low integrity, before Job assignment and resume.
Adversarial capsules cannot read or modify an ordinary workspace file, write a
user-owned low-integrity directory, connect to loopback, open an unrelated
named pipe, or modify the host user's registry.
Several logical scalar results can cross atomically as fields of one named
result struct; arbitrary independent outputs, multiple owned results, retained
borrows, and pointer fields remain blocked.
Safely budgeted persistent processes and GPU dispatch also remain. Those gaps
still block the heavy runtimes needed for graphics, codecs, physics, and
real-time audio even though their contracts can already be reviewed.

## Priority order

| Priority | Gap | Why it blocks progress | Definition of done |
| --- | --- | --- | --- |
| 0 | Public 0.1 stabilization | Internal maturity gates do not by themselves define a coherent public language promise. | `fgx release-check --target 0.1 --strict` passes every documented profile, execution, compatibility, standard-foundation, distribution, and examples/evidence gate, including published checksummed clean-install evidence from the supported host matrix. |
| 1 | Live native capsule calls | C/C++/Rust and pinned Zig 0.14 scalar/slice/restricted-struct calls are live, process-tree-contained, and CPU/memory-bounded; every runner has a default-or-declared deadline, first-time compilation is separately contained and bounded, and warm hosts reuse compiled runners while keeping fresh per-call processes. | Every live call remains gated, audited, and contained under portable tree-wide wall/CPU/memory/output/filesystem/network limits; persistent native workers are either proven to preserve those per-call guarantees or explicitly rejected. |
| 2 | Native data boundaries | Scalar slices, immutable and mutable reviewed-record slices, borrowed, fixed-byte-length mutable, and bounded resizable UTF-8 inputs, recursively composed scalar-leaf C/C++/Rust/Zig structs with bounded recursively fixed-size scalar or reviewed-struct array fields, twelve reviewed numeric vector layouts (ten integer plus `f32`/`f64`, including full-range `u64` and 64-bit-host-range `usize`), one owned vector of a reviewed pointer-free record, and UTF-8 strings are live. Mutable record slices preserve fixed count and join the complete transactional copy-back set. One named result struct can carry multiple logical scalar results atomically; independent/multiple owned outputs remain blocked. | The application portfolio's retained strings and aggregate pipeline buffers, ABI-approved structs with pointer fields, and multiple owned outputs cross native boundaries with recorded layout, ownership, length, deallocation, and lifetime rules. |
| 3 | Live shared data plane and pipelines | A deterministic pipeline now moves scalar, struct, byte-artifact, and reviewed typed-vector values under receipted stage-count, declared wall/CPU, sequential peak-memory, owned-byte, and Windows filesystem-I/O budgets; derived or source-declared dependency DAGs support stable topological order, immutable fan-out, exclusive transactional move handoff, explicit version-resolved sequential fixed-length vector updates, ordered multi-input fan-in, stage-boundary cancellation, content-addressed stage identities, bounded verified process-local reuse, trusted-root integrity-checked cross-process reuse, non-downgradable HMAC-SHA256 group authentication, and pathless immutable artifact restoration for read-only native stages. Active-stage interruption/resume, filesystem artifact replay/asymmetric reuse, and parallel scheduling remain. | Source-declared graphs support deterministic fan-out/fan-in, version-resolved transactional updates and exclusive moves, portable aggregate resource budgets, governed filesystem artifact replay/resume, active-stage cancellation, and bounded parallel scheduling without weakening receipts or stage isolation. |
| 4 | Native fast path for useful app code | Eligible private direct-call generics, coherent generic/blanket impls with associated items, required/default trait dispatch, monomorphic scalar named/effectful function values and indirect calls, contextually typed non-capturing scalar lambdas from annotated locals and reassignments, direct higher-order arguments, or typed `Fn` returns, a bounded borrowed-String subset, and fresh function-local or nested branch-, loop-, and match-arm-local `Vec[T]` values (scalar primitives, bounded recursively fixed scalar/immutable-borrowed-view/declared-enum/named-struct-value arrays, borrowed immutable strings/scalar slices, declared enums, and backend-lowerable named structs) now lower for C/Zig; `Map`, ordinary owned-string operations, broader Vec ownership and mutable-view/other unsupported owned-generic array leaves and elements, capturing/remaining-position closures, and wider generic forms remain interpreter-only or native-blocked. Runtime-only effects remain explicit function-level blockers. | The common owned `Vec`/`String`/`Map`, generic, and closure forms required by the demonstration applications lower with explicit allocation, cleanup, ownership, and effect rules; every other form remains an honest FIR blocker. |
| 5 | Safety hardening | The borrow checker is well-tested but still research-grade for production claims. | Published rules, an adversarial persisted corpus, seeded properties, rotating shrinkable campaigns, and independent move/alias/lifetime reference models gate releases; affine closure and suspended-task ownership rules cover the remaining application runtime shapes. |
| 6 | Async/concurrency | Current hosted I/O is synchronous, unsuitable for render queues, responsive tools, servers, or streaming. | Structured nested tasks, continuations, cancellation/deadlines, bounded queues, blocking select, wall-clock/reactor readiness, and effect-aware async file/network I/O have deterministic test-mode behavior and governed live behavior. |
| 7 | Domain runtime: graphics/media | A reviewed GPU capsule is not yet a renderer. | A governed production-credible graphics/media path loads validated scene/action manifests, invokes a pinned text/3D or GPU renderer and production encoder, and emits linked preview/final artifacts and receipts under explicit budgets. |
| Outcome | Real application platform | Individual language and runtime proofs do not by themselves constitute an installable useful application workflow. | A published binary runs the bounded `forgepack` workflow from clean install with typed results/errors and reproducible evidence; richer signed artifact groups and later async/network profiles reuse the same governed contracts. |

The ranking is intentional: do not build a full graphics abstraction before
Forgentex can invoke and govern the native renderer that would implement it.

## Progress ledger

Updated as implementation slices land. “Partial” means a bounded, tested
contract exists but the definition of done in the priority table is not yet
met; it must not be read as permission to claim the later capability.

| Gap | Status | Estimate | Landed evidence | Remaining exit criteria |
| --- | --- | ---: | --- | --- |
| Public 0.1 stabilization | Partial | **95%** | `docs/public_0_1_profile.md` separates the interpreter-first public contract from historical internal labels; `docs/public_0_1_execution_matrix.md` freezes three designated dual-view/check/interpreter fixtures plus a host-compiled C11 scalar/struct/enum/control-flow subset and explicit native blockers; `docs/public_0_1_compatibility.md` and `src/public_compat.rs` freeze 49 catalog codes, 11 versioned public report schemas, required fields, and deprecation rules; `PUBLIC_STANDARD_FOUNDATION` freezes six deterministic allocation-free core modules and `standard_foundation_gaps()` fails on any module/function drift (`public_0_1_standard_foundation` gate now **pass**); `fgx release-evidence --json` reruns the frozen fixture suite (dual-view round-trip, checker cleanliness, recorded interpreter results) and republishes four documented supported limits under `forgentex.public_0_1_evidence.v1`, with `examples_evidence_gaps()` backing the `public_0_1_examples_evidence` gate (now **pass**); `src/distribution.rs` freezes the artifact/host/backend/prerequisite manifest (`fgx dist-manifest --json`, `forgentex.distribution.v1`), the README documents the install path and prerequisites, and the `release-artifacts` CI job builds release binaries, computes SHA-256 checksums, and clean-install verifies them (`distribution_gaps()` now leaves exactly one honest residual, so `public_0_1_distribution` is **partial**, not planned); `fgx release-check --target 0.1 --json` reports `gates_pass=20 gates_partial=1 gates_planned=0` and `--strict` refuses a premature release | A green release CI run that publishes the checksummed clean-install proof across the supported host matrix — a CI-runtime fact that cannot be asserted from a source checkout |
| Live native calls | Partial | **97%** | C/C++/Rust and pinned Zig 0.14 scalar/slice/restricted-struct calls; isolated process tree; mandatory default-or-declared wall, CPU, memory, and combined-output budgets with per-limit provenance in the consolidated canonical `forgentex.native_receipt.v1`/`native_failure.v1` shapes; tree-wide wall timeout/background cleanup; separately contained first-time compiler trees with a hard 60-second ceiling and a Windows 64-process fan-out cap; Windows aggregate and Unix inherited CPU/memory limits **installed before the runner executes any foreign instruction**; Windows live runners are created suspended inside a stable per-user, zero-capability AppContainer layered over `DISABLE_MAX_PRIVILEGE`/`LUA_TOKEN`, exactly three verified restricting SIDs, and a kernel-verified low integrity label, then assigned to the Job and resumed; kernel queries fail closed unless the AppContainer SID and zero capability count match; adversarial capsules cannot read or modify an ordinary workspace file, write a user-owned low-integrity directory, connect to a live loopback listener, open an unrelated named pipe, or modify host HKCU; receipts expose AppContainer/profile/capability evidence alongside the token, restricting-SID count, integrity level, and identity policy; Windows live runners admit only their direct process and record `process_limit: 1`/`peak_tree_processes: 1`; every Windows runner receives only controlled system/AppContainer variables plus an optional toolchain-runtime `PATH`, while Unix clears all but that optional path, and every runner moves from the workspace into private scratch; Windows Jobs install/query the complete `0xff` UI restriction mask and cumulative class-8 tree I/O counters; declared Windows `limit.fs_bytes` conservatively bounds aggregate read/write/other transfers including runner protocol overhead and fails with a typed receipt, while unavailable Unix `fs_bytes` and all native `net_bytes` limits fail preflight instead of being ignored; direct filesystem, network, subprocess, environment, chain, and GPU capability requests fail before compiler lookup, while host-owned governed artifact publication remains allowed; receipts freeze the compute-or-host-artifact capability profile and honest filesystem/network isolation state; best-effort sampled `peak_tree_memory_bytes` records aggregate Windows memory; crash/output-overflow containment and explicit audit redaction; host-owned compiled-runner reuse verifies the executable SHA-256 at lookup, immediately before execution, and before retention, rebuilds changed entries, records digest/cache-state evidence, and preserves receipt-frozen `fresh_process_per_call` execution that explicitly rejects persistent native workers; bounded request bytes cross an anonymous stdin pipe rather than argv or the scratch filesystem, with explicit transport evidence; scratch is created before compilation and reports protected owner/System/Restricted-Code/AppContainer DACL or mode-0700 proof | Verify hermetic external-network denial plus supported Windows-version compatibility; Ubuntu cgroup-v2 aggregate budgets plus filesystem-I/O accounting and namespace/seccomp isolation; macOS aggregate descendant/I/O accounting plus a supported OS sandbox mechanism |
| Native data boundaries | Partial | **99%** | Bounded C/C++/Rust/Zig scalar slices and immutable reviewed `&[Record]` plus mutable reviewed `&mut [Record]` slices; borrowed UTF-8 strings; fixed-byte-length mutable UTF-8 `&mut str`; bounded resizable UTF-8 `&mut String`; and `@repr(C)` structs with scalar, recursively nested non-empty fixed-size scalar/reviewed-struct arrays, or nested-struct fields. Record slices preserve exact element/leaf grouping before the private wire, rebuild runner-owned typed arrays, cap each and cumulative aggregate reservation at 64 MiB by host ABI size under the one-million-element ceiling, and record only `borrowed_input_bytes`; mutable record slices expose only runner-owned non-const pointers, require unchanged record count, validate all returned leaves and complete groups, recursively restore tagged records, and join the all-or-nothing non-aliased mutation transaction. Mutable strings require a direct variable and runner-owned bytes, validate complete UTF-8 before interpreter replacement, use exact fixed length or a 16-MiB immutable capacity, cap aggregate resizable reservations at 64 MiB before compiler lookup, transact the complete non-aliased mutation set, and record aggregate input bytes without payloads. Struct inputs flatten in declaration/index order, C/C++/Rust/Zig runners are live-verified on Windows, outputs rebuild arrays/nested records and recursively restore type tags, generated C declarations use native multidimensional declarators in dependency order, Rust/Zig declarations and recursive array dimensions are source-validated, repeated array elements count toward the shared leaf limit, and recursive-struct/deeper-than-16-array/more-than-1,024-leaf layouts are refused; bounded C/C++/Rust/Zig owned `Vec[u8]`, `Vec[i8]`, `Vec[u16]`, `Vec[i16]`, `Vec[u32]`, `Vec[i32]`, `Vec[u64]`, `Vec[i64]`, `Vec[isize]`, `Vec[usize]`, `Vec[f32]`, `Vec[f64]`, one `Vec[Record]` with the same pointer-free scalar-leaf review, and UTF-8 `str` outputs with required typed deallocators, ABI-sized byte accounting, copy-before-free, exact leaf grouping, exact full-range unsigned parsing, integer range checks or finite width-aware float validation on copy-back, atomic recursive reconstruction, and byte-accounted receipts; exact `u64` values also survive JSON, scalar dispatch, arithmetic/comparison, and canonical pipeline hashing; C++ preflight lexically proves explicit C linkage for both exports without trusting comments or strings; byte artifacts remain `Vec[u8]`-only and retain declared media/size/path policy, SHA-256 receipts, and scoped immutable persistence | Retained strings, record-vector pipeline handoff, multiple owned outputs, and pointer fields |
| Shared data plane/pipelines | Partial | **98%** | `fgx pipeline-run`: checked scalar/restricted-struct handoff and all eleven native-reviewed numeric vector layouts with deterministic byte/hash receipts; exclusive atomic move, legacy update, and explicit `name@N` sequential update contracts; binding-derived and source-declared DAGs with stable ordering, immutable fan-out, ordered fan-in, cancellation, and versioned graph/budget receipts; stage-count, declared-wall/CPU-sum, sequential-memory, owned-byte, and Windows Job filesystem-transfer ceilings; independently auditable cache identities; plus a caller-owned `PipelineCache` bounded to 256 entries/64 MiB by default, whose read-only native `cpu`/`alloc` entries bind canonical program, exact toolchain, inputs, grants, limits, and target, then revalidate all identity hashes plus output type/bytes/SHA-256 before cancellation/budget/target commit under `forgentex.pipeline_cache_restore.v1`. `PipelineCache::persistent` and CLI `--cache-dir` add bounded versioned files, content digests, no-clobber atomic publication, corruption refusal/recovery, explicit non-symlink trusted roots, and tested cross-process reuse. `authenticated_persistent` plus a separate bounded key file add HMAC-SHA256 group authentication, constant-time tags, downgrade refusal, wrong-key/tamper misses, and key redaction. Pathless governed `Vec[u8]` artifacts restore exact payloads and rebuild media/byte/hash receipts under current policy; policy changes miss. Secrets, path-bearing artifacts, render/encode, move/update, managed, and I/O-accounted stages remain ineligible | Aggregate buffers, active-stage interruption/resume, portable aggregate filesystem plus enforceable network budgets, filesystem artifact replay and asymmetric verifier-only cache restoration, retry/resume policy, and parallel scheduling |
| Native fast path | Partial | **99%** | Optimizer, C/Zig source emission, deterministic monomorphization of fully concrete private direct-call generics, coherent bounded generic/blanket impls with methods/constants/types, required/default static trait dispatch, concrete standalone/generic associated-type projections, monomorphic scalar named/effectful function values and indirect calls with runtime-effect contract enforcement, deterministic lifting of contextually typed non-capturing scalar lambdas from annotated locals and reassignments, direct higher-order arguments, and typed `Fn` returns, sub-16-MiB allocation-free borrowed String inspection, and bounded fresh function-local or nested branch-, loop-, and match-arm-local `Vec[T]` values for scalar primitives, recursively fixed arrays of scalar, declared enum, backend-lowerable named-struct, or immutable borrowed string/scalar-slice leaves with at most 16 dimensions and 1,024 total value leaves, borrowed immutable strings/scalar slices, declared enums, and backend-lowerable named structs, with layout-specific C/Zig storage, path-correct scope/control/arm cleanup, and FIR ownership blockers. `pop(v)` has its real `Option[T]` checker type and lowers when consumed immediately, stored in a local option later handled by `is_some`/`is_none`/`unwrap_or`, returned directly, or returned from an unmodified stored pop local. Context-typed `Some`/`None` now initialize the same supported scalar, borrowed string/slice, array, enum, and struct option layouts in annotated locals and direct or unmodified-local returns. C uses layout-specific by-value options saved before vector cleanup and Zig uses `?T` evaluated before `defer`; no option owns borrowed storage. Borrowed-view array vectors own only their outer array table; named-value arrays copy aggregate values into owned element storage. C dependency-orders wrappers and Zig derives recursive names | Vec signatures/returns/aliases, option parameters/reassignment/immediate untyped construction and parameter/callee-derived returns, and mutable-view/other unsupported owned-generic element layouts; native Map and owned String operations; exported/ambiguous generics, generic function values, and capturing/remaining-position native closures |
| Safety hardening | Partial | **99%** | Borrow checks, deterministic traps, bounded half-open slice-range projections with constant-range disjointness proofs, 40 curated conformance cases including named-owned closure capture moves and copyable-`Fn` mutable-borrow capture rejection, plus explicit affine `FnOnce` call/transfer and mutable capture-root ownership, four persisted rejection regressions and four persisted acceptance regressions spanning field-sensitive moves, non-lexical loop loans, constant-index disjointness, and affine closure reinitialization, 526 independently modeled move programs, 610 alias/reborrow programs (including constant/dynamic index pairs, exhaustive range pairs, range-versus-index checks, conditional joins inside loops, exhaustive two-, three-, and four-level nested-loop target joins in both write orders, concrete mutable-borrow instantiations through generic wrappers, and recursively normalized associated-type projections), 292 lifetime-provenance programs (including deep selective projections, recursive Vec/array borrow carriers, generic identity/wrapper chains, a 72-case combined carrier/wrapper depth matrix with direct and aliased traversal, projected self/mutual recursion, and associated-type borrow returns), 38 independently modeled affine-closure programs, 256 deterministic seeded long-form properties, and a 192-case default runtime-seeded campaign across moves, descendant loans, projections, variable-depth nested loops, deep lifetime wrappers, and balanced affine call/transfer/spawn/reinitialization traces; weekly CI covers eight seeds at 96 cases per family (4,608 generated programs), campaign failures persist minimized source/replay metadata, and strict release requires zero per-family mismatches plus eight persisted cases including four acceptance cases | Promote newly discovered minimized failures into the persisted corpus when a campaign produces one |
| Async/concurrency | Partial | **99%** | Deterministic cooperative `Task[T]` handles with explicit parked/pending/woken lifecycle, retry-safe nonblocking `task_poll`, logical-timer-aware consuming `task_wait`, bounded consuming `task_race` with stable winner selection and loser cancellation, consuming `task_timeout` with equal-deadline task priority and timeout cancellation, atomic wake-source rebinding through `task_wait_on`, and bounded typed `task_then` continuation chains that preserve readiness, propagate callable effects, and cancel atomically; bounded shared one-shot wake-token broadcast, bounded nonblocking token selection, deterministic blocking token waits, and borrowed heterogeneous `ReadinessArm` selection plus source-preserving `readiness_wait_timeout` across runnable tasks, buffered-or-closed channel reads, open channel send capacity, capability-confined regular-file read and no-clobber create availability, and tokens; waits advance only to the nearest represented logical timer or bounded timeout, choose the lowest ready input, preserve task/channel handles where contracted, preserve every source on heterogeneous timeout, and cancel task or token losers according to successful consuming operations; bounded logical clocks/timer queues with stable deadline ordering plus alias-visible waiter-safe cancellation, same-clock rescheduling with fresh tie order, and copyable fixed-rate `IntervalTimer` handles that issue fresh catch-up-aware one-shot tokens; bounded affine `TaskGroup[T]` scopes with full and incremental ordered joins, capacity release, cooperative FIFO backpressure, owned child inputs, and cancellation propagation; explicit concurrency effects and native-backend runtime-effect refusal; byte-stable `forgentex.concurrency_receipt.v1` transition evidence including blocking task progress/races/timeouts, wake-source transfers, poll outcomes, continuation attachment/resume, readiness adapters/waits/timeouts, timer/interval control, selection, timer/event wakes, scheduler/backpressure steps, and capacity/closed refusals | Real scheduling, nested structured scopes, statement-frame suspension, OS/reactor wake sources, wall-clock integration, active interruption, open-stream/socket readiness arms, and effect-aware async I/O |
| Graphics/media runtime | Partial | **99%** | A bounded `ScenePlan` reaches a contained C software renderer; immutable raw RGB crosses to separately gated PPM or standards-based PNG encoders. Distinct `preview` (through 1920x1080, 64 samples) and `final` (through 3840x2160, 4096 samples) profiles fail closed before dispatch. Optional atomic manifests bind selected-frame index/count/FPS, samples-per-pixel/Q16 shutter, and adaptive minimum-samples/Q16-noise intent from distinct scene fields into render receipts; legacy renderers retain explicit single-frame/null-quality defaults. A declared bounded sequence ID now requires that frame manifest and produces a deterministic per-frame cache-key identity bound to the exact scene/frame/settings, engine, native boundary/toolchain/runner, and artifact contract. Render and encode receipts expose distinct SHA-256 identities for the exact native executables verified immediately before launch, while bounded engine-name/core-version pairs record declared semantic identity on both sides. An exact optional sRGB contract and a bounded single RGB8 visualization pass must each match across raw-frame render and encode stages and are recorded in both receipts; PNG carries a validated `sRGB` chunk, exact stored-DEFLATE/level-0 compression, non-interlaced RGB8 layout, no-filter scanline, CRC-32/Adler-32 integrity, and exact single-IDAT `IHDR/sRGB/IDAT/IEND` ordering contracts checked against independently parsed container bytes. Optional CPU/GPU device intent must exactly match one render effect. The executable path remains contained CPU software rendering. The host validates RGB8 structure/dimensions before commit and encode dispatch, and PNG has complete checksum/decoded-pixel conformance. `core.music` supplies deterministic authored time, cues/conflicts, and bounded scene-action entries | Real text/3D or live GPU renderer, sequence scheduling and cache storage/reuse/resume, broader color transforms/ICC profiles, multi-pass and numeric AOV outputs, verified adaptive convergence implementation, publisher signatures plus physical device/driver identity, action-table manifest and cue-collection schemas, multi-change tempo maps, audio analysis/decode, alpha/palettes/higher-depth/interlaced layouts, broader ancillary chunks and split-IDAT layouts, Sub/Up/Average/Paeth or adaptive scanline filtering, fixed/dynamic Huffman and production compression tuning, artifact signing/provenance, and audio/video codec dispatch |
| Real application platform | Partial | **90%** | `fgx app-run`, required exact major/minor Forgentex compatibility checked before dispatch, type-directed positional `bool`/string/portable-path/integer/finite-float CLI values with range and unsupported-shape refusal, manifest-scoped synchronous filesystem I/O, bounded discovery, binary reads, no-clobber atomic publication, canonical JSON/binary primitives, declared create-new artifacts, typed scalar/path/void results, structured phase-coded failures, `forgentex.application_run.v1`, and `forgentex.application_error.v1`; the complete `forgepack` v1 command workflow has frozen/golden bytes, spec-relative deterministic build, structured inspect, strict verify, verify-first confined no-clobber extraction, versioned receipts, and a self-contained ten-check evidence command exercised through a copied release binary in CI; `fgx new --template app` scaffolds a runnable `[application]` contract, declared create-new `text/plain` artifact, and its `output/` root, asserted end-to-end through `fgx check --project` and `fgx app-run` with no manual edits, while the default `lib` template stays byte-identical; `hosted_io` now admits the pure in-process `alloc` effect, so an entry can construct the bytes it publishes instead of only forwarding bytes it read, and `examples/apps/wav_waveform` renders a real 2.6 MB 16-bit PCM loop's amplitude envelope to a governed PPM artifact; an owned `Vec[T]` now lends an immutable `&[T]` to helper parameters, reviewed native capsule inputs, and `v[start .. stop]` windows without moving, so real payloads reach the native boundary directly | A published green clean-install CI artifact, richer artifact groups/signing, then async/network profiles; a hosted application still cannot invoke native capsules and pipeline application inputs are still scalars, so external file bytes have no governed route to native code |

These are qualitative milestone-completion estimates as of **2026-08-16**, not
test coverage, schedule confidence, or release probability. They are calibrated
against the definition of done and remaining exit criteria above, use coarse
resolution to avoid false precision, and stay below 100% while any required
proof is absent. The unweighted nine-gap snapshot is **87%**.

This audit directly re-ran three high-signal evidence paths:

- public 0.1 release evaluation: **20 pass, 1 partial, 0 planned**;
- `forgepack` packaged workflow: **10/10 evidence checks pass**;
- borrow conformance: **1,962/1,962 cases pass**, including 1,922 generated
  reference-model cases with zero mismatches.

### Ubuntu and macOS native-containment verification plan

Run this plan at commit `ace36b8` or later from the repository root. Ubuntu
needs a working `cc` such as `build-essential`; macOS needs the Xcode command
line tools. No test uses external networking.

First capture the exact environment:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
```

Then run these twelve test groups on **both Ubuntu and macOS**:

```sh
cargo test native_runner_applies_default_cpu_memory_and_output_budgets -- --nocapture
cargo test successful_native_receipt_records_applied_resource_budgets -- --nocapture
cargo test native_cpu_budget_terminates_busy_work_before_wall_fallback -- --nocapture
cargo test native_memory_budget_prevents_unbounded_committed_allocation -- --nocapture
cargo test contained_runner_ -- --nocapture
cargo test live_c_capsule_receipt_is_rendered_in_json_audit -- --nocapture
cargo test live_c_runner_removes_ambient_environment_and_workspace_authority -- --nocapture
cargo test live_native_preflight_rejects_unisolated_os_capabilities -- --nocapture
cargo test live_native_preflight_rejects_unenforced_byte_limits -- --nocapture
cargo test tampered_cached_native_runner_is_rebuilt_before_execution -- --nocapture
cargo test live_native_request_uses_anonymous_stdin_without_a_scratch_file -- --nocapture
cargo test live_native_scratch_has_protected_access_before_runner_creation -- --nocapture
```

Required result: every command exits zero. The receipt assertions verify
default 30,000 ms CPU, 512 MiB memory, and 402,653,184 combined-output budgets
with source `default`, while explicit limits report source `declared`. Every
receipt must also report `runner_execution_model: fresh_process_per_call`,
`runner_environment_policy: clear_except_toolchain_path`, and
`runner_working_directory_policy: private_scratch`. On these Unix hosts it must
also report `windows_restricted_token: null` and
`windows_restricting_sid_count: null`, `windows_integrity_level: null`, and
`windows_appcontainer: null`,
`windows_appcontainer_capability_count: null`, and
`windows_appcontainer_profile: null`, plus
`runner_process_identity_policy: inherited_unix_identity`; the Windows token
proof is intentionally a separate platform-only test. The ambient-authority
test must prove that a live C runner cannot see the host home environment or a
workspace-relative marker. Capability preflight must reject direct filesystem,
network, subprocess, environment, chain, and GPU authority without consulting
a compiler, while governed host-owned artifact persistence remains green.
Ubuntu/macOS must also reject `limit.fs_bytes` and every platform must reject
live-native `limit.net_bytes` until those limits have real enforcement.
Ubuntu/macOS must report `process_limit: null`, `peak_tree_processes: null`, and
`process_limit_source: unavailable`; those values are an honest TODO, not proof
of a Unix process-count ceiling. The CPU and memory denial tests must bound
busy/allocation-heavy children, and the containment tests must prove that no
background descendant survives. The cache-tamper test must show a changed
runner is rebuilt without execution, record a 64-hex-character
`runner_artifact_sha256` and `rebuilt_after_integrity_mismatch`, then record
`verified_hit` on the next call. The request-transport test must consume a real
input through `anonymous_stdin` while proving that `request.txt` is absent from
the live runner scratch directory. The scratch-access test must report
`owner_mode_0700` and observe exact mode `0700` on Ubuntu/macOS.

On Ubuntu and macOS, `RLIMIT_CPU` and `RLIMIT_AS` are installed in `pre_exec`
and inherited per process. Therefore `peak_tree_memory_bytes` is expected to be
`null`; this is truthful and does **not** prove aggregate Unix tree accounting.
Retain the environment block, all twelve command results, and any failing
test/error text. The detailed rationale and Windows comparison are in
[`native_containment_platform_verification.md`](native_containment_platform_verification.md).

### TODO: Ubuntu and macOS source-graph pipeline verification

The parser, canonical-AST, admission, and Windows live-C execution paths are
verified locally. Run the same authored-graph path on **both Ubuntu and macOS**
to retain cross-host native runner evidence. Capture the environment block from
the containment plan above, then run:

```sh
cargo test source_graph -- --nocapture
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/source_graph.fgx --schedule source --allow-effects cpu --json
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/source_graph.fgx --schedule graph --allow-effects cpu --json
```

Required result: all commands exit zero. The source-scheduled report must have
`mode: deterministic_source_graph`, graph `origin: source_declaration`,
`declaration: preview`, and execution order
`produce_right,produce_left,combine`. The derived-graph comparison must have
`origin: derived_bindings`; its ready-node order may follow capsule source order,
but its final `total` must also be `18`. Retain both JSON reports with the exact
commit and host/toolchain versions. No external network is used.

### TODO: Ubuntu and macOS transactional-move verification

The parser, exclusivity admission, denial/cancellation rollback, cache-mode
identity, and live C vector move are verified on Windows. Run this slice on
**both Ubuntu and macOS**, retaining the environment block described above:

```sh
cargo test move_binding_roundtrips_between_views -- --nocapture
cargo test move_handoff -- --nocapture
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/move_handoff.fgx --schedule graph --allow-effects alloc,cpu --json
```

Required result: every command exits zero. The CLI report must show
`move_sources: ["samples"]` and `handoff_committed: true` for
`sum_moved_samples`, final `sample_sum: -200`, and no final shared entry named
`samples`. The rollback test must retain the relevant source after denied
execution, deterministic stage-boundary cancellation, and owned-byte budget
exhaustion. No external network is used.

### TODO: Ubuntu and macOS transactional-update verification

The live C mutable-slice copy-back, atomic source/target commit, before/after
hash evidence, exclusivity refusal, and denial/cancellation/budget rollback are
verified on Windows. Run this slice on **both Ubuntu and macOS** with the same
environment capture:

```sh
cargo test update_binding_roundtrips_and_is_reported -- --nocapture
cargo test update_handoff -- --nocapture
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/update_handoff.fgx --schedule graph --allow-effects alloc,cpu --json
```

Required result: every command exits zero. The CLI report must show
`update_sources: ["samples"]`, `handoff_committed: true`, distinct 64-character
`before_sha256`/`after_sha256` values, final samples
`[-400, -200, 0, 200]`, and `sample_sum: -400`. The rollback test must preserve
the original `[-200, -100, 0, 100]` samples and zero target after denial,
stage-boundary cancellation, and an owned-byte exhaustion detected after native
copy-back. No external network is used.

### TODO: Ubuntu and macOS owned-float boundary verification

The C and Rust `Vec[f32]`/`Vec[f64]` paths are live-verified on Windows. The
pinned Zig runner is source-validated locally but needs a host with Zig 0.14.
Run the complete cross-toolchain matrix on **both Ubuntu and macOS** from the
exact commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
zig version
```

Then run:

```sh
cargo test c_rust_and_zig_owned_float_outputs_are_finite_typed_and_byte_accounted -- --nocapture
cargo test live_c_rust_and_zig_owned_float_outputs_return_typed_arrays -- --nocapture
cargo test owned_float_response_narrows_f32_and_rejects_non_finite_elements -- --nocapture
cargo test zig_owned_float_runners_use_exact_descriptors_and_typed_copies -- --nocapture

cargo run --quiet --bin fgx -- run examples/capsule/make_gains_f32_c.fgx --entry gains_from_c --live
cargo run --quiet --bin fgx -- run examples/capsule/make_gains_f32_rust.fgx --entry gains_from_rust --live
cargo run --quiet --bin fgx -- run examples/capsule/make_gains_f32_zig.fgx --entry gains_from_zig --live
cargo run --quiet --bin fgx -- run examples/capsule/make_precise_f64_c.fgx --entry precise_from_c --live
cargo run --quiet --bin fgx -- run examples/capsule/make_precise_f64_rust.fgx --entry precise_from_rust --live
cargo run --quiet --bin fgx -- run examples/capsule/make_precise_f64_zig.fgx --entry precise_from_zig --live
```

Required result: every command exits zero. Every `f32` run must return
`[0.10000000149011612, -1.5, 1.25, 3.5]` and report
`owned_output_bytes: 16`; every `f64` run must return
`[1.0000000000000002, -2.5, 0.0, 3.141592653589793]` and report
`owned_output_bytes: 32`. Retain the environment block, command output, and
any compiler diagnostic. A host without exactly Zig 0.14 has not completed the
Zig portion; record it as pending rather than treating a skipped test as proof.

### TODO: Ubuntu and macOS float-pipeline verification

The immutable `Vec[f32]`/`Vec[f64]` pipeline handoff is live-verified with C on
Windows. Run the same producer/consumer pipeline on **both Ubuntu and macOS**
from the exact commit being evaluated. A C11 compiler is required; this slice
does not require Zig.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
```

Then run:

```sh
cargo test reviewed_float -- --nocapture
cargo test deterministic_pipeline_hands_reviewed_float_buffers_to_immutable_slices -- --nocapture
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/float_buffers.fgx --allow-effects alloc,cpu --json
```

Required result: all commands exit zero. The focused suite reports three
passing tests. The pipeline report has status `ok`, four native stages, an
`f32` buffer receipt with 4 elements/16 bytes, an `f64` receipt with 4
elements/32 bytes, `little_endian_ieee754_v1` encoding for both, `gain_sum`
equal to `6.0`, and `precise_sum` equal to `2.25`. Retain the environment
block, complete JSON report, and any compiler diagnostic. A skipped live test
does not count as proof; confirm that the report says every stage is native.

### TODO: Ubuntu and macOS full-range owned-`u64` verification

The C, Rust, and pinned-Zig `Vec[u64]` paths are live-verified on Windows through
`UINT64_MAX`. The generated runner preserves `uint64_t`/`u64` storage, accounts
eight bytes per element, and parses the complete unsigned range exactly before
interpreter exposure. Run the complete matrix on **both Ubuntu and macOS** from
the exact commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
zig version
```

Then run:

```sh
cargo test c_rust_and_zig_owned_u64_outputs_are_full_range_and_byte_accounted -- --nocapture
cargo test live_c_rust_and_zig_owned_u64_outputs_preserve_the_full_range -- --nocapture
cargo test owned_u64_response_preserves_values_above_the_signed_range -- --nocapture
cargo test owned_u64_max_is_copied_before_exactly_once_free -- --nocapture
cargo test unsigned_runtime_carrier_roundtrips_json_and_a_live_scalar_boundary -- --nocapture
cargo test zig_owned_u64_runner_uses_exact_descriptor_and_typed_copy -- --nocapture
cargo test zig_owned_u64_output_rejects_a_mismatched_descriptor -- --nocapture

cargo run --quiet --bin fgx -- run examples/capsule/make_ids_u64_c.fgx --entry ids_from_c --json-input examples/run/make_ids_u64.json --live
cargo run --quiet --bin fgx -- run examples/capsule/make_ids_u64_rust.fgx --entry ids_from_rust --json-input examples/run/make_ids_u64.json --live
cargo run --quiet --bin fgx -- run examples/capsule/make_ids_u64_zig.fgx --entry ids_from_zig --json-input examples/run/make_ids_u64.json --live
```

Required result: every command exits zero. Each live run must return
`[3000000000, 5000000000, 7000000000, 18446744073709551615]` and report
`owned_output_bytes: 32`; the scalar/JSON test must retain `UINT64_MAX` exactly.
Retain the environment block, command output, and any
compiler diagnostic. A missing or differently versioned Zig executable leaves
the Zig rows pending.

### TODO: Ubuntu and macOS owned-`isize` boundary verification

The C and Rust `Vec[isize]` paths are live-verified on 64-bit Windows. The
generated C descriptor uses `ptrdiff_t`, byte accounting uses the host pointer
width, and the pinned Zig descriptor/source checks are green locally. Complete
the live cross-toolchain matrix on **both Ubuntu and macOS** from the exact
commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
rustc --print cfg | grep 'target_pointer_width="64"'
cargo --version
cc --version
zig version
```

Then run:

```sh
cargo test c_rust_and_zig_owned_isize_outputs_use_host_pointer_width -- --nocapture
cargo test live_c_rust_and_zig_owned_isize_outputs_return_pointer_width_arrays -- --nocapture
cargo test zig_owned_isize_runner_uses_exact_descriptor_and_typed_copy -- --nocapture
cargo test zig_owned_isize_output_rejects_a_mismatched_descriptor -- --nocapture

cargo run --quiet --bin fgx -- run examples/capsule/make_offsets_isize_c.fgx --entry offsets_from_c --json-input examples/run/make_offsets_isize.json --live
cargo run --quiet --bin fgx -- run examples/capsule/make_offsets_isize_rust.fgx --entry offsets_from_rust --json-input examples/run/make_offsets_isize.json --live
cargo run --quiet --bin fgx -- run examples/capsule/make_offsets_isize_zig.fgx --entry offsets_from_zig --json-input examples/run/make_offsets_isize.json --live
```

Required result: the pointer-width command prints
`target_pointer_width="64"` and every test/run exits zero. Each live run must
return `[-5000000000, 0, 5000000000, 10000000000]` and report
`owned_output_bytes: 32`. Retain the environment block, command output, and any
compiler diagnostic. A missing or differently versioned Zig executable leaves
the Zig rows pending.

### TODO: Ubuntu and macOS wide-integer pipeline verification

The immutable full-range `Vec[u64]` and host-width `Vec[isize]` pipeline
handoffs are live-verified with C on 64-bit Windows. Run the same pipeline on
**both Ubuntu and macOS** after confirming a 64-bit target:

```sh
git rev-parse HEAD
uname -a
rustc --version
rustc --print cfg | grep 'target_pointer_width="64"'
cargo --version
cc --version

cargo test every_reviewed_integer_layout_has_canonical_pipeline_receipts -- --nocapture
cargo test deterministic_pipeline_hands_reviewed_wide_integer_buffers_to_immutable_slices -- --nocapture
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/wide_integer_buffers.fgx --allow-effects alloc,cpu --json
```

Required result: all commands exit zero and the target is 64-bit. The pipeline
report has status `ok`, four native stages, a `u64` receipt with 3 elements/24
bytes, an `isize` receipt with 3 elements/24 bytes, `id_sum` equal to
`18000000000`, and `offset_sum` equal to `500000000`. Retain the complete
environment and JSON output. A skipped live test does not count as proof.

### TODO: Ubuntu and macOS pipeline cache-identity verification

Cache identity is live-verified with C on Windows. Because the exact runner
artifact and toolchain are intentionally bound, keys need only repeat on the
same host/toolchain; Windows, Ubuntu, and macOS keys are not expected to match.
Run on **both Ubuntu and macOS**:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version

cargo test cache_identity -- --nocapture
cargo test deterministic_pipeline_hands_reviewed_typed_buffers_to_immutable_slices -- --nocapture
cargo test pipeline_redacts_sensitive_stage_and_shared_input_values -- --nocapture

cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/typed_buffers.fgx --allow-effects alloc,cpu --json > pipeline-cache-1.json
cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/typed_buffers.fgx --allow-effects alloc,cpu --json > pipeline-cache-2.json
grep -o '"key_sha256":"[0-9a-f]*"' pipeline-cache-1.json > pipeline-cache-1.keys
grep -o '"key_sha256":"[0-9a-f]*"' pipeline-cache-2.json > pipeline-cache-2.keys
diff -u pipeline-cache-1.keys pipeline-cache-2.keys
```

Required result: every test exits zero, each JSON report has four
`forgentex.pipeline_cache_identity.v1` receipts with 64-character component and
final hashes, and the extracted key files are identical. The focused typed
buffer test must also prove input and pipeline-limit changes alter the final
key while preserving unrelated component hashes. The redaction test must emit
`cache_identity: null`. Retain both JSON reports and the environment block.

### TODO: Ubuntu and macOS pipeline I/O-budget refusal verification

Windows live runs now enforce cumulative `--max-fs-bytes` from Job Object
read/write/other transfer counters. Ubuntu and macOS do not yet expose an
equivalent aggregate tree counter to Forgentex, and no host exposes enforceable
aggregate network-byte accounting. Verify that both Unix hosts fail closed:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version

cargo test pipeline_unavailable_io_budgets_fail_before_dispatch -- --nocapture

if cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --max-fs-bytes 1073741824 --json; then
  echo "unexpected filesystem-budget success" >&2
  exit 1
fi

if cargo run --quiet --bin fgx -- pipeline-run examples/pipeline/music_preview.fgx --allow-effects cpu --max-net-bytes 1 --json; then
  echo "unexpected network-budget success" >&2
  exit 1
fi
```

Required result: the focused test exits zero; both CLI commands exit nonzero
with `status: rejected`, zero stages, and diagnostics naming unavailable
aggregate OS or network accounting. They must not compile or execute a native
stage. Retain both JSON reports. Future Ubuntu/macOS accounting work should
replace these refusal expectations only when a kernel-backed aggregate counter
is installed and queried.

### TODO: Ubuntu and macOS nested-struct boundary verification

The nested C and Rust input/output paths are live-verified on Windows. The Zig
0.14 runner and recursive source checks are generated and unit-tested here, but
the actual Zig call still needs Ubuntu and macOS evidence. Run this complete
matrix on **both Ubuntu and macOS** from the exact commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
zig version
```

Then run:

```sh
cargo test live_c_nested_struct_boundary_flattens_and_rebuilds_scalar_leaves -- --nocapture
cargo test live_rust_nested_struct_boundary_flattens_and_rebuilds_scalar_leaves -- --nocapture
cargo test live_zig_nested_struct_boundary_flattens_and_rebuilds_scalar_leaves -- --nocapture
cargo test live_c_capsule_preserves_nested_struct_types_across_the_boundary -- --nocapture
cargo test zig_nested_struct_runner_constructs_and_serializes_scalar_leaves -- --nocapture
cargo test rust_nested_struct_requires_every_reachable_declaration_to_match -- --nocapture
cargo test selected_struct_header_orders_embedded_dependencies_before_users -- --nocapture
cargo test recursively_embedded_native_struct_is_rejected_without_recursing_forever -- --nocapture
```

Required result: every command exits zero, `zig version` is exactly `0.14.x`,
and each of the first three filtered library-test results contains `1 passed`.
Each native call must rebuild
`{"id":7,"bounds":{"min_x":0,"max_x":30,"min_y":-10,"max_y":60}}`.
Retain the environment block, all command outputs, and any compiler diagnostic.
A missing or differently versioned Zig executable leaves the Zig row pending;
a Rust/C pass does not substitute for it.

The active struct-value slice is complete at the implementation level: a
`@repr(C)` named value with scalar, recursively nested non-empty fixed-size
scalar/reviewed-struct arrays, or recursively nested scalar-leaf fields passes
by value to and from the reviewed C/C++/Rust/Zig boundaries through generated
exact-layout declarations. Inputs flatten in declaration and array-index order;
returned fields serialize through the child response, validate atomically,
rebuild array/nested object shapes, and recover every declared nested type tag
before reaching the interpreter. Recursive struct embedding, array depth above
16, and more than 1,024 scalar leaves fail closed. Cross-host Zig execution
evidence remains in the TODO sections. A named
result such as
`DualOutput { result, status }` is therefore the supported way to return several
logical scalar values atomically. This does not authorize several independent
ABI outputs, multiple owned buffers, pointer fields, string fields, zero-length
arrays, or retained foreign references.

### TODO: Ubuntu and macOS fixed-array struct verification

The C, C++, and Rust `[T; N]` struct-field paths are live-verified on Windows.
The pinned Zig `[N]T` runner is generated and its exact declaration is checked
here, but its real call still needs Zig 0.14 on Ubuntu and macOS. Run on **both
Ubuntu and macOS** from the exact commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
c++ --version
zig version
```

Then run:

```sh
cargo test live_c_fixed_array_struct_field_round_trips_by_value -- --nocapture
cargo test live_cpp_fixed_array_struct_field_round_trips_by_value -- --nocapture
cargo test live_rust_fixed_array_struct_field_round_trips_by_value -- --nocapture
cargo test live_zig_fixed_array_struct_field_round_trips_by_value -- --nocapture
cargo test live_c_capsule_preserves_fixed_array_fields_across_the_boundary -- --nocapture
cargo test fixed_array_zig_runner_uses_exact_array_type_and_leaf_order -- --nocapture
cargo test zig_fixed_array_struct_requires_the_exact_foreign_array_layout -- --nocapture
cargo test native_struct_fixed_arrays_reject_zero_length_nested_array_and_excessive_shapes -- --nocapture
```

Required result: every command exits zero, `zig version` is exactly `0.14.x`,
and each of the first four filtered library-test results contains `1 passed`.
Each live call must rebuild `{"id":7,"samples":[-9,22,-27,44]}`. Retain the
environment block, all command output, and any compiler diagnostic. A skipped
or differently versioned Zig run remains pending.

### TODO: Ubuntu and macOS reviewed-struct array verification

Fixed-size arrays of reviewed `@repr(C)` structs are live-verified through C,
C++, and Rust on Windows. The pinned Zig runner has exact source/layout and
leaf-order checks here; its real call still needs Zig 0.14 on Ubuntu and macOS.
Run on **both Ubuntu and macOS** from the exact commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
c++ --version
zig version
```

Then run:

```sh
cargo test live_c_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_cpp_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_rust_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_zig_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_c_capsule_preserves_struct_array_fields_across_the_boundary -- --nocapture
cargo test struct_array_zig_runner_uses_exact_nested_layout_and_leaf_order -- --nocapture
cargo test rust_struct_array_requires_the_exact_foreign_array_layout -- --nocapture
cargo test zig_struct_array_requires_the_exact_foreign_array_layout -- --nocapture
cargo test recursively_array_embedded_native_struct_is_rejected_without_recursing_forever -- --nocapture
cargo test native_struct_fixed_arrays_reject_zero_length_nested_array_and_excessive_shapes -- --nocapture
```

Required result: every command exits zero, `zig version` is exactly `0.14.x`,
and each of the first four filtered library-test results contains `1 passed`.
Each live call must rebuild
`{"id":7,"points":[{"x":-9,"y":19},{"x":-28,"y":38},{"x":-47,"y":57}]}`.
Retain the environment block, all command output, and any compiler diagnostic.
A skipped or differently versioned Zig run remains pending.

### TODO: Ubuntu and macOS nested reviewed-array verification

Multidimensional fixed-size arrays of scalars and reviewed `@repr(C)` structs
are live-verified through C, C++, and Rust on Windows. Generated C declarations
use native multidimensional declarators, while Rust and Zig preserve the exact
recursive array type. The pinned Zig runner has source/layout and leaf-order
checks here; its real call still needs Zig 0.14 on Ubuntu and macOS. Run on
**both Ubuntu and macOS** from the exact commit being evaluated.

Capture:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
c++ --version
zig version
```

Then run:

```sh
cargo test live_c_nested_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_cpp_nested_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_rust_nested_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_zig_nested_struct_array_field_round_trips_by_value -- --nocapture
cargo test live_c_capsule_preserves_nested_struct_array_fields_across_the_boundary -- --nocapture
cargo test nested_struct_array_zig_runner_uses_exact_dimensions_and_leaf_order -- --nocapture
cargo test zig_nested_struct_array_requires_the_exact_foreign_dimensions -- --nocapture
cargo test selected_struct_header_emits_native_multidimensional_array_declarators -- --nocapture
cargo test native_struct_nested_arrays_reject_excessive_array_depth -- --nocapture
cargo test native_struct_fixed_arrays_reject_zero_length_nested_array_and_excessive_shapes -- --nocapture
```

Required result: every command exits zero, `zig version` is exactly `0.14.x`,
and each of the first four filtered library-test results contains `1 passed`.
Each live call must rebuild
`{"id":9,"points":[[{"x":-9,"y":19},{"x":-28,"y":38}],[{"x":-47,"y":57},{"x":-66,"y":76}]]}`.
Retain the environment block, all command output, and any compiler diagnostic.
A skipped or differently versioned Zig run remains pending.

The first pipeline slice is also complete. `fgx pipeline-run` executes each
capsule with one ordered `bind <source> read` per scalar, restricted-struct, or
reviewed owned-vector input and exactly one `bind <target> rw` output in
source-declaration order. It snapshots the sources from a Forgentex-owned
shared store, calls the normal live capsule gate with the complete argument
list, validates the output type, and then writes the target. A struct value may be uninitialized
until an earlier stage writes it; an attempted read before that is rejected.
Every attempted stage is recorded. This is deliberately not a general DAG,
queue, artifact job, or parallel execution system.

The owned-artifact pipeline slice is complete. A producer may write governed
`Vec[u8]` bytes to a shared value, and a later capsule may consume an immutable
`&[u8]` view through the existing bounded native slice boundary. The producer's
artifact metadata remains attached to its stage audit. Eligible pathless
artifacts can now restore from bounded memory, trusted-root files, or
HMAC-authenticated records; the host reconstructs the artifact receipt from
current media/size policy and validated exact bytes/hash. This does not
authorize mutable artifact views, ownership fan-out, `artifact.path`
publication replay, render/encode skipping, or parallel access.

The reviewed numeric-buffer pipeline slice is complete for all eleven native
layouts. Producers may write the nine reviewed integer vectors (including
full-range `u64` and host-width `isize`) or finite `f32`/`f64` vectors, and
later stages may consume matching immutable slices. Each producer records a
`forgentex.pipeline_buffer.v1` receipt with element count, byte count, and a
deterministic hash over canonical little-endian integer or exact IEEE-754 bits.
Runtime byte accounting must agree before the shared target commits. Immutable
fan-out is supported in graph mode. Mutable views, aggregate element layouts,
ownership fan-out, and parallel access remain blocked.

The pipeline-wide admission slices are complete for stage count and native
compute/memory declarations. Callers may require the sums of enforced
`limit.wall_ms` and `limit.cpu_ms` declarations to fit run-wide maxima; because
stages execute sequentially, the largest declared `limit.mem_mb` must fit a
run-wide peak-memory ceiling. Cumulative reviewed owned-vector bytes remain
capped before target share commit. `forgentex.pipeline_run.v1` embeds a
`forgentex.pipeline_budget.v1` receipt with planned and observed consumption.
Preflight overruns dispatch nothing; an owned-byte overrun records the producing
stage but atomically withholds its shared-state write.

The first aggregate pipeline-I/O slice is live on Windows. `--max-fs-bytes`
sums each native runner Job's kernel-reported read, write, and other transfers;
`forgentex.pipeline_stage_io.v1` preserves the per-stage evidence and
`forgentex.pipeline_budget.v1` records the cumulative total. The counter is
conservative because it includes anonymous protocol and runtime/executable I/O.
An overrun withholds the stage output, cache identity, and target commit.
Managed stages fail preflight under this limit. Ubuntu/macOS filesystem limits
and every network-byte limit remain explicit pre-dispatch refusals.

The dependency-graph slices now include both derived and authored graphs.
`--schedule graph` derives edges
from existing read/move/update/rw bindings, resolves producers independently
of declaration order, executes a stable topological order, permits multiple immutable readers
of one share, and rejects cycles or missing roots before dispatch. The
`forgentex.pipeline_graph.v1` receipt records origin, optional declaration name,
nodes, edges, roots, fan-out shares, and exact execution order. Ordered
multi-input stages join all producer/root dependencies before dispatch.
`pipeline <name>:` / `PIPELINE <name> ... ENDPIPELINE` is now canonical
source-level graph syntax; `--schedule source` requires exactly one declaration,
validates its complete node/edge manifest against typed bindings before
dispatch, and uses authored stage order only as the ready-node tie-breaker.

The first transactional move-handoff slice is complete. `bind <share> move`
acts as one ordered capsule input and is admitted only when that share has one
exclusive consumer and differs from the stage's `rw` target. The source remains
in Forgentex-owned state until the native result, cancellation, artifact,
buffer, cache-identity, and budget checks all pass; source removal and target
insertion then commit together. Denial, error, cancellation, and budget refusal
retain the source. Stage JSON records `move_sources` and `handoff_committed`,
the shared-namespace report distinguishes movers, and cache input identity binds
the read/move mode. Mutation uses the distinct `update` mode described next.

The transactional update path is complete for both legacy exclusive and
explicit sequential use. `bind <share> update` requires one exclusive consumer;
`bind <share>@N update` retains version `N`, publishes `N+1`, and permits earlier
immutable readers to keep the prior snapshot. Both require a reviewed numeric vector share, a matching
`&mut[T]` native input, and a distinct `rw` target. The existing native bridge's
length/type/range-validated copy-back is held pending while output, cancellation,
artifact, cache, and budget checks run. On success the source replacement or
next-version publication and target output commit together; every refusal
retains all old values. Stage
evidence records update intent plus element/byte/encoding and before/after
SHA-256 plus source/target versions, and copy-back bytes count toward the
pipeline owned-byte ceiling. Cache identities bind `update` mode, exact source
reference, and updated-source output contracts. Graph planning treats each
successful `N+1` as a producer and rejects missing, mixed, duplicate, or
overflowing version chains before dispatch.

The first pipeline cancellation slice is complete. Library callers can share a
`PipelineCancellationToken`, checked before dispatch and again before a
successful stage commits; the CLI adds deterministic `--stop-after-stages`.
`forgentex.pipeline_cancellation.v1` records the source, decision, committed
stage count, and last attempted stage. An already-running native child remains
governed by its containment deadline rather than being interrupted, and its
result is withheld if external cancellation is observed before commit.

The cache-identity and bounded restoration foundation is complete. After every
successful non-sensitive native stage passes cancellation, output, artifact,
and budget validation, `forgentex.pipeline_cache_identity.v1` binds typed
ordered inputs, exact boundary/runner/toolchain code identity, declared effects
plus enforced containment policy, native and pipeline limits, and output
contract. Component and final hashes use unambiguous length-prefixed SHA-256.
Repeated runs keep the same identity even when duration or compiled-runner
cache status changes. Sensitive-input stages publish no hash. Bounded memory,
trusted-root integrity records, HMAC-authenticated group records, and pathless
artifact reconstruction are live; filesystem publication replay, retry, and
whole-run resume remain disabled.

The first twenty-three async/concurrency slices are complete in the deterministic interpreter.
`task_spawn` captures one typed callable and moves one input into an affine
`Task[T]`; `task_await` runs it once, while `task_cancel` prevents pending work
from running. `Channel[T]` is a bounded FIFO with immediate send/receive results
instead of hidden blocking. `TaskGroup[T]` adds a bounded structured scope whose
children share one result type, cannot escape, join in insertion order, and
receive cancellation as a group. `task_group_await_next` adds an explicit
cooperative scheduler step: it runs one child, releases its capacity slot, and
leaves the group open for bounded incremental spawning, joining, or cancellation.
`task_group_spawn_wait` builds bounded cooperative backpressure on that step:
it admits immediately below capacity, or advances the oldest child before
refilling the released slot and returning that completed result.
`task_spawn_parked` and `task_wake` add a deterministic parked-to-pending
scheduler transition. Parked tasks can be cancelled, cannot run before an
explicit wake, and expose wake/refusal decisions in the concurrency receipt.
This state is established before execution; running callable statement frames
do not yet suspend or resume.
`task_then` adds bounded typed deferred continuation composition. It consumes
`Task[T]`, attaches one `T -> U` callable, and returns the sole `Task[U]`
handle without executing any stage. Chaining preserves pending, parked, or
wake-token-waiting state; final await runs stages in attachment order, while
cancellation prevents the whole chain from running. Callable effects and
affine `FnOnce` ownership propagate at attachment, chain depth is capped at
1,024, and `task.then`/`task.resume` receipts expose attachment depth and
ordered completion or failure. Arbitrary statement frames still cannot
suspend and resume.
`task_poll` adds the retry-safe nonblocking boundary needed before continuation
frames can suspend. It borrows `Task[T]`, returns `None` without mutation for
explicitly parked or unsignaled token-waiting state, and executes runnable work
once as `Some(T)`. Pending polls preserve wake registrations; terminal success
or failure closes the runtime handle, and `task.poll` receipts distinguish
parked, waiting, completed, and error decisions. A running callable still
cannot yield from the middle of its statement frame.
`task_wait_on` adds atomic deferred-task suspension behind a new one-shot wake
source. It consumes an open task or continuation chain, transfers any old
waiter registration, and returns the sole handle without executing work. The
same 1,024-waiter ceiling is preflighted before mutation; already-signaled gates
admit immediately, cancelled gates refuse, cancellation releases the new slot,
and `task.wait_on` receipts expose the source/gate binding and admission state.
This composes with retry-safe polling but still does not capture a partially
executed statement frame.
`task_wait` adds a consuming blocking path for a single task. Runnable work
executes immediately; a same-clock timer gate advances only to its deadline,
fires all due timers stably, reclaims the selected timer slot, and then runs the
complete chain. Cross-clock timers reject, while explicit parked state and
unsignaled program-only tokens trap `reactor_stalled`. The operation requires
task and timer effects and records `reactor.wait`, `timer.fire`, wake, and
terminal `task.wait` evidence without consulting host time.
`task_race` extends that blocking path to a bounded homogeneous `Vec[Task[T]]`.
The lowest ready task wins, or the clock advances to the nearest represented
input timer before selection. Only the winner's complete chain executes; every
loser is cancelled and releases its waiter registration while independently
aliased wake tokens retain their one-shot state. Empty, duplicate, closed,
over-capacity, cross-clock, and source-less sets refuse deterministically, and
`task.race` plus loser `task.cancel` receipts expose the stable decision.
`task_timeout` adds a consuming bounded deadline around one task and returns an
`Option[T]`. Ready work and same-clock task timers at or before the deadline
win as `Some`; otherwise logical time advances to the deadline, the task is
cancelled without running, and `None` is returned. Parked and program-token
tasks therefore time out instead of stalling. Equal deadlines favor the task,
zero-delay waits preserve already-ready work, eager delay evaluation is
rechecked, and later independently aliased timer tokens remain scheduled after
timeout cancellation. `task.timeout`, `task.cancel`, reactor, and timer
receipts expose the outcome and exact logical progress.
`WakeToken` adds bounded one-shot program-driven broadcast. `task_spawn_on`
registers a task without running it, shared token aliases preserve one signal
state, `wake_token_signal` releases the current waiter set, and each task
records its wake when next observed by the cooperative scheduler. Registration
is capped at 1,024; cancellation and manual wake release capacity; late tasks
start ready.
`AsyncClock` now supplies the first scheduler-owned event source without host
time. `timer_after` creates a wake token at a bounded logical deadline;
`clock_advance` fires due timers by deadline and creation order, releases queue
capacity, and records each wake plus an advance summary. Zero-delay timers are
ready without queue admission. Each clock is capped at 1,024 timers and logical
tick 1,000,000,000. `timer_cancel` withdraws waiter-free pending deadlines and
releases capacity with alias-visible cancellation. `timer_reschedule` preserves
the token/waiter set while moving a same-clock deadline; zero delay becomes
ready immediately, while positive delays receive a fresh stable tie-order
position. `timer_interval` adds a copyable fixed-rate handle whose aliases share
one cadence. `timer_interval_next` issues fresh one-shot tokens, exposing missed
deadlines as immediately-ready ticks in order and reserving future ticks in the
same bounded queue. `timer_interval_cancel` stops future issuance without
revoking existing tick tokens. Range, exhaustion, and capacity failures trap
deterministically; host reactor integration remains open.
`wake_token_select` consumes a bounded vector of distinct, waiter-free arms.
It returns `None` without mutation when no arm is ready; otherwise the lowest
ready input index wins and every losing one-shot arm is cancelled. Losing
logical timers release their clock slots immediately, as does a selected timer
made ready by a manual signal.
`wake_token_wait` supplies the first blocking readiness loop for the
deterministic reference runtime. It consumes the same bounded arm set, returns
immediately for an already-ready arm, or advances the supplied logical clock
only to the nearest selected-arm timer. Due timers fire in stable order before
the lowest ready arm wins and losers are cancelled. Cross-clock timers are
rejected, and an arm set without a ready or future source traps instead of
hanging.
`ReadinessArm` generalizes the same stable wait across borrowed token, task,
and channel observations. Task arms become ready when runnable or when their
registered wake source fires; channel read arms become ready when buffered or
closed, while `readiness_arm_channel_send` becomes ready exactly when the
channel is open and below capacity. The read and send directions are distinct
sources, and adapters stay connected to later send, receive, and close changes.
Hosted `readiness_arm_file_read` arms apply the existing `fs.read` grant and
path-confinement policy, observe regular-file availability without charging
content bytes, and follow create-after-adapter changes at deterministic wait
boundaries. Missing paths may time out logically, but no OS polling occurs.
Hosted `readiness_arm_file_create` arms apply `fs.write` confinement to the
no-clobber atomic-publication destination: an absent path is ready, an existing
file/directory/symlink is pending, and an intervening publication updates the
borrowed observation without creating or reserving anything at adaptation.
Consuming the arm set preserves every task/channel handle for await, receive,
cancellation, or further mutation. Only losing token arms are cancelled.
Open-stream/socket arms, suspended continuations, active task interruption,
and a host reactor remain open.
`readiness_wait_timeout` bounds that heterogeneous observation with a logical
deadline and returns `Option[usize]`. Ready arms and represented timers at or
before the deadline win under the existing stable ordering and losing-token
policy. A timeout returns `None` without cancelling any observed source, so all
borrowed task/channel handles and token aliases retain their state. Empty arm
sets provide deterministic logical sleep, firing generally due clock timers;
range, duplicate, closed, waiter, capacity, and cross-clock checks remain
enforced and `readiness.wait_timeout` receipts distinguish selection from
timeout.
The `concurrency.task`, `concurrency.channel`, and `concurrency.timer` effects
keep scheduling, communication, and logical-time control explicit. This
is a reference-mode contract, not parallel execution or asynchronous I/O.
`forgentex.concurrency_receipt.v1` now records deterministic task, group, and
channel transitions on successful entry runs, including queue/child counts,
capacities, cancellation, ordered completion, and full/closed refusal
decisions. Non-concurrent run JSON remains unchanged.

## Where we are now

The shortest accurate status is:

- **Language work is no longer the only blocker.** Forgentex can model useful
  domains with structs, payload enums, exhaustive matching, collections,
  strings, traits, effects, ownership checks, and deterministic interpreter
  execution.
- **Governed native computation is usable today.** Bounded C/C++/Rust/Zig
  capsules can receive scalar values, slices, strings, and reviewed structs,
  and can return scalars, recursively composed scalar-leaf structs, one reviewed
  owned integer vector, or one owned UTF-8 string.
- **Bounded batch composition is usable today.** `fgx pipeline-run` can execute
  deterministic one-writer graphs with immutable fan-out, ordered fan-in,
  resource admission, cancellation at stage boundaries, and receipts.
- **The first application shell is usable for bounded file tools.**
  `fgx app-run` reads a package-level entry/effect/limit contract, enforces
  canonical package-relative read/write roots and cumulative byte budgets, runs
  the supply-chain audit, rejects an incompatible declared Forgentex line
  before dispatch, emits `forgentex.application_error.v1` for pre-dispatch JSON
  failures, and emits `forgentex.application_run.v1` with mutually exclusive
  typed results and structured errors after dispatch. Arbitrary
  bytes can cross the runtime as `Vec[u8]`, and new text/binary outputs can be
  published atomically without clobbering an existing destination. Named
  outputs can additionally be constrained by manifest artifact contracts and
  receipted with exact hashes.
- **Distribution is source-complete but operationally unproven.** All public
  0.1 gates are computed. The only residual is a published, green
  checksummed/clean-install CI artifact across the supported host matrix.
- **The main application-platform gaps are still runtime gaps.** Canonical JSON,
  the first binary primitives, declared create-new file artifacts, and the
  frozen `forgepack` v1 bytes, golden archive, and package-level
  build/inspect/verify/extract commands and self-contained packaged-binary
  evidence are now present, but a published green clean-install CI artifact,
  network application profiles,
  active cancellation, a real async reactor, and production domain services
  are not finished. Recursive
  walking/general metadata, snapshot isolation under
  concurrent directory mutation, atomic replacement, and directory durability
  also remain distinct from the shipped typed-path, bounded-listing, and
  no-clobber publication primitives.

This means application development should begin now, but with the correct first
class of application: a deterministic command-line/batch tool whose expensive
or platform-specific work sits in governed capsules. A server, collaborative
desktop tool, game loop, or real-time media engine would force too many missing
runtime layers at once.

### Capability truth table

| Application need | Status now | What may be built honestly | Required next step |
| --- | --- | --- | --- |
| Pure domain logic | Usable | Validators, planners, transforms, deterministic business rules | Improve library ergonomics as concrete apps expose friction |
| Native algorithms | Usable within reviewed ABI | Codecs, checksums, numeric transforms, render experiments, file-format kernels | Add only reviewed layouts; keep unsupported ABI forms blocked |
| Deterministic batch graph | Usable with authored or derived graphs, stage identities, bounded process-local reuse, trusted-root cross-process reuse, HMAC-authenticated key-sharing groups, and pathless artifact restoration | Multi-stage asset/data tools with immutable handoff, receipts, deterministic source manifests, and verified read-only native-stage restoration | Add filesystem artifact replay/asymmetric restoration, resume, and retries |
| Multiple logical native results | Usable as one named value | `ResultInfo`, `DecodeResult`, or `BuildSummary` scalar-leaf result structs | Design independent outputs only when ownership/rollback is explicit |
| Immutable pipeline buffers | Usable for all nine reviewed integer and two floating-point layouts | Byte, sample, ID, index, count, timestamp, signal, and numeric-analysis pipelines, including full-range `u64` | Design aggregate buffers from concrete application pressure |
| Cross-stage ownership and mutation | Usable for exclusive move-to-distinct-target and fixed-length reviewed-vector update stages | Consume one or more shares, or mutate one or more reviewed vectors, only when the distinct target commits; rollback retains every original source and target | Add explicit versions before sequential/fan-out updates; never reinterpret `read` or `move` as hidden mutation |
| Manifest-backed file application | Partial, usable for bounded synchronous file tools | Required Forgentex compatibility, type-directed positional scalar/path CLI values, typed portable paths, sorted all-or-error directory listings, package-relative text/binary reads, no-clobber atomic publication, named artifact contracts and SHA-256 receipts, canonical JSON, UTF-8/little-endian byte primitives, canonical scopes, cumulative budgets, supply-chain audit, and a v3 run report | Add stable error/result types and explicit recursive-walk/atomic-replace contracts only when required |
| Network inside a running application | Hosted proof only | Explicit single-file `net-run` experiments | Add manifest network scopes after the async/readiness contract is stable |
| Async/concurrency | Deterministic reference model | Test scheduler, cancellation, queue, timer/interval control, and heterogeneous task/channel/token wait semantics | Add suspended continuations, file/socket readiness sources, active task interruption, and a host reactor |
| Packaging/distribution | Partial | Build and test local release binaries | Publish clean-install CI artifacts, then add app package manifests |
| Production operations | Not supported | Local demos and controlled batch jobs | Configuration, secrets, logging, migrations, update/rollback, support policy |

## What qualifies as a real Forgentex application

A larger example is not automatically an application. The first app can be
small, but it must satisfy all of these properties:

1. A user supplies real input through a documented CLI or file contract rather
   than by editing a fixture.
2. The application validates that input with Forgentex domain types and reports
   useful, stable diagnostics.
3. At least one meaningful transformation runs through Forgentex code; a thin
   wrapper around an unrelated executable is not sufficient.
4. Native work, when present, uses declared effects, limits, capability policy,
   exact ABI ownership, and structured failure receipts.
5. Output is useful outside the test suite: a file, report, package, or service
   response with a versioned format.
6. Success and failure can be reproduced from a clean install using documented
   commands.
7. Unit, integration, denial, malformed-input, budget, and compatibility tests
   cover the user-visible contract.
8. The build produces a distributable artifact with version, checksums, license,
   and host prerequisites.

The first app does **not** need general parallelism, a GUI, GPU acceleration, or
network services. It does need an end-to-end user workflow and a supportable
artifact.

## Recommended first real app: `forgepack`

The recommended first application is a versioned custom file-format tool named
`forgepack`. It should build, inspect, verify, and extract a deterministic
archive containing a manifest plus bounded binary assets.

This is a better first proof than a web server because it exercises the
strongest existing parts of Forgentex: domain modeling, validation, integer and
byte buffers, checksums, immutable pipeline handoff, native capsules, effect
gates, budgets, and receipts. It also forces the missing application basics
that every later project will need: serialization, stable errors, packaging,
and clean-install operation. Typed normalized paths, bounded deterministic
directory listings, binary file I/O, and no-clobber atomic output publication
are now available through the application runtime.

### `forgepack` MVP

The first supported format should be intentionally small:

- fixed magic, format version, flags, entry count, manifest length, and payload
  length;
- UTF-8 logical paths normalized to a portable slash form;
- deterministic entry order and duplicate-path rejection;
- per-entry offset, byte length, media type, and SHA-256;
- one canonical manifest encoding;
- bounded total entries, path bytes, manifest bytes, and payload bytes;
- no compression, encryption, streaming, symlinks, or incremental update in
  version 1.

The first commands should be:

```text
forgepack build <manifest> <output>
forgepack inspect <archive> --json
forgepack verify <archive>
forgepack extract <archive> <directory>
```

`build` and `verify` are the release-critical paths. `extract` must reject path
traversal and write only beneath its granted output root. Every command should
produce a stable result schema so agents and humans receive the same facts.

### `forgepack` implementation boundary

Forgentex should own the format types, validation, deterministic ordering,
capability request, diagnostics, and command orchestration. A small reviewed
native capsule may initially own SHA-256 or bulk byte copying, but it must not
own path policy or silently access arbitrary files. The runtime should own
bounded reads, atomic output publication, and output-root containment.

Ordinary Forgentex code can now request synchronous text or arbitrary-byte
reads, direct legacy writes/copies, and no-clobber atomic text/binary
publication through the manifest-backed `hosted_io` application runtime. The
runtime owns package-relative path containment and cumulative byte admission.
Typed portable paths and bounded, sorted directory listings close the minimum
filesystem-discovery primitives needed by `forgepack`. The source-backed
`core.serialize` closes the first serialization blocker. Declared create-new
file artifacts now bind logical output identity, root, media type, byte ceiling,
required status, and exact published hash. The `forgepack` v1 format contract,
first golden archive, bounded deterministic build/inspect/verify/extract
commands, and a self-contained clean-install evidence workflow are shipped.
Publication of a green release CI artifact remains operational evidence outside
the source checkout.
Using test-only `io-run`, hard-coded fixtures, or a capsule with unrestricted
filesystem access remains prototype-only.

### Canonical serialization foothold (shipped 2026-07-24)

The new contract is deliberately narrower than a general reflection or object
serialization system:

- `core_serialize_json_canonical(str) -> str` parses one JSON value and emits
  compact bytes with object keys in ascending UTF-8 order, normalized strings,
  and deterministic numbers under Forgentex's current `i64`/`f64` JSON model;
- `core_serialize_json_is_canonical(str) -> bool` is a non-trapping predicate
  for validators and archive inspection;
- duplicate object keys, leading-zero/missing-digit numbers, trailing commas,
  non-finite floats, malformed Unicode escapes, unpaired surrogates, more than
  128 nested containers, more than 1,000,000 values, and values over 16 MiB are
  rejected;
- UTF-8 encode/decode and exact little-endian `u16`, `u32`, and `i32`
  encode/read helpers provide the fixed-width header pieces required by the
  first archive format;
- byte concatenation checks the combined 16 MiB ceiling before allocating the
  destination;
- these operations are pure deterministic transforms. They do not request the
  historical `serialize` effect, so an app using them needs only the effects of
  its real I/O.

This is a Forgentex canonical JSON profile, not a claim of RFC 8785/JCS
compatibility. It intentionally normalizes through the current numeric value
model rather than preserving input number lexemes. It also does not serialize
arbitrary Forgentex structs or enums by reflection. `forgepack` must construct
its versioned manifest JSON explicitly, include a schema/version field, and
verify canonical bytes before trusting hashes or offsets.

The v1 contract is frozen in `docs/forgepack_v1.md`: a 24-byte little-endian
header, flags fixed to zero, canonical
`forgentex.forgepack_manifest.v1`, at most 4,096 sorted unique portable paths,
4 MiB of manifest bytes, and 1 GiB of contiguous payload. `src/forgepack.rs`
shares one strict parser between inspection and hash verification, and the
checked-in two-entry golden fixture has corruption coverage.

`fgx forgepack build` now consumes an exact versioned build spec, resolves every
source and output parent beneath the spec directory, performs bounded reads,
sorts logical paths, and atomically publishes a create-new archive.
`fgx forgepack verify` performs a bounded read through the same strict parser
and recomputes every entry hash. Both emit versioned count/size/archive-hash
receipts, and repeated builds are byte-identical.

`fgx forgepack inspect` emits every structurally validated manifest entry
and archive size/hash fact through `forgentex.forgepack_inspect.v1`. It
explicitly distinguishes structural validation from payload hash verification,
so diagnostic inspection never overclaims integrity.

`fgx forgepack extract` verifies the complete archive before output access,
treats an explicit existing directory as its grant, rejects file/ancestor
conflicts and symlink components in a full preflight, and publishes each file
atomically in create-new mode. Its receipt explicitly reports that extraction
is not a whole-directory transaction.

`fgx forgepack evidence --strict --json` now materializes its own text and
binary fixtures, covers build, reproducible rebuild, inspect, verify, extract,
no-clobber denial, corruption denial, byte comparison, and cleanup, and emits
the ten-check `forgentex.forgepack_evidence.v1` report. The clean-install script
copies a selected `fgx` binary to a fresh directory before running that
workflow, and the release-artifact CI job invokes the same command. The
remaining proof is a published green CI artifact, which cannot be computed
from source alone.

### Declared application artifacts (shipped 2026-07-24)

`[application.artifact.<name>]` now declares a logical name, portable output
root, lowercase media type, per-artifact maximum bytes, required/optional
status, and `create_new` publication mode. Source code publishes through
`core_io_publish_text_artifact` or `core_io_publish_bytes_artifact` with a typed
`Path`.

The runtime rejects unknown names, root escapes, a second publication under one
name, oversized values, existing destinations, and required artifacts that were
never published. It checks the artifact ceiling before committing the
run-global write budget or touching the destination. Successful publication
records exact path, byte count, SHA-256, media type, and mode in
`forgentex.application_run.v1`. Ordinary writes deliberately remain outside the
artifact list, so a write transcript cannot be mistaken for a distributable
output. A later trap does not roll back an already published artifact; its
receipt remains visible because whole-run filesystem transactions are still a
separate gap.

## Application-enablement sequence

### Phase 0: preserve and distribute the current foundation

Deliver:

- green formatting, lint, library, CLI, example, and strict internal release
  gates;
- a green public release workflow that publishes checksummed binaries and
  proves a clean install on every supported host;
- a short compatibility/support policy for the public interpreter-first
  surface.

Exit criterion: a new user can download `fgx`, verify its checksum, run
`dist-manifest`, reproduce public evidence, and execute the frozen examples
without the source tree.

### Phase 1: add the minimum application shell

Deliver:

- **Partial shipped:** `[application]` now requires an exact major/minor
  Forgentex compatibility line and fixes entry point, `hosted_io` runtime,
  filesystem effects, and cumulative read/write byte limits; `[permissions]`
  fixes canonical package-relative roots. Incompatible toolchains fail before
  dispatch.
- **Shipped:** positional CLI arguments decoded from declared
  `bool`/string/portable-path/integer/finite-float parameter types with exact
  syntax/range checks, stable exit behavior, a supply-chain preflight, and
  `forgentex.application_run.v1` human/JSON reports.
- **Partial shipped:** normalized relative runtime paths, symlink-aware
  containment, bounded text/binary reads, direct legacy writes/copies,
  no-clobber atomic text/binary publication, and denial/budget receipts.
- **Partial shipped:** `Path` provides strict portable parsing/joining and
  component operations; `core_io_list_dir` returns sorted `DirEntry` listings
  under cumulative entry/path-byte admission and returns no partial list.
- **Partial shipped:** bounded canonical JSON, UTF-8 conversion, exact
  `u16`/`u32`/`i32` little-endian encode/read, and bounded concatenation are
  source-backed in `core.serialize`.
- **Partial shipped:** named create-new file artifacts bind root, media type,
  maximum bytes, required status, and exact published hash. Undeclared ordinary
  writes never appear as artifacts.
- **Shipped:** supported scalar/path/void entry results retain their declared
  type and JSON-native value; completed failures carry stable code, phase, and
  message fields, while pre-dispatch JSON failures use
  `forgentex.application_error.v1`. Unsupported aggregate results are refused
  before dispatch.
- The minimum Phase 1 shell is complete. Recursive walking, atomic replacement,
  and directory durability should be added only when a concrete workflow
  requires them.

Exit criterion: a two-command file tool can be written without a custom Rust
host wrapper and has denial tests for every filesystem capability.

### Phase 2: make deterministic pipelines reusable by applications

Deliver:

- **Shipped:** human/agent source-level graph declarations are canonical AST
  items, match their complete typed binding-derived graph before dispatch, and
  supply the deterministic ready-node tie-break order;
- **Shipped for bounded process-local read-only native stages:** content-addressed
  keys bind typed inputs, exact code/toolchain, capability policy, effects,
  limits, and output contract; private in-memory entries revalidate identity,
  output type/bytes/hash, cancellation, and budgets before target commit;
- **Shipped for explicit trusted roots:** versioned integrity-checked files add
  bounded strict decoding, content-digest filenames, no-clobber publication,
  corruption refusal/recovery, and cross-process CLI reuse using those keys;
- **Shipped for symmetric key-sharing groups:** distinct version-2 records add
  HMAC-SHA256 over content plus digest, constant-time verification, explicit
  downgrade refusal, wrong-key/tamper misses, and key redaction;
- **Shipped for pathless immutable byte artifacts:** restore the cached payload
  and reconstruct `forgentex.native_artifact.v1` from current media/size policy
  plus exact bytes/hash; policy drift misses;
- add governed filesystem publication replay and asymmetric signatures if
  verifier-only access or distinct producer identity is required;
- explicit retry policy for retry-safe stages only;
- **Shipped:** exclusive `move` input handoff consumes sources only with target
  commit; fixed-length reviewed-vector `update` either replaces an unversioned
  source or retains explicit version `N` and publishes `N+1`, while writing its
  distinct target atomically; graph-visible sequential consumers are covered;
- **Windows filesystem-transfer slice shipped:** aggregate Job read/write/other
  counters are receipted and enforced; add portable filesystem and enforceable
  network accounting;
- resumable run metadata without treating an incomplete stage as committed.

Exit criterion: `forgepack build` and the asset-preview demo can resume or reuse
unchanged stages while producing byte-identical receipts.

### Phase 3: turn the concurrency model into an I/O runtime

Deliver:

- suspended/resumable continuations;
- a readiness reactor with file, socket, timer, task, and channel wake sources;
- extend stable deterministic blocking `select` from task/channel/token arms to file and socket sources;
- structured nested scopes, cancellation propagation, and deadlines;
- bounded async file and network operations with effect and resource accounting;
- active child interruption that preserves the existing atomic commit rule.

Exit criterion: a program can serve concurrent requests and cancel a slow
operation without leaking a task, child process, file handle, or partial output.

### Phase 4: domain runtimes and production operations

Deliver domain services only after the shared runtime contracts exist:

- HTTP client/server, routing, request limits, TLS boundary, and streaming;
- database/transaction and migration contracts;
- image/audio decode and encode profiles;
- GPU/render queue integration;
- structured logs, metrics, configuration, secrets, crash reports, updater,
  rollback, and long-term compatibility policy.

Exit criterion: later apps reuse the same path, async, configuration, error,
budget, and packaging contracts instead of inventing private hosts.

## Planned demonstration applications

The demos should be developed in dependency order. Each one must close a
specific platform gap instead of only adding surface area.

| Order | Demo | Purpose and user workflow | Can start now | Completion evidence |
| --- | --- | --- | --- | --- |
| 0 | `scoped-copy` | Copy a user-selected arbitrary binary file between manifest-approved package roots | Shipped as the application-shell/artifact foothold | Entry/effect/limit/artifact manifest, `Vec[u8]` round trip, typed artifact path, no-clobber destination, exact SHA-256 receipt, scope/traversal/budget denials |
| 0a | `directory-inventory` | Count a real directory through typed portable paths and a deterministic bounded listing | Shipped as the discovery/path foothold | Sorted `DirEntry` contract, entry/path-byte budgets, traversal/unportable-name rejection, application receipt |
| 0b | `media-visualizer` | Turn one seed into a planned scene, contained 64×36 RGB render, and locally viewable PPM | Shipped as the first compute/media application | Installed Windows CLI run, source-declared three-stage graph, mandatory aggregate budgets, exact native media/byte/hash evidence, scoped host publication, and repeat-run no-clobber denial |
| 1 | `native-batch` | Read numeric parameters, produce a reviewed `Vec[u32]`, reduce it through an immutable slice, and return one `BatchSummary` struct with value/status fields | Yes; its two boundary proofs now exist in `extended_integer_buffers.fgx` and `dual_output_c.fgx` | Installed CLI run, exact summary, stable buffer receipt, denial/budget/range tests |
| 2 | `asset-preview` | Convert a validated scene/action manifest into raw RGB and then a persisted preview image | Yes, by extending `render_encode_preview.fgx` | Manifest schema, cache key, linked scene/render/encode receipts, clean output-root proof |
| 3 | `forgepack` | Build, inspect, verify, and safely extract a deterministic archive | Serialization, declared artifacts, frozen/golden v1 bytes, corruption tests, reproducible bounded build, structured inspect, strict verify, verify-first confined extraction, traversal/collision/no-clobber tests, stable receipts, self-contained ten-check evidence, and copied-binary clean-install execution are shipped | Published green release-artifact CI proof |
| 4 | `job-server` | Accept a bounded build job over HTTP, expose status, and return the produced receipt/artifact | No; wait for Phase 3 and HTTP runtime | Load/cancellation/deadline tests, restart recovery, request/resource receipts |
| 5 | `waveform-preview` | Decode an audio file, compute waveform/markers, and emit a deterministic visual preview | Offline prototype after Phase 1; real-time later | Bounded codec capsule, sample-buffer receipt, deterministic marker/image hashes |
| 6 | `3d-text-music-visualizer` | Turn an authored timeline and action manifest into a governed rendered sequence | Continue its contract work now; production rendering is Phase 4 | Cue/action manifest, frame graph, renderer/encoder receipts, preview/final profiles |

The first two demos are integration fixtures and release evidence. `forgepack`
is the first application-capstone candidate. `job-server` is the proof that
async and program I/O are real. The waveform and 3D demos should consume those
shared runtime services rather than introducing one-off orchestration.

## Next implementation slices

The completed application-shell, typed-outcome, `forgepack`, reviewed-buffer,
and packaged-evidence slices are recorded above rather than repeated as future
work. The current order by dependency and release value is:

1. **Publish the public distribution proof.** Run the existing release workflow
   on the supported remote host matrix and retain checksums plus clean-install
   evidence. This is the sole source-external blocker for public 0.1 and the
   bounded `forgepack` capstone.
2. **Close portable native containment gaps.** Extend the landed Windows
   zero-capability AppContainer proof to hermetic external-network denial and
   the supported Windows build matrix;
   add Ubuntu cgroup-v2 plus namespace/seccomp containment, and add macOS
   aggregate descendant accounting plus a supported OS sandbox. Keep
   persistent native workers rejected; compiled executable reuse already
   preserves performance without weakening per-call containment.
3. **Extend safe cache reuse beyond pathless read-only values.** Read-only native
   stages now have bounded private-memory lookup, trusted-directory integrity
   records, non-downgradable HMAC-SHA256 group-authenticated cross-process
   reuse, and pathless immutable artifact reconstruction. Add filesystem
   publication replay, asymmetric verifier-only provenance where needed,
   retry-safe eligibility, and whole-run resume before enabling implicit global
   skipping.
4. **Extend portable pipeline I/O budgets.** Exclusive move-to-distinct-target
   and explicit version-resolved fixed-length vector update handoffs now commit
   atomically with before/after evidence. Extend the landed Windows filesystem-transfer ceiling
   to Ubuntu/macOS and add enforceable network accounting.
5. **Implement statement-frame suspension and host reactor readiness.** Deterministic
   task/channel/token waits are landed; preserve their stable winner, borrowed-
   handle, and receipt rules while adding suspended continuations, file/socket wake
   sources plus wall-clock deadlines.
6. **Generalize blocking `select`, active interruption, and async I/O.** Add
   file/socket/I/O arms and terminate governed children without partial
   commit.
7. **Expand native boundaries and the fast path from concrete app pressure.**
   Prioritize aggregate pipeline handoff, retained strings, multiple
   owned outputs, pointer-field policy, and `Vec`/`String`/`Map` or closure forms
   only when the next demonstration needs them.
8. **Promote the graphics/media foothold.** Freeze action-table/cue-collection
   manifests, then add a pinned real text/3D or GPU renderer and production
   encoder with linked preview/final receipts.
9. **Build `job-server`, then expand media demos.** Prove that the shared app,
   async, packaging, cancellation, and observability layers compose without
   one-off host wrappers.

## Safe mutable pipeline handoff

The current `bind source read` contract must stay immutable. Allowing an
`&mut [T]` capsule input to consume that share would make fan-out order-dependent
and would hide a write behind a read edge.

The distinct source-level `move` binding is now implemented for exclusive
consume-to-distinct-target handoff. It covers ownership admission, atomic source
removal/target insertion, and rollback, but intentionally does not mutate a
source value in place. The explicit `name@N` update slice now implements every
rule below:

1. The planner grants one stage exclusive authority to produce the next share
   version while immutable readers may retain the input snapshot.
2. The runner receives a bounded copy and returns a fixed-length mutation
   envelope; it never retains the original pointer.
3. The runtime validates the entire normal output, mutation envelope, byte
   budget, element ranges, cancellation state, and policy decision first.
4. Output and mutated share versions commit in one transaction or neither
   commits.
5. A successful update creates a new share version. Later graph edges name or
   deterministically resolve that version; earlier immutable readers retain the
   prior snapshot.
6. A receipt records element type, length, and before/after hashes without
   logging the payload.
7. Failure, denial, budget exhaustion, timeout, or cancellation leaves both the
   target and source versions unchanged.

Sequential explicit update chains and immutable reads of prior snapshots are
implemented and tested. Competing producers of the same next version remain
rejected; updates are deterministic graph edges, not concurrent shared memory.

## Claims and release gates

The following boundaries are non-negotiable:

- Do not infer a multi-output ABI from several `output` declarations while the
  runners, parser, type checker, ownership model, and rollback protocol still
  implement one return value.
- Do not pass a mutable slice through a `read` binding.
- Do not call deterministic task simulation a production async runtime.
- Do not call the public 0.1 distribution gate complete until published CI
  artifacts prove clean install on every supported host.
- Do not call an example a real app until it meets the end-to-end criteria above.
- Do not add persistent native workers unless per-call containment, budgets,
  cancellation, output caps, and failure receipts remain enforceable.

Every application slice must run:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
fgx release-check --target v1.0 --strict
fgx release-check --target 0.1 --json
```

Public readiness additionally requires the remote release workflow and its
published clean-install artifact. A local source checkout can verify the
workflow definition and build locally, but cannot manufacture that operational
evidence.

## Milestone P0: public 0.1 stabilization (standard foundation and evidence shipped)

Two of the three previously-open public gates are now backed by real computed
logic instead of hardcoded `partial`/`planned` stubs, mirroring the existing
`compatibility_gaps()` pattern:

- **Standard foundation.** `PUBLIC_STANDARD_FOUNDATION` (`src/public_compat.rs`)
  freezes a deliberately small, deterministic, allocation-free core — `core.mem`,
  `core.slice`, `core.math`, `core.bits`, `core.option`, and `core.result` — as
  the guaranteed 0.1 foundation. `standard_foundation_gaps()` cross-checks the
  frozen inventory against the live `standard_library_modules()` catalog and
  fails on any missing or renamed module or function, so the
  `public_0_1_standard_foundation` gate cannot silently drift. The full library
  keeps its host-effect, SIMD, crypto, agent, and media modules available but
  outside the frozen minimal promise, documented in
  `docs/public_0_1_profile.md#standard-foundation`.
- **Examples and evidence.** `fgx release-evidence [--json] [--strict]` reruns
  the three frozen `examples/public_0_1/*` fixtures at report time — dual-view
  round-trip, checker cleanliness, and every recorded interpreter result — and
  republishes four supported limits, each a literal substring of the frozen
  execution matrix, under schema `forgentex.public_0_1_evidence.v1`.
  `examples_evidence_gaps()` backs the `public_0_1_examples_evidence` gate;
  strict mode exits non-zero if any fixture stops reproducing its recorded
  result or any supported limit stops being documented.

**Distribution** now has real machinery instead of a `planned` stub.
`src/distribution.rs` freezes the toolchain manifest — the `fgx` and
`forgentex-lsp` artifacts, their install paths, the supported host triples, the
interpreter/C/Zig backends, and the required/optional prerequisites — behind
`fgx dist-manifest --json` (schema `forgentex.distribution.v1`). The README
documents the install path and prerequisites, and the `release-artifacts` CI job
builds the release binaries, computes SHA-256 checksums, and clean-install
verifies them by running `dist-manifest` and `release-evidence --strict` from a
copied binary. `distribution_gaps()` checks all of that from source and leaves
exactly one honest residual: a green release CI run that publishes the
checksummed clean-install proof across the supported host matrix, which a source
checkout cannot assert. The gate is therefore **partial**, not planned.

`fgx release-check --target 0.1 --json` now reports
`gates_pass=20 gates_partial=1 gates_planned=0` — every public criterion is
machine-checked, with distribution's lone residual being the operational CI
proof. The decision remains `public_0_1_not_ready` until that CI run lands.

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
  A declared `limit.wall_ms` bounds and terminates the live process tree; if it
  is absent, a mandatory 30,000 ms wall ceiling applies. First-time compiler
  and linker steps run in their own contained process trees with a hard 60,000
  ms deadline.
  Every call also removes surviving descendants when its direct runner exits.
  CPU and memory budgets are installed before execution. Windows accounts
  across the Job Object; Unix enforcement is inherited per-process and uses
  whole-second CPU granularity.
- Stdout and stderr are drained concurrently under one combined byte budget,
  defaulting to 402,653,184 bytes and lowerable with `limit.output_bytes`.
  Overflow terminates the process tree and rejects the partial response, so a
  noisy capsule cannot deadlock on a full pipe or grow parent capture without
  bound.
- Every call uses the existing `forgentex.capsule_call.v1` transcript with the
  capsule, language, input/output text, and decision. Successful native calls
  now attach `forgentex.native_receipt.v1` with a derived-boundary hash,
  toolchain fingerprint, elapsed native duration, effective wall-policy source,
  compiler containment/ceiling, applied limits, and actual stdout/stderr
  capture sizes.
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
5. A native child exceeding `limit.output_bytes` is terminated with a stable,
   payload-free response-phase receipt; its cached compiled runner remains
   reusable for a later conforming call.
6. Windows compiler Job Objects reject process fan-out above 64 active tree
   members, while live runner Jobs admit only the direct process and prove that
   a foreign child cannot execute.
7. Every platform records `runner_execution_model: fresh_process_per_call`,
   clears ambient runner environment state, and starts the runner in private
   scratch rather than the workspace.
8. Windows installs and reads back the complete Job Object UI restriction mask
   before the suspended child resumes.
9. Direct OS capability requests fail with
   `native_capsule_capability_unsupported` before compiler lookup; governed
   host-owned artifact publication remains available.
10. Windows enforces declared `limit.fs_bytes` against cumulative whole-Job I/O
    accounting and records a typed `native_capsule_io_limit`; unsupported Unix
    filesystem and all native network-byte limits fail before dispatch.
11. Windows live runners start suspended with a restricted, low-integrity
    primary token; the host kernel-queries the exact restricting-SID profile,
    integrity label, and privileges before Job assignment/resume. Receipts
    record that evidence, and a foreign write to an existing medium-integrity
    workspace file is denied without changing its bytes.
12. The same suspended Windows runner enters a stable per-user AppContainer
    with zero capabilities. Kernel token queries verify the profile SID and
    capability count before resume; adversarial capsules cannot read the
    workspace probe, write a user-owned low-integrity directory, connect to
    loopback, open an unrelated named pipe, or modify host HKCU.

Windows external-network matrix coverage, aggregate Unix tree budgets, and
Unix OS isolation remain follow-up slices; they must preserve the same gate
and transcript semantics.

This is the most valuable next code slice because it makes every governed C
library potentially usable by a program without weakening the trusted core.

## Milestone B: native buffers and structs (v1.2 scalar slices shipped)

Media cannot be built on a scalar boundary. Add one explicit ownership model
instead of trying to infer it from a C signature.

| Boundary | Initial rule |
| --- | --- |
| `&[T]` input | **Implemented for scalars and reviewed records:** Forgentex-owned read-only data becomes a runner-owned typed buffer; the host receives `const T *` plus `size_t len` for the call and may not retain it. Record elements use the pointer-free recursively composed scalar-leaf `@repr(C)` review, preserve exact record/leaf grouping, and share a 64-MiB ABI-size reservation under the one-million-element ceiling. |
| `&mut [T]` input | **Implemented for scalars and reviewed records:** copy-in/copy-back through a runner-owned typed buffer; modified element/record count is fixed, every returned scalar leaf and complete record group is validated, nested arrays/records and tags are rebuilt, the full non-aliased mutation set validates before copy-back, and the caller must pass a direct variable. |
| `str` input | **Implemented as a bounded borrow:** C/C++ receives `const uint8_t *` plus `size_t len`; Rust and Zig receive the corresponding raw pointer-length pair. UTF-8 is capped at 16 MiB, hex-framed through the private request file so NUL/newline bytes are unambiguous, reconstructed in runner-owned memory, and valid only for the call. Receipts record aggregate `borrowed_input_bytes`, never contents. |
| `&mut str` input | **Implemented as fixed-length UTF-8 copy-back:** the runner owns mutable bytes for the call, the capsule receives pointer plus byte length, and the parent requires the original length plus valid UTF-8 before transactional replacement. |
| `&mut String` input | **Implemented as bounded resizable UTF-8 copy-back:** the capsule receives pointer, mutable byte-length pointer, and immutable 16-MiB capacity. The runner rejects length beyond capacity, the parent validates complete UTF-8, aggregate reserved capacity is capped at 64 MiB before compiler lookup, and all mutable inputs commit together. |
| `Struct` | **Implemented, by value:** a named `@repr(C)` value with supported scalar primitive leaves may pass as an input or the one return value in C/C++/Rust/Zig. Fields may recursively reference other reviewed named structs and contain recursively nested non-empty fixed-size arrays of scalars or reviewed structs, with at most 16 array dimensions. Generated exact-layout declarations fix the ABI; a returned value is fully parsed before it is exposed. One result struct can carry several logical scalar fields atomically. Pointers, strings, owned buffers, packed/custom-aligned layouts, field endian annotations, and several independent outputs remain blocked. |
| Owned output buffer | **Implemented for twelve reviewed numeric layouts and one reviewed record layout per boundary:** C/C++/Rust/Zig returns the matching `FgxOwned* { ptr, len, capacity }` descriptor for `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `isize`, `usize`, `f32`, `f64`, or a named pointer-free scalar-leaf `@repr(C)` record. Each exports a matching typed `<capsule>_free`, stays under 64 MiB by ABI byte size, and is copied before exactly-once deallocation. C++ requires explicit C linkage for both symbols and rejects misleading comments/string literals before compiler lookup. `u64` and 64-bit `usize` preserve their complete unsigned native ranges through the exact runtime carrier; `isize`/`usize` use the reviewed host pointer width and `ptrdiff_t`/`size_t` in C/C++. Numeric elements are range/width validated; record elements require complete scalar-leaf groups and rebuild nested arrays/records atomically. Multiple buffers remain blocked. |
| Owned string | **Implemented for UTF-8 output:** C/C++/Rust/Zig returns `FgxOwnedStr { ptr, len, capacity }` with byte counts and a matching `<capsule>_free`. The child copies before deallocation; the parent rejects invalid UTF-8 before constructing `str`. Multiple owned values remain blocked. |

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
`make_pixels_u32_c`/`make_pixels_u32_rust` and
`make_marks_i64_c`/`make_marks_i64_rust` extend the reviewed integer set to
`u32` and `i64`: the `u32` producers return four elements each above `i32::MAX`
and the `i64` producers return values spanning ±5,000,000,000, proving that the
wider unsigned and 64-bit layouts round-trip exactly and stay range-checked on
copy-back. `make_ids_u64_{c,rust,zig}` preserves the unsigned 64-bit ABI for
values through `u64::MAX` using the exact unsigned runtime carrier.
`make_offsets_isize_{c,rust,zig}` adds signed pointer-width elements, uses
`ptrdiff_t` for the C ABI, and accounts bytes from the host width. The reviewed
numeric set is now `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, full-range `u64`,
`i64`, `isize`, host-range `usize`, `f32`, and `f64`.
`make_sizes_usize_cpp` and `make_greeting_cpp` prove C++ owned-buffer/string
parity, exact C linkage for both exports, 32-/14-byte receipts, and the same
copy-before-free validation paths. `make_gains_f32_{c,rust,zig}` proves
width-aware `f32` canonicalization (including `0.1`), while
`make_precise_f64_{c,rust,zig}` preserves precision-sensitive double values.
Both layouts reject non-finite elements and retain byte-accounted receipts.
`make_greeting_c`, `make_greeting_cpp`, `make_greeting_rust`, and
`make_greeting_zig` prove the owned
string contract with embedded comma/newline and multibyte UTF-8. The receipt
accounts fourteen bytes; invalid UTF-8 is rejected only after the foreign
allocation has been copied and freed exactly once.
`checksum_utf8_c`, `checksum_utf8_cpp`, `checksum_utf8_rust`, and
`checksum_utf8_zig` prove the
borrowed input contract with Unicode, embedded NUL, and newline bytes while the
receipt records only the five-byte footprint.
`persist_bytes_c` proves parent-owned, scoped, immutable publication without
revealing the destination to foreign code. The next composition work is moving
owned artifacts through the audited pipeline. `clamp_zig` proves the first pinned,
process-isolated Zig scalar call path; `sum_i16_zig` and `add_gain_i16_zig`
prove bounded pointer/length input and atomic copy-back; `render_params_zig`
proves exact by-value struct input and output declarations.

## Milestone C: application composition

The first runtime primitive is now `fgx pipeline-run`. It uses existing
`share` and `bind` declarations rather than adding implicit global state. A
stage may have one or more scalar, restricted `@repr(C)` struct, or reviewed
owned-vector inputs, with ordered `read` bindings, and exactly one output with
one `rw` target binding. Source
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
immutable consumers across the original reviewed typed layouts. `Vec[i16]` sample data
feeds an `&[i16]` reducer, whose scalar result seeds a `Vec[i32]` producer and
an `&[i32]` reducer. Repeated runs prove stable values and buffer hashes;
effect denial proves the last committed shared state remains atomic.

`examples/pipeline/extended_integer_buffers.fgx` proves that the same immutable
handoff and canonical receipt contract applies beyond the original layouts. Its
producer returns `Vec[u32]` values above `i32::MAX`; the consumer reads
`&[u32]` and returns an `i64` sum of 6,000,000,000. The runtime records three
elements and twelve canonical bytes without narrowing the data.

`examples/pipeline/float_buffers.fgx` extends the immutable handoff to
`Vec[f32]` and `Vec[f64]`. Matching read-only slice consumers receive the live
native arrays, while `forgentex.pipeline_buffer.v1` records exact finite
little-endian IEEE-754 bit patterns, byte counts, and deterministic hashes.
The `f32` path refuses values that are not exactly representable at the
declared width; both paths reject non-finite values and mutable views before
commit.

`examples/pipeline/wide_integer_buffers.fgx` completes parity with the current
native numeric ownership set. It carries `Vec[u64]` values above `u32::MAX`
through the exact unsigned carrier and carries `Vec[isize]` with host-width byte
accounting. Both feed matching immutable slices and emit canonical integer
receipts; full-range unsigned elements hash as exact little-endian bytes.

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

`examples/pipeline/render_encode_final.fgx` adds the first distinct final-
render dispatch contract. `render.profile final` permits positive dimensions
through 3840x2160, while `preview` remains capped at 1920x1080; both limits are
checked before native dispatch and the selected profile is bound into the
render receipt. The executable fixture uses a small deterministic 64x36 frame
to keep conformance cheap, then proves the same raw-RGB and separately gated
PPM handoff. This is a 4K-bounded control path, not evidence of 4K performance,
GPU rendering, quality sampling, animation, or a production codec.

The raw-frame integrity slice makes that handoff a host-enforced contract
rather than a media-type assertion. `application/x-forgentex-rgb` is eight
little-endian dimension bytes followed by exactly three RGB8 bytes per pixel.
The renderer result must be structurally valid and match its `ScenePlan`
dimensions before shared-state commit; the encoder independently validates the
same structure before native dispatch. Encode receipts now expose the verified
input width, height, and `rgb8` format. Adversarial tests cover a well-sized but
scene-mismatched renderer header and a malformed generic producer, with neither
case reaching a committed governed encode.

`examples/pipeline/render_encode_png.fgx` adds exact codec dispatch beyond the
PPM proof. `encode.profile png` requires `image/png`, while `ppm` now likewise
requires `image/x-portable-pixmap`; mismatched profile/media claims fail before
execution. The contained PNG encoder publishes a deterministic RGB8 image with
IHDR, CRC-protected chunks, a zlib stream of stored DEFLATE blocks, Adler-32,
and IEND. Independent Rust conformance parses the container, checks every CRC
and the Adler checksum, reconstructs all scanlines, and compares every decoded
pixel to the renderer formula. Compression tuning, palettes/alpha, color
management, metadata, animation, audio/video encode/decode, and external codec
libraries remain deliberately open.

The selected-frame manifest slice makes independent frame work auditable.
Render policy may name integer scene fields for zero-based frame index and
total frame count together with an FPS value; the three declarations are
atomic, field names must be distinct from dimensions and seed, count is capped
at 1,000,000, and FPS at 240. Invalid indices/counts fail before renderer
dispatch. `forgentex.render_receipt.v1` records frame index, count, and FPS;
single-frame renderers preserve the explicit `0 / 1 / null` identity. The PNG
fixture renders frame 12 of 240 at 30 FPS and incorporates the authored index
into its deterministic pixels. This selected-frame contract does not itself
provide sequence iteration, scheduling, resumability, or cache reuse; the
separate identity-only cache-key slice below builds on it.

The quality-manifest slice binds how that selected frame is rendered. A second
atomic policy pair names scene fields for samples per pixel and Q16 shutter;
the fields must be distinct from all render identity fields. Preview accepts
1–64 samples, final accepts 1–4096, and shutter accepts 0–65536 (closed through
one full frame). Invalid values fail before native dispatch, successful render
receipts expose both values, and legacy renderers report null quality fields.
The PNG fixture uses 256 samples and a half-frame shutter, with both settings
participating in its decoded deterministic pixels. Multi-pass output, verified
adaptive convergence, and physical motion blur remain open.

The color-contract slice gives the untagged raw RGB handoff one exact optional
meaning. `render.color_space srgb` and `encode.color_space srgb` must appear on
both sides of the handoff or neither; unsupported names and omissions on only
one side fail before encoder dispatch. Both receipts expose `srgb`, while
legacy render/encode pairs retain `null`. The PNG fixture emits a CRC-protected
`sRGB` chunk with perceptual intent, and independent conformance verifies that
chunk alongside every existing checksum and decoded pixel. This does not yet
provide conversion, linear-light working spaces, wide-gamut/ICC profiles, or
color-managed compositing.

The device-contract slice makes renderer selection explicit without claiming a
GPU runtime. Optional `render.device cpu|gpu` must agree with exactly one
matching `cpu.render` or `gpu.render` effect; unsupported values, mismatches,
and dual render effects fail capsule validation. Successful render receipts
carry the selection and legacy policies retain `null`. The executable PNG
fixture proves `cpu`; live GPU capability requests remain blocked before
compiler lookup until a governed GPU runner and identity/budget contract
exists.

The pass-contract slice selects one display-oriented RGB8 visualization from
`beauty`, `depth`, `normal`, or `object_id`. Optional `render.pass` and
`encode.input_pass` values must match across the raw-frame edge; unknown values
and one-sided declarations fail before encoder dispatch. Render and encode
receipts preserve the selected `beauty` pass in the PNG fixture, while legacy
pairs retain `null`. This is not yet simultaneous multi-pass rendering,
floating-point depth/normal data, arbitrary AOVs, or lossless object-ID output.

The adaptive-intent slice extends the quality manifest with an atomic pair of
scene fields for minimum samples and Q16 noise threshold. It requires the
existing samples/shutter pair, all four quality fields must be distinct,
minimum samples are bounded by the authored samples-per-pixel value, and noise
must be from 1 through 65536. Invalid values fail before renderer dispatch;
receipts expose `32 / 655` for the PNG fixture and legacy renderers retain
`null / null`. Both values participate in decoded-pixel conformance, proving
the foreign renderer received them, but this does not prove an adaptive
convergence algorithm or performance benefit.

The engine-identity slice adds an atomic `render.engine` plus
`render.engine_version` pair. Names are bounded lowercase artifact tokens and
versions are exact numeric `major.minor.patch` cores with leading-zero and
integer-overflow refusal. The PNG fixture receipts
`forgentex_software_rgb8@1.0.0`; legacy renderers preserve `null / null`.
Because the sandbox declarations participate in the native boundary hash, an
identity change also changes reviewed execution evidence. This remains
declared identity, not a signed renderer binary, device attestation, or vendor
driver proof.

The frame-cache-identity slice adds optional `render.sequence_id` for renderers
that already declare the complete selected-frame manifest. Sequence IDs use a
bounded lowercase artifact-token grammar. Successful declared-sequence renders
emit a deterministic `frame_cache_key_sha256` binding the sequence, renderer,
canonical scene hash and exact frame/profile/quality settings, engine identity,
effects, native boundary, toolchain and compiled-runner SHA-256, plus the raw
artifact media/path contract. Identical runs preserve the key and changing the
frame identity changes it; legacy and single-frame renderers retain nulls.
This establishes an auditable cache lookup namespace only. There is no frame
store, hit/restore path, sequence iteration, scheduling, resumability, or reuse.

The media-binary-identity slice carries native execution's already verified
runner SHA-256 into the domain receipts as `renderer_binary_sha256` and
`encoder_binary_sha256`. The hash is taken from the exact executable checked
immediately before launch, so it identifies the renderer and encoder bytes
rather than relying on declared names or cache paths. The PNG fixture proves
the two binaries have distinct 64-character identities and that identical
reruns preserve each hash. This is executable-byte attestation, not publisher
signing, a trusted-vendor chain, driver identity, or physical-device proof.

The encoder-engine-identity slice adds an optional atomic `encode.engine` plus
`encode.engine_version` pair alongside that executable hash. It uses the same
bounded lowercase token and exact numeric core-version rules as renderers,
including leading-zero and overflow refusal. PNG receipts now pair declared
`forgentex_png_rgb8@1.0.0` semantics with exact executable bytes, while legacy
PPM encoders retain `null / null`. This is declared codec identity, not a claim
of PNG compression breadth, publisher signing, or external-library provenance.

The deterministic-compression slice makes the PNG fixture's existing container
behavior explicit through atomic `encode.compression stored_deflate` and
`encode.compression_level 0` declarations. The pair is PNG-only; unsupported
modes, one-sided declarations, nonzero/ambiguous/overflowing levels, and PPM
use fail validation. Receipts expose the pair, legacy PPM encoders retain
nulls, and independent conformance already requires every emitted DEFLATE
block to be stored. Fixed/dynamic Huffman coding and production compression
tuning remain open rather than being implied by the level field.

The exact-PNG-layout slice adds an atomic PNG-only `encode.pixel_format rgb8`
plus `encode.interlace none` pair. Unsupported pixel layouts, Adam7, missing
halves, and PPM use fail validation; encode receipts preserve the pair and
legacy PPM remains null. Independent IHDR parsing verifies 8-bit truecolor,
standard compression/filter methods, and non-interlaced output, connecting the
manifest to actual container bytes. Alpha, palettes, grayscale, higher bit
depths, and interlaced PNG remain open.

The exact-scanline-filter slice adds PNG-only `encode.scanline_filter none`.
Other strategies, duplicate declarations, and PPM use fail validation; encode
receipts preserve the value while legacy PPM remains null. Independent
conformance inflates IDAT and verifies filter type 0 at the start of every row,
separately from the IHDR filter-method check. Sub, Up, Average, Paeth, and
adaptive per-row selection remain open.

The PNG-integrity slice adds an atomic PNG-only pair:
`encode.chunk_checksum crc32` and `encode.stream_checksum adler32`. Missing
halves, alternate algorithms, duplicates, and PPM use fail validation; encode
receipts preserve both values while legacy PPM remains null. Independent
conformance recomputes the CRC for every chunk and Adler-32 for the complete
inflated stream. These error-detection checks do not provide artifact signing
or publisher provenance.

The exact-PNG-chunk-order slice adds PNG-only
`encode.chunk_order ihdr_srgb_idat_iend`. Alternate values, duplicates, and
PPM use fail validation; receipts preserve the value while legacy PPM remains
null. Independent parsing now requires exactly one IHDR, sRGB, IDAT, and IEND
in that sequence. Broader ancillary-chunk sets and split-IDAT layouts remain
outside this deterministic fixture contract.

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
- **All scalar local Vec layouts shipped:** the same bounded ownership proof now
  covers `bool`, every signed/unsigned integer primitive (including pointer-sized
  integers), and both floating-point primitives, with layout-specific
  C/Zig allocation, push/pop, indexing, writes, cleanup, host compilation, and
  executed behavior evidence. Struct and string layouts remained explicit blockers at this slice.
- **Enum-element local Vec layouts shipped (`fa68f97`):** declared data-less and
  payload enums now reuse the bounded owned-local Vec representation as native
  value elements. Generated C/Zig layouts cover push, discarded pop, checked
  indexing and writes, payload matching, and save-before-cleanup early returns;
  host-executed tests cover both enum forms. Struct/string elements, aliases,
  escapes, signatures/returns, and value-returning `pop` remained blocked at this slice.
- **Struct-element local Vec layouts shipped (`21968d3`):** backend-lowerable
  named structs now use the same bounded owned-local representation. C emits
  public helpers only after public definitions and private helpers only after
  private definitions; Zig emits helpers after all struct declarations. Tests
  host-execute private, exported, and nested-field layouts with push, discarded
  pop, indexed mutation/field reads, and save-before-cleanup returns. Direct
  string/owned elements, aliases, escapes, signatures/returns, and
  value-returning `pop` remained blocked at this slice.
- **Borrowed-string local Vec layout shipped (`4efa33b`):** local `Vec[str]`
  owns and frees only its bounded C pointer/Zig slice table while literals and
  caller-backed UTF-8 bytes remain immutable borrows for the enclosing call.
  Host-executed tests cover multibyte byte length, content equality/containment,
  indexed replacement, discarded pop, and cleanup. Owned/nested generic
  elements such as `Vec[Option[T]]`, aliases, escapes, signatures/returns, and
  value-returning `pop` remain blocked.
- **Immutable-slice local Vec layouts shipped (`6def056`):** local
  `Vec[&[T]]` for scalar `T` owns only a bounded descriptor table while every
  element buffer remains an immutable caller borrow. C emits helpers after the
  registered slice typedef; Zig generates every scalar-slice layout. Host tests
  cover descriptor replacement, nested checked indexing, both length layers,
  discarded pop, and cleanup. Mutable-slice, owned/generic elements, aliases,
  escapes, signatures/returns, and value-returning `pop` remain blocked.
- **Fixed-scalar-array local Vec layouts shipped (`3f230f5`):** local
  `Vec[[T; N]]` now lowers for scalar `T` and `1 <= N <= 1024` as bounded owned
  value storage. C emits the array wrapper typedef before its vector helper and
  uses the compile-time inner length for checked nested indexing; Zig emits only
  the array layouts used by the program. Normal and optimized builds plus
  host-executed integer, bool, and float tests cover push, indexed replacement,
  nested access, length, discarded pop, and cleanup. Zero-length, excessive,
  nested, and aggregate-element arrays remained blocked at this slice.
- **Nested-scalar-array local Vec layouts shipped (`504247a`):** the fixed-array
  layout now recurses through at most 16 non-empty dimensions and admits at
  most 1,024 total scalar leaves per element. C emits inner array wrapper
  typedefs before their users and vector helper; Zig derives recursive layout
  names only for used vectors. Host-executed C/Zig tests cover value-copy push,
  indexed replacement, three-level bounds checks, discarded pop, and cleanup.
  Exact-limit source tests admit depth 16 and 1,024 leaves, while depth 17,
  1,025 leaves, zero dimensions, and borrowed leaves failed preflight at this
  slice.
- **Borrowed-view array local Vec layouts shipped (`58f0259`):** recursive
  array leaves may now be immutable `str` or `&[scalar]` descriptors as well as
  scalar values. The vector owns and frees only its outer table; every UTF-8 or
  scalar buffer remains caller-owned. C dependency-orders slice and array
  wrappers before vector helpers, and Zig derives recursive borrowed-view names.
  Normal/optimized builds plus host C/Zig execution cover descriptor-array push,
  indexed replacement, nested UTF-8/slice inspection, discarded pop, and
  cleanup. Mutable-slice leaves remain blocked.
- **Named-value array local Vec layouts shipped (`61dff92`):** recursive array
  leaves may now be declared enum or backend-lowerable named-struct values,
  copied into owned vector element storage under the existing 16-dimension and
  1,024-value-leaf limits. C places wrappers after complete enum/public/private
  struct definitions and before dependent structs; Zig derives recursive
  named-value helper names. Normal/optimized generation, host C/Zig execution,
  nested private-struct compilation, and explicit mutable-view/`Option` refusal
  evidence are covered.
- **Immediately consumed local Vec pop values shipped (`5e836ba`):** `pop(v)`
  now infers `Option[T]`, and every supported native element layout can consume
  it directly through `is_some`, `is_none`, or `unwrap_or`. C/Zig helpers
  preserve empty/non-empty behavior without constructing a fake default.
  Literal/already-evaluated fallbacks keep eager cross-backend evaluation
  observationally equivalent; returned options and vector-observing fallbacks
  remain blocked.
- **Stored local Vec pop options shipped (`2ddf921`):** an annotated local
  `Option[T]` may now retain a pop result and later flow through `is_some`,
  `is_none`, or `unwrap_or` for every already-supported vector element layout.
  C emits a layout-specific by-value `{ has_value, value }` representation and
  Zig uses `?T`; neither option owns the vector buffer. Normal and optimized
  generation plus host C/Zig execution cover empty/non-empty scalar, borrowed
  string/slice, fixed and named-value array, enum, and struct cases. Fallbacks
  remain literal/already-evaluated so eager interpreter ordering is preserved;
  returned options still fail preflight.
- **Returned local Vec pop options shipped (`59c15a0`):** a function may return
  `Option[T]` only when every value return is a direct pop or an unmodified
  annotated local produced by pop. C copies the layout-specific option into a
  return temporary before freeing every live vector; Zig evaluates `?T` before
  lexical `defer` cleanup. Normal/optimized generation and host C/Zig execution
  cover empty/non-empty scalar, borrowed string, fixed named-value array, and
  struct returns. A provenance preflight rejects parameter-sourced, reassigned,
  or otherwise unproven options before source emission.
- **Constructed native options shipped (`c31188a`):** context-typed `Some` and
  `None` initialize annotated local `Option[T]` values and direct or
  unmodified-local returns for every existing scalar, borrowed string/slice,
  fixed array, enum, and backend-lowerable struct value layout. C uses fully
  initialized by-value `{ has_value, value }` structs; Zig uses `?T` without
  allocation. Normal/optimized generation and live host C/Zig behavior agree.
  Parameters, reassignment, immediate untyped construction, callee-derived
  return provenance, and owned nested option layouts remain blocked.
- **Nested branch-local Vec ownership shipped:** fresh scalar vectors may be
  declared in arbitrarily nested `if`/`else` scopes. C emits lexical cleanup for
  normal branch exits plus full-stack cleanup before returns; Zig uses `defer`.
- **Loop-local Vec ownership shipped:** C tracks nested loop ownership bases and
  frees the active iteration's vectors on normal completion, `break`, or
  `continue`, without touching outer vectors; returns still clean the full live
  stack. Zig's lexical `defer` provides the matching behavior.
- **Match-arm-local Vec ownership shipped (`d008ea6`):** fresh scalar vectors
  lower inside data-less and payload-enum arms. C frees only the arm suffix on
  normal case exit and retains full-stack save-before-free return cleanup; Zig
  uses the arm block's lexical `defer`. Host-executed C/Zig behavior and
  generated cleanup ordering are covered, while arm-local aliases/escapes stay
  behind `native_vec:ownership_escape`.
- Continue from the local native-Vec foothold to broader source-lowered Vec
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
  inside loops, plus 32, 64, and 128 programs over independent two-, three-,
  and four-level conditional joins in nested loops.
  A slice-range increment adds 49 exhaustive constant/dynamic half-open range
  pairs and 28 range-versus-index liveness cases. A generic-instantiation
  increment adds 16 all-pairs mutable-root calls through zero to three generic
  wrapper layers. An associated-type increment adds 16 all-pairs calls through
  one to four recursively normalized projection layers. Conformance and strict
  release require zero `mutable_borrow_alias` mismatches across all 610 cases.
  The strict release floor is now 610.
- **Three-level loop-join reference slice (`b210d08`):** 64 exhaustive cases
  add an independent third conditional loop target to the existing 32
  two-level cases. Both alias-then-direct and direct-then-alias write orders are
  checked against every initial/outer/middle/inner/direct left/right target
  combination. All cases agree with the production checker, bringing the fixed
  alias/reborrow matrix to 466 and the complete default reference/regression
  cross-check to 1,600 programs before the 40 curated cases.
- **Four-level loop-join reference slice (`7afa686`):** 128 exhaustive cases
  add a fourth conditional loop target to the fixed two- and three-level
  matrices. Both write orders are checked against every initial/outer/second/
  third/inner/direct left/right target combination. All cases agree with the
  production checker, bringing the alias/reborrow matrix to 594 and the complete
  default reference/regression cross-check to 1,800 programs before the 40
  curated cases. Strict release requires the full 594-case alias floor.
- **Generic borrow-carrier reference slice (`3db6b93`):** 72 cases compose
  recursive `Vec`/array carrier depths one through three with generic
  identity/wrapper depths one through three, direct or typed-alias traversal,
  and parameter or deliberately unproven origins. All cases agree with the
  production checker, bringing the lifetime matrix to 276 and the complete
  default reference/regression cross-check to 1,672 programs before the 40
  curated cases. Strict release requires the full 276-case lifetime floor.
- **Associated-type ownership reference slice (`8f809e9`):** concrete
  associated projections now normalize before field/index typing and every
  safety classification. Sixteen move cases distinguish copyable and owned
  leaves, 16 alias cases cover all two-root pairs, and 16 lifetime cases compare
  parameter with unproven origins through one-to-four recursive projection
  layers and direct/typed-alias traversal. All 48 agree with the production
  checker; move/alias/lifetime floors rise to 526/610/292 and the complete
  default reference/regression cross-check rises to 1,848 programs before the
  40 curated cases.
- **Lifetime-provenance reference slice:** an independent parameter/unproven
  origin lattice generates 292 direct, conditional, match, loop/while,
  helper-call, wrapper, field/index projection, deep selective projection, and
  projected self/mutual recursion return programs, including local aliases,
  recursive Vec/array borrow carriers composed with generic identity/wrapper
  chains, associated-type return shapes, and selective returned-argument
  contracts. Strict release requires zero `escaping_reference` mismatches.
- **Seeded soundness-property slice:** four fixed-seed generators add 64 long
  move sequences, 64 descendant-loan sequences, 64 projected-alias sequences,
  and 64 depth-4-to-8 lifetime wrapper chains. All 256 cases have stable replay
  IDs and a separate strict-release mismatch count.
- **Runtime campaign slice:** the default release run adds 32 cases per family
  across moves, descendant aliases, projections, variable-depth nested loops,
  and lifetime wrappers,
  while CLI callers may supply a bounded seed and case count. Versioned replay
  commands identify every case; on mismatch, model-equivalent subsequences or
  shallower wrappers are tested and the smallest reproducer is emitted in the
  JSON report. Four rejection and four acceptance regressions now replay on
  every run;
  `--failure-dir` writes minimized source plus versioned metadata, and weekly CI
  exercises eight seeds at 96 cases per family (4,608 generated programs) while
  retaining failure artifacts.
  Extend next by promoting newly discovered failures when a campaign produces
  a minimized mismatch; this eight-seed validation produced none.
- Extend the shipped bounded task groups and channels only as the borrow rules
  cover nested task ownership, cancellation, suspension, and scheduler joins;
  do not introduce arbitrary threads as a shortcut.

## The 3D-text-music-visualizer vertical slice

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
