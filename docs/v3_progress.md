# v3 Progress Ledger (living document)

**Purpose.** The single place to see how far v3 is: what shipped (with commits
and verification evidence), what is in flight, what is next, and *why* the next
thing is next. Updated with every slice. Companion to the plans in
`v3_application_maturity.md` and `v3_backend_plan.md` — those say what v3 *is*;
this says where we *are*.

**Reading it:** ✅ shipped & verified · 🟡 partial · ⬜ not started.
Every ✅ links a commit; every claim names its verification.

---

## Dashboard

| Track | Milestone | Status | Evidence |
| --- | --- | --- | --- |
| lang.1 control flow | if/else, while, break/continue, match | ✅ | `36c6a89`, `61536e6`, `c6d3e6b`, `d507a8c` — interpreted + C-lowered + CFG-sound, tests in `tests/control_flow.rs` |
| lang.2 sum types | enums, exhaustive match, payload variants, C tagged union | ✅ | `1e5efc3`, `d507a8c`, `fef8275`, `16f8667` — tests in `tests/enums.rs`, host-compiler-gated C test |
| lang.3 collections | Vec, Map, string library (concat/len/slice/find/contains/split) | ✅ interpreter / 🟡 native borrowed String + local/FFI ownership subset | `85525c0` lowers bounded allocation-free String inspection; `b89f7e4` lowers bounded fresh function-local `Vec[i32]`; the reviewed capsule ABI returns byte-accounted `Vec[u8]`, `Vec[i16]`, `Vec[i32]`, and UTF-8-validated owned `str` across C/Rust/Zig with typed deallocation. Vec/String signatures in source backends, Map, and ordinary owned String operations remain FIR-blocked |
| rt.1 live capsules | Python/Node/Go/Java live + C# placeholder; triple-gated; audited; CLI | ✅ | `15e025d`…`7eb4a4e` — real subprocess round-trips in `tests/capsule_runtime.rs` |
| dom.1 program I/O | hosted runtime: fs + net + capsules in one gated, audited run | ✅ sync / 🟡 deterministic concurrency foundation | Cooperative typed tasks, bounded affine task groups with ordered join and cancellation propagation, bounded FIFO channels, and explicit concurrency effects are implemented; real async I/O and scheduling remain |
| v3.0 backend boundary | `Backend` trait, backend catalog, zero-dep default proven | ✅ | `655a5b1` — `fgx backends --json`, dependency-section test |
| v3.1 FGIR | deterministic IR, `fgx ir`, contracts, C/Zig emit from FIR | 🟡 (substantial) | `655a5b1` — receipts + contract preflights; remaining: broaden instruction coverage |
| v3.2–v3.6 backend | IR optimization, Cranelift JIT, REPL/hot-reload, LLVM, DWARF | 🟡 v3.2 slices 1–3: optimizer + `fgx build --opt` + inlining | trap-preserving; -31% C on demo; inline→fold collapses chains to literals; 4 inlining safety guards proven; `tests/optimizer.rs` |
| **lang.4 fn values/generics** | **function values ✅ → generics ✅ → closures ✅ → effectful Fn ✅ → bounds ✅ → traits ✅ → methods/dispatch ✅ → associated constants ✅ → concrete associated types ✅ → generic marker impls ✅ → defaults ✅ → generic impl items ✅ → associated-type projections ✅ → generic Fn binders ✅ → native named/effectful Fn subset ✅ → native non-capturing closure subset ✅** | **🟡 slices 1–6, including 5a–5k, shipped** | `Fn[forall[T: Bound], …]` preserves bounds and instantiates freshly per interpreter call; monomorphic scalar named-function values and contextually typed non-capturing closures lower to C/Zig through the same function-pointer ABI while function-level runtime-effect blockers remain enforced. Closure context covers annotated locals and their reassignments, direct arguments to statically known higher-order functions, and typed `Fn` returns; generic values, captures, and other lambda positions stay blocked; 69 tests in `tests/fn_values.rs` |
| safe.1 borrow hardening | spec + adversarial corpus + fuzz + differential | 🟡 generated reference + persisted campaign slices | Published staged/CFG rules and deterministic traps are backed by 30 curated cases, four checked-in minimal regressions, 510 independently modeled move programs, 277 alias/reborrow programs (including constant/dynamic index pairs and conditional joins inside loops), 132 lifetime-provenance programs (including field/index projections and self/mutual recursion), 256 fixed seeded long-form properties, and a 128-case default runtime-seeded campaign. Weekly CI adds four 256-case seed runs; bounded custom seeds/case counts are replayable, and mismatches persist minimized source plus metadata for artifact retention and corpus promotion. `borrow-conformance` and strict release require zero per-family/corpus mismatches. Deeper projection/recursion combinations, slice-range projections, and richer nested-loop coverage remain |
| rt.2 live data plane | shared namespace live at runtime | 🟡 linear deterministic pipeline | Scalar/struct handoff, governed owned artifacts, reviewed `Vec[u8]`/`Vec[i16]`/`Vec[i32]` immutable buffer handoff with byte/hash receipts, one-writer audit, and linked preview render/encode receipts are live in `fgx pipeline-run`; DAG/fan-in/out/mutable buffers/scheduling remain (`e392cc9`, `0218eb7`, `ee2104e`, `0c7a96b`) |
| rt.3 warm workers | persistent host runtimes, capsule pipelines | 🟡 Python/Node/TS/Go workers, auto-respawn, linear pipelines, and native compile reuse; **warm is the `--live` default** (`--cold` opts out) | Python 8.6×, Go 9.8× (compile-once); pid-proofs, exception/panic survival, transport healing, deterministic pipeline tests, and fresh native processes |
| dom.2–dom.4 | persistence, web runtime, audio/graphics | 🟡 graphics/media foothold | Scoped immutable artifact persistence, a contained deterministic 16x16 renderer, separate governed PPM encoder, linked receipts, and C/Zig-lowerable authored time with versioned cues plus bounded scene-action entries; no 3D/GPU/production codec or audio-analysis runtime yet (`ba68812`, `0218eb7`, `ee2104e`, `244e6be`, `37f0eb6`, `b3e3cef`) |
| app.cap | one real app end-to-end | 🟡 visual.0 pixel + authored-time proof | Versioned cue → validated action link and conflict resolution plus fixed seed → restricted `ScenePlan` → raw RGB → PPM with linked hashes/resource evidence; action manifests and render-receipt cue binding remain (`0218eb7`, `ee2104e`, `244e6be`, `37f0eb6`, `b3e3cef`) |
| v3.7 codebook / v3.8 latent | learned token optimizer / latent envelope | ⬜ (design docs exist) | `89b45a9`, `8b65ea2` |
| **debugger (dev loop)** | trace + breakpoint + post-mortem → conditional/watch → expr-stepping → interactive → DAP | **🟡 v0 shipped (user-prioritized)** | `fgx debug`, `forgentex.debug_trace.v1`, 6 tests in `tests/debugger.rs`; plan in `debugger_plan.md` |

