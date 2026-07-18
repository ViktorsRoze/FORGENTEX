# v3 — Application-Maturity Track (Toward Real Software)

A clear-eyed maturity assessment found that Forgentex today is a strong
*research prototype* of an agent-native, polyglot **governance and
verification** workflow — but it cannot yet build shipping software (audio
editors, DSP processors, agent systems, web apps, trading bots, file-format
tools, 3D visualizers, knowledge apps). Three things are missing, in priority
order. This track plans them. The backend track (`docs/v3_backend_plan.md`,
v3.0–v3.6) supplies the remaining piece — linking full executables.

## The governing policy: borrow the runtime, govern in Forgentex

The zero-dependency rule stays where it earns its keep — the **trusted core**
(parser, AST, typecheck, borrow/effect checker, formatter, patch engine,
diagnostics) remains pure and auditable. But the **runtime, standard library,
and domain layers may borrow heavily from C / C++ / Rust (and some Python)**,
governed exactly like capsules: pinned, checksummed, audited (v2.3), typed at
the boundary, and capability-sandboxed. Reimplementing collections, async I/O,
HTTP, audio, or graphics from scratch would be wasteful and off-thesis.
Forgentex's job is to **govern and wire**; the heavy runtimes are borrowed.

This is the same dependency *boundary* the backend policy already drew, applied
one layer out: pure core, borrowed-and-governed runtime/stdlib/domain.

---

## Track 1 — Live capsule runtime (the keystone, highest priority)

Today capsules execute only against declared `mock` fixtures; the interpreter
never calls them. Until a running program can call capsules with live data and
compose them, **nothing composes at runtime** — this is the single biggest gap.

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `rt.1` | Live capsule invocation | A running Forgentex program calls a capsule with live runtime values (not fixtures): marshal the program's values across the typed boundary, dispatch to the backend (FFI for C/C++/Rust/Zig; subprocess/embed for Python/JS/JVM/.NET/Go), and return the real result into program flow. Gated by effects/sandbox at call time. `forgentex.capsule_call.v1`. Turns capsules from verified specs into live components. **In progress: live Python invocation shipped.** A capsule called like a function (`to_fahrenheit(c)`) is now first-class: the type checker checks the arguments against the capsule's typed inputs and yields its output type; the interpreter marshals the live arguments to JSON, dispatches to the host Python (reusing the v2 bridge, gated by the capsule's sandbox approval), and parses the real result back into a value. It is a guarded capability on the interpreter (`CapsuleHost`) — the default deterministic interpreter forbids live calls (`capsule_call_not_permitted`), so nothing spawns a host process unless the runtime is explicitly enabled — and every call is recorded in an audit transcript (`run_entry_with_capsules`). **Python, Node/TypeScript, and Go** are wired over a uniform JSON boundary (each reusing its v2 bridge): the interpreter marshals a JSON argument array and parses a JSON result, so adding a stdlib-JSON host language is just a driver. Go proves it reaches a compiled, statically-typed runtime — the generated driver `json.Unmarshal`s each argument into its typed parameter, calls the entry, and `json.Marshal`s the result. There is a CLI entry point: `fgx run <file> --entry <fn> --json-input <in> --live` runs with the live runtime on and emits the result plus the `capsule_calls` audit transcript as JSON. **JVM (Java)** is wired too via the type-directed path it requires (no stdlib JSON): the generated driver parses each CLI argument into its declared type and prints the result in JSON form (numbers/booleans as-is, strings quoted), so the interpreter still parses back uniformly — verified live for both numeric and string outputs. Four host languages are now live (Python, Node/TypeScript, Go, Java), all covered by host-gated subprocess tests. **Multiple values come back as a struct:** a capsule may declare a struct output, the host returns a JSON object, and the runtime tags it with the declared struct type so typed code consumes it by field access (no tuples needed). **.NET (C#)** is also wired over the same type-directed boundary as the JVM, as a best-effort placeholder — it compiles and runs through `dotnet build`, but is **unverified on a box without a .NET SDK**, so its host-gated test skips locally and only proves out in CI. (Practically, the target apps don't require .NET; it is kept wired so the dispatch is complete, not a gap.) **Effect-level gating** is in: beyond the compile-time sandbox-coverage check, the caller can restrict which effects a live capsule may use (`CapsuleHost::with_effects`, `fgx run --live --allow-effects a,b`). A capsule declaring an ungranted effect is denied *before* it reaches a host (`capsule_effect_denied`) and the denial is audited — so the deny path needs no runtime and is tested unconditionally. Two layers now guard a live call: static sandbox coverage and the runtime caller allowlist. **Native dispatch is live too:** C/C++/Rust and pinned Zig scalar, slice, restricted-struct, and bounded owned-buffer calls run in contained child processes with effect gates, resource budgets, structured receipts, and compiled-runner reuse. Remaining: broader typed/native outputs, aggregate Unix process-tree budgets, safely budgeted persistent native workers, and GPU dispatch. |
| `rt.2` | Live shared-namespace data plane | **In progress — deterministic linear data plane shipped:** `fgx pipeline-run` moves checked scalar/restricted-struct values, governed owned byte artifacts, and reviewed `Vec[i16]`/`Vec[i32]` buffers through explicit `share`/`bind` declarations with one writer, immutable typed reads, canonical byte/hash receipts, stage audit, and linked render/encode receipts. Remaining: explicit graphs, fan-in/out, mutable and additional typed buffers, cancellation, pipeline-wide budgets, cache keys, and parallel scheduling. |
| `rt.3` | Warm workers + capsule pipelines | Persistent host-runtime workers remove per-call spawn/compile cost, while a bounded pipeline primitive composes governed stages. **In progress — Python, Node/TypeScript, and Go warm workers shipped; warm is the `--live` default:** one persistent process per capsule per run, line-delimited JSON, exception/panic survival, and one respawn/retry after transport death. Measured Python 10-call loop: 1352ms cold → 157ms warm (8.6×); Go: 7534ms → 766ms (9.8×). Native C/Rust/Zig calls reuse compiled runners but retain fresh contained processes and per-call budgets. `fgx pipeline-run` now provides deterministic linear scalar/struct/owned-byte/typed-buffer handoff, one-writer audit, and linked render/encode receipts. Remaining: JVM/.NET workers, safely budgeted persistent native processes, and general graph/control-flow orchestration with fan-in/out, cancellation, pipeline budgets, caching, and scheduling. |

