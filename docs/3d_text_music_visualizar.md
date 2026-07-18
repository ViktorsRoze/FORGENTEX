# 3d-text-music-visualizar

**Status:** governed pixel-preview, encoder-handoff, and deterministic music-
timeline foundations shipped. Forgentex can model, check, review, and execute
a bounded scene-to-raw-frame-to-image pipeline with linked render/encode
receipts, and `core.music` now defines bar/beat/tick conversion, cue
quantization, a bounded two-segment tempo map, and a versioned `MusicCue` with
deterministic conflict resolution plus bounded `SceneAction` entries. Real
text/3D graphics, physics, production codecs, GPU rendering, and 4K output
remain planned governed capsules and must not be claimed as shipped.

## Project description

`3d-text-music-visualizar` is an agent-assisted music-visual production tool.
It turns an audio piece, a musical arrangement, timed markers, a creative brief,
and supplied or generated text and symbol elements into a reproducible 3D scene
and a rendered music-video asset, up to 4K output.

The artist describes intent in musical terms rather than keyframing every
object: "at bar 5, letters begin to fall like metal rain; at bar 9, the camera
flies through the word; at the final hit, time slows and the symbols become
transparent." The system translates that intent into a typed, bar-accurate
scene plan. A renderer then executes the plan using deterministic seeds and
explicit quality settings.

The LLM is a constrained creative planner, not an unrestricted renderer or code
author. It may propose scene beats, text transformations, camera routes, and
effect parameters, but its result must conform to the typed scene schema,
declared asset policy, timing grid, performance budget, and capability grants
before rendering begins.

## Inputs

- An audio file or audio reference, with BPM, time signature, duration, and
  optional stems or onset/energy analysis.
- A global narrative brief describing the song's mood, visual world, palette,
  symbols, and progression.
- A bar/beat marker sheet. Each marker identifies a musical location and what
  must happen there; markers may cover a full bar, a beat, or a precise time.
- Text, glyphs, logos, symbols, and optional generated assets with ownership
  and use-policy metadata.
- Render constraints: aspect ratio, frame rate, 4K target, codec, seed, GPU
  budget, allowed effects, and preview/final quality mode.

## Example: 16-bar visual brief

At 120 BPM in 4/4, 16 bars last 32 seconds. The global brief is: *a cold
industrial city of floating typography warms into a luminous release.*

| Bars | Musical marker | Visual instruction |
| --- | --- | --- |
| 1-4 | Sparse intro | A title made of translucent chrome glyphs slowly rotates in darkness. The camera makes a quiet approach. |
| 5-8 | Percussion enters | Symbols cascade from above, collide with the title, and accumulate as a controlled physics field. Each snare sends a ripple through the letters. |
| 9-12 | Main hook | The camera follows a 3D flight path through the opening of a large word. Motion blur, volumetric beams, and brighter reflections rise with the arrangement. |
| 13-15 | Breakdown | Time slows. Falling symbols float in suspension, with increased translucency and refractive highlights. |
| 16 | Final hit | The camera exits into a wide shot as the assembled symbols burst into the final wordmark; lighting resolves to white and the last frame holds cleanly. |

The tool derives a timeline of beats, cues, camera keys, object emitters,
physics events, lighting states, and render passes. The artist can revise the
brief, a marker, or one scene node without regenerating unrelated work.

## What the demo produces

1. A validated `MusicBrief` and a time-normalized marker map.
2. A declarative `ScenePlan`: text/symbol instances, materials, camera curves,
   emitters, physics constraints, lighting, and per-bar transitions.
3. A reviewable render manifest containing asset hashes, model/version details,
   seed, frame range, GPU target, renderer version, and output settings.
4. Low-resolution previews for direction approval, followed by a frame sequence
   and an encoded 4K video when the final render is authorized.
5. An audit trail showing which LLM plan, semantic patch, asset, capability,
   and render configuration produced the result.

## Why this is a strong Forgentex demonstration

This project uses Forgentex where it is distinctive instead of attempting to
reimplement a graphics ecosystem in the language core.

- **Human direction and agent efficiency:** the creative scene remains readable
  in `.fgx`, while the same canonical scene/orchestration program has a compact
  `.fgai` representation for an LLM or agent. Both map to one AST, so a human
  review and an agent edit cannot silently diverge.
- **Semantic, reviewable iteration:** "change the hook camera from a dolly to a
  corkscrew flight path" becomes a patch against a stable scene node, not an
  opaque rewrite of a large file. Forgentex can type-check the preview and show
  its changed effects, targets, allocations, and capability implications before
  it is applied.
