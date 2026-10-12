# Token-Efficiency Findings

The tables on this page come from two different instruments, and they must not
be confused:

- **`heuristic-v1`** — the fast estimator in `src/tokens.rs`. It needs no
  tokenizer and is fine for local budgets.
- **Real byte-pair encoding** — `cl100k_base` and `o200k_base`, measured
  independently during the 2026-09-05 external evaluation
  (`docs/language_evaluation_2026-09-05.md`, finding F1), and reproducible here
  with `fgx token-report --bpe-command` (see the end of this page).

This page used to say "treat the *ratios* as the signal". That advice was wrong.
The heuristic's ratios do not merely misjudge how big a difference is — on real
BPE they sometimes point the other way. **No heuristic ratio on this page is
evidence that one surface costs fewer tokens than another**; only the measured
table is.

## Measured with real BPE

Actual encoder counts for `fgx fmt --to fgx|fgai` output, LF-normalised, with the
trailing newline. Text only: no request framing, prompt, tool schema, or
reasoning.

| File under `examples/` | heuristic agent/human | `cl100k_base` human → agent | `o200k_base` human → agent | What actually happened |
| --- | ---: | ---: | ---: | --- |
| `hello.fgx` | 0.86 | 37 → 32 | 37 → 32 | 13.5% smaller |
| `media/audio_mix.fgx` | 0.82 | 151 → 128 | 151 → 128 | 15.2% smaller |
| `audio/src/dsp.fgx` | **0.97** | 869 → 877 | 870 → 878 | **~0.9% larger** |
| `capsule/normalize_python.fgx` | 0.89 | 148 → 150 | 149 → 150 | ~0.7–1.4% larger |
| `capsule/web_session.fgx` | 0.89 | 314 → 313 | 315 → 315 | unchanged |
| `smoke/inline_tests.fgx` | **0.97** | 121 → 130 | 123 → 132 | **~7.3% larger** |
| `apps/wav_waveform/src/main.fgx` | 0.95 | 1,093 → 1,144 | 1,102 → 1,153 | ~4.7% larger |
| `apps/media_visualizer/src/main.fgx` | 0.89 | 1,457 → 1,459 | 1,459 → 1,471 | ~0.1–0.8% larger |
| **Combined** | — | **4,190 → 4,233** | **4,206 → 4,259** | **~1.0–1.3% larger** |

What that says, plainly:

1. **The agent surface is not a token saving in general.** It is smaller on
   small, declaration-heavy files and slightly *larger* on body-heavy ones, and
   larger overall across this sample. The heuristic said the opposite for every
   row.
2. **The likely reason is structural.** BPE already encodes common English
   keywords (`struct`, `return`, `while`) as single tokens, so shortening them
   saves little, while the dense form's extra punctuation and unfamiliar
   two-letter spellings can each cost a token. The heuristic charged
   `ceil(len / 4)` per word, which systematically overvalued shortening them.
3. **This is a small convenience sample**, not a benchmark. It is enough to
   disprove "the compact surface always saves tokens"; it is not enough to
   characterise real workloads.

## The heuristic tables below are kept as history

They record what the project believed at the time and why. Read their ratios as
a description of the *estimator*, not of the encodings.

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

## Estimated surfaces (heuristic-v1) — superseded by the measured table above

| File | human | agent | agent/human | minified | compressed | best/human |
| --- | --- | --- | --- | --- | --- | --- |
| hello.fgx | 50 | 43 | 0.86 | 41 | 41 | 0.82 |
| media/audio_mix.fgx | 226 | 211 | 0.93 | 205 | 205 | 0.91 |
| audio/src/dsp.fgx | 1150 | 1112 | 0.97 | 1061 | 1033 | 0.90 |
| capsule/normalize_python.fgx | 193 | 171 | 0.89 | 171 | 171 | 0.89 |
| capsule/web_session.fgx | 435 | 387 | 0.89 | 381 | 381 | 0.88 |
| smoke/inline_tests.fgx | 160 | 155 | 0.97 | 143 | 143 | 0.89 |

## What the project concluded at the time (superseded)

The points below were drawn from the heuristic and are **contradicted by the
measured table above** wherever they claim a saving. They stay so the reasoning
that led here remains visible.

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

