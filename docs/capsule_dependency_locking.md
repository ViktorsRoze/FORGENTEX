# Capsule Dependency Locking and Supply-Chain Audit (v2.3)

Capsule dependencies reference external package managers and GitHub
starting projects. v2.3 parses each reference into structured fields,
produces a deterministic lockfile, and grades supply-chain risk so capsules
only depend on pinned — ideally checksummed — sources. It builds on the
pinning rule from v2.0 (`capsule_unpinned_dependency`).

## Dependency reference grammar

```text
<ecosystem>:<name>[==<version>|@<revision>][#<algo>:<digest>]
```

Examples:

| Reference | Ecosystem | Name | Version / revision | Checksum |
| --- | --- | --- | --- | --- |
| `pypi:numpy==2.2.0#sha256:...` | pypi | numpy | 2.2.0 | sha256 |
| `github:org/repo@<40-hex>` | github | org/repo | commit | none |
| `npm:preact@10.24.0#sha512:...` | npm | preact | 10.24.0 | sha512 |
| `pypi:requests` | pypi | requests | none (floating) | none |

`@<value>` is a revision when the ecosystem is `github` or the value is a
commit-like hex string (>= 7 hex chars); otherwise it is a version. The raw
reference is retained losslessly. Recognized checksum algorithms: `sha256`,
`sha512`, `blake3`.

## Risk grading

| Risk | Condition |
| --- | --- |
| `low` | pinned and carries a checksum |
| `medium` | pinned (exact version or commit) but no checksum |
| `high` | unpinned or floating (bare name, `@main`, `@master`, `latest`, `*`) |

## Lockfile

```text
fgx capsule-lock <file> [--json]
```

`forgentex.capsule_lock.v1` is a deterministic, sorted record of every
capsule dependency (deduplicated by raw reference) with its structured
fields and the set of capsules that require it. Determinism makes the
lockfile reviewable in diffs and reproducible across runs.

## Supply-chain audit

```text
fgx capsule-audit <file> [--json] [--strict]
```

`forgentex.capsule_audit.v1` grades each dependency and reports an overall
status equal to the worst dependency risk (`high` > `medium` > `low`).
`--strict` exits non-zero when any dependency is high risk, so CI can block
floating or unpinned capsule dependencies.

## Relationship to `fgx check`

`fgx check` already rejects unpinned dependencies at the contract level
(`capsule_unpinned_dependency`, v2.0). v2.3 adds the lock and audit tooling
on top: structured parsing, the deterministic lockfile, and risk grading
that distinguishes checksummed (low) from version-only (medium) pins.

## Acceptance gates (v2.3)

- `pypi:numpy==2.2.0#sha256:...` parses to ecosystem/name/version/checksum
  and grades `low`.
- `github:org/repo@<commit>` parses as a revision and grades `medium`.
- A version-only dependency grades `medium`; a floating one grades `high`.
- `fgx capsule-lock --json` emits a sorted `forgentex.capsule_lock.v1` with
  `required_by` for each dependency.
- `fgx capsule-audit --strict` exits non-zero on any high-risk dependency.
