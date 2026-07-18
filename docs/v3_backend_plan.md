# v3 Plan — When C-Emission Runs Out

v2.12 gave Forgentex a real native backend by emitting C and driving the
host compiler. That borrows a world-class optimizing backend for free, so
**size, optimization, and target coverage are already solved without any
Rust dependency.** v3 is not about making the backend "bigger." It is about
the few capabilities C-emission genuinely cannot give us — and building them
behind a clean dependency boundary so the trusted core stays zero-dep.

## The three scenarios where C-emission runs out

1. **JIT / REPL / hot-reload.** You cannot shell out to `cc` per expression.
   An interactive loop — and instant re-run of a single function after a
   semantic patch — needs in-process codegen.
2. **IR-level control.** Guaranteed tail calls, precise stack maps, custom
   calling conventions, and SIMD scheduling that C cannot express cleanly.
3. **Dev-loop compile speed.** Regenerating and recompiling large C on every
   edit is slow; agents iterate fast and need a tight loop.

## Governing rule

See the **backend dependency policy** in `docs/native_polyglot_plan.md`:
the core stays zero-dependency forever; any backend dependency is a separate
crate behind a `cargo` feature flag, off by default, pinned and audited with
the v2.3 supply-chain machinery; C-emission remains the always-available
fallback. v3 is where we *deliberately and narrowly* take the first such
dependency — in the backend layer only, never in the core.

Sequencing note: v3 follows the v2 polyglot phase. The front-end and
semantics should be stable first (as the original demo doc put it, reach for
LLVM "after semantics stabilize"). These are plans; exit criteria describe
the target.

## Milestones

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `v3.0` | Backend boundary + policy | A stable `Backend` interface with the existing interpreter and C-emit AOT path refactored behind it; `cargo` feature scaffolding so optional backends compile out by default; the dependency policy documented and enforced (default build proven zero-dep). `forgentex.backend_info.v1` lists available backends and their feature/dependency status. No new dependencies. |
| `v3.1` | Forgentex IR (FIR) | Mature the existing lowering IR into a documented, deterministic, serializable mid-level IR that every backend consumes (interpreter, C, future JIT/LLVM). The front-end lowers once; backends are interchangeable. `fgx ir <file>` emits inspectable FIR. Still zero-dep. |
| `v3.2` | Dependency-free IR optimization | Const-folding, dead-code elimination, function inlining, and simple scalar replacement as FIR passes — improving both the emitted C and the interpreter, and shrinking generated C for faster host compiles. Pure Rust, no dependencies. `fgx build --opt` reports the passes applied. Addresses IR-level control (partial) and compile speed. |
| `v3.3` | Cranelift JIT backend (first optional dependency) | In-process native codegen from FIR for JIT execution, behind `--features jit-cranelift` (**default off**; the default build stays zero-dep). The dependency is pinned and audited through the capsule supply-chain machinery. Enables fast `fgx run` with no `cc` round-trip. The first deliberate, isolated break of the no-deps rule — backend layer only. **Scenario: JIT.** |
| `v3.4` | REPL + hot function reload | On the JIT: `fgx repl` for live evaluation, and hot-swap of a single function after a semantic patch (patch → recompile just that function → re-run its tests instantly). This is the agent dev-loop payoff — it makes the semantic-patch core *fast* to iterate. **Scenario: REPL / hot-reload / compile speed.** |
| `v3.5` | LLVM backend (optional, peak optimization) | A feature-flagged `backend-llvm` path from FIR for the heaviest AOT optimization and IR-level features C cannot express well (guaranteed tail calls, precise control). Pinned/audited; never default. Only for workloads that prove they need it. **Scenario: IR-level control + peak performance.** |
| `v3.6` | Debug info + native profiling | DWARF/line tables mapping native frames back to `.fgx`/`.fgai` symbols, so native artifacts are debuggable and profilable, extending the review/audit story from build time to run time. Addresses C-emission's debug-fidelity limitation. |

## Dependency order

```text
v3.0 backend boundary ── v3.1 FIR ──┬── v3.2 IR optimization (zero-dep)
                                    ├── v3.3 Cranelift JIT ── v3.4 REPL/hot-reload
                                    └── v3.5 LLVM backend
v3.0/3.1 ──────────────────────────── v3.6 debug info
```

Build order: 3.0 → 3.1 → 3.2 → 3.3 → 3.4 → 3.5 → 3.6. Everything through
3.2 is zero-dependency; 3.3 is the first opt-in dependency; 3.5 is the
heaviest and most optional.

## What we are explicitly NOT doing

- Not replacing C-emission — it stays the default, zero-dep, all-targets
  AOT path and the v2.13 FFI link unit.
- Not putting any dependency in the trusted core.
- Not reaching for MLIR. If FIR + Cranelift + (optional) LLVM are not
  enough, that is a much later conversation.

