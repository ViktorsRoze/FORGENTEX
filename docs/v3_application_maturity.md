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
| `rt.1` | Live capsule invocation | A running Forgentex program calls a capsule with live runtime values (not fixtures): marshal the program's values across the typed boundary, dispatch to the backend (FFI for C/C++/Rust/Zig; subprocess/embed for Python/JS/JVM/.NET/Go), and return the real result into program flow. Gated by effects/sandbox at call time. `forgentex.capsule_call.v1`. Turns capsules from verified specs into live components. **In progress: live Python invocation shipped.** A capsule called like a function (`to_fahrenheit(c)`) is now first-class: the type checker checks the arguments against the capsule's typed inputs and yields its output type; the interpreter marshals the live arguments to JSON, dispatches to the host Python (reusing the v2 bridge, gated by the capsule's sandbox approval), and parses the real result back into a value. It is a guarded capability on the interpreter (`CapsuleHost`) — the default deterministic interpreter forbids live calls (`capsule_call_not_permitted`), so nothing spawns a host process unless the runtime is explicitly enabled — and every call is recorded in an audit transcript (`run_entry_with_capsules`). **Python, Node/TypeScript, and Go** are wired over a uniform JSON boundary (each reusing its v2 bridge): the interpreter marshals a JSON argument array and parses a JSON result, so adding a stdlib-JSON host language is just a driver. Go proves it reaches a compiled, statically-typed runtime — the generated driver `json.Unmarshal`s each argument into its typed parameter, calls the entry, and `json.Marshal`s the result. There is a CLI entry point: `fgx run <file> --entry <fn> --json-input <in> --live` runs with the live runtime on and emits the result plus the `capsule_calls` audit transcript as JSON. **JVM (Java)** is wired too via the type-directed path it requires (no stdlib JSON): the generated driver parses each CLI argument into its declared type and prints the result in JSON form (numbers/booleans as-is, strings quoted), so the interpreter still parses back uniformly — verified live for both numeric and string outputs. Four host languages are now live (Python, Node/TypeScript, Go, Java), all covered by host-gated subprocess tests. **Multiple values come back as a struct:** a capsule may declare a struct output, the host returns a JSON object, and the runtime tags it with the declared struct type so typed code consumes it by field access (no tuples needed). **.NET (C#)** is also wired over the same type-directed boundary as the JVM, as a best-effort placeholder — it compiles and runs through `dotnet build`, but is **unverified on a box without a .NET SDK**, so its host-gated test skips locally and only proves out in CI. (Practically, the target apps don't require .NET; it is kept wired so the dispatch is complete, not a gap.) **Effect-level gating** is in: beyond the compile-time sandbox-coverage check, the caller can restrict which effects a live capsule may use (`CapsuleHost::with_effects`, `fgx run --live --allow-effects a,b`). A capsule declaring an ungranted effect is denied *before* it reaches a host (`capsule_effect_denied`) and the denial is audited — so the deny path needs no runtime and is tested unconditionally. Two layers now guard a live call: static sandbox coverage and the runtime caller allowlist. **Native dispatch is live too:** C/C++/Rust and pinned Zig scalar, slice, restricted-struct, and bounded owned-buffer calls run in contained child processes with effect gates, CPU/memory/wall/combined-output budgets, deadlock-free concurrent response draining, structured receipts, and compiled-runner reuse. Remaining: broader typed/native outputs, aggregate Unix process-tree budgets, safely budgeted persistent native workers, broader OS isolation, and GPU dispatch. |
| `rt.2` | Live shared-namespace data plane | **In progress — deterministic cancellable budgeted dependency data plane shipped:** `fgx pipeline-run` moves checked scalar/restricted-struct values, governed owned byte artifacts, and all eleven native-reviewed numeric vector layouts through immutable reads, exclusive atomic move, legacy fixed-length update, or explicit `name@N` sequential update contracts. Derived and source-declared DAGs provide exact manifests, stable topological order, fan-out/fan-in, cancellation, byte/hash evidence, and pipeline-wide stage/compute/memory/owned-byte limits; Windows additionally enforces cumulative native-stage filesystem transfers. Non-sensitive stages emit exact cache identities. A caller-owned bounded `PipelineCache` restores eligible read-only native `cpu`/`alloc` results from memory or persistent versioned files after revalidating request identity, every component hash, output type/bytes/SHA-256, cancellation, and budgets. Explicit trusted roots use no-clobber integrity records; `authenticated_persistent` and CLI `--cache-key-file` add a non-downgradable v2 HMAC-SHA256 record with constant-time tag verification, wrong-key/tamper refusal, key redaction, and cross-process tests. Pathless governed `Vec[u8]` artifacts now restore their immutable payload and reconstruct `forgentex.native_artifact.v1` from current media/size policy plus exact bytes/hash; policy changes miss. Direct/transitive secrets, path-bearing artifacts, render/encode, move/update, managed, and I/O-accounted stages remain ineligible. HMAC authenticates the key-sharing group but neither encrypts values nor identifies one signer. Remaining: aggregate buffers, active-stage interruption/resume, portable filesystem/network budgets, filesystem publication replay, asymmetric verifier-only provenance, retries/resume, and parallel scheduling. |
| `rt.3` | Warm workers + capsule pipelines | Persistent host-runtime workers remove per-call spawn/compile cost, while a bounded pipeline primitive composes governed stages. **In progress — Python, Node/TypeScript, and Go warm workers shipped; warm is the `--live` default:** one persistent process per capsule per run, line-delimited JSON, exception/panic survival, and one respawn/retry after transport death. Measured Python 10-call loop: 1352ms cold → 157ms warm (8.6×); Go: 7534ms → 766ms (9.8×). Native C/C++/Rust/Zig calls reuse compiled runners but retain fresh contained processes and per-call budgets. Pipelines provide deterministic cancellable typed handoff, derived/source DAGs, immutable fan-out, ordered fan-in, exact stage identities, bounded process-local restoration, trusted-root integrity checking, HMAC-authenticated cross-process reuse, and pathless immutable artifact restoration. Remaining: JVM/.NET workers, safely budgeted persistent native processes, active-stage interruption/resume, filesystem artifact replay/asymmetric reuse, and parallel scheduling. |

