# Token-Efficiency Findings

Measured with `fgx token-report` and the heuristic tokenizer in
`src/tokens.rs`. The tokenizer is a heuristic, not an exact GPT/BPE count,
so treat the *ratios* as the signal. Re-run with a real tokenizer before
publishing external claims.

## A correction worth recording

The first measurement reported `.fgai` as token-*neutral* vs `.fgx`. That
was largely a **tokenizer artifact**: the estimate charged one token per
single space, but real byte-pair encoding usually merges a leading space
into the following token for free. After fixing the tokenizer (small space
runs are free; only larger indentation and newlines cost), and after making
dictionary compression token-aware (it selects entries by token saving, not
character saving, so it can never increase tokens), the picture changed.

This is the value of measuring: the original "it's neutral" conclusion was
wrong because the *measurement* was wrong, not because the encoding was bad.

## Measured surfaces (heuristic-v1, corrected)

| File | human | agent | agent/human | minified | compressed | best/human |
| --- | --- | --- | --- | --- | --- | --- |
| hello.fgx | 50 | 43 | 0.86 | 41 | 41 | 0.82 |
| media/audio_mix.fgx | 226 | 211 | 0.93 | 205 | 205 | 0.91 |
| audio/src/dsp.fgx | 1150 | 1112 | 0.97 | 1061 | 1033 | 0.90 |
| capsule/normalize_python.fgx | 193 | 171 | 0.89 | 171 | 171 | 0.89 |
| capsule/web_session.fgx | 435 | 387 | 0.89 | 381 | 381 | 0.88 |
| smoke/inline_tests.fgx | 160 | 155 | 0.97 | 143 | 143 | 0.89 |

## What the numbers say (honestly)

1. **The `.fgai` agent syntax does save tokens — modestly.** agent/human is
   0.86–0.97, roughly a 3–14% reduction, better on declaration-heavy files
   (short `S`/`F` keywords replacing `struct`/`fn`, no `-> :` ceremony) and
   smaller on body-heavy numeric code where the identifiers and operators
   dominate and are the same in both forms.

2. **Minification adds a little more.** Dropping blank lines and
   indentation (`minify_agent`, lossless at the canonical-AST level, capsule
   `CODE` blocks preserved verbatim) shaves a few percent on body-heavy
   files (dsp 1112→1061). Under a faithful tokenizer small indentation is
   nearly free, so this is a minor lever.

3. **Token-aware compression no longer loses, but rarely wins.** It only
   helps where long identifiers repeat (dsp 1061→1033); otherwise the
   dictionary is not worth its header and the source is returned unchanged.

4. **Best lossless per-file result is ~0.82–0.91 of the human tokens** — a
   real but bounded win. The canonical AST remains the expensive surface and
   should never be sent to an agent.

## The ceiling, and where the real win is

Per-file encoding has a low ceiling: once keywords are single letters and
insignificant whitespace is gone, the remaining tokens are structural
punctuation (`{ } ( ) , : -> ; [ ]`), identifiers, and one newline per
statement. Squeezing those further (joining statements, dropping
delimiters) needs a denser grammar *and parser*, for single-digit extra
percent — diminishing returns.

The order-of-magnitude win is not per-file at all; it is **not sending the
file**: project maps (skeletons), context packets (one symbol's typed/effect
slice), semantic patches (a few lines to change one behavior), and
diff-only re-reads. The next measurement should compare a realistic multi-
edit task — full-file resend vs map+context+patch — which is where a large
difference, if any, will appear.

## Prelude aliases (dual-named builtins)

Twelve high-frequency builtins now carry a two-letter agent alias
(`core_math_clamp_i16` → `cl`), readable in `.fgx`, short in `.fgai`, AST
identical, verified lossless (see `docs/prelude_aliases.md`). On
builtin-calling code the effect is real: `audio_mix.fgx` improved from
agent/human 0.93 → **0.82** and best/human 0.91 → **0.79**. Code that does
not call builtins is unchanged. This is the largest per-file lever found so
far, because the canonical builtin names are long and the calls are
frequent — but it still only applies to builtin-heavy code, and the
order-of-magnitude win remains in not sending the file at all.

