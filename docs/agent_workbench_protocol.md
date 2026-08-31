# Forgentex Agent Workbench Protocol

Stage 19 adds compiler-native packets for agent tools. The goal is to let an
agent inspect and review a small, stable slice of the project without reading
every source file.

## Context Packets

```powershell
fgx context @function.mix_samples --deps types,effects,tests --max-tokens 2048 --json
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
- requested and admitted dependency categories
- an optional `heuristic-v1` content-token budget and exact admission decision
- explicit exclusions with category, reason, item count, and estimated tokens

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

### Budget admission

`--max-tokens <n>` is a positive budget over the packet's selected content:
the symbol source, compact symbol-map lines, and admitted dependency slices.
The fixed JSON envelope, project/source metadata, diagnostics, and unsafe-review
awareness remain outside this content estimate and are never silently removed.

The selected symbol and its compact map are mandatory. Requested effect
evidence is also mandatory because effects are review-critical; a budget below
that floor fails with `context_token_budget_too_small`. Requested `types` and
`tests` are considered in request order and included only when the complete
category fits. The packet's additive `included_deps`, `excluded`, and `budget`
fields make every decision explicit. Unrequested categories use
`reason=not_requested`, budget refusals use `reason=token_budget`, and unrelated
project symbols use `reason=focused_symbol_only`.

All workbench and project-map token annotations now use the same
`heuristic-v1` estimator as `fgx token-report`. Standalone maps report each
symbol's formatted semantic cost rather than repeating the containing file's
whitespace count.

### Stateful hash elision

Persist a full packet, then use it as the last-seen state:

```powershell
fgx context '@function.render_waveform' `
  --project examples/apps/wav_waveform `
  --deps types,effects,tests `
  --out target/waveform-context.json

fgx context '@function.render_waveform' `
  --project examples/apps/wav_waveform `
  --deps types,effects,tests `
  --since target/waveform-context.json `
  --max-tokens 54 `
  --json
```

Full packets carry a SHA-256 `packet_hash`, hashes for the symbol map/source
and optional type/effect/test sections, plus `state_sections` describing which
content the caller received. `--since` accepts only the v1 schema with valid
hash shapes and the same symbol. A section is elided only when the baseline
both records that content in its state and carries the exact current hash.
Missing, changed, or previously unrequested sections are emitted normally.

Delta packets set `mode=delta`, bind `baseline_packet_hash`, and list
`elided_sections`. Requested effects remain explicit even when unchanged.
`logical_context`, `total_context`, and `saved_by_hash_elision` distinguish the
complete selected state from transmitted selected content. On the waveform
fixture, logical selected content falls from 883 transmitted tokens to 54; the
complete serialized JSON packet falls from 1,896 heuristic tokens to 1,026
because hashes, framing, policy, effects, and abstention evidence remain.
The former stateless packet measured 1,492 tokens, so the state-capable full
packet pays 404 tokens up front while each unchanged delta saves 466 versus a
stateless resend. The waveform case therefore breaks even on the second packet.

The baseline is caller-owned context state, not an authenticity proof. Forging
a baseline can cause that caller to omit information it falsely claims to
possess, but cannot alter source, checker results, effects, or runtime policy.
Packet/section hashes are content identities, not signatures.

### Optional project codebook

`--codebook <forgentex.codebook.json>` applies a current project semantic tier
to symbol source, compact map lines, and type-source strings. The packet
binds the exact artifact SHA-256 and reports unencoded/encoded logical costs.
Effects, diagnostics, unsafe slices, policy, and exclusion evidence remain
readable. Section hashes are computed over canonical unencoded content, and a
baseline created under another codebook cannot elide current sections.

Codebook use is opt-in because dictionary references have transport overhead.
On the small waveform packet, exact and bounded parameterized patterns lift
selected saving to 115 tokens and complete JSON saves 69 tokens, so automatic
admission applies. The
project map remains negative and stays plain. Integrations compare complete
payloads independently rather than inferring one surface from another.

The CLI performs that comparison automatically. `--codebook` returns the
strictly smaller of the complete coded and ordinary readable packets. Presence
of `codebook_ref` identifies applied encoding; a rejected candidate returns the
exact no-codebook JSON without admission overhead. `--force-codebook` bypasses
admission only for diagnostics and conformance tests.

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
- optional codebook artifact identity plus transport and decoded patch costs
- the full patch review JSON, including diagnostics and signature status

`review --codebook <file>` accepts an artifact-bound `#FGCB` patch only after
the codebook passes the current-project check. Decoding happens before target
location, precondition validation, signature validation, in-memory application,
and type checking; all later phases therefore operate on the canonical `.fgp`
text. A wrong artifact hash fails rather than attempting another dictionary.

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
- Context budgets apply to selected content rather than serialized JSON framing;
  integrations must reserve a small envelope allowance in the provider budget.
- Patch signing is still the Stage 18 deterministic mock verifier, not real cryptography.