- **Typed, governed integrations:** audio analysis, an LLM provider, a font
  shaper, a physics engine, a GPU renderer, and FFmpeg-style encoding belong
  behind typed foreign capsules. Each declares inputs/outputs, pinned
  dependencies, effects, sandbox policy, and deterministic fixtures. The core
  stays small and auditable while using mature media software.
- **Safety and cost visibility:** effect declarations make file access, network
  calls, LLM use, GPU compute, allocation, and encoding explicit. A render
  request can be denied if it exceeds the project's allowed capabilities or
  memory/device budget.
- **Reproducible creative output:** the brief, scene plan, model revision,
  asset hashes, seed, target GPU architecture, and renderer settings are part
  of the render manifest. The same approved plan should be reproducible instead
  of being a one-off, untraceable generation.
- **Performance-aware systems work:** Forgentex already has target awareness,
  SIMD review, FIR/C/Zig backend work, benchmarks, and GPU-capsule review. They
  give the future renderer an auditable path from high-level scene intent to
  hardware-specific execution.

## Proposed architecture

```text
music + marker sheet + global brief + approved text/symbol assets
                         |
                         v
              typed analysis and planning layer
       audio features + constrained LLM -> ScenePlan
                         |
                         v
    Forgentex validation, patch review, capability and budget gates
                         |
                         v
  renderer capsules: text geometry -> physics -> camera -> lighting -> frames
                         |
                         v
             compositor / video encoder -> preview or 4K master
```

The stable boundary is the `ScenePlan`, not the prompt or a renderer-specific
project file. That keeps an LLM replaceable, allows both a real-time preview
renderer and an offline-quality renderer, and makes the creative result
testable.

### Core scene concepts to add

- `MusicalTime` (bar, beat, tick, seconds) and tempo maps.
- `Cue`, `SceneSection`, and priority/conflict rules for marker-driven events.
- `TextElement` and `SymbolElement` with font, geometry, material, ownership,
  and transform data.
- `CameraPath` using splines, look-at targets, shake, focus, shutter, and
  speed ramps.
- `Emitter` and `PhysicsBody` for cascades, collisions, attraction fields,
  slow motion, and deterministic random seeds.
- `Material` and `LightingRig` for PBR materials, transparency/translucency,
  refraction, volumetrics, shadows, exposure, and color grading.
- `RenderProfile` for preview/final settings, 3840x2160 output, frame rate,
  samples, motion-blur shutter, codec, and resource limits.

## Capability model

The following effects should be explicit and reviewable. Some are current and
some remain proposed extensions to the existing effect vocabulary.

| Capability | Example effect | Rule |
| --- | --- | --- |
| Read approved media, fonts, and symbols | `fs.read`, `media.decode` | Paths and asset hashes must be in the project manifest. |
| Ask a planner model | `network.http`, `llm.infer` | Model/provider/version, prompt policy, and output schema are recorded. No arbitrary source code is accepted. |
| Run simulation and GPU rendering | `physics.simulate`, `gpu.compute`, `gpu.render` | Device architecture, memory cap, frame/sample budget, and deterministic seed are required. |
| Encode output | `fs.write`, `media.encode` | Only an approved output directory and codec/profile may be used. |

`gpu.compute` already has a review model for CUDA capsules. `gpu.render` is a
first-class effect for governed render contracts, and `media.encode` now gates
the bounded PPM encoder proof. `media.decode`, `llm.infer`, and
`physics.simulate` remain to be added; production codec profiles and manifests
also remain future work.

## Honest implementation plan

### visual.0 - Planning and verification demo

Partially shipped: the restricted `ScenePlan`, deterministic seed policy,
bounded preview profile, first pixel artifact/receipt, and deterministic music
timing library are live. `core.music` supplies typed `MusicalTime` and
`TempoMap` records, one-based bar/beat conversion, nearest-grid cue
quantization, bounded tempo-change validation, and integer-millisecond
conversion with conformance tests. The typed creative brief, multi-change
tempo maps, and `MusicCue` version 1 are source-backed and C/Zig-lowerable.
Cues carry approved numeric action references, half-open intervals, tracks,
priorities, and a stable ID tie-break. `SceneAction` version 1 now validates an
individual referenced entry across kind, target, fixed-point parameter, seed,
and instance budget. The typed creative brief, action-table manifest, cue
collections/sections, richer scene validation, dual-view examples, and
semantic patch examples remain. This milestone is not a 4K video.