**Native rt.1 boundary update (2026-08-09).** C/C++/Rust/Zig capsules now
receive bounded borrowed UTF-8 `str` inputs through an explicit read-only
pointer-length ABI and fixed-byte-length mutable `&mut str` through a mutable
pair. Bounded resizable `&mut String` adds a mutable byte-length pointer and
immutable 16-MiB capacity; aggregate reservation stops at 64 MiB. Private
framing preserves NUL/newline bytes, each borrow lasts only for the call, and
receipts record byte counts without payloads. Mutable arguments must be direct
variables backed by runner-owned temporary bytes; the parent commits copy-back
only after the full response preserves required fixed lengths, respects
resizable capacity, and validates as UTF-8. Mixed mutable strings and scalar
slices commit as one transaction only after exact input coverage, non-aliasing,
type, length, and capacity validation.
One owned vector of a reviewed pointer-free scalar-leaf `@repr(C)` record now
returns through an exact typed descriptor/free pair in all four native
languages. The 64-MiB limit uses host ABI record size; runners copy complete
records before same-domain free, and the parent admits only complete validated
leaf groups before atomically rebuilding nested records/arrays and restoring
record tags. Immutable `&[Record]` inputs now use the same layout as a
runner-owned typed array behind a call-scoped const pointer/length pair. Exact
record/leaf validation, one-million-record and cumulative 64-MiB ABI-size
limits, and payload-free borrowed-byte receipts apply. Mutable aggregate
copy-back and record-vector pipeline handoff remain open.
Native runners without `limit.wall_ms` now receive a mandatory 30,000 ms
deadline, while first-time compiler/linker trees are contained under a hard
60,000 ms deadline. Receipts expose both the effective runner policy source
and the compile ceiling.

## Track 2 — Language breadth (so app logic can be written in `.fgx`)