**Test/gate baseline** (updated per slice): 672 lib + 48 CLI = **720 tests**,
clippy `-D warnings` clean, `fgx release-check --target v1.0 --strict` exit 0.

---

## The critical-gap ranking (production-readiness assessment, 2026-07-05)

Honest answer given to "is it production-ready?": **no** — usable today for
typed polyglot orchestration and the agent-governance workflow; not yet for
large complex projects. Gaps, ranked by how hard they block "large & complex":

1. **Incomplete reusable/native abstraction path (`lang.4`)** — function
   values, generic functions, closures, effect-annotated `Fn` contracts,
   built-in `Eq` bounds, user-defined traits/impls, eligible private direct-call
   generic monomorphization, required methods, and checked interpreter trait
   dispatch, unique static C/Zig trait-call specialization, associated
   constants, and concrete associated-type bindings in trait contracts are
   shipped, along with coherent bounded generic/blanket impls. Default methods
   and generic impl associated items are also shipped with safe pure backing
   functions, concrete overrides, symbolic contract substitution, and unique
   native generic specialization. Standalone and bound-generic associated-type
   projections now validate and normalize recursively for interpreter and
   native-specialized use. Generic `Fn` binders preserve bounds and instantiate
   per indirect interpreter call. Monomorphic scalar named-function values and
   indirect calls now lower to C/Zig, including effect-annotated contracts whose
   effects otherwise satisfy the native function contract; runtime-only effects
   remain blocked at function level. Contextually typed non-capturing scalar
   closures are deterministically lifted into private functions for the same
   C/Zig ABI from annotated locals and their reassignments, direct statically
   known higher-order arguments, or typed `Fn` returns. Exported generic ABI policy, native generic function values,
   capturing and remaining-position closures, dynamic dispatch, and broader
   ambiguous specialization remain.
