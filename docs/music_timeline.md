# Deterministic music timeline contract

`core.music` is the source-backed authored-time boundary for the visualizer.
It has no clock, decoder, beat detector, filesystem access, or hidden runtime
effect. The same module type-checks and lowers through the interpreter, C, and
Zig project paths.

## Time model

- `MusicalTime` uses one-based `bar` and `beat` plus a zero-based tick within
  the beat.
- Conversion receives explicit beats-per-bar and ticks-per-beat values.
- Integer milliseconds round down and use widened `u64` intermediates.
- Cue quantization chooses the nearest grid line; exact half-grid ties move
  forward.
- The bounded `TempoMap` has one absolute change tick and two constant BPM
  segments. General ordered tempo maps remain deferred until native collection
  lowering can support them without breaking C/Zig consumers.

## `MusicCue` version 1

`MusicCue` is the first marker/cue schema:

| Field | Rule |
| --- | --- |
| `version` | Must equal `1`. |
| `cue_id` | Non-zero stable identity. |
| `action_id` | Non-zero reference into an approved scene-action table; never source code or a renderer command. |
| `track` | Zero-based and less than the declared track count. |
| `start_tick` | Absolute, quantized timeline tick. |
| `duration_ticks` | Non-zero; the widened end tick must fit within the declared timeline. |
| `priority` | Higher value wins a conflict. |

Cue intervals are half-open: `[start_tick, start_tick + duration_ticks)`. Two
cues conflict only when those intervals overlap on the same track. Touching at
an end boundary is not a conflict. Higher priority wins; equal priority uses
the lower `cue_id` as the deterministic tie-break. The combined
`core_music_conflict_winner_id` operation returns zero for non-conflicting
cues and consumes each owned cue once, matching the affine ownership model.

## `SceneAction` version 1

`SceneAction` is one approved action-table entry referenced by
`MusicCue.action_id`:

| Field | Rule |
| --- | --- |
| `version` | Must equal `1`. |
| `action_id` | Non-zero and equal to the referencing cue's action ID. |
| `kind` | `1` reveal, `2` emit, `3` camera key, or `4` material key. |
| `target_id` | Non-zero reference to an approved scene target. |
| `parameter_q16` | Signed deterministic fixed-point parameter whose meaning is defined by the action kind. |
| `seed` | Emit actions require a non-zero deterministic seed. |
| `max_instances` | Non-zero and no larger than the caller's instance budget; non-emitter actions must equal one. |

`core_music_cue_action_valid` first copies the scalar link identity, then
consumes and validates each owned value exactly once. This proves the cue
references the supplied approved entry and that both fit the caller's track,
timeline, and instance budgets. It does not look up a global registry or grant
the action any runtime capability.

## Deliberate limits

The schema schedules and validates individual approved action entries; it does
not yet define a collection/table manifest, cue collections, section nesting,
cross-track constraints, audio-derived markers, or a pipeline receipt.
Validation helpers remain scalar/struct-only so the current native fast path
stays available.
