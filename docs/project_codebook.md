# Project Codebook — A Local, Learned Token Optimizer (v3.7)

**Implementation status:** semantic-tier foundation shipped. The current
tool learns repeated identifiers/dotted paths, delimiter-bounded naming
morphology, whole type shapes, complete effect sets, and exact plus bounded
one-/two-slot parameterized expression/leaf-statement patterns from the
canonical AST and `.fgai`
renderings of the package's own modules, writes a bounded content-addressed
artifact, and provides deterministic build/check/measure/stats commands plus
guarded map/context/patch exchange and stable lineage evolution. Templates with
more than two slots or non-atomic slots, capsule-shape, and direct source
tiers remain later slices.

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

1. **Identifiers** — function, struct, field, and type names that recur. Shipped.
2. **Type signatures** — recurring whole types (`Result[usize, str]`,
   `&mut [i16]`, generic shapes). Shipped for declaration, local, cast, and
   capsule-boundary type shapes.
3. **Pattern n-grams** — recurring expression/statement subtrees (a common
   call with its argument shape, a guard-and-return idiom). Exact canonical
   expressions and leaf statements are shipped. Parameterized templates replace
   one or two formatter-derived atomic variable/literal leaves with ordered
   `$0`/`$1` slots; larger-arity and non-atomic templates are not shipped.
4. **Naming morphology** — the project's naming *scheme*
   (`core_<area>_<op>_<type>`, `is_*`, `*_mut`, `_i16`/`_u32` suffixes). This
   is the powerful one: learning the grammar lets the codebook compress
   *generatively* — even `core_X_Y_Z` names that are not individually frequent
   compress via area/op/type sub-tables. Shipped for repeated `_`/`.`-bounded
   prefixes, suffixes, and interior spans; self-delimiting handles can encode a
   new identifier that follows a learned scheme.
5. **Effect sets** — recurring `effects [...]` combinations. Shipped for
   complete function and capsule declarations.
6. **Capsule boundaries** — recurring boundary-card shapes across languages.

## What it produces (the codebook artifact)

A deterministic, content-addressed file (e.g. `forgentex.codebook.json`),
committable to the repo:

- **Token-aware selection.** Candidates are ranked by *measured* expected
  corpus savings, `freq_i x (cost_i - code_cost_i)`, using the BPE-faithful
  tokenizer — never character counts. Each proposed entry is retained only if
  the actually encoded corpus improves; serialized definition cost remains
  visible in the artifact budget and break-even report.
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

## Current commands

```text
fgx codebook build [project] [--previous forgentex.codebook.json] [--out forgentex.codebook.json] [--budget <512..65536>] [--cryptic] [--json]
fgx codebook check [project] [--codebook forgentex.codebook.json] [--json]
fgx codebook measure [project] [--codebook forgentex.codebook.json] [--json]
fgx codebook stats [--codebook forgentex.codebook.json] [--json]
fgx codebook encode <input> --codebook forgentex.codebook.json --out <payload>
fgx codebook decode <payload> --codebook forgentex.codebook.json --out <expanded>
fgx map --project [project] --codebook forgentex.codebook.json [--force-codebook] --out <map>
fgx context <symbol> --project [project] --codebook forgentex.codebook.json [--force-codebook] --json
```

Report schema: `forgentex.codebook.v1` (entries with freq/cost/saving, tiers,
corpus hash, tokenizer id, total expected savings, break-even interactions).

The current artifact is one `semantic` tier with explicit `identifier`,
`morphology`, `type`, `effect_set`, `expression`, and `statement` entry kinds.
Parameterized forms add `expression_template` and `statement_template` kinds.
Mnemonic codes are default;
`--cryptic` selects compact ordinal codes. The codebook records its collision-
free sigil, exact `heuristic-v1` serialized cost, original/encoded corpus cost,
gross saving, first-interaction net, and break-even. `check` validates artifact
self-accounting against the current project and fails when state is stale. `measure` distinguishes
current from stale corpora without claiming that source exchange already uses
the dictionary.

On `examples/apps/wav_waveform` with a 1,024-token budget, exact and
parameterized byte-index/image-write expressions dominate nine selected
entries. One canonical agent-corpus pass falls from 1,473 to 1,307 tokens (166
saved); the artifact costs 1,024 tokens, so first use is negative and break-even
is interaction 7. A 512-token budget selects the image-write template plus one
exact expression, costs 452 tokens, saves 91 per pass, and breaks even at
interaction 5. This correctly
classifies the tiny demo as a weak codebook candidate rather than manufacturing
a headline win.

## Honest caveats