## Parallel v3 tracks: token efficiency & agent communication

Independent of the backend track (and zero-dependency, so they live in the
core), v3 also carries a learned token-optimizer milestone and a latent
agent-communication protocol:

| Label | Goal | Exit criteria |
| --- | --- | --- |
| `v3.7` | Project codebook (local token optimizer) | An opt-in, per-project learned codebook: a deterministic corpus census over the canonical AST, token-aware budgeted selection of abbreviations with stable, collision-free, mnemonic codes, and a committable `forgentex.codebook.v1` artifact. Agents load it once and exchange source/maps/patches in a compressed dialect that references it; `fgx` expands losslessly (never-larger guard, round-trip conformance). `fgx codebook build/check/measure/stats` plus codebook-aware `compress`/`map`/`context`/`patch`, with a measured break-even report so the "recommended for large projects" guidance is evidenced. Generalizes the global prelude into an auto-learned project tier. See `docs/project_codebook.md`. |
| `v3.8` | Latent agent-message protocol | The typed, signed, model-versioned envelope and audit layer for **latent** multi-agent collaboration (inspired by LatentMAS). Forgentex does not compute latents (that needs a model runtime and stays out of the zero-dep core); it provides a `latent` typed boundary with a mandatory model fingerprint `{model_id, revision, layer, dtype, shape, hash}` and a `latent_model_mismatch` check, a content-addressed + signed latent envelope built on the trust fabric (v2.19) and hashing (v2.16), data-plane flow by name (v2.2/v2.18), and runs LatentMAS itself as a gated `python` capsule (v2.17, `gpu`/`alloc` effects). `forgentex.latent_session.v1`, with envelope-vs-text transport measured separately from LatentMAS's inference metrics. See `docs/latent_agent_protocol.md`. |

## Current implementation status

- `v3.0` is implemented in the core crate: interpreter, C, and Zig are behind
  a shared `Backend` interface; `fgx backends --json` emits
  `forgentex.backend_info.v1` with feature and dependency status; Cargo has
  empty default features plus off-by-default scaffolds for `jit-cranelift`,
  `backend-llvm`, and `backend-mlir`; tests assert the default dependency
  section remains empty.
- `v3.1` is partially implemented as deterministic FGIR
  (`forgentex.backend_ir.v1`), frontend contract snapshots, enum/payload type
  declarations, single-file `fgx ir <file>` generation, and per-function
  backend readiness contracts for the current interpreter/C/Zig backends. The
  C/Zig contracts now block runtime-only effects, generic runtime types,
  review-only assembly, interpreter-only Option/Result intrinsics, and foreign
  capsule calls before source emission, and build preflight also blocks generic
  FIR struct fields or enum payloads before source emission. The interpreter
  backend preflights the selected run entry through its FIR contract and executes
  `fgx run --backend interp` entries plus Forgentex callees from FIR-backed
  bodies, including the default demo run and hosted
  `fgx run --live`; the compatibility helpers `fgx io-run` and `fgx net-run`
  also execute through generated FGIR; direct in-memory interpreter helper APIs
  lower to FGIR by default, with explicit `*_with_ir` variants for precomputed
  lowerings; C/Zig artifact builds preflight those
  FIR contracts before source emission and write FIR/receipt metadata
  (`forgentex.fgir.json`, `forgentex.backend_build.json`) beside successful
  source builds. Eligible private generic direct-call graphs are deterministically
  monomorphized before this preflight; unresolved/exported/uncalled forms stay
  in FIR and remain explicitly blocked. Source-only and optimized builds use
  the same preparation order. C/Zig declarations and function signatures are
  now emitted from FIR, and C/Zig
  bodies can emit from FIR instructions for straight-line statements plus
  structured `if`/`else`, `for`, `while`, `break`, and `continue` markers. C
  and Zig now also emit enum `match` bodies from FIR
  `match`/`case`/`endmatch` instructions, including tagged-union payload
  bindings. C/Zig body emission now errors when FIR emission cannot complete
  instead of silently falling back to AST statement walkers, and those failures
  include a cursor-level FIR reason. The remaining v3.1 work is to keep FIR as
  the single lowering contract for future native backends.
- `v3.2+` remains planned.

## Application-maturity track (toward real software)

The backend track above makes Forgentex *fast and linkable*; a separate,
higher-level track makes it *able to build real applications*. A maturity
assessment found three gaps blocking shipping software (audio editors, DSP
processors, agent systems, web apps, trading bots, file tools, 3D, knowledge
apps), planned in priority order in **`docs/v3_application_maturity.md`**:

- **Track 1 — Live capsule runtime** (`rt.1`–`rt.3`): the keystone. A running
  program calls capsules with live data and composes them; the shared
  namespace becomes a live cross-language data plane; warm workers + pipelines
  make it perform. *Highest priority — without it nothing composes at runtime.*
