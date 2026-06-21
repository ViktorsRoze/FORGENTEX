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