2. **Capsule call latency (`rt.3`)** — warm Python/Node/Go workers,
   auto-respawn, warm-by-default execution, linear pipelines, and host-owned
   native compilation reuse are shipped. JVM/.NET warm workers remain;
   persistent native processes stay deferred until per-call budgets can be
   preserved.
3. **Fast path misses the useful language** — the v3.2 optimizer is live and
   trap-preserving; eligible generics, borrowed String inspection, and bounded
   fresh local `Vec[i32]` now lower to C/Zig. Broader Vec ownership/layouts,
   Map, owned String operations, capturing/remaining-position closures, and JIT remain.
4. **Safety claim not fully hardened (`safe.1`)** — the checker now has 30
   curated cases plus independent differential nets for 510 whole/partial-move
   programs and 277 alias-reborrow programs covering descendant loans,
   shallow/nested/index projections, branch joins, loop-carried uses, and
   conditional joins inside loops, plus 132 lifetime-provenance programs with
   field/index and recursive shapes, 256 fixed seeded long-form properties, and a
   128-case default runtime-seeded campaign with bounded custom seeds, shrinking,
   replay artifacts, four checked-in minimal regressions, and a weekly four-seed
   1,024-case campaign that retains minimized failure files. Field/index and
   deeper projection/recursion shapes, slice-range proofs, and richer nested-loop coverage remain.
5. **No real async scheduling/I/O** — deterministic cooperative tasks, bounded structured task groups, and bounded channels now define the reference contract, but hosted I/O remains blocking and there are no worker threads or wakeups.
6. **Stdlib breadth** — no time, in-language JSON, regex; byte-only strings;
   no `?`-style error propagation.
7. **Dev loop at scale** — ~~no debugger~~ (v0 tracing debugger shipped
   2026-07-08, user-prioritized: `fgx debug` with canonical-node-id traces,
   breakpoints, trap post-mortems); still no profiler/REPL or package
   registry for Forgentex libraries.

Fix order chosen: **1 → 2 → 3 → 4 → 5**, stdlib continuously, capped by a
deliberately small `app.cap` to prove the composition honestly.

---

## Slice log (newest first)

