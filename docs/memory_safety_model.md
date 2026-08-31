# Memory Safety Model — Rust by Default, C Where You Ask

Forgentex's answer to "will it catch memory bugs like Rust, or let me shoot
myself like C?" is: **both, in three explicit zones.** Safe by default,
unsafe only where you opt in, and foreign code governed at its boundary. This
document explains the model and — more importantly — *why this choice is good
and practical* for an agent-native, polyglot language.

## The three zones

| Zone | Behaves like | Memory issues possible? | Who guarantees safety |
| --- | --- | --- | --- |
| **Forgentex safe code** | Rust | No | the compiler (compile-time + bounds checks) |
| **Forgentex `unsafe`** | C | Yes — but explicit and reviewed | the human reviewer |
| **Foreign capsules** (C/C++/Python/…) | that language | Yes inside the capsule | the typed boundary + sandbox |

### Zone 1 — Safe code (the default): like Rust

The default surface is memory-safe, checked before anything runs:

- **Ownership + borrow checking.** A CFG dataflow borrow checker computes moved
  paths, alias places, and liveness as fixpoint analyses over the control-flow
  graph, so use-after-move and illegal aliasing on references/slices are
  compile-time errors. A conformance suite (`fgx borrow-conformance`, 40 curated
  cases plus 1,922 generated/reference and regression programs across move,
  alias, lifetime, affine-closure, property, and runtime-seeded families) pins it;
  `fgx borrow-report` shows the dataflow evidence. See
  [borrow_checker_v1.md](borrow_checker_v1.md).
- **Bounds-checked indexing.** Array/slice access is guarded — in the
  interpreter and in the emitted C (a generated `fgx_bounds_index(i, len)`), so
  an out-of-bounds access traps instead of corrupting memory.
- **No raw pointers in safe code.** No dereference, no pointer arithmetic
  without opting out.

In the safe subset a buffer overflow, dangling reference, or use-after-move
**cannot ship** — they are caught at check time or trapped at runtime.

### Zone 2 — `unsafe`: like C, but explicit and auditable

Like Rust, `unsafe` is the opt-out for raw pointers, inline assembly, and ABI
control. Inside it you *can* have C-style memory bugs — that is the point. The
difference from C is that it is **explicit and surfaced for review**: every
`unsafe`/assembly block declares its effects, targets, and clobbers, and the
compiler's review report flags exactly which code traded safety for control.
The danger is named, located, and small.

### Zone 3 — Foreign capsules: governed, not verified

When you bring in C/C++/Rust/Python/… as a capsule, Forgentex does **not**
verify that foreign code's internal memory safety — a C capsule can have a
buffer overflow inside it. What Forgentex governs is the **boundary**: a typed
input/output contract, declared effects, sandbox capabilities, and
pinned/audited dependencies. Foreign code is therefore "as safe as that
language, but typed, sandboxed, and gated" — contained rather than verified.

## Why this choice is good and practical

**1. Safety scales with *risk*, not with code volume.** The vast majority of
code lives in Zone 1 and is checked automatically. Human review effort
concentrates on the small Zones 2 and 3 — the only places where memory bugs
are even possible. That is the whole "humans audit the dangerous parts" thesis
made mechanical: the compiler proves the boring 95% safe so reviewers can spend
their attention on the 5% that matters.

**2. It fits how agents actually work.** Agents write most of the code, and
they make memory mistakes. Safe-by-default means those mistakes are **caught by
the compiler, not shipped** — and the machine-readable diagnostics let the
agent repair them. A purely-C model would push that burden onto humans at
review time; a no-escape-hatch model would make whole classes of systems work
impossible. Safe default + explicit escape hatch is the combination that lets
agents move fast *and* stay safe.

**3. It is the only realistic model for a polyglot language.** Forgentex's
thesis is *borrow, don't reimplement* — stand on C/C++/Rust/Python runtimes.
But you cannot formally verify the memory safety of the entire borrowed
ecosystem, and reimplementing it all in a safe language would throw away the
point. The practical middle path is exactly Zone 3: **let foreign code be as
safe as its own language, and govern the boundary** — types, effects, sandbox,
provenance. You get the whole ecosystem without pretending to verify it, and
the blast radius of a foreign memory bug is bounded by the sandbox.

