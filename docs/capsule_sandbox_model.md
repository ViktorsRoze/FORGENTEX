# Capsule Sandbox Model (v2.3)

v2.3 makes a capsule's constrained environment explicit, checkable, and
enforced at the boundary — without invoking a foreign runtime. It defines
the sandbox capability model, reconciles declared effects against granted
capabilities, parses resource limits, produces an enforcement plan, and
gates mock execution on sandbox approval.

Live native execution now applies process-tree containment plus wall, CPU, and
memory limits as described below. Filesystem/network syscall isolation and
byte accounting remain deferred. v2.3 continues to own the policy contract
used by those runtime slices.

## Sandbox rules

A capsule's `sandbox` rules carry four kinds of entry:

```fgx
foreign backend python version "3.13" effects [network.read]:
    sandbox "net.listen 127.0.0.1:8000"   # capability grant + scope
    sandbox "limit.wall_ms 1000"          # resource limit
    sandbox "limit.mem_mb 256"
    sandbox "fs.none"                      # explicit denial
    sandbox "audit.redact token"           # transcript policy for one input
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

`limit.<key> <integer>` declares a resource limit. The live native C/C++
bridge enforces `limit.wall_ms` for the complete native process tree. Windows
Job Objects and Unix process groups also prevent descendants from surviving a
normally completed native call. `limit.cpu_ms` and `limit.mem_mb` are also
installed before native execution: Windows applies aggregate Job Object
budgets, while Unix processes inherit whole-second `RLIMIT_CPU` and per-process
`RLIMIT_AS`. Filesystem/network byte limits remain executor-plan data for
future enforcement.
Valid keys: `cpu_ms`, `mem_mb`, `wall_ms`, `fs_bytes`, `net_bytes`. An
unknown key or non-integer value is `sandbox_invalid_limit`.

### Audit redaction

`audit.redact <input-name>` marks one declared capsule input as sensitive in
runtime audit data. It is policy, not a capability grant, and does not change
the value sent to the foreign runtime. `CapsuleCall.inputs` replaces the value
with `{"redacted":true}` and records the binding name in `redacted_inputs`.
Pipeline stage reports apply the same marker to the stage input and matching
shared source value. If foreign stderr accompanies a failed redacted call, the
audit and caller receive the stable failure kind plus a redaction notice, not
the raw diagnostic. Unknown or missing input names are blocking
`sandbox_invalid_redaction` diagnostics.

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

- `deny` — any uncovered effect, invalid limit, or invalid redaction.
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
      "redacted_inputs": ["token"],
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
- Audit redactions name declared inputs, grant no capability, and unknown names
  are rejected.
- High-risk capsules and over-broad sandboxes require `review`.
- `fgx capsule-run --sandbox` refuses denied capsules; the plain mock run
  is unchanged.