The language is currently tiny: `if` with no `else`, `for` over ranges only,
no `while`/`match`, fixed-size arrays only, no growable collections, no string
library, no general enums/closures/generics. You cannot write a parser or
business logic in it yet.

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `lang.1` | Control-flow completeness | `else`/`else if`, `while`, `break`/`continue`, and `match`. Each round-trips in both syntaxes, type-checks, interprets, and lowers to C. **Every new keyword follows the dual-syntax keyword convention below: a long readable word in `.fgx`, a ~2-letter dense token in `.fgai`.** The minimum to write real branching logic. **Complete.** `else`: `else_body` on `Stmt::If`, dense `EL`/`EI`, real CFG branch (sound join). `while`: `Stmt::While`, dense `WH`, `while_not_bool` check, CFG loop with back edge (sound loop-carried liveness). `break`/`continue`: `Stmt::Break`/`Stmt::Continue`, dense `BR`/`CN`, interpreter `Control` variants, `break_outside_loop`/`continue_outside_loop` check, and CFG edges to the loop exit/header via a loop-target stack (sound). `match`: `Stmt::Match` over enums (see `lang.2`), dense `MT` with `<pat> => { … }` arms, modeled as an N-way CFG branch so every arm is analyzed and merged (sound). |
| `lang.2` | Sum types + pattern matching | General `enum`s with data variants (beyond the hard-coded `Option`/`Result`) and exhaustive `match`, foundational for real domain modeling. **Shipped.** *Data-less enums:* `Item::Enum` in both syntaxes (dense `EN Name { A; B; C }`), variant values (`Color::Red`, lexed as a single path token so no new `Expr` node), structural equality, type-checked (`unknown_enum` / `unknown_enum_variant`), interpreted, lowered to C as a `typedef enum`. *Exhaustive `match`:* `Stmt::Match` with variant patterns and a `_` wildcard, dense `MT … { <pat> => { … } }`, an exhaustiveness check (`non_exhaustive_match` names the missing variants), `match_not_enum` / `pattern_type_mismatch`, first-match interpretation, C `switch`, and a sound N-way CFG branch. *Payload-carrying variants:* a variant may carry one optional typed payload (`Circle(i32)`; multi-field via a struct payload), constructed `Shape::Circle(r)` (a `::` call), with binding patterns `case Shape::Circle(r):` that destructure the payload into a typed arm local. Arity and payload-type are checked (`variant_arity`, `variant_payload_type`, `binding_on_data_less_variant`); the binding's type flows into the arm; the borrow checker tracks it as a fresh owned local (CFG def + safety env) so a moved payload is caught. *Native lowering:* a payload enum lowers to a C **tagged union** (`struct { enum tag; union { … } data; }`); construction is a compound literal (`(Shape){ .tag = …, .data.Circle = … }`) and `match` switches on `.tag`, binding the payload out of the union into a typed arm local. The generated C is exercised by a host-compiler-gated test. So all of `lang.2` — data-less enums, exhaustive `match`, and payload variants — round-trips, type-checks, interprets, borrow-checks, and compiles to native C. |
| `lang.3` | Collections + strings | Growable `Vec`/`List`, a `Map`, and a real `String`/string library — backed by a small borrowed Rust/C runtime, exposed as governed core types. The interpreter supports generic growable vectors, deterministic maps, and the String operation surface. **Native owned subset expanded:** C/Zig lower fresh annotated function-, nested branch-, loop-, and match-arm-local `Vec[T]` values for scalar primitives, recursively fixed arrays of scalar, declared enum, backend-lowerable named-struct, or immutable borrowed string/scalar-slice leaves with at most 16 dimensions and 1,024 total value leaves, borrowed immutable strings/scalar slices, declared enums, and backend-lowerable named structs, with push, discarded/immediately consumed pop values or stored local pop `Option[T]` values later handled by `is_some`/`is_none`/`unwrap_or` or returned unchanged; direct pop returns, context-typed `Some`/`None` locals and proven returns, length, checked indexing, and indexed writes are also covered. Borrowed elements and borrowed-view arrays own only their descriptor table; fixed scalar and named-value arrays are copied into owned element storage. Growth requires explicit `effects [alloc]` and is capped at 1,048,576 elements per vector; C saves returned options before path-correct cleanup on normal scope/arm exit, `break`, `continue`, and return, while Zig evaluates them before lexical `defer` cleanup. Aliases, Vec signatures/returns, mutable-view/other unsupported owned-generic array leaves and element layouts, option parameters/reassignment/immediate untyped construction, parameter/callee-derived returns, state-dependent pop fallbacks, direct allocator calls, and calls that could escape ownership retain FIR blockers. Remaining: broader native Vec ownership and elements, Map, and owned String operations. |
| `lang.4` | Generics + closures/traits | Parametric types, first-class functions, and interface dispatch, for reusable libraries. **In progress — slices 1–6, including 5a–5k, shipped:** named/effectful/generic function values, inferred generics, by-value closures, deterministic private C/Zig specialization, structural `Eq`, explicit traits/impls, required/default pure methods, checked/native-specialized dispatch, associated constants/types/projections, and bounded generic/blanket impls. Slice 6 (`be14b0f`) adds higher-rank `Fn[forall[...], ...]` contracts; `9e7fcd2` lowers the pure monomorphic scalar named-function subset and indirect calls to C/Zig, `7333f9a` extends that ABI to effect-annotated signatures while preserving function-level runtime-effect blockers, and `f140f71`/`5e3c320`/`c2b15c3`/`5a988fb` deterministically lift non-capturing scalar lambdas from annotated locals and their reassignments, direct statically known higher-order arguments, or typed `Fn` returns into private C/Zig functions. Remaining: native generic function values, capturing and remaining-position closures, exported generic ABI policy, and dynamic trait dispatch. Design record: `docs/lang4_plan.md`. |