**4. Control is available when you genuinely need it.** Systems work — DSP
kernels, assembly, ABI, zero-copy buffers — sometimes *requires* raw control.
A safe-only language forbids it; C makes it the default. Forgentex makes it
**opt-in and visible**, so you keep C's ceiling without paying C's everyday
cost.

**5. The guarantee survives compilation.** Safety comes from the front-end
checks plus the runtime guards the backend emits (bounds checks). When
Forgentex emits C and the host compiler builds it, the C compiler does not need
to re-prove anything — the safety was established before lowering, and the
emitted guards are still there. So "compiles to C" does not mean "inherits C's
unsafety."

## Honest caveats (current state)

- **The borrow checker is research-grade *today* — and hardening is underway,
  not hand-waved.** Independent nets now cross-check 526 move, 610 alias, 292
  lifetime-provenance, and 38 affine-closure programs, including recursive
  collection/generic carriers, exhaustive two-/three-/four-level loop joins, and recursively
  normalized associated-type ownership shapes. Fixed and runtime-seeded
  properties add 448 programs, while four minimized regressions remain pinned.
  `FnOnce` makes explicitly typed one-shot closure locals affine, moves captured
  mutable roots into their environment, and is consumed by calls, transfers,
  and task spawn. The runtime campaign now includes balanced long affine traces,
  and weekly rotation covers eight seeds at 96 cases per family. Promotion of
  any newly discovered minimized failures remains milestone `safe.1` work in
  [v3_application_maturity.md](v3_application_maturity.md). Read it as
  "Rust-style and working today, with proper hardening on the roadmap."
- **The heap surface is deliberately narrow.** Safe source still cannot call
  `malloc`/`free`. The interpreter owns general growable collections; the C/Zig
  fast path currently admits only fresh annotated function-, branch-, loop-, or
  match-arm-local `Vec[T]` values for scalar primitives, borrowed immutable
  string/scalar-slice views, recursively fixed arrays of scalar, declared enum,
  backend-lowerable named-struct, or immutable borrowed string/scalar-slice
  leaves with at most 16 dimensions and 1,024 total value leaves, declared
  enums, and backend-lowerable named structs. Explicit
  `effects [alloc]` is required; growth is capped at
  1,048,576 elements, access is bounds-checked, and cleanup covers normal scope
  exit, loop control, and every live return. `pop` results may be consumed
  immediately or stored in a local `Option[T]` and later inspected by `is_some`,
  `is_none`, or `unwrap_or` with a literal or already-evaluated fallback. C uses
  a layout-specific by-value option and Zig uses `?T`; neither representation
  owns vector storage, and the fallback restriction preserves eager ordering.
  A direct pop result or an unmodified stored pop option may also return by
  value: C saves it before freeing all live vectors, while Zig evaluates it
  before lexical `defer` cleanup.
  Whole-vector aliases, escapes,
  signatures/returns, mutable-view or other unsupported owned/generic array
  leaves, other owned/generic element layouts, option parameters, reassignment,
  immediate untyped construction, parameter/callee-derived returns, and
  state-dependent pop fallbacks
  are FIR-blocked, so
  unsafe shallow copies cannot reach
  source emission. Expanding this subset requires the same explicit ownership
  and deallocation proof, not direct allocator access from safe code.
- **Integer overflow is a correctness, not a memory-safety, matter.** Overflow
  is not auto-checked in emitted C by default (it wraps, unless you use the
  `core_math_checked_*` builtins). Bounds and aliasing — the memory-safety
  guarantees — are enforced.

## In one line

> **Rust by default, C only where you explicitly ask for it or borrow it** —
> safe-by-default with auditable, contained escape hatches, so safety is the
> norm and danger is always named, located, and small.