## The edit-task benchmark (the real win, measured)

`fgx edit-bench` models a realistic multi-edit task and confirms the
hypothesis. Whole-file strategy = read the file once then re-emit it on each
edit (`file * (1+N)`); patch strategy = read the symbol map once, then per
edit read only the target symbol and emit a small semantic patch
(`map + N * (symbol + patch)`).

| File | functions | patch/whole @ 8 edits | break-even |
| --- | --- | --- | --- |
| media/audio_mix.fgx | 2 | 0.52 | 1 edit |
| audio/src/dsp.fgx | 12 | 0.12 | 1 edit |
| capsule/web_session.fgx | 0 | 0.06 | 1 edit |

The patch strategy is cheaper **from the very first edit**, and the
advantage widens with file size: ~2x on a tiny 2-function file, **~8x on a
12-function file**. This is the order-of-magnitude token win — it comes from
*not sending the file* (read one symbol, emit a small patch), exactly as the
per-file measurement predicted. Per-file encoding tricks (dense syntax,
aliases, compression) are single-digit to ~20% on top; the structural win is
here.

Reproduce: `fgx edit-bench <file> [--edits <n>] [--json]`.

## Budgeted focused context

`fgx context` now turns the structural strategy into an enforceable content
budget. It reports `heuristic-v1` costs for the selected symbol, map, types,
effects, tests, mandatory floor, and admitted total. Optional dependency slices
are atomic: the complete category fits or an exclusion explains its refusal.
Requested effect evidence is never budget-elided.

```powershell
fgx context '@function.render_waveform' `
  --project examples/apps/wav_waveform `
  --deps types,effects,tests `
  --max-tokens 883 `
  --json
```

The fixed JSON/report envelope is intentionally outside the selected-content
budget, so a provider integration must reserve framing tokens. Counts still use
the heuristic tokenizer; real-tokenizer and model-task validation remain
required before external efficiency claims.

The same waveform packet now supports stateful `--since` elision. With an
unchanged full packet as baseline, selected content drops from 883 transmitted
tokens to 54 because the symbol source, map, types, and tests become verified
hash references while effects remain explicit. Counting the complete JSON
payload is less dramatic but still material: 1,896 to 1,026 heuristic tokens,
a **45.9% reduction**. This complete-packet figure is the honest operational
metric; the 93.9% selected-content reduction describes only the elidable
payload.

State metadata is not free. Before section hashes, the stateless waveform
packet was 1,492 tokens. The state-capable full packet adds 404 tokens, while an
unchanged delta saves 466 tokens relative to each old stateless resend. This
fixture reaches break-even on interaction two and saves thereafter; one-shot
context requests should not claim an advantage from stateful elision.

## Project codebook foundation

`fgx codebook build/check/measure/stats` now measures the persistent-dictionary
idea before enabling encoded exchange. The semantic tier learns repeated
identifiers/dotted paths, delimiter-bounded naming morphology, complete
AST-derived type/effect shapes, exact expression/leaf-statement patterns, and
one-/two-slot parameterized patterns over atomic leaves, then chooses entries
under the actual serialized artifact budget. A proposed
entry must increase actual corpus saving, so overlap cannot consume budget for
zero incremental benefit.

The waveform project demonstrates why break-even reporting matters: a
1,024-token budget produces a codebook costing 1,024 tokens and saves 166 tokens
per corpus exchange, so it becomes positive on interaction 7. This is a
valid result, not a failure: persistent codebooks are intended for larger or
long-lived projects. Codebook-aware map/context/patch output and real-tokenizer
model-success evaluation remain required before claiming an end-to-end win.

The first map/context integration confirms that warning. On the waveform
project, the original identifier/type tier could not amortize context metadata.
Exact and parameterized expression patterns lift selected saving to 115 tokens
and complete context improves 2,001 → 1,932, so automatic admission applies.
The self-describing project map remains 907 → 998 and stays plain. Both surfaces
round-trip exactly and reject stale dictionaries; admission remains
surface-specific rather than inferred from corpus totals.

Whole-payload admission now makes that rule executable: the waveform map stays
plain while the context is encoded, and an explicit diagnostic force flag can
materialize the larger coded candidate for round-trip evidence. No normal
codebook request selects a coded map or context candidate larger than its
readable fallback.

