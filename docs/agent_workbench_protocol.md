# Forgentex Agent Workbench Protocol

Stage 19 adds compiler-native packets for agent tools. The goal is to let an
agent inspect and review a small, stable slice of the project without reading
every source file.

## Context Packets

```powershell
fgx context @function.mix_samples --deps types,effects,tests --json
```

The JSON schema is `forgentex.agent_context.v1`.

The packet includes:

- project summary: package name, version, source root, module count, and token estimate
- source summary: module, path, source hash, and module token estimate
- compact symbol map lines compatible with `.fgmap`
- `symbol_source`: the selected symbol rendered in human `.fgx`
- optional dependency slices from `--deps`
- diagnostics scoped to the selected symbol, including suggested patch kinds
- unsafe/assembly slices for nearby review awareness
- token estimates for the selected context

Supported dependency names:

- `types`: structs from the project type surface needed by the symbol; if the
  symbol only uses primitive/slice types, the packet includes the compact
  project struct surface as ambient type context.
- `effects`: declared, inferred, missing, invalid, and capability effects for
  the selected function.
- `tests`: matching JSON run fixtures from `examples/run`.

Example shape:

```json
{
  "schema": "forgentex.agent_context.v1",
  "symbol": "@function.mix_samples",
  "requested_deps": ["types", "effects", "tests"],
  "source": {
    "module": "media.audio",
    "path": "examples/project/media/audio.fgx",
    "hash": "71103d7398bc3cc9",
    "tokens": 51
  },
  "symbol_map": [
    "M media.audio examples/project/media/audio.fgx hash=71103d7398bc3cc9 tokens~51",
    "  F mix_samples @function.mix_samples params=4"
  ],
  "token_estimate": {
    "symbol": 34,
    "types": 22,
    "tests": 70,
    "total_context": 138
  }
}
```

Without `--json`, `fgx context` prints a short text wrapper followed by the same
packet.

## Review Packets

```powershell
fgx review patches/signed_add_crypto_effect.fgp --json
```

The JSON schema is `forgentex.agent_review.v1`.

The command reads a `.fgp` patch, locates its `TARGET` in the manifest project,
applies it to an in-memory preview, runs type checking, and embeds the existing
`forgentex.patch_review.v1` report. It never writes patched source.

Use `--source <file>` when reviewing a patch against a standalone file outside
the manifest project:

```powershell
fgx review patches/signed_add_crypto_effect.fgp --source examples/media/audio_mix.fgx --json
```

Review packets include:

- patch path and located source path
- semantic negotiation phases: `propose`, `check`, `review`, `apply`, `sign`
- machine-readable explanation strings for human review
- patch and source token estimates
- the full patch review JSON, including diagnostics and signature status

Signed Stage 18 envelopes are verified before patch operations run. Invalid
signed envelopes fail before context mutation.

## Agent Workflow

A minimal agent workflow is:

1. Read the `.fgmap` or run `fgx map --project --out examples/project.fgmap`.
2. Request the target slice:

   ```powershell
   fgx context @function.mix_samples --deps types,effects,tests --json
   ```

3. Produce a semantic `.fgp` patch against the stable node ID.
4. Preview and explain it:

   ```powershell
   fgx review patch.fgp --json
   ```

5. Let a human inspect the explanation packet and patch review.
6. Apply through the existing patch workflow when approved:

   ```powershell
   fgx patch examples/media/audio_mix.fgx patch.fgp --review
   ```

## Limits

- Context lookup currently uses manifest projects.
- Test slicing is filename-based and intentionally conservative.
- Review applies patches to an in-memory preview only.
- There is no long-lived workbench server, LSP, or editor UI yet.
- Patch signing is still the Stage 18 deterministic mock verifier, not real cryptography.
