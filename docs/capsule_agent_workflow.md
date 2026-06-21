# Capsule Agent Workflow (v2.9)

Agents need to find, slice, and review foreign capsules without reading
whole projects. v2.9 adds two compact, machine-readable surfaces over the
capsule reviews from v2.0–v2.8, both via `fgx capsule-context`.

## Inventory

```text
fgx capsule-context <file> [--json]
```

`forgentex.capsule_inventory.v1` lists every capsule with its language,
risk class, effect/dependency/binding/fixture counts, and an **aggregated
decision** (`allow` / `review` / `reject`). This is the agent's index: scan
it to decide which capsule to open.

## Context packet

```text
fgx capsule-context <file> --capsule <name> [--json]
```

`forgentex.capsule_context.v1` is one capsule's full review packet, merging
every relevant analyzer into a single decision plus a human-readable risk
summary:

- contract validity (`fgx capsule` status),
- sandbox decision (`fgx sandbox`),
- dependency supply-chain risk (`fgx capsule-audit`),
- the class-specific review where applicable — native/GPU (`fgx
  native-review`), shell (`fgx shell-review`), or smart-contract (`fgx
  contract-review`),
- the typed boundary (inputs/outputs), shared-namespace bindings, and
  effects.

The **overall decision** is the worst signal among them (`reject` >
`review` > `allow`), so an agent gets a single go/no-go plus the reasons,
in one call, without running six separate reviews.

## Example

```text
fgx capsule-context examples/capsule/saxpy_cuda.fgx --capsule saxpy
# risk=gpu decision=review
#   contract=review_required sandbox=review dependency_risk=medium native=review
#   risk: gpu capsule / dependency risk: medium / sandbox: review / native review: review
```

A managed Python/TypeScript capsule with pinned deps aggregates to `allow`;
a CUDA, shell, or smart-contract capsule aggregates to at least `review`.

## Acceptance gates (v2.9)

- `fgx capsule-context <file>` lists all capsules with an aggregated
  decision.
- `fgx capsule-context <file> --capsule <name>` merges contract, sandbox,
  dependency, and class-specific reviews into one `overall_decision`.
- `rw` shared bindings and non-low dependency risk appear in the risk
  summary.
- An unknown capsule name errors clearly.
