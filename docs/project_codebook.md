# Project Codebook — A Local, Learned Token Optimizer (v3.7)

An **opt-in** feature (recommended for large projects): Forgentex analyzes a
specific project, learns the abbreviations that save the most tokens *for that
project's vocabulary*, and writes a persistent, committable **codebook**.
Agents load the codebook once and then exchange the project's source, maps,
and patches in a compressed dialect that references it — paying for the
abbreviations once and saving on every interaction afterward.

## Where it sits among the existing levers

Forgentex already has three token levers; the codebook is the missing
**learned, project-scoped, persistent** one:

| Lever | Scope | Lifetime | Chosen by |
| --- | --- | --- | --- |
| Prelude aliases | global | shipped with the toolchain | hand-curated (12 builtins) |
| `fgx compress` | one payload | ephemeral | per-payload dictionary |
| **Project codebook** | **one project** | **persistent, committed** | **learned from the corpus** |

It generalizes the prelude: the 12 hand-picked aliases become the global tier,
and the codebook is the auto-learned project tier layered on top — never
conflicting, always extending.

## The core principle

Forgentex owns a shared AST above the source, so the codebook is learned over
**semantic units**, not raw text. It abbreviates whole identifiers
(`core_math_clamp_i16`), whole type signatures (`Result[usize, str]`,
`&mut [i16]`), recurring call/expression patterns, common effect sets, and
capsule-boundary shapes — anything that recurs in *this* project. And like
everything else in Forgentex it is **lossless and reversible**: `fgx` expands
the compressed dialect back to canonical AST and round-trip holds, so the
compiler validates by expanding and the human can always read the expansion.

## What it learns (corpus analysis)

A deterministic census over the project's canonical AST collects candidates
with their frequency and measured token cost:

1. **Identifiers** — function, struct, field, and type names that recur.
2. **Type signatures** — recurring whole types (`Result[usize, str]`,
   `&mut [i16]`, generic shapes).
3. **Pattern n-grams** — recurring expression/statement subtrees (a common
   call with its argument shape, a guard-and-return idiom).
4. **Naming morphology** — the project's naming *scheme*
   (`core_<area>_<op>_<type>`, `is_*`, `*_mut`, `_i16`/`_u32` suffixes). This
   is the powerful one: learning the grammar lets the codebook compress
   *generatively* — even `core_X_Y_Z` names that are not individually frequent
   compress via area/op/type sub-tables.
5. **Effect sets** — recurring `effects [...]` combinations.
6. **Capsule boundaries** — recurring boundary-card shapes across languages.

## What it produces (the codebook artifact)

A deterministic, content-addressed file (e.g. `forgentex.codebook.json`),
committable to the repo:

- **Token-aware selection.** Candidates are ranked by *measured* expected
  savings, `freq_i x (cost_i - code_cost_i)`, minus the one-time definition
  cost, using the BPE-faithful tokenizer — never character counts. The
  never-larger guard from `fgx compress` applies: an entry that does not save
  tokens is never included.
- **Budgeted and tiered.** A `core` tier (small, highest-value, always
  loaded) and an `extended` tier (loaded on demand for deep work), each capped
  so the codebook itself fits a context budget. The tool reports the
  savings-vs-size curve.
- **Collision-safe codes.** Codes are drawn from a reserved namespace (a
  collision-free sigil, as `compress` already picks) or verified-unused short
  tokens, so a code can never mean a project identifier or another entry.
- **Mnemonic by default.** Codes are derived from their expansion where
  possible (`core_math_clamp_i16` -> `cmci`) so the model keeps semantic
  anchoring; a `--cryptic` mode trades readability for maximum savings. This
  is a deliberate token-vs-comprehension knob, not an accident.
- **Self-describing.** Compressed payloads carry a one-line header
  referencing the codebook by hash and tier (`#FGCB <hash> tier=core`), so any
  tool or agent knows exactly which codebook to expand with — mirroring the
  existing `#FGAI-Z` header.

## How agents use it

```text
load the codebook once  ->  exchange every file/map/patch in the compressed
dialect that references it  ->  fgx expands + validates against the AST
```

The codebook amortizes: you pay `C` tokens to load it once, and save `s`
tokens on the average file or patch, so it is net positive after `C / s`
interactions. Large projects do thousands of interactions, so the win
compounds — hence **recommended for large projects** and pointless for tiny
ones. The tool reports the break-even count on the actual project, so the
recommendation is evidenced, not assumed (the project's measure-don't-assume
rule).

## Professional guarantees

- **Deterministic & reproducible.** Same corpus -> same codebook (content-
  addressed). `fgx codebook --check` fails CI when the committed codebook is
  stale, so it never drifts from the code.
- **Stable code assignment.** Regeneration does not churn existing codes when
  adding new ones, so an agent's cached context stays valid across updates
  within a major version; freed codes are not reused for new meanings.
- **Tokenizer-pinned.** The codebook records which tokenizer model it
  optimized for; savings are reported against that model and re-verified
  before any external claim.
- **Auditable, not obfuscation.** `code -> expansion` is human-readable and
  reviewable; the expansion is always one command away. This is a shared
  abbreviation table, not a cipher.
- **Non-invasive.** It is an overlay. The canonical `.fgx`/`.fgai`/AST are
  untouched; disabling the codebook changes nothing about correctness.

## Commands (proposed)

```text
fgx codebook build [--out forgentex.codebook.json] [--budget <tokens>] [--cryptic]
fgx codebook check        # CI: committed codebook matches the corpus
fgx codebook measure      # amortized savings + break-even on this project
fgx codebook stats        # entries, tiers, top savers
fgx compress --codebook <file>      # compress using the project codebook
fgx map|context|patch --codebook <file>   # emit in the compressed dialect
```

Report schema: `forgentex.codebook.v1` (entries with freq/cost/saving, tiers,
corpus hash, tokenizer id, total expected savings, break-even interactions).

## Honest caveats

- **Break-even is real.** On small projects or few interactions the load cost
  can exceed the savings; the tool says so, and the feature stays off by
  default.
- **Cognitive cost.** Overly cryptic codes can hurt a model's reasoning even
  while saving tokens; mnemonic-by-default and a measured quality check guard
  against optimizing tokens at the expense of correctness.
- **Tokenizer dependence.** Savings are tokenizer-specific; the codebook is
  pinned to a tokenizer and re-measured if that changes.

## Milestone (v3.7)

A self-contained, **zero-dependency**, front-end/tooling feature (it fits the
core's zero-dep rule and can proceed in parallel with the v3 backend track):
the corpus census, token-aware budgeted selection with stable collision-free
codes, the committable `forgentex.codebook.v1` artifact, `fgx codebook
build/check/measure/stats`, codebook-aware `compress`/`map`/`context`/`patch`,
and a measured break-even report on a real project. Lossless round-trip and
the never-larger guard are conformance requirements.