**Native lang.3 update (`85525c0`).** The allocation-free borrowed String
surface now lowers to C/Zig: byte length (`str_len` and `.len`), containment,
and content equality/inequality share a sub-16-MiB bound. A host C compiler and
executed harness prove UTF-8 byte length, empty-needle behavior, and equality.
Owned construction/slicing, Option search, split-to-Vec, and Map remain
explicitly blocked until their ownership/deallocation representation ships.

**Native lang.3 owned-Vec update (`b89f7e4`, expanded through `5e836ba`).** Conservative local
`Vec[T]` representations for `bool`, all integer primitives, both
floating-point primitives, recursively fixed scalar/immutable-borrowed-view or
declared enum/named-struct value arrays with at most 16 dimensions and 1,024
total value leaves, borrowed immutable strings/scalar slices,
declared enums, and backend-lowerable named structs now lower across C/Zig source, normal build, and
optimized build paths. Growth requires `effects [alloc]`; it has a hard
1,048,576-element limit,
allocation-failure/capacity traps, bounds checks, and exactly-once cleanup on
live return paths, normal nested branch/loop/match-arm exits, and loop `break`/`continue`
(explicit C ownership boundaries; Zig `defer`). Native C/Zig compilation and
execution prove push/pop/index/length/write behavior, nested branches/loops,
data-less/payload-enum and private/exported/nested-field struct element storage,
fixed-array scalar/named-value/descriptor copies and nested checked indexing, borrowed-string/slice
inspection, loop control, early returns, and the capacity trap. Whole-vector aliases,
escapes, signatures/returns, mutable-view/other unsupported owned-generic array
leaves and element types, option parameters/reassignment/immediate untyped
construction, parameter/callee-derived returns, and state-dependent pop
fallbacks are intentionally
FIR-blocked until source lowering gains signature and escape-aware ownership.

**Native lang.3 capsule ownership update.** The process-isolated
capsule ABI now returns the reviewed numeric vector set (`Vec[u8]`, `Vec[i8]`,
`Vec[u16]`, `Vec[i16]`, `Vec[u32]`, `Vec[i32]`, full-range `Vec[u64]`, `Vec[i64]`,
`Vec[isize]`, host-range `Vec[usize]`, `Vec[f32]`, `Vec[f64]`) and owned UTF-8
`str` across C, C++, Rust, and pinned Zig 0.14. One `Vec[Record]` output is also
accepted when the named record passes the recursively composed pointer-free
scalar-leaf `@repr(C)` review. C++ preflight requires explicit
C linkage for both the return and deallocator symbols. Typed
descriptors use byte-accounted length/capacity, a 64-MiB ceiling,
copy-before-free, exact allocator-domain deallocators, and parent-side
range/width/finiteness or UTF-8 validation. Float runners use round-trip-safe
text and canonicalize `f32` at its declared width. String bytes use a numeric
private envelope, preserving embedded separators and multibyte text. This
expands the governed foreign boundary; it does not yet make Vec/String
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

- **Keyword brevity is a hypothesis, not a measured lever.** It was adopted on
  the strength of the `heuristic-v1` estimator, which charges `ceil(len/4)` per
  word and so rewards shortening any keyword. Real BPE disagrees: common
  keywords such as `struct` and `return` are already single tokens, and across
  the measured sample in `docs/token_efficiency_findings.md` the dense surface
  is ~1% *larger* overall. Measure a proposed density rule with
  `fgx token-report --bpe-command` before treating it as a saving.
- The canonical `.fgai` format minimizes insignificant whitespace (no padding
  before `{`, single-space token separation), and the `agent-min` surface
  (`minify_agent`) drops indentation entirely.
- `.fgx` is never compressed this way — it stays fully readable. Round-trip
  (`canonical_eq`) must hold across both surfaces.

## Track 2.5 — Borrow-checker hardening (research-grade → proper)

The v1.0 borrow checker is real and passes 40 curated conformance cases plus a
526-program independently modeled move-state matrix, but it is not yet
*production-grade*. This milestone moves it to proper engineering
rigor, and it proceeds **in lockstep with `lang.2`–`lang.4`** — the checker
must soundly cover every construct the language gains.

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `safe.1` | Production-grade borrow checker | The points below, gated in `release-check`. |