| Date | Commit | Slice | Gate |
| --- | --- | --- | --- |
| 2026-07-17 | `fff5733` | safe.1 reference slice: independently model and exhaustively cross-check 510 root/field move programs across linear flow, branch joins, and loops; enforce zero mismatch in strict release | 664 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-17 | `0c7a96b` | rt.2 typed-buffer pipeline: immutable `Vec[i16]`/`Vec[i32]` handoff, canonical byte/hash receipts, atomic denial, and mutable-view rejection | 663 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-16 | `5a988fb` | lang.4 native closure extension: retain lexical binding types and lift non-capturing scalar lambdas assigned to existing `Fn` locals | 637 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-16 | `c2b15c3` | lang.4 native closure extension: lift non-capturing scalar lambdas from typed `Fn` return positions and return native function pointers in C/Zig | 637 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-16 | `5e3c320` | lang.4 native closure extension: infer inline non-capturing scalar lambda contracts from statically known higher-order parameters and lift them through the existing C/Zig ABI | 637 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-16 | `f140f71` | lang.4 native closure subset: deterministically lift annotated local non-capturing monomorphic scalar lambdas for C/Zig; retain `op:lambda` for captures and unsupported positions | 637 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-16 | `7333f9a` | lang.4 native effectful subset: erase static effect metadata from monomorphic scalar C/Zig function-pointer ABIs while preserving runtime-effect contract blockers | 635 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-16 | `9e7fcd2` | lang.4 native subset: pure monomorphic scalar named-function values, higher-order parameters, reassignment, and indirect calls in C/Zig | 634 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-15 | `be14b0f` | lang.4 slice 6: higher-rank generic `Fn` binders with bound-preserving assignment and fresh indirect-call inference | 633 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-15 | `502447d` | lang.4 slice 5k: standalone/bound-generic associated-type projections with recursive checking, interpreter use, and concrete C/Zig normalization | 631 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-15 | `326b7c5` | lang.4 slice 5j: coherent generic impl methods, constants, and associated types with symbolic checking, interpreter selection, and C/Zig specialization | 628 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `23f4509` | lang.4 slice 5i: safe pure default trait methods with overrides, interpreter fallback, and generic C/Zig specialization | 625 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `01a81ac` | lang.4 slice 5h: coherent bounded generic/blanket marker impls with structural matching and C/Zig erasure | 622 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `fe1ffc9` | lang.4 slice 5g: concrete associated-type bindings with `Self::Item` contract resolution and native specialization | 618 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `633ac99` | lang.4 slice 5f: typed associated constants with explicit impl selection, interpreter evaluation, and C/Zig inlining | 615 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `8ffdc0d` | lang.4 slice 5e: unique concrete trait-call specialization for C/Zig with conservative ambiguity blockers | 612 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `3ccfc7e` | lang.4 slice 5d: checked trait-qualified dispatch through validated impl mappings, with explicit native blockers | 611 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `ff7e766` | lang.4 slice 5c: required trait method contracts with complete exact backing-function mappings | 607 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `30fb96b` | lang.4 slice 5b: user-defined marker traits, explicit coherent impls, and zero-cost native specialization | 605 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `154f8e5` | lang.4 slice 5a: enforced `T: Eq` bounded generics with wrapper propagation and preserved native specialization | 600 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `2ac5c56` | lang.4 slice 4: effect-annotated `Fn` values with subset-compatible contracts and indirect-call/task propagation | 594 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `b89f7e4` | lang.3 native Vec subset: bounded fresh local `Vec[i32]`, explicit allocation/C/Zig cleanup, capacity/ownership blockers | 586 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `85525c0` | lang.3 native String subset: bounded allocation-free length/containment/content equality for C/Zig plus honest owned-operation blockers | 583 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `dcb0aef` | lang.4 native subset: deterministic eligible private generic monomorphization for C/Zig across build/source/opt paths | 580 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `b3e3cef` | visual.0 action edge: versioned `SceneAction`, seeded/bounded kinds, affine-safe cue/action linkage | 578 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `37f0eb6` | visual.0 cue edge: versioned `MusicCue`, bounded validation, half-open conflicts, affine-safe stable winner | 577 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `244e6be` | visual.0 authored-time edge: `core.music` positions, quantization, bounded tempo change, C/Zig-lowerable consumer | 575 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `ee2104e` | visual.0 encoder handoff: raw RGB → separately gated PPM with linked render/encode receipts | 573 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `0218eb7` | visual.0 pixel proof: bounded `ScenePlan` → contained software PPM → governed render receipt | 569 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-14 | `df98e03` | native-call v1.19: bounded C/Rust owned `Vec[i16]` outputs | 565 lib + 48 CLI, clippy 0, v1.0 strict 0 |
| 2026-07-09 | `5f372fb` | v3.2 slice 3: function inlining — call-free candidates, 4 safety guards | 508 tests, clippy 0, v1.0 strict 0 |
| 2026-07-09 | `36c2fe0` | v3.2 slice 2: `fgx build --opt` — optimize before FIR; -31% C on demo | 502 tests, clippy 0, v1.0 strict 0 |
| 2026-07-08 | `8c6bef6` | v3.2 slice 1: optimizer — const-fold, branch-elim, DCE; trap-preserving | 500 tests, clippy 0, v1.0 strict 0 |
| 2026-07-08 | `1ebc372` | rt.3 slice 4: auto-respawn; warm becomes the `--live` default | 493 tests, clippy 0, v1.0 strict 0 |
| 2026-07-08 | `a707891` | rt.3 slices 2–3: warm Node + Go workers — Go 9.8× (compile-once) | 491 tests, clippy 0, v1.0 strict 0 |
| 2026-07-08 | `b536c09` | rt.3 slice 1: warm Python workers — `--warm`, 8.6× on a 10-call loop | 488 tests, clippy 0, v1.0 strict 0 |
| 2026-07-08 | `3eb1bb3` | debugger v0: canonical-node-id traces, breakpoints, trap post-mortem, `fgx debug` | 484 tests, clippy 0, v1.0 strict 0 |
| 2026-07-08 | `ef8650f` | lang.4 slice 3: closures — |x: T| expr, capture by value | 478 tests, clippy 0, v1.0 strict 0 |
| 2026-07-05 | `93171f2` | lang.4 slice 2: generic functions + uniform FIR preflight on raw C/Zig builds | 473 tests, clippy 0, v1.0 strict 0 |
| 2026-07-05 | `fb061ce` | lang.4 slice 1: function values + indirect calls | 468 tests, clippy 0, v1.0 strict 0 |
| 2026-07-05 | `655a5b1` | v3.0 backend boundary + v3.1 FGIR + dom.1 hosted runtime | 461 tests, clippy 0, v1.0 strict 0 |
| 2026-07-02 | `46424ef` | lang.3 string ops (slice/find/contains/split) | 444 tests |
| 2026-07-02 | `afc3134` | lang.3 growable Map[K,V] | 442 tests |
| 2026-07-02 | `fd8cc71` | lang.3 string library (concat/stringify/length) | 438 tests |
| 2026-07-02 | `41a30ef` | lang.3 growable Vec[T] | 434 tests |
| 2026-06-24 | `7eb4a4e` | rt.1 caller effect-level gating | 429 tests |
| 2026-06-24 | `fd1a4a0` | rt.1 .NET placeholder (wired, unverified here) | 427 tests |
| 2026-06-24 | `5999e32` | rt.1 struct (multi-value) capsule outputs | 426 tests |
| 2026-06-24 | `9519189` | rt.1 live JVM (type-directed path) | 424 tests |
| 2026-06-24 | `28db728` | rt.1 live Go (uniform JSON boundary) | 422 tests |
| 2026-06-24 | `6fa0313` | rt.1 live Node/TS + `fgx run --live` | 421 tests |
| 2026-06-24 | `15e025d` | rt.1 keystone: live Python invocation | 419 tests |
| (earlier) | `36c6a89`…`16f8667` | lang.1 complete; lang.2 complete | 393→416 tests |

## Standing conventions (apply to every slice)

- **Slice discipline:** parser (both syntaxes) → formatter (dense `.fgai`
  keyword) → typecheck → interpreter → CFG/borrow soundness → C/Zig (lower or
  honestly contract-block) → tests → docs → full gate → commit.
- **Full gate:** `cargo test` + `cargo fmt` + `cargo clippy --all-targets -- -D
  warnings` + `fgx release-check --target v1.0 --strict` (exit 0).
- **Honesty rules:** never claim verified what didn't run here (e.g. .NET);
  backends must *reject* what they can't lower rather than miscompile; every
  new construct must be modeled in the CFG or the borrow checker is unsound.
- Commits end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