## Track 2 — Language breadth (so app logic can be written in `.fgx`)

The language is currently tiny: `if` with no `else`, `for` over ranges only,
no `while`/`match`, fixed-size arrays only, no growable collections, no string
library, no general enums/closures/generics. You cannot write a parser or
business logic in it yet.

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `lang.1` | Control-flow completeness | `else`/`else if`, `while`, `break`/`continue`, and `match`. Each round-trips in both syntaxes, type-checks, interprets, and lowers to C. **Every new keyword follows the dual-syntax keyword convention below: a long readable word in `.fgx`, a ~2-letter dense token in `.fgai`.** The minimum to write real branching logic. **Complete.** `else`: `else_body` on `Stmt::If`, dense `EL`/`EI`, real CFG branch (sound join). `while`: `Stmt::While`, dense `WH`, `while_not_bool` check, CFG loop with back edge (sound loop-carried liveness). `break`/`continue`: `Stmt::Break`/`Stmt::Continue`, dense `BR`/`CN`, interpreter `Control` variants, `break_outside_loop`/`continue_outside_loop` check, and CFG edges to the loop exit/header via a loop-target stack (sound). `match`: `Stmt::Match` over enums (see `lang.2`), dense `MT` with `<pat> => { … }` arms, modeled as an N-way CFG branch so every arm is analyzed and merged (sound). |
| `lang.2` | Sum types + pattern matching | General `enum`s with data variants (beyond the hard-coded `Option`/`Result`) and exhaustive `match`, foundational for real domain modeling. **Shipped.** *Data-less enums:* `Item::Enum` in both syntaxes (dense `EN Name { A; B; C }`), variant values (`Color::Red`, lexed as a single path token so no new `Expr` node), structural equality, type-checked (`unknown_enum` / `unknown_enum_variant`), interpreted, lowered to C as a `typedef enum`. *Exhaustive `match`:* `Stmt::Match` with variant patterns and a `_` wildcard, dense `MT … { <pat> => { … } }`, an exhaustiveness check (`non_exhaustive_match` names the missing variants), `match_not_enum` / `pattern_type_mismatch`, first-match interpretation, C `switch`, and a sound N-way CFG branch. *Payload-carrying variants:* a variant may carry one optional typed payload (`Circle(i32)`; multi-field via a struct payload), constructed `Shape::Circle(r)` (a `::` call), with binding patterns `case Shape::Circle(r):` that destructure the payload into a typed arm local. Arity and payload-type are checked (`variant_arity`, `variant_payload_type`, `binding_on_data_less_variant`); the binding's type flows into the arm; the borrow checker tracks it as a fresh owned local (CFG def + safety env) so a moved payload is caught. *Native lowering:* a payload enum lowers to a C **tagged union** (`struct { enum tag; union { … } data; }`); construction is a compound literal (`(Shape){ .tag = …, .data.Circle = … }`) and `match` switches on `.tag`, binding the payload out of the union into a typed arm local. The generated C is exercised by a host-compiler-gated test. So all of `lang.2` — data-less enums, exhaustive `match`, and payload variants — round-trips, type-checks, interprets, borrow-checks, and compiles to native C. |
| `lang.3` | Collections + strings | Growable `Vec`/`List`, a `Map`, and a real `String`/string library — backed by a small borrowed Rust/C runtime, exposed as governed core types. The data structures every application needs. **In progress: growable `Vec[T]` shipped (interpreter).** The first dynamic data structure: `vec_new()` makes an empty vec, `push(v, x)` / `pop(v)` mutate it in place, `v[i]` indexes, and `v.len` is its length. It reuses the `Type::Generic` machinery (no new type node) and the interpreter's array value; the type checker knows `Vec[T]` indexes to `T` and `.len` is `usize`, `vec_new()` binds polymorphically to an annotated `Vec[T]` (same-constructor generic assignability), and the borrow checker treats `push`/`pop` as borrows (not moves) while a `Vec` passed to a user function still moves (sound). **String library started too:** `str_concat(a, b)` and `int_to_str(n)` build strings, `str_len(s)` / `s.len` measure them (byte length), all type-checked (`str`/`usize`) and interpreted. **`Map[K, V]` shipped:** `map_new()`, `map_set(m, k, v)` (insert or overwrite), `map_get(m, k)` → `Option`, `map_has(m, k)` → `bool`, `map_remove(m, k)` → `Option`, and `m.len` — insertion-ordered (deterministic runs), any-key equality, typed via the same generic machinery as `Vec`. **String ops complete for the interpreter surface:** `str_slice(s, a, b)` (byte range, traps on invalid bounds), `str_find` → `Option`, `str_contains` → `bool`, and `str_split(s, sep)` → `Vec[str]` (composing with vec indexing/len). **Native owned subset shipped (`b89f7e4`):** C/Zig lower fresh, annotated function-local `Vec[i32]` values with `push`, discarded `pop`, `.len`, checked indexing, and indexed writes. Growth requires explicit `effects [alloc]` and is capped at 1,048,576 elements; C inserts cleanup on every return after evaluating the result, while Zig uses `defer`. Aliases, signatures/returns, nested declarations, non-`i32` elements, value-returning `pop`, direct allocator calls, and calls that could escape ownership retain FIR blockers. Remaining: broader native Vec ownership/element layouts, Map, and owned String operations. |
| `lang.4` | Generics + closures/traits | Parametric types, first-class functions, and interface dispatch, for reusable libraries. **In progress — slices 1–6, including 5a–5k, shipped:** named/effectful/generic function values, inferred generics, by-value closures, deterministic private C/Zig specialization, structural `Eq`, explicit traits/impls, required/default pure methods, checked/native-specialized dispatch, associated constants/types/projections, and bounded generic/blanket impls. Slice 6 (`be14b0f`) adds higher-rank `Fn[forall[...], ...]` contracts; `9e7fcd2` lowers the pure monomorphic scalar named-function subset and indirect calls to C/Zig, `7333f9a` extends that ABI to effect-annotated signatures while preserving function-level runtime-effect blockers, and `f140f71`/`5e3c320`/`c2b15c3`/`5a988fb` deterministically lift non-capturing scalar lambdas from annotated locals and their reassignments, direct statically known higher-order arguments, or typed `Fn` returns into private C/Zig functions. Remaining: native generic function values, capturing and remaining-position closures, exported generic ABI policy, and dynamic trait dispatch. Design record: `docs/lang4_plan.md`. |

