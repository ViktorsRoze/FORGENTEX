# Capsule Sandbox Model (v2.3)

v2.3 makes a capsule's constrained environment explicit, checkable, and
enforced at the boundary — without invoking a foreign runtime. It defines
the sandbox capability model, reconciles declared effects against granted
capabilities, parses resource limits, produces an enforcement plan, and
gates mock execution on sandbox approval.

Real OS-level process isolation and live execution remain deferred. v2.3
owns the *policy contract and enforcement plan* a future sandbox executor
will apply.

## Sandbox rules

A capsule's `sandbox` rules carry three kinds of entry:

```fgx
foreign backend python version "3.13" effects [network.read]:
    sandbox "net.listen 127.0.0.1:8000"   # capability grant + scope
    sandbox "limit.wall_ms 1000"          # resource limit
    sandbox "limit.mem_mb 256"
    sandbox "fs.none"                      # explicit denial
```

### Capability grants

A grant is `<capability> [scope]`. The capability's prefix is its category:

| Category | Example capabilities | Scope |
| --- | --- | --- |
| `fs` | `fs.read`, `fs.write` | host path |
| `net` | `net.connect`, `net.listen` | host:port |
| `proc` | `proc.spawn` | command |
| `env` | `env.read` | variable |
| `chain` | `chain.read`, `chain.write` | network/VM |
| `gpu` | `gpu.compute` | device |

A `<category>.none` rule is an explicit denial — it documents intent and
grants nothing.

### Resource limits

`limit.<key> <integer>` declares a limit a future executor will enforce.
Valid keys: `cpu_ms`, `mem_mb`, `wall_ms`, `fs_bytes`, `net_bytes`. An
unknown key or non-integer value is `sandbox_invalid_limit`.

## Effect reconciliation

Every declared effect maps to a capability category:

- `network.*` / `net.*` -> `net`
- `io.*` / `fs.*` -> `fs`
- `proc.*` / `process.*` / `subprocess.*` -> `proc`
- `env.*` -> `env`
- `chain.*` -> `chain`, `gpu.*` -> `gpu`
- everything else (`alloc`, `cpu.*`, `crypto.*`, `memory.*`) is
  compute-only and needs no grant.

An effect whose category needs a grant but has none is
`sandbox_effect_uncovered` — blocking at `fgx check`. A capsule cannot
declare `network.read` while sandboxed to `fs.none` or `net.none`.

A granted category with no matching effect is an *unused grant*: not an
error, but it pushes the capsule's decision to `review` because an
over-broad sandbox is a review signal.

## Decision

Per capsule:

- `deny` — any uncovered effect or invalid limit.
- `review` — high-risk class (shell/native/gpu/smart_contract) or any
  unused grant.
- `allow` — otherwise.

The plan's overall status is the worst capsule decision.

## CLI

```text
fgx sandbox <file> [--json] [--strict]   # forgentex.sandbox_plan.v1; --strict fails on deny
fgx check <file> --json                  # sandbox diagnostics inline
fgx capsule-run <file> --sandbox         # gate mock execution on approval
```

`fgx capsule-run --sandbox` refuses to run a capsule whose effects are not
covered by its sandbox — the v2.3 link between execution and policy. Without
`--sandbox`, the deterministic mock run from v2.1 is unchanged.

## Report schema

`forgentex.sandbox_plan.v1`:

```json
{
  "schema": "forgentex.sandbox_plan.v1",
  "file": "...",
  "status": "allow | review | deny",
  "capsules": [
    {
      "id": "@capsule.backend",
      "name": "backend",
      "risk_class": "managed",
      "decision": "allow",
      "grants": [{"capability": "net.listen", "scope": "127.0.0.1:8000"}],
      "limits": [{"key": "wall_ms", "value": "1000"}],
      "effects": ["network.read"],
      "uncovered_effects": [],
      "unused_grants": []
    }
  ]
}
```

## Acceptance gates (v2.3)

- A capsule declaring `network.read` with a `net.*` grant is `allow`; with
  only `fs.none` it is `deny` and `fgx check` reports
  `sandbox_effect_uncovered`.
- Resource limits parse into the plan; invalid limits are rejected.
- High-risk capsules and over-broad sandboxes require `review`.
- `fgx capsule-run --sandbox` refuses denied capsules; the plain mock run
  is unchanged.
