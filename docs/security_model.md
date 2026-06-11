# Forgentex Security, Sandboxing, and Supply Chain

Stage 24 adds a conservative security layer around package projects. It is still
mocked and local-only, but it gives agents and humans a stable way to review
trust, declared permissions, sandbox policy, package signatures, and
reproducible build metadata before package actions run.

## Goals

Stage 24 guarantees:

- untrusted packages declare permission manifests before using build scripts,
  codegen, test runners, or agent tools
- untrusted packages declare permissions for `fs.*`, `network.*`, `crypto.*`,
  `io.*`, `agent.*`, and `ledger.*` effects
- package signatures are represented and mock-verified before publish/verify
  workflows report success
- reproducible metadata is summarized as a deterministic source hash
- `fgx test` and matrix-aware `fgx build` fail before package action if a
  supply-chain audit has high-severity issues

Stage 24 does not execute package build scripts. It only models and gates them.
Real cryptographic package verification, remote registries, artifact signing,
and OS-level sandbox execution remain future work.

## Manifest Fields

Security metadata lives in `Forgentex.toml`.

```toml
[security]
trust = "untrusted"
signed_by = "agent-public-demo"
signature_algorithm = "Ed25519"
signature = "mock-sig-codec-app"
reproducible = true
```

Accepted trust levels:

- `trusted`: local first-party package; permissions are still reported
- `untrusted`: package actions and effect capabilities must be declared

Mock signature verification succeeds when:

- `signed_by` is present
- `signature_algorithm = "Ed25519"`
- `signature` starts with `mock-sig-`

This is intentionally not real cryptography. It proves the workflow boundary.

## Permissions

Permission manifests use the `[permissions]` section:

```toml
[permissions]
fs.read = ["src"]
fs.write = ["build"]
network = ["https://agent.local"]
crypto = ["verify"]
io = ["stdout"]
build.scripts = true
codegen = true
tests = true
agent.tools = true
```

Effect permissions are category-based:

- `fs.*` requires `fs` or an `fs.*` permission
- `network.*` requires `network`
- `crypto.*` requires `crypto`
- `io.*` requires `io`
- `agent.*` and `ledger.*` require `agent`

Action permissions are exact:

- `build_script` requires `build.scripts`
- `codegen` requires `codegen`
- `test_runner` requires `tests`
- `agent_tool` requires `agent.tools`

## Sandbox Policy

Sandbox policy uses the `[sandbox]` section:

```toml
[sandbox]
build_scripts = "deny"
codegen = "deny"
tests = "sandboxed"
agent_tools = "deny"
```

Accepted policy strings are currently review labels. Stage 24 treats `deny` as
a hard failure when an untrusted package also declares that action. `sandboxed`
and `allow` are reported and accepted when the matching permission is declared.

## Package Actions

Untrusted packages may declare action entrypoints in `[security]` or
`[actions]`:

```toml
[security]
trust = "untrusted"
build_script = "scripts/build.ps1"
codegen = "tools/generate.ps1"
test_runner = "scripts/test.ps1"
agent_tool = "tools/agent.ps1"
```

If any of those are present without the matching permission, `fgx audit`,
`fgx test`, and matrix-aware `fgx build` fail before doing package work.

## Audit

Run:

```powershell
fgx audit examples/packages/codec_app --json
```

The command emits `forgentex.audit.v1` with:

- package name and version
- trust level
- permission manifest entries
- sandbox labels
- declared package actions
- mock signature status
- reproducible source hash
- supply-chain issues with severity, code, message, and required permission

High-severity issues make the audit fail. Medium-severity issues produce a
warning status.

## Verify

Run:

```powershell
fgx verify examples/packages/codec_app --json
```

The command emits `forgentex.verify.v1` with signature and reproducibility
checks for every package in the local dependency graph. Verification passes only
when all packages have accepted mock signatures and `reproducible = true`.

## Build/Test Gate

`fgx test` and matrix-aware `fgx build` call the supply-chain audit first:

```powershell
fgx test --project examples/packages/codec_app --all-targets --json
fgx build examples/packages/codec_app --backend c --all-targets --json
```

This is the Stage 24 acceptance gate: untrusted packages cannot reach package
test/build workflows with undeclared arbitrary-code actions or undeclared
effect capabilities.