- **Track 2 — Language breadth** (`lang.1`–`lang.4`): `else`/`while`/`match`,
  general enums + pattern matching, growable collections + strings, then
  generics/closures — so app logic can actually be written in `.fgx`.
- **Track 2.5 — Borrow-checker hardening** (`safe.1`): move the v1.0 borrow
  checker from research-grade to *production-grade* — a written rule spec, a
  several-hundred-case adversarial corpus, property-based + fuzz testing of the
  soundness invariant (no false negatives), a differential cross-check against
  an independent reference model, and sound coverage of every new language
  construct. Proceeds in lockstep with `lang.2`–`lang.4`.
- **Track 3 — Domain bindings** (`dom.1`–`dom.4`): async + real program I/O,
  persistence + custom file formats, a web/server runtime, and audio/graphics
  runtimes — each a **governed capsule over borrowed C/C++/Rust (and Python)**.
- **App capstone** (`app.cap`): ship one real app end-to-end as proof.

Governing policy: the pure zero-dependency **core** stays pure, but the
**runtime, stdlib, and domain layers borrow heavily from C/C++/Rust/Python**,
pinned and audited like capsules — Forgentex governs and wires; it does not
reimplement runtimes.

## Cross-cutting principles

- **Default build is zero-dep and auditable.** Optional backends are feature
  flags, off by default.
- **One lowering contract (FIR).** Adding a backend never touches the
  front-end or the core.
- **Fallback always exists.** C-emission covers any program a fancier
  backend can; the fancier backend is a speed/interactivity choice.
- **Dogfood supply-chain control.** Every backend dependency is pinned,
  checksummed, and audited like a capsule dependency.
- **Never regress v1.0 / v2.x.** Semantics stabilize before LLVM.

## Future built-in knowledge-base library

Forgentex should grow a built-in library and CLI surface for working with
Markdown knowledge bases, with first-class support for Google's Open Knowledge
Format (OKF). OKF represents knowledge as directories of `.md` files with YAML
frontmatter and lightweight conventions for indexes, logs, links, and bundles.
A standard-library layer for reading, validating, organizing, and rewriting
those files would make project knowledge bases portable, reviewable, and easy
for agents to traverse.

The goal is not to replace existing documentation tools. The goal is to give
agents a stable, local, version-controlled knowledge format they can maintain
while working: normalize frontmatter, build `index.md` navigation, update
`log.md`, validate links, extract concept graphs, and keep architecture notes,
runbooks, decisions, and domain knowledge organized in a way both humans and
agents can read. This should streamline agent work on large knowledge bases and
make long-running projects less dependent on ad-hoc markdown conventions.

Possible v3 ideas:

- `fgx kb init/check/map` for OKF-style bundles: create the directory shape,
  validate required frontmatter, verify relative links, check reserved files
  such as `index.md` and `log.md`, and emit a machine-readable bundle report.
- `core.kb` and `core.md` stdlib modules for parsing Markdown structure,
  YAML frontmatter, links, headings, code fences, citations, and lightweight
  concept metadata without pulling those concerns into the compiler core.
- Asset sidecars for non-text knowledge: images, audio, video, PDFs, and
  datasets stay as original files under `assets/` or `data/`, while Forgentex
  validates adjacent markdown/JSON sidecars for captions, OCR, transcripts,
  keyframes, schemas, hashes, provenance, licensing, and freshness.
- Region-grounded visual knowledge: support COCO-like JSON sidecars for images
  where polygons, boxes, masks, or points select exact regions and each region
  carries rich human comments, labels, claims, uncertainty, source citations,
  and links to related concepts. This lets a VLM/LLM inspect an image through
  stable visual anchors instead of vague whole-image captions.
- Agent-safe update primitives: append to `log.md`, create or rewrite concept
  pages, preserve frontmatter ordering, update backlinks, and produce reviewable
  diffs instead of letting agents freely rewrite a whole knowledge tree.
- Knowledge graph extraction: generate a deterministic graph from markdown
  links, tags, resources, citations, and media sidecars, then expose it through
  `fgx map`/`fgx context` so agents can load only the relevant concepts.
- Citation and evidence helpers: store source spans, file hashes, timestamps,
  and media timecodes so agent answers can point back to exact markdown lines,
  data rows, transcript ranges, or video/audio segments.
- Rebuildable indexes as cache, not truth: local search, embeddings, thumbnails,
  OCR text, and transcript summaries can be regenerated from canonical `.md`
  files and assets, keeping the repository auditable.
- Capsule-backed enrichers for heavy processing: OCR, speech-to-text, video
  keyframe extraction, PDF parsing, and embedding generation should run through
  governed capsules with explicit effects, pinned tools, and recorded receipts.