**Native lang.3 update (`85525c0`).** The allocation-free borrowed String
surface now lowers to C/Zig: byte length (`str_len` and `.len`), containment,
and content equality/inequality share a sub-16-MiB bound. A host C compiler and
executed harness prove UTF-8 byte length, empty-needle behavior, and equality.
Owned construction/slicing, Option search, split-to-Vec, and Map remain
explicitly blocked until their ownership/deallocation representation ships.

**Native lang.3 owned-Vec update (`b89f7e4`).** A conservative local
`Vec[i32]` representation now lowers across C/Zig source, normal build, and
optimized build paths. Growth requires `effects [alloc]`; it has a hard
1,048,576-element limit,
allocation-failure/capacity traps, bounds checks, and exactly-once cleanup on
live return paths (explicit C cleanup; Zig `defer`). Native C compilation and
execution prove push/pop/index/length/write behavior, early returns, and the
capacity trap. Whole-vector aliases, escapes, signatures/returns, nested Vec
declarations, other element types, and value-returning `pop` are intentionally
FIR-blocked until source lowering gains signature and escape-aware ownership.

**Native lang.3 capsule ownership update (v1.22–v1.23).** The process-isolated
capsule ABI now returns `Vec[u8]`, `Vec[i16]`, `Vec[i32]`, and owned UTF-8 `str`
across C, Rust, and pinned Zig 0.14. Typed descriptors use byte-accounted
length/capacity, a 64-MiB ceiling, copy-before-free, exact allocator-domain
deallocators, and parent-side element/UTF-8 validation. String bytes use a
numeric private envelope, preserving embedded separators and multibyte text.
This expands the governed foreign boundary; it does not yet make Vec/String
signatures or owned operations source-lowerable inside ordinary C/Zig
functions.