Semantic patch transport follows the same rule. A representative five-line
waveform patch measures 58 tokens plain and 141 with its self-describing
codebook envelope, so normal encoding skips it. Forced conformance mode proves
decode → precondition → apply → check produces the same clean review and keeps
transport cost distinct from the 58-token canonical patch.

Stable evolution adds generation/parent/tombstone state and therefore raises
the tiny artifact by 31 tokens. In return, unchanged builds retain exact bytes,
surviving identifiers keep their codes, and removed codes are never recycled
for new meanings. This protects cached agent context from semantic alias churn.

On the waveform corpus, exact repeated byte-index and image-write expressions,
parameterized patterns, and marginal-gain selection lift per-pass saving from
62 to 166 tokens and break-even from 16 to 7 interactions. A synthetic fixture
proves complete type/effect/statement units round-trip as codes. Exact entries
remain formatter-derived; parameterized entries use only the explicit `$0`
transport slot described below. Neither defines inferred macro semantics.

Bounded parameterized patterns generalize one or two atomic AST leaves without
defining macros. `push(image, u8($0))` encodes both learned and unseen literal
arguments through a self-delimiting handle plus canonical byte length. The
512-token waveform artifact consequently improves from 51 to 91 saved tokens
per pass and from interaction-9 to interaction-5 break-even. UTF-8 statement
arguments round-trip exactly, while malformed frames fail artifact-bound decode.

Two-slot forms use ordered `$0`/`$1` segments and one canonical frame per
argument. Static-segment reconstruction preserves a literal `$1` inside another
argument. None of the checked-in demos selects a two-slot entry—the second frame
is not profitable there—while a synthetic repeated statement family does.
Selected-only handle reservation still improves 4,096-token saving from 254 to
256 on waveform and from 191 to 193 on audio.

Naming morphology adds reversible `_`/`.`-bounded prefix, suffix, and interior
fragments with self-delimiting handles. A synthetic naming-family fixture proves
that a previously unseen identifier can use the learned prefix. On
`media_visualizer` at 4,096 tokens, three morphology entries improve corpus
saving from 317 to 327 tokens while break-even remains interaction 9.

## Polyglot: boundary vs body (the equalizer), measured

Foreign capsule code is verbatim — it must stay valid in its own language,
so none of the encoding levers above can shrink it. The only token lever for
foreign code is **avoidance**: keep the agent's working set to each capsule's
typed *boundary card*, not its body. `fgx capsule-tokens <file>` measures the
verbatim body against a compact boundary card; the `equalizer` is body / card.

| Capsule | language | body (lines) | card | equalizer | saved |
| --- | --- | --- | --- | --- | --- |
| order_total | java | 287 (21) | 42 | **6.8x** | 85% |
| clamp_sample | c | 81 (6) | 26 | 3.1x | 68% |
| saxpy | cuda | 102 (5) | 39 | 2.6x | 62% |
| frontend | typescript | 80 (4) | 32 | 2.5x | — |
| price_oracle | solidity | 63 (7) | 31 | 2.0x | 51% |
| normalize_py | python | 47 (5) | 31 | 1.5x | 34% |
| backup | bash | 40 (2) | 33 | 1.2x | 18% |

The equalizer scales with body size and language verbosity: it is largest
for the verbose business language (Java, 6.8x) and dense systems code (C,
CUDA), smallest for terse two-line scripts. These are *toy* bodies; real
capsules are 50–200 lines, where the same mechanism reaches 10–50x. The point
is structural: **a verbose language costs the same to *use* as a terse one,
because the agent sees the boundary, not the body.**

Shared-namespace dedup is a second polyglot lever: a type declared once in
Forgentex versus restated in every language that binds it. On `web_session`
the shared `Session` is bound by a Python and a TypeScript capsule, saving
the restatement (conservatively, since each language's schema is more verbose
than the one Forgentex declaration). `capsule-tokens` reports this too.

Reproduce: `fgx capsule-tokens <file> [--json]`.

## Reproduce

```text
fgx token-report <file> [--json]
```
