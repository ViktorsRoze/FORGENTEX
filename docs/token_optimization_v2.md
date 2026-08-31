# Token Optimization v2: Capsule Compression, Compact Docs, and the Idea Catalog

This document designs three v2 token-efficiency tracks:

1. compressed agent views for foreign-language capsules, losslessly
   decompressible into the `.fgx` review surface;
2. a compact controlled-language documentation register ("doc-compact")
   for comments, docs, reviews, and commit messages;
3. a catalog of generation-side and reading-side token-saving techniques
   for the whole toolchain.

Everything here must obey one rule from the start: **measure before
believing**. Every technique lands behind the token-efficiency benchmark
corpus (`docs/token_efficiency_benchmarks.md`) with before/after token
counts from a real tokenizer and a task-success check with a real model.
A technique that saves 30% of tokens but lowers an agent's edit success
rate is a net loss; the benchmark must catch that, not a hunch.

## 1. Foreign capsule compression (`.fgai` views for foreign code)

Foreign code inside capsules (Python, JavaScript, and the rest of the
catalog in `docs/v2_roadmap.md`) gets the same dual-representation
treatment as Forgentex itself:

```text
capsule source (canonical)  <->  compressed agent view  <->  .fgx review view
```

The key design decision: **compression cannot change the foreign
language's semantics, so it is an encoding, not a dialect.** A Python
capsule still contains real Python. The compressed view is a deterministic,
lossless re-encoding for agent reading:

- strip comments and blank lines into a side table (kept for review view);
- canonical minimal whitespace for the language;
- a dictionary header that assigns short handles to repeated identifiers,
  dotted paths, and string literals (`D1=numpy.fft.rfft`), applied only
  when the tokenizer-measured saving is positive;
- region hashing: unchanged functions/classes collapse to one line
  (`PY.f normalize @h:9f2a… 14 lines`), expandable on demand exactly like
  `.fgmap` symbol retrieval — an agent reads the capsule skeleton and
  requests only the bodies it needs;
- the typed boundary block (inputs, outputs, deps, effects, sandbox) is
  never compressed: it is the review-critical surface.

Decompression to the review view regenerates the full readable source with
comments restored, and `fgx fmt` idempotence applies: the round trip
`compress(decompress(view)) == view` is a conformance requirement, the same
contract `.fgx <-> .fgai` already has.

**Generation rule:** agents *read* the compressed view but *write* native
foreign code (or a semantic patch against a region hash). LLMs are trained
on idiomatic Python/JS; forcing them to emit a dictionary-compressed
encoding would trade output quality for input savings. The toolchain
compresses after generation, deterministically. This rule stays until the
benchmark corpus proves a model generates equally well in compressed form.

Deliverables:

- capsule compression spec per language (Python first)
- `fgx capsule view <file> --compressed` and `--review`
- round-trip and idempotence conformance cases
- tokenizer-measured compression ratios in the benchmark corpus

## 2. Doc-compact: the controlled compact-language register

Documentation, review annotations, commit messages, and diagnostics prose
get a compact register — simplified telegraphic language ("caveman speech")
that drops low-information words while keeping every load-bearing term:

```text
full:    This function clamps the input sample to the valid i16 range
         before mixing, because overflow would wrap and corrupt audio.
compact: clamp sample to i16 range before mix. overflow wraps, corrupts audio.
```

Rules of the register (so it stays parseable and unambiguous, not slang):

- verb-first imperative sentences; drop articles, copulas, and hedges;
- keep all type names, node IDs, effect names, and diagnostic codes
  verbatim — never abbreviate load-bearing identifiers;
- one fact per sentence; no pronouns across sentences;
- numbers, units, and conditions stay explicit (`if len > 4096: chunk`);
- a fixed small vocabulary for structure words (`because -> why:`,
  `for example -> ex:`, `note that ->` omitted).

Both renderings live in the same doc comment: compact is canonical (what
agents read and write), and the review surface may show either. Expansion
to full prose is a presentation feature, not storage. Realistic saving is
20–40% on prose; the benchmark corpus measures it per artifact type.

Where it applies first: doc comments (`##`), review annotations (`#@`),
patch review summaries, and agent context packets. Where it never applies:
diagnostic codes and machine-readable JSON fields, which are already a
fixed schema.

## 3. Idea catalog: advanced token-saving techniques

### Reading side (what agents consume)

1. **Skeleton-first retrieval.** `.fgmap` and capsule skeletons give names,
   types, and hashes; bodies load on demand. Already core — extend to
   foreign capsules and docs.
