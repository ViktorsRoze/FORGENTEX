# Forgentex Graph Artifact

Stage 31 adds `.fggraph`, a deterministic graph artifact for agents, CI, and
human review tooling.

The graph is not a new source format. It is a compiler-owned snapshot that ties
the existing `.fgx` human view, `.fgai` agent view, canonical AST hash, symbols,
effects, diagnostics, target facts, and inline test references together.

## Commands

```powershell
fgx graph dump examples/smoke/inline_tests.fgx --out build/inline.fggraph
fgx graph validate build/inline.fggraph --json
fgx graph view build/inline.fggraph --to fgx
fgx graph view build/inline.fggraph --to fgai
fgx graph roundtrip examples/smoke/inline_tests.fgx --json
fgx graph context build/inline.fggraph '@function.clamp_byte' --deps effects,tests --json
fgx graph patch build/audio.fggraph patches/add_fast_path.fgp --out build/audio_patched.fggraph --json
fgx graph diff build/audio.fggraph build/audio_patched.fggraph --json
```

Quote node IDs such as `@function.clamp_byte` in PowerShell.

## Artifact Content

The artifact starts with:

```text
FGGRAPH schema="forgentex.graph.v1" compiler="forgentex-stage31" source="..."
PROJECT name="..." version="..." modules=N symbols=N
```

It then emits deterministic records:

- `MODULE`: source path, package, source kind, source hash, canonical AST hash,
  imports, exports, and embedded human/agent renderings.
- `SYMBOL`: structs, functions, and inline tests with stable node IDs.
- `EFFECT`: declared, inferred, missing, and capability facts per function.
- `DIAGNOSTIC` and `BORROW`: compiler diagnostics attached to node IDs.
- `PATCH_HISTORY`: reserved patch provenance slot.
- `GRAPH_HASH`: stable FNV-1a hash over the graph body.

## Schemas

Stage 31 exposes these JSON reports:

- `forgentex.graph_validate.v1`
- `forgentex.graph_roundtrip.v1`
- `forgentex.graph_context.v1`
- `forgentex.graph_patch.v1`
- `forgentex.graph_diff.v1`

## Review Consequences

`fgx graph diff` summarizes consequences agents should surface before a patch is
accepted:

- `public_api_changed`
- `unsafe_changed`
- `capability_changed`
- `target_support_changed`
- `tests_changed`

The graph patch command applies semantic `.fgp` patches through embedded module
source and reports whether the graph-patched source reparses to the same
canonical AST as the direct patch result.
