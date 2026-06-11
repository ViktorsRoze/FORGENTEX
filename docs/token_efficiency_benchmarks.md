# Forgentex Token Efficiency Benchmarks

This plan defines benchmark projects for showing that Forgentex can express
reviewable systems programs with fewer source and agent-context tokens than a
typical systems-language workflow. These benchmarks are about token efficiency,
not raw runtime speed. Runtime performance remains covered by
`docs/performance_tooling.md`.

## Goals

- Show how much verified behavior can fit in a small `.fgx` or `.fgai` program.
- Compare human syntax, agent syntax, canonical AST, project maps, and context
  packets as different token surfaces over the same program.
- Keep every demo deterministic so token counts, outputs, and review packets
  are stable in CI.
- Include safety evidence: effects, bounds checks, borrow rules, tests,
  diagnostics, and release-review metadata should count as part of the value,
  not as hidden work.
- Avoid vague "shorter code" claims by reporting concrete counts and ratios.

## Core Metrics

Each token-efficiency benchmark should record:

| Metric | Meaning |
| --- | --- |
| Source tokens | Tokens in the `.fgx` human syntax file. |
| Agent tokens | Tokens in the `.fgai` agent syntax rendering. |
| Canonical AST tokens | Tokens in `fgx ast --canonical` output. |
| Project-map tokens | Token estimate from `fgx map` or `fgx map --project`. |
| Context-packet tokens | Token estimate from `fgx context <symbol> --json`. |
| Patch tokens | Tokens needed for a semantic patch that changes one behavior. |
| Verified behaviors | Count of public functions, inline tests, checked effects, borrow-report evidence, and deterministic run cases. |
| Behavior density | Verified behaviors per 1,000 source tokens. |
| Review density | Verified behaviors plus diagnostics per 1,000 context-packet tokens. |

When comparison implementations exist, the report should compare Forgentex
against plain Rust, Zig, and C references under the same requirements. The
comparison must include supporting tests and safety annotations, not just the
core function body.

## Main Benchmark Project: Butterfly Ray Grid

Planned path:

```text
examples/bench/butterfly_ray_grid.fgx
examples/bench/butterfly_ray_grid.json
```

The benchmark simulates a ray of light moving through a deterministic 2D grid of
balls. A tiny change to the initial angle should cause the ray to leave through
a different cell or side after enough reflections. This makes the program easy
to explain visually while still exercising nontrivial control flow, fixed-size
data, numeric stability, and deterministic review.

Required model:

- A `Ray` with position, direction, and accumulated path hash.
- A `Ball` or `Disc` with center, radius, and reflectivity.
- A fixed grid of balls stored in arrays or slices.
- Deterministic fixed-point or integer-scaled math to avoid platform-dependent
  floating behavior in the first version.
- A bounded loop with `max_bounces`.
- Collision selection that chooses the nearest valid hit.
- Reflection update and boundary-exit detection.
- Two entry angles separated by a tiny deterministic delta.

Planned small-library dependencies:

- `core.fixed` for deterministic scaled arithmetic
- `core.stats` for precise summary statistics over repeated ray runs
- `core.geom` for rays, vectors, discs, and reflection helpers
- `core.grid` for grid indexing and boundary exits
- `core.hash` for stable path fingerprints
- `core.rng` for seeded perturbation tests when randomized variants are added

Required outputs:

- `exit_a`: side and cell for the baseline ray.
- `exit_b`: side and cell for the perturbed ray.
- `bounce_count_a` and `bounce_count_b`.
- `path_hash_a` and `path_hash_b`.
- `divergence_step`: first bounce where the two paths differ, or `max_bounces`
  when they remain equivalent.

Why this is useful for token-efficiency claims:

- The program is small enough to inspect, but rich enough to need structs,
  arrays, loops, branches, helper functions, tests, and deterministic numeric
  conventions.
- The agent can request only the collision or reflection symbol through
  `fgx context`, making compact context packets meaningful.
- A semantic patch can change one physical rule, such as reflectivity,
  boundary behavior, or angle delta, and the patch-token cost can be measured.
- The output is visually intuitive: a tiny angle change ends somewhere else.

Acceptance gates:

- `fgx check examples/bench/butterfly_ray_grid.fgx --json` passes.
- `fgx run examples/bench/butterfly_ray_grid.fgx --entry butterfly_ray_demo --json-input examples/bench/butterfly_ray_grid.json` returns deterministic exits and hashes.
- `fgx bench examples/bench/butterfly_ray_grid.fgx --entry butterfly_ray_demo --json-input examples/bench/butterfly_ray_grid.json --target x86_64-unknown-linux-gnu --json` emits benchmark evidence.
- `fgx map examples/bench/butterfly_ray_grid.fgx` reports stable symbol token estimates.
- `fgx context @function.trace_ray --deps types,effects,tests --json` stays small enough for agent review.
- A tiny angle-delta patch changes the exit cell while preserving tests and
  deterministic bounds.

## Supporting Token-Efficiency Demo Programs

These smaller demos should exist so the token-efficiency story covers multiple
program shapes.

| Demo | Planned path | What it proves |
| --- | --- | --- |
| Bitstream parser | `examples/bench/token_bitreader.fgx` | Compact state machine, bounds checks, and deterministic parse diagnostics. |
| Packet/header validator | `examples/bench/token_packet_header.fgx` | Struct layout, endian-style field handling, result types, and tests. |
| Audio kernel | existing `examples/media/audio_mix.fgx` and `audio_mix_simd.fgx` | Numeric loops, SIMD review path, perf reports, and scalar-vs-optimized comparison. |
| Precise statistics | `examples/bench/token_stats_summary.fgx` | Checked sums, fixed-point means, variance, histograms, and deterministic rounding evidence. |
| Borrow safety micro-suite | `examples/bench/token_borrow_safety.fgx` | Ownership, local aliases, non-lexical borrow endings, reborrows, two-phase mutable activation, projection alias analysis, interprocedural lifetime contracts, loop-carried borrow liveness, conditional borrow-state joins, and borrow-report evidence in a small file. |
| Effectful workflow | existing `examples/io/file_copy.fgx` and `examples/network/agent_ping.fgx` | Explicit effects and deterministic mock runtime APIs without large boilerplate. |
| Agent patch workflow | existing `examples/agent/signed_patch.fgx` | Patch review, crypto-effect boundaries, signed envelopes, and compact review packets. |
| Inline spec suite | existing `examples/smoke/inline_tests.fgx` | Executable specs embedded close to code with stable diagnostic xfail checks. |

## Reporting Plan

The first implementation can use existing commands:

```powershell
fgx fmt <file> --to fgai
fgx ast <file> --canonical
fgx map <file>
fgx context @function.<name> --deps types,effects,tests --json
fgx bench <file> --entry <entry> --json-input <input> --target x86_64-unknown-linux-gnu --json
fgx borrow-report <file> --json
fgx borrow-conformance --json
```

Later, add a dedicated report command once the corpus is stable:

```powershell
fgx token-report examples/bench/butterfly_ray_grid.fgx --entry butterfly_ray_demo --json
```

Planned `forgentex.token_efficiency.v1` fields:

- source file and entry function
- source, agent, AST, map, and context token counts
- public symbols and verified behavior counts
- diagnostics and borrow/effect evidence counts
- borrow conformance suite status
- patch-token examples
- optional comparison implementation paths
- behavior-density and review-density ratios

## Fairness Rules

- Count tests, effects, safety annotations, manifests, and required helper code.
- Do not compare a fully checked Forgentex demo against an unchecked reference
  snippet.
- Keep inputs and expected outputs identical across languages.
- Prefer deterministic fixed-point arithmetic until the runtime has stable
  cross-backend floating behavior.
- Report both human-readable source tokens and agent-context tokens; either one
  alone can mislead.
- Treat token efficiency as a reviewability metric, not a replacement for
  runtime performance, correctness, or safety.

## Implementation Order

1. Add `butterfly_ray_grid.fgx` and deterministic JSON input.
2. Add inline tests or golden run expectations for baseline and perturbed rays.
3. Add project-map and context-packet token snapshots.
4. Add a semantic patch that changes the angle delta or reflectivity rule.
5. Add a comparison note for Rust/Zig/C once the Forgentex behavior is stable.
6. Add `fgx token-report` only after the manual report shape has settled.