2. **Stable node-ID addressing.** Refer to code as `@function.mix_samples`
   instead of quoting it. Quoting is the single largest token waste in
   agent workflows.
3. **Hash-elision of unchanged regions.** Any artifact (function body,
   capsule region, doc section) collapses to `@id @h:…` when its hash
   matches the agent's last-seen state; only deltas stream.
4. **Dictionary headers.** Per-file short handles for repeated long
   identifiers and paths, applied only when tokenizer-positive.
5. **Canonical formatting as a cache key.** One canonical form means
   repeated reads are byte-identical, which maximizes provider prompt-cache
   hits — often a bigger real-cost saving than any encoding trick.
6. **Diff-only re-reads.** After a patch, agents receive the semantic diff
   against the last context packet, never the re-rendered file.
7. **Dependency-sliced context.** `fgx context <symbol> --deps` returns the
   minimal type/effect/test closure; token budget declared in the request,
   enforced by the server.
8. **Token-cost annotations.** Maps and context packets carry measured
   token counts per node so agents can plan retrieval under a budget.
9. **BPE-aware naming policy.** Lint that flags identifiers that tokenize
   badly (rare subwords, mixed scripts); prefer common-subword snake_case.
   Measured against the benchmark tokenizer, never guessed.
10. **No line numbers in agent views.** Spans live in the source map;
    agent views address by node ID only, so edits never shift context.

### Generation side (what agents produce)

11. **Semantic patches over file rewrites.** Already core: a 6-line `.fgp`
    replaces a 200-line file emission.
12. **Patch preconditions with expected diagnostics.** A patch declares
    what the checker should report afterward; one round trip replaces a
    generate–check–regenerate loop. Saved round trips dwarf saved tokens.
13. **Compiler repair plans.** `fgx fix --plan` turns a diagnostic into a
    one-shot structured fix the agent instantiates, instead of free-form
    reasoning about the error text.
14. **Pattern instantiation by ID.** A catalog of reviewed code templates
    (`pattern.fast_path_simd`, `pattern.bounds_checked_loop`); the agent
    emits the pattern ID plus bindings, the toolchain expands it.
15. **Default elision in agent mode.** Canonical defaults (visibility,
    effects inferred as none, standard layouts) are omitted in `.fgai` and
    restored by the formatter.
16. **Grammar-constrained decoding.** Publish the `.fgai`/`.fgp` grammar in
    a constrained-decoding format so providers that support it never waste
    tokens on malformed output or retries.
17. **Write native, compress after.** For foreign capsules and prose, the
    agent emits the natural form; deterministic toolchain compression
    handles the reading-side savings (see section 1).
18. **Batched patch operations.** One `.fgp` with N operations and one
    review report, instead of N patch round trips.
19. **Doc-compact for all agent-written prose.** Commit messages, review
    notes, and explanations use the section-2 register with schema fields.
20. **Abstention tokens.** Context packets state what was *excluded* and
    why (`excluded: 14 symbols, no dep path`), so agents don't spend turns
    asking for things the server already ruled out.

Implementation update: `fgx context` now accepts `--max-tokens`, uses the
shared `heuristic-v1` estimator, admits optional dependency categories only as
complete slices, and emits explicit inclusion/exclusion and budget evidence.
Requested effects are safety-mandatory and cause a stable budget-floor error
rather than being elided. Project maps now carry per-symbol semantic estimates.
This completes the local-tooling portion of items 7, 8, and 20; provider-level
prompt budgeting and real-tokenizer calibration remain separate integration
work.

Item 3 now has a local stateful implementation as well. Full context packets
carry section hashes and a packet hash; `fgx context --since <packet>` verifies
the baseline symbol/state and omits only byte-identical symbol/map/type/test
sections. Effects remain explicit. Logical/transmitted/saved token evidence and
the serialized-packet benchmark prevent hash/framing overhead from being hidden.
Long-lived provider cache integration remains future work.

### Guardrails

- Review-critical surfaces (unsafe blocks, ABI, effects, capsule
  boundaries, crypto) are never compressed, elided, or telegraphed.
- Every technique ships with a benchmark entry: tokens before/after and
  task success rate with at least one real model.
- Compression is always deterministic and lossless; if a round trip can't
  reproduce the canonical bytes, it does not ship.

## Sequencing

Doc-compact and the reading-side catalog items build on v1.0 tooling and
can start immediately. Capsule compression depends on the v2 capsule
specification (`v2.0`/`v2.1` in `docs/v2_roadmap.md`) and lands as its own
milestone alongside the Python prototype.