### Dual-syntax keyword convention (`.fgai` brevity)

Binding rule for every keyword added from `lang.1` onward: **`.fgx` keeps the
long, readable word for human review; `.fgai` uses a dense ~2-letter token for
agents.** Same AST node, two views — this is the dual-syntax thesis applied to
keywords. Proposed table (finalize at `lang.1`, avoiding collisions with the
existing `S`/`F`/`R`/`UF`/`LET`/`SET`/`REQ`/`IF`/`FOR` tokens):

| Concept | `.fgx` (human) | `.fgai` (agent) |
| --- | --- | --- |
| if | `if` | `IF` *(existing)* |
| else | `else` | `EL` |
| else if | `else if` | `EI` |
| while | `while` | `WH` |
| match | `match` | `MT` |
| match arm | `case <pat>:` | `<pat>=>` |
| break | `break` | `BR` |
| continue | `continue` | `CN` |
| enum | `enum` | `EN` |
| for | `for` | `FOR` *(existing; may shorten to `FR`)* |

Density rules for the `.fgai` surface:

- **Keyword brevity is the primary lever** — the measured tokenizer
  (`docs/token_efficiency_findings.md`) shows short keywords and fewer
  newlines move tokens, while single spaces are nearly free under BPE. So
  prioritize 2-letter keywords and statement/brace compaction; keep just
  enough whitespace to tokenize unambiguously.
- The canonical `.fgai` format minimizes insignificant whitespace (no padding
  before `{`, single-space token separation), and the `agent-min` surface
  (`minify_agent`) drops indentation entirely.
- `.fgx` is never compressed this way — it stays fully readable. Round-trip
  (`canonical_eq`) must hold across both surfaces.

## Track 2.5 — Borrow-checker hardening (research-grade → proper)

The v1.0 borrow checker is real and passes 30 curated conformance cases plus a
510-program independently modeled move-state matrix, but it is not yet
*production-grade*. This milestone moves it to proper engineering
rigor, and it proceeds **in lockstep with `lang.2`–`lang.4`** — the checker
must soundly cover every construct the language gains.

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `safe.1` | Production-grade borrow checker | The points below, gated in `release-check`. |

What "proper" means here, concretely:

- **Soundness is the hard invariant — no false negatives.** The checker must
  *never* accept a program that violates ownership/aliasing (rejecting some
  safe programs is acceptable; accepting an unsafe one is not). Established by
  three independent nets:
  1. a large **adversarial conformance corpus** — the first generated slice now
     covers 510 exhaustive whole-value/partial-field move programs across
     straight-line flow, branch joins, and loop back-edges. Expand the same net
     to reborrow chains, drop ordering, early-return/panic paths, and
     self-referential attempts;
  2. **property-based + fuzz testing** that generates programs and asserts the
     soundness invariant directly (reuses the existing fuzz harness);
  3. a **differential cross-check against an independent reference model** —
     shipped for owned root/field move availability (`forgentex.borrow_reference.v1`),
     with zero mismatches required by strict release checks. Extend the separate
     model to alias, reborrow, and lifetime rules so two implementations must
     agree across the wider safety surface.
