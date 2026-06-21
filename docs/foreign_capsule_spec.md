# Foreign Capsule Specification (v2.0)

This document defines the v2.0 foreign code capsule: its AST shape, the two
source syntaxes, the manifest/contract fields, dependency pinning rules,
effect and sandbox declarations, and the review-report schema. v2.0 is the
*specification and review* milestone — capsules are parsed, validated, and
reported, but not executed (deferred to v2.3) and not lowered by native
backends (review-only).

## What a capsule is

A capsule embeds a block of another programming language (Python,
JavaScript, Solidity, …) inside a Forgentex program behind an explicit,
typed, reviewable boundary. Foreign code is external code: easy to use, but
never invisible. The capsule *contract* maps to the Forgentex AST and is
fully reviewable; the foreign *code body* is carried verbatim as an opaque
payload in both source views.

## AST shape

```text
Capsule {
  id:        "@capsule.<name>"   // stable node ID
  name:      Forgentex-side handle
  language:  "python" | "javascript" | "solidity" | ...
  version:   runtime/language version, e.g. "3.13"
  env:       optional target environment, e.g. "cpython-3.13-linux-x86_64"
  effects:   declared capsule effects, e.g. [fs.read, network.write]
  inputs:    typed bindings at the Forgentex boundary
  outputs:   typed bindings at the Forgentex boundary
  deps:      pinned dependency references
  sandbox:   sandbox policy rules
  fixtures:  deterministic mock cases (inputs -> outputs) at the boundary
  code:      verbatim foreign source lines
  attrs:     review annotations
}
```

Capsules are `Item::Capsule` in the AST, flow through canonical AST
serialization (`"kind": "capsule"`), and appear in project maps, graph
artifacts, and agent context with a `CAP` line.

## Human syntax (`.fgx`)

```fgx
foreign normalize_py python version "3.13" env "cpython-3.13-linux-x86_64" effects [alloc]:
    input value: i32
    output normalized: i32
    dep "pypi:numpy==2.2.0#sha256:0000...0000"
    sandbox "fs.none"
    code:
        import numpy as np

        def normalize(value):
            # clamp into the unsigned byte range
            return int(np.clip(value, 0, 255))
```

The `code:` block captures every line indented at least 8 spaces verbatim
(the 8-space base indent is stripped). Contract lines are read from raw
source, so dependency references and code may contain `#` without being
treated as comments.

## Agent syntax (`.fgai`)

```text
CAP normalize_py python "3.13" env "cpython-3.13-linux-x86_64" effects [alloc]
  IN value i32
  OUT normalized i32
  DEP "pypi:numpy==2.2.0#sha256:0000...0000"
  SBX "fs.none"
  CODE 5
import numpy as np

def normalize(value):
    # clamp into the unsigned byte range
    return int(np.clip(value, 0, 255))
ENDCAP
```

`CODE <n>` is followed by exactly `n` verbatim lines, then `ENDCAP`. The
explicit line count makes the code body collision-free and lossless: there
is no delimiter a foreign language could accidentally contain. Both views
parse to the same canonical AST (`fgx roundtrip` verifies it), and both
formatters are idempotent.

## Contract validation

`fgx check` and `fgx capsule` enforce the non-negotiable contract. Each rule
is a blocking diagnostic:

| Diagnostic | Rule |
| --- | --- |
| `capsule_missing_version` | a runtime/language version is required |
| `capsule_missing_typed_boundary` | at least one typed input or output is required |
| `capsule_missing_sandbox` | a sandbox policy is required |
| `capsule_empty_code` | a non-empty code body is required |
| `capsule_unpinned_dependency` | every dependency must be pinned |

### Dependency pinning

A dependency is pinned when it carries an exact version (`==`), a
commit/revision (`@<hex>`, ≥7 hex chars), or a checksum/artifact hash
(`#sha256:`, `#sha512:`, `#blake3:`). Floating references are rejected:
bare names, `*`, `latest`, and branch names such as `@main` / `@master`.

## Risk classes

Capsules are classified for review:

| Class | Languages | Extra review |
| --- | --- | --- |
| `managed` | python, javascript, typescript, java, c#, go, … | standard |
| `shell` | bash, powershell, sh | command allowlists, strict sandbox |
| `native` | c, c++ | ABI, allocator, memory-boundary review |
| `gpu` | cuda | GPU target, resource limits, CPU fallback |
| `smart_contract` | solidity, vyper | compiler/EVM pins, gas, wallet/secret-key denied by default |

A managed capsule with a complete contract reviews as `pass`. A high-risk
capsule with a complete contract reviews as `review_required`. Any contract
diagnostic makes the capsule `rejected`.

## Review report schema

`fgx capsule <file> --json` emits `forgentex.capsule_review.v1`:

```json
{
  "schema": "forgentex.capsule_review.v1",
  "file": "...",
  "status": "pass | review_required | rejected",
  "capsules": [
    {
      "id": "@capsule.normalize_py",
      "name": "normalize_py",
      "language": "python",
      "version": "3.13",
      "env": "cpython-3.13-linux-x86_64",
      "risk_class": "managed",
      "high_risk": false,
      "effects": ["alloc"],
      "inputs": ["value: i32"],
      "outputs": ["normalized: i32"],
      "dependencies": [{"source": "pypi:numpy==2.2.0#sha256:...", "pinned": true}],
      "sandbox": ["fs.none"],
      "code_lines": 5,
      "native_backend": "not_lowerable",
      "status": "pass",
      "diagnostics": [],
      "notes": ["native backends: not lowerable (review-only)"]
    }
  ]
}
```

## Deterministic mock fixtures and execution (v2.1)

A capsule may declare deterministic fixtures mapping input literals to
expected output literals at the typed boundary:

```fgx
    input value: i32
    output normalized: i32
    mock 300 -> 255
    mock -5 -> 0
    mock 42 -> 42
```

Agent form uses `MOCK <in> -> <out>`. Fixtures are validated against the
boundary at `fgx check`:

| Diagnostic | Rule |
| --- | --- |
| `capsule_fixture_arity` | fixture input/output counts must match the boundary |
| `capsule_fixture_type` | each fixture literal must match its binding type |

`fgx capsule-run` exercises fixtures without a foreign runtime — the v2.1
deterministic mock execution hook:

```text
fgx capsule-run <file> [--capsule <name>] [--json]          # run all fixtures
fgx capsule-run <file> --input "300"                        # execution hook
```

Running all fixtures emits `forgentex.capsule_run.v1` with a per-case
pass/`type_mismatch`/`arity` status. The `--input` hook looks up the
fixture whose inputs match and returns its outputs deterministically, or a
`no_matching_fixture` trap. Real foreign execution remains deferred to a
sandbox-backed milestone; these hooks make the typed boundary reproducible
and testable now.

## CLI

```text
fgx capsule <file> [--json] [--strict]   # review report; --strict fails on rejected
fgx capsule-run <file> [--capsule <name>] [--input <values>] [--json] [--strict]
fgx check <file> --json                  # capsule + fixture diagnostics inline
fgx fmt <file> --to fgai|fgx             # capsule-aware dual formatting
fgx roundtrip <file> --to fgai           # capsule-aware roundtrip equality
```

## Backend stance

Native backends (C, Zig) do not lower capsules. The review report states
`native_backend: not_lowerable` rather than silently dropping the code.
Execution and sandbox-backed running arrive in later v2 milestones.

## Acceptance gates (v2.0)

- A Python 3.13 capsule declares an exact runtime, typed boundary, effects,
  pinned dependencies, and sandbox policy, and reviews as `pass`.
- Unpinned packages, floating GitHub branches, missing sandbox policy, and
  missing typed boundary are rejected at `fgx check`.
- Review reports classify high-risk languages (shell, native, GPU,
  smart-contract) and flag smart-contract secret-key access.
- `.fgx` and `.fgai` capsule views share one canonical AST, and both
  formatters are idempotent.

## Acceptance gates (v2.1)

- A Python 3.13 capsule declares deterministic `mock` fixtures that validate
  against the typed boundary; mismatched arity or type is rejected.
- `fgx capsule-run` runs all fixtures and reports `forgentex.capsule_run.v1`.
- The `--input` execution hook returns the matching fixture's outputs
  deterministically, or a `no_matching_fixture` trap.
- Fixtures survive `.fgx` <-> `.fgai` roundtrip and formatter idempotence.
