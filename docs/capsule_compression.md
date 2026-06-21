# Agent-View Compression and Doc-Compact (v2.10)

v2.10 implements the deterministic, lossless half of the token-optimization
catalog ([`docs/token_optimization_v2.md`](token_optimization_v2.md)): a
dictionary compressor for the agent (`.fgai`) rendering, plus the
documentation of the doc-compact writing register. Whether compression
nets fewer *tokens* (not just characters) is judged by `fgx token-report`
(the measurement step), not asserted here.

## Agent-view compression

```text
fgx compress <file> [--emit] [--json]
```

The file is rendered to its `.fgai` agent form and compressed with a
dictionary of repeated identifiers and dotted paths:

```text
#FGAI-Z v1 sigil=§ dict=8
#D §0=clamp_i16_from_i32
#D §1=apply_gain_q15
<body with §0, §1, ... substituted for the words>
```

Properties (all conformance-tested):

- **Deterministic** — the same input always produces the same output.
- **Lossless** — `decompress(compress(x)) == x` exactly. The header records
  a `dict=N` count so even a body line beginning with `#D` (a foreign-code
  comment) round-trips correctly.
- **Collision-free** — the handle sigil is chosen as a character absent
  from the source, so a handle can never be confused with original text.
- **Never larger** — if the dictionary header outweighs the savings, the
  source is returned unchanged (ratio ≤ 1.0).

`--emit` prints the compressed artifact; otherwise a
`forgentex.capsule_compress.v1` report (original/compressed chars,
dictionary entries, ratio, `roundtrip_ok`) is printed.

Character compression is modest (a few percent) and sometimes zero — which
is exactly why it is gated behind measurement. The compressor exists so the
token-report can answer, with evidence, whether dictionary substitution is
worth applying for a given tokenizer.

## Doc-compact register

Doc-compact is a *writing register* for comments, docs, reviews, and commit
messages — telegraphic, verb-first, articles dropped, but with every
load-bearing identifier, type, node ID, and diagnostic code kept verbatim.
It is **not** an automatic lossless transform (compacting English is
inherently lossy), so v2.10 specifies it as a convention agents write in
and humans can expand for display, rather than a `fgx` command. Its payoff
is measured per artifact by the token-report, on prose actually written in
the register. See [`docs/token_optimization_v2.md`](token_optimization_v2.md).

## Acceptance gates (v2.10)

- `fgx compress` is deterministic and lossless across the example corpus;
  `decompress(compress(x)) == x`.
- A body line starting with `#D` survives the round trip.
- The compressed artifact is never larger than the source.
- Whether compression reduces tokens is reported by `fgx token-report`, not
  assumed.