The hypothesis at this point was that the order-of-magnitude win is not
per-file at all but in **not sending the file**: project maps, context packets,
semantic patches, and diff-only re-reads. The cost model below tested it — and,
once compared against a competent baseline rather than whole-file resend, did
not support it for single files. See the next section.

## Prelude aliases (dual-named builtins)

Twelve high-frequency builtins now carry a two-letter agent alias
(`core_math_clamp_i16` → `cl`), readable in `.fgx`, short in `.fgai`, AST
identical, verified lossless (see `docs/prelude_aliases.md`). On
builtin-calling code the *heuristic* improved: `audio_mix.fgx` went from
agent/human 0.93 → 0.82 and best/human 0.91 → 0.79. Under real BPE that file's
agent form is 15.2% smaller — the best result in the measured sample, and
consistent with aliases replacing long multi-token names — but no measurement was
taken before and after the change, so it does not isolate the aliases' effect. Code that does
not call builtins is unchanged. This is the largest per-file lever found so
far, because the canonical builtin names are long and the calls are
frequent — but it still only applies to builtin-heavy code, and the
order-of-magnitude win remains in not sending the file at all.

## The edit-task cost model — and the baseline that was missing

`fgx edit-bench` is a **cost model**, not a measurement of a coding task: no
agent performs the edits, nothing checks both strategies reach the same result,
and every count is `heuristic-v1`. It compares three strategies for N edits:

- **Whole-file resend** — `file * (1 + N)`. A deliberately weak baseline.
- **Text patch** — read the file once, keep it, send a small patch per edit:
  `file + N * patch`. What a competent conventional agent actually does.
- **Map + symbol + semantic patch** — `map + N * (symbol + patch)`.

Both patch strategies are charged the same patch size, so the comparison is
about what each strategy *reads*. The representative patch is a real v1
semantic patch against the file, with a `NODE_HASH` precondition, and the model
refuses to run unless the patch engine accepts it.

| File | functions | semantic / whole-file @ 8 edits | **semantic / text patch** @ 8 edits |
| --- | ---: | ---: | ---: |
| `hello.fgx` | 1 | 1.46 | **1.52** |
| `media/audio_mix.fgx` | 2 | 0.68 | **1.83** |
| `audio/src/dsp.fgx` | 12 | 0.15 | **0.94** |
| `apps/wav_waveform/src/main.fgx` | 6 | 0.20 | **1.32** |
| `capsule/web_session.fgx` | 0 | rejected — no functions to edit | rejected |

**What this page used to claim, and why it was wrong.** It reported the
semantic-patch strategy as "~2x cheaper on a 2-function file, ~8x on a
12-function file", and called that "the order-of-magnitude token win". Every one
of those figures compared against re-sending the whole file on every edit, a
workflow no competent agent uses. Against the text-patch baseline the same model
shows **no general advantage**: the semantic strategy costs more on three of four
files and is roughly even on the largest. The row for `web_session.fgx` was worse
than optimistic — the file has no functions, so the old model charged nothing
for reading the edit target and still invented a function patch, reporting a
94% saving on edits that could not happen. It is now rejected.

Two further corrections: the old representative patch used a
`REPLACE_STATEMENT` operation the patch protocol does not have, and the
per-edit figures were heuristic counts, which can point the wrong way.

What remains plausible, and unmeasured: that map + symbol retrieval wins when a
file is too large to hold in context, when edits span many files, or when the
conversation cannot cache what it read. Those are the cases worth measuring
with real agents and real tokenizers before claiming anything.

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

## Measuring it yourself

`fgx token-report` always reports the heuristic, labelled `"ratio_kind":
"heuristic"` with a caveat attached. Add `--bpe-command` to measure every surface
with a real tokenizer as well; the measured counts are reported beside the
heuristic, never merged into it:

```powershell
pip install tiktoken
fgx token-report examples/audio/src/dsp.fgx `
  --bpe-command python --bpe-arg tools/tokenizer/tiktoken_count.py `
  --bpe-arg o200k_base --bpe-name tiktoken:o200k_base --json
```

Any tokenizer can stand behind `--bpe-command`: it receives one surface's text on
stdin and prints a single integer. A tokenizer that fails, or prints anything
else, fails the whole measurement rather than leaving a table that mixes measured
and estimated rows.