- **Break-even is real.** On small projects or few interactions the load cost
  can exceed the savings; the tool says so, and the feature stays off by
  default.
- **Cognitive cost.** Overly cryptic codes can hurt a model's reasoning even
  while saving tokens; mnemonic-by-default and a measured quality check guard
  against optimizing tokens at the expense of correctness.
- **Tokenizer dependence.** Savings are tokenizer-specific; the codebook is
  pinned to a tokenizer and re-measured if that changes.
- **Semantic metadata has a cost.** Explicit entry kinds and richer candidates
  increase the small artifact even while improving per-pass savings; both sides
  remain visible in the break-even report.
- **Partial encoded exchange.** Direct `.fgx`/`.fgai` source does not consume
  the artifact yet. Project maps, focused context, and semantic-patch transport
  support guarded encoding, while effects remain readable and context state
  hashes stay canonical.

## Encoding integration

`encode` writes a self-describing `#FGCB v1` payload bound to the exact
artifact hash, corpus hash, and tier; `decode` refuses another codebook and
restores the original bytes. `map --codebook` requires manifest project mode so
corpus currency can be checked before emission. `context --codebook` similarly
refuses stale artifacts, encodes symbol/map/type strings, leaves effects and
review evidence untouched, and reports `saved_by_codebook` separately from
stateful hash elision. A baseline generated under another codebook cannot elide
sections under the current one.

The tiny waveform fixture remains a negative map case: its project map would
grow from 907 to 998 heuristic tokens after the self-describing header, so map
encoding stays off. Parameterized expression patterns improve the context:
selected content falls from 883 to 768 and complete JSON falls from 2,001 to
1,932, so context encoding is admitted automatically. The current CLI does
not pretend that corpus-only substitution savings guarantee transport savings.

The larger naming-heavy `media_visualizer` fixture demonstrates the morphology
tier directly. At a 4,096-token budget it selects three morphology entries
(`.color_space`, `.engine_version`, and `_preview_free`), improves corpus saving
from 317 to 327 tokens, and retains interaction-9 break-even. A synthetic
fixture separately proves that a learned prefix encodes an unseen identifier.

Normal map/context operation now enforces that conclusion automatically. It
constructs both complete candidates and applies the codebook only when the
encoded payload is strictly smaller. Otherwise maps stay byte-identical to the
plain form and report `codebook=skipped_not_smaller`; contexts return the exact
ordinary no-codebook JSON, identified by `codebook_ref=null`. `--force-codebook`
exists only to inspect encoded candidates and prove decode round trips.

Semantic patch transport uses the same envelope and admission rules.
`fgx codebook encode` skips token-negative patches unless forced; `patch` and
`review` accept
`--codebook`, require a current corpus, validate payload identity, and decode
before target lookup, preconditions, signatures, operations, or type checking.
Review JSON binds the artifact and reports transport and canonical patch costs.
Patch syntax and safety meaning are never defined in the compressed dialect.

Parameterized invocations use a separate self-delimiting handle namespace and
one exact byte-length frame per ordered atomic argument. Decode reconstructs
from static segments before ordinary aliases expand, rejects malformed or
noncanonical frames in artifact-bound payloads, and supports UTF-8 strings
without delimiter escaping. This is reversible transport syntax, not a
Forgentex macro facility.

The checked-in demos do not select a two-slot entry because its second frame
would cost more than it saves; the never-larger optimizer correctly declines
it. A synthetic repeated statement family proves two-slot generative exchange,
including literal `$1` text inside the first argument. Restricting handle
reservation to inherited and selected entries also improves the 4,096-token
waveform result from 254 to 256 saved tokens and audio from 191 to 193.

## Stable evolution

`codebook build --previous <file>` treats the supplied artifact as the prior
generation. An unchanged corpus and budget returns the exact same bytes and
artifact hash. A changed corpus advances `generation`, binds `parent_hash`, and
preserves every surviving expansion→code assignment. Removed meanings move
their codes into sorted `retired_codes`; those tombstones are inherited by
later generations and are never assigned to a new meaning, even when an old
identifier later returns. Mode, tokenizer, tier, project, and sigil mismatches
fail rather than silently rebuilding an incompatible lineage.

## Milestone (v3.7)

A self-contained, **zero-dependency**, front-end/tooling feature (it fits the
core's zero-dep rule and can proceed in parallel with the v3 backend track):
the corpus census, token-aware budgeted selection with stable collision-free
codes, the committable `forgentex.codebook.v1` artifact, `fgx codebook
build/check/measure/stats`, codebook-aware `compress`/`map`/`context`/`patch`,
and a measured break-even report on a real project. Lossless round-trip and
the never-larger guard are conformance requirements.