The composition dependency is now available as a deliberately narrow proof.
`music_preview.fgx` hands compact native `SceneMetrics` between stages, while
`render_preview.fgx` turns a fixed seed into a restricted `ScenePlan` and hands
that plan to a real contained C software renderer. The renderer produces a
deterministic persisted 16x16 PPM plus `forgentex.render_receipt.v1`, recording
scene/output hashes, seed, dimensions, profile, renderer/toolchain identity,
effects, and resource budgets.

`render_encode_preview.fgx` advances the same proof by making the trust
boundary explicit: the renderer emits a 776-byte raw-RGB artifact in memory,
then a separately gated `media.encode` capsule persists the deterministic PPM.
`forgentex.encode_receipt.v1` binds its exact input hash to the raw render hash,
plus the final output hash, encoder/toolchain identity, effects, and budgets.

This closes visual.0's pixel-proof edge and its first deterministic authored-
time edge. The `ScenePlan` is four scalar fields, the output is one PPM image,
and the timing module does not decode or analyze audio. There is no text
geometry, animated scene/cue schema, GPU dispatch, production codec, or richer
nested scene boundary yet.

### visual.1 - Governed domain contracts

Define and review capsules for audio feature extraction, LLM planning, font
shaping/text mesh generation, a deterministic physics engine, a wgpu/Vulkan or
Blender-compatible renderer, and an FFmpeg-style encoder. Pin every dependency
and expose their effects, buffers, GPU targets, and sandbox/resource limits.

### visual.2 - Preview renderer

Implement live capsule invocation and a graphics runtime capable of loading the
validated `ScenePlan`, generating text/symbol geometry, simulating the planned
events, and producing an approval preview. Keep preview quality and final
quality as distinct `RenderProfile`s.

### visual.3 - Offline 4K master render

Add robust audio decoding/beat analysis, shader/material support, cascades and
deterministic physics, depth-of-field and motion blur, translucent materials,
advanced lighting, frame-sequence caching, color-managed compositing, and a
bounded encoder pipeline. A final render is accepted only when its asset hashes,
seed, renderer version, GPU target, capability grants, and output profile are
captured in the manifest.

## Missing Forgentex features and how to create them

| Needed feature | Current position | Forgentex plan |
| --- | --- | --- |
| 3D graphics/GPU runtime | CUDA/GPU capsules are reviewable, but a live graphics runtime is not yet available. | Deliver `dom.4`: a pinned Rust/C capsule around wgpu/Vulkan, with typed scene buffers, GPU architecture/resource review, and live native dispatch. |
| Audio/video codecs | A separately governed PPM encoder proves raw-frame handoff, `media.encode` gating, persistence, and linked receipts; full media decode and production image/video codecs are not available. | Add governed codec capsules (for example an FFmpeg-compatible boundary), bounded codec/profile policies, and `media.decode`. Keep codec implementation outside the trusted core. |
| Physics | No scene-physics API exists. | Bind a deterministic physics engine through a capsule, define stable body/constraint/event types, seed all stochastic behavior, and add golden-state tests. |
| LLM orchestration | Managed capsules can provide a typed boundary, but no music-visual planner schema or provider policy exists. | Add `llm.infer`, a versioned `ScenePlan` JSON schema, strict output validation, model/version audit data, and a no-arbitrary-code policy. |
| Real 4K render jobs | A deterministic software pixel preview, raw-frame handoff, PPM encoder, and governed receipts are live, but there is no 3D/GPU renderer, async I/O, caching, production codec, or job runtime. | Add richer scene/frame boundaries, then queueing, cancellation, budgets, production encoding, and cache manifests as governed services. |
| Music-aware timing | `core.music` now provides typed musical positions, one bounded two-segment tempo map, `MusicCue` version 1, and bounded `SceneAction` entries. Cue/action IDs must match; emitters require deterministic seeds and all actions obey caller instance limits. It does not decode audio or schedule collections. | Define the action-table manifest and cue collection/section schema, generalize ordered tempo maps after native collection lowering, add the audio-feature boundary, then bind validated cues/actions into render receipts. |

## Demo success criteria

The first credible demo should accept the 16-bar brief above and produce a
validated, inspectable scene plan in both Forgentex views. It should reject
misaligned cues, unsafe asset paths, unapproved LLM/model changes, missing GPU
limits, non-deterministic physics settings, and output settings outside the
project policy. Once the graphics and codec runtime milestones are complete,
the same plan becomes the input for a repeatable 4K music-video render.

The project demonstrates Forgentex's intended role clearly: it is the
agent-native, typed, capability-gated, reviewable control layer that makes
powerful existing media and AI runtimes safe to compose into a serious creative
tool.
