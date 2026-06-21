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