- **A written specification of the borrow rules** — the model the checker
  implements, so behavior is *defined*, not emergent, and the reference model
  and the implementation share one spec.
- **Precision — reduce false positives.** Tighten non-lexical borrow endings,
  add two-phase borrows, and improve interprocedural lifetime inference, with a
  tracked false-positive regression corpus so valid safe programs are not
  needlessly rejected.
- **Full-language coverage.** Sound handling of every construct the language
  gains: `enum` variant bindings and `match` patterns (`lang.2`), element
  borrows of growable collections (`lang.3`), closures capturing by reference
  or move, and generics (`lang.4`).
- **Every rejection carries a witness** (already via `fgx borrow-report`) and
  every acceptance is backed by the soundness corpus.
- **CI gate:** the conformance + fuzz + differential suites run in
  `fgx release-check`; zero soundness failures required to ship.

Honest scope: this reaches *production-grade engineering rigor* (written spec,
large adversarial + fuzz + differential testing, full-language coverage,
soundness-as-invariant). A **machine-checked formal soundness proof**
(RustBelt-level) is a stretch goal beyond this milestone, not a requirement of
it.

## Track 3 — Domain bindings / runtimes (the actual app capabilities)

Each domain capability is delivered as a **governed capsule over C/C++/Rust
(and some Python)** — borrowed, not reimplemented.

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `dom.1` | Async + real program I/O | Extend the shipped deterministic tasks, bounded structured task groups, cancellation propagation, and channels with real scheduling plus file/network I/O *inside running programs* (generalizing the gated `io-run`/`net-run` demo builtins into first-class effectful program I/O). Foundational for servers, bots, and file tools. |
| `dom.2` | Persistence + custom file formats | Real binary read/write and serialization, and a codec-authoring path building on the existing bit ops + ABI/layout — enough for custom file formats and knowledge-base storage. |
| `dom.3` | Web / server runtime | A real HTTP server runtime (a governed Rust/Go capsule) so the "web one-roof" becomes a deployable app serving requests, with the typed shared session live. |
| `dom.4` | Audio + graphics runtimes | miniaudio (real-time audio I/O) and a graphics/GPU binding (wgpu/Vulkan via a Rust/C capsule) for audio editors and 3D visualizers. Heavy, latest, fully governed. |

## App capstone (definition of done)

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `app.cap` | Ship one real app end-to-end | Build and run *one* application from the target list end to end on the new runtime — most likely a small **web app** (backend + frontend capsules over `dom.3`, live shared session) or a **custom file-format tool** (`dom.2`) — as proof the tracks compose into shipping software. |

## Dependency order

```text
rt.1 live invocation ──┬── rt.2 live data plane ── rt.3 warm workers/pipelines ──┐
                       │                                                          │
lang.1 control flow ── lang.2 sum types ── lang.3 collections/strings ── lang.4 ─┤── app.cap
        │                    │                    │                  │            │
        safe.1 borrow-checker hardening tracks lang.2/3/4 (sound coverage) ───────┤
                                                                                  │
backend v3.0–v3.6 (linked executables) ───────────────────────────────────────── │
                                                                                  │
dom.1 async+IO ── dom.2 persistence ── dom.3 web ── dom.4 audio/graphics ─────────┘
```

Build order, highest priority first: **`rt.1` (the keystone) → `lang.1` →
`lang.3` → `dom.1` → `rt.2`/`rt.3` and `dom.2/3/4` in parallel → `app.cap`**.
`rt.1` plus `lang.1`/`lang.3`/`dom.1` is the minimum viable "build a real
program" set; everything else widens what kinds of apps are reachable.

## Honest framing

This is the path from "research prototype + governance toolchain" to "you can
build real software with it." It is multi-quarter work, and it deliberately
leans on borrowed C/C++/Rust/Python runtimes — that is the point. Forgentex's
durable value is being the **typed, gated, audited, token-efficient layer that
brings those runtimes and agents together**, not reimplementing them. The pure
core stays pure; the app stack stands on borrowed, governed shoulders.