What "proper" means here, concretely:

- **Soundness is the hard invariant — no false negatives.** The checker must
  *never* accept a program that violates ownership/aliasing. Conservative
  rejection preserves soundness, but production precision is regression-gated
  too. Established by three independent nets:
  1. a large **adversarial conformance corpus** — 40 curated cases, four
     persisted rejection regressions, and four acceptance regressions cover
     moves, borrows, reborrows, loops, branches, closures, lifetimes,
     projections, contracts, raw-pointer gates, and false-positive-sensitive
     precision paths;
  2. **property-based + fuzz testing** — 256 fixed seeded programs plus a
     reproducible 192-case default runtime campaign span moves, descendant
     loans, projections, depth-two-to-five nested loops, lifetime wrappers, and
     balanced long affine-closure state traces;
  3. a **differential cross-check against an independent reference model** —
     `forgentex.borrow_reference.v1` now covers 526 move, 610 alias/reborrow,
     292 lifetime-provenance, and 38 affine-closure programs. The first three
     families cover recursively
     normalized associated-type ownership shapes; the alias matrix exhausts two-,
     three-, and four-level conditional loop-target joins in both write orders;
     the lifetime matrix composes recursive collection carriers with generic
     wrapper chains. Strict release requires zero mismatches plus the complete
     per-family floors.
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
| `dom.1` | Async + real program I/O | **Synchronous application foothold shipped:** `fgx app-run` requires and preflights an exact major/minor Forgentex compatibility line, decodes positional arguments from declared `bool`/string/portable-path/integer/finite-float types with syntax and range enforcement, then binds the package entry to manifest-declared filesystem effects, canonical roots, cumulative budgets, declared artifacts, a supply-chain preflight, and `forgentex.application_run.v1`; typed scalar/path/void results and phase-coded failures are stable, with pre-dispatch JSON errors using `forgentex.application_error.v1`. Typed paths, bounded listings/reads, no-clobber publication, and exact artifact hashes run inside ordinary Forgentex functions. Deterministic heterogeneous waits and source-preserving timeout waits select borrowed runnable-task, buffered-or-closed channel-read, open channel-send, capability-confined regular-file-read and no-clobber-create, and token arms with stable logical-timer progress; pending logical timers support waiter-safe cancellation and same-clock rescheduling with stable tie order, copyable fixed-rate intervals issue fresh catch-up-aware one-shot tick tokens, and tasks support bounded typed deferred continuation chains, retry-safe polling, atomic wake-source rebinding, consuming logical-timer-aware blocking waits, bounded homogeneous races with loser cancellation, and consuming logical deadlines that cancel stalled work while preserving explicit readiness state. The remaining dom.1 work is open-stream/socket readiness, statement-frame suspension, OS/reactor scheduling, wall-clock integration, active interruption, async file I/O, and manifest-scoped networking. |
| `dom.2` | Persistence + custom file formats | **Path/binary/serialization/artifact foothold plus the complete `forgepack` v1 command and evidence workflow shipped:** applications parse portable `Path`, enumerate bounded sorted `DirEntry`, read `Vec[u8]`, emit canonical JSON and fixed-width little-endian bytes, and publish declared root/size/media-constrained files with exact SHA-256 receipts. The capstone archive has frozen/golden bytes, bounded reproducible build, structured inspect, strict verify, verify-first confined no-clobber extraction, corruption/traversal/collision tests, versioned receipts, and self-contained ten-check evidence invoked through copied release binaries in CI. Remaining: a published green clean-install CI artifact, recursive metadata where required, atomic replacement/directory durability, richer artifact groups/signing, and production codec paths. |
| `dom.3` | Web / server runtime | A real HTTP server runtime (a governed Rust/Go capsule) so the "web one-roof" becomes a deployable app serving requests, with the typed shared session live. |
| `dom.4` | Audio + graphics runtimes | miniaudio (real-time audio I/O) and a graphics/GPU binding (wgpu/Vulkan via a Rust/C capsule) for audio editors and 3D visualizers. Heavy, latest, fully governed. |

## App capstone (definition of done)

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `app.cap` | Ship one real app end-to-end | `scoped-copy` proves bounded binary I/O and declared hashed artifact publication; `directory-inventory` proves typed paths and bounded discovery. `forgepack` v1 builds reproducibly, inspects structurally, verifies strictly, extracts safely, and runs a self-contained ten-check workflow through a copied release binary. The source-side capstone evidence is complete; a published green clean-install CI artifact remains an external operational proof, and a later slice can build a small web app over `dom.3`. |

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
