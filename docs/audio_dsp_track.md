# Audio DSP Track: Native DSP Core + miniaudio Capsule

This document is the standing plan for making Forgentex capable of real
audio work — the long-term goal being complex audio products (a Sound
Forge–class editor). It records the architecture, the honest split between
what is buildable now and what is blocked on foundational work, and a
phased milestone plan. Keep it in mind across all future development:
every native-backend, FFI, and capsule-execution decision should move this
track forward.

## Strategy: polyglot, not from-scratch

Forgentex will **not** reimplement OS audio backends, GUI stacks, or
codecs. Those are decades of cross-platform work. Instead:

- **Reuse existing native libraries through capsules** (C/C++/Rust/Zig).
  [miniaudio](https://miniaud.io) (`miniaudio.h`) is the first target: a
  single-header, zero-dependency, public-domain / MIT-0 library that
  already abstracts WASAPI, DirectSound, and WinMM on Windows, plus Core
  Audio, ALSA, PulseAudio, and JACK elsewhere, behind one callback API.
- **Build the pure signal-processing core natively in Forgentex.** Format
  conversion, gain, mixing, resampling, filters, metering, and ring
  buffers are just arithmetic over sample buffers — exactly what the
  language and interpreter already express, and what the effect/borrow
  systems can make safe.

## The seam

miniaudio's low-level API hands the program exactly one realtime-thread
function:

```c
void data_callback(ma_device* dev, void* out, const void* in, ma_uint32 frames);
```

The architecture splits precisely at that callback:

```text
+-------------------------- miniaudio capsule (C) --------------------------+
|  device enumeration, open/close, backend (WASAPI/CoreAudio/ALSA/...),     |
|  the realtime audio thread, and *calling* the callback                    |
|                                                                           |
|        inside the callback, it invokes -->                                |
|  +------------------- Forgentex-native DSP core --------------------+     |
|  |  format conversion, gain, mix, resample, biquad/EQ, metering,    |     |
|  |  ring buffer  --  pure, realtime-safe, effects: none             |     |
|  +------------------------------------------------------------------+     |
+---------------------------------------------------------------------------+
```

miniaudio owns the OS and the realtime thread; Forgentex owns *what runs
inside the callback*. miniaudio's own `ma_*` helpers (conversion,
`ma_resampler`, `ma_pcm_rb`) are the reference spec for the native core's
conventions: normalized `f32` samples, interleaved frames, the same
resampler math.

## Honest status: runnable now vs deferred

- **Native DSP core — buildable and runnable today.** It is plain `.fgx`
  over slices and arithmetic. It runs through the interpreter on real
  sample buffers (JSON input), is type/borrow/effect-checked, and emits C
  source through the existing C backend.
- **miniaudio capsule — a reviewable contract now, executable later.** The
  v2.0–v2.5 capsule system parses, type-checks the boundary, pins the
  dependency, reconciles effects against the sandbox, and reviews it, but
  it does **not** yet compile, link, or call C. Live playback is blocked
  on the foundational work below.
- **Live audio is blocked on three foundations** (tracked outside this
  document): a real native backend that lowers and runs code (Cranelift or
  LLVM), a working C FFI, and real capsule execution. Until those land,
  the DSP core is validated offline and the capsule is review-only.

The realtime path will never be the AST interpreter (too slow, allocates).
The interpreter is for *testing* the DSP core; the realtime callback runs
the **C emitted from the same `.fgx`**, called by miniaudio.

## Realtime-safety as a checked property

The audio callback's most common bug is doing something non-realtime-safe
(allocating, locking, syscall) on the audio thread, causing dropouts.
Forgentex's effect system can *enforce* the rule that most frameworks can
only document:

- Add an `audio.*` effect category and a `cpu.realtime` capability.
- DSP functions intended for the callback are declared `effects: none`
  (or only `cpu.realtime`), and the checker forbids `alloc`, `io.*`, and
  `network.*` on that call path.
- A function reachable from the realtime callback that allocates or does
  I/O is a checker error, not a runtime glitch.

This is a genuine, audio-specific differentiator and should be built as
part of the track.

## Native DSP core contents (the `core.audio` module)

Phased, but the intended surface:

- **Format conversion**: `i16 <-> f32`, `i24 <-> f32`, `u8 <-> f32`, with
  clamping and optional dither; `f64` interop.
- **Frame layout**: interleave / deinterleave, channel count changes
  (mono<->stereo), simple channel mapping.
- **Level**: `apply_gain`, `mix_buffers` (generalizing the existing
  `mix_samples` example), peak and RMS metering.
- **Filters**: a stateful `biquad` (lowpass / highpass / peaking EQ) — the
  EQ primitive — and a simple one-pole smoother.
- **Resampling**: linear first, then a windowed-sinc resampler matching
  miniaudio's conventions.
- **Buffering**: a fixed-capacity ring buffer suitable for the
  producer/consumer split between app and callback.

All declared `effects: none`, borrow-checked, interpreter-tested against
golden buffers, and C-emittable.

## Precision: exact fixed-point, deterministic float

Audio must be numerically correct, not subject to silent rounding
surprises. The DSP core therefore provides an **exact Q15 fixed-point
path** (`apply_gain_q15`, `mix_q15`): pure integer multiply and arithmetic
shift, bit-exact and identical on every backend and platform, with no
IEEE-754 rounding. Float kernels remain available but the precise path is
fixed-point. The full position and plan — exact integer/fixed-point math,
float determinism across backends, explicit casts and rounding, and
checked arithmetic — is in [`docs/numeric_determinism.md`](numeric_determinism.md).

## miniaudio capsule contract shape

A review-only artifact today (advances v2.6 native capsules):

```fgx
struct AudioConfig:
    sample_rate: u32
    channels: u8
    frames_per_buffer: u32

foreign miniaudio_device c version "c11" env "windows-x86_64" effects [audio.output, alloc]:
    input config: AudioConfig
    output started: bool
    dep "github:mackron/miniaudio@<pinned-commit>#sha256:<digest>"
    sandbox "audio.output default-device"
    sandbox "limit.wall_ms 0"
    code:
        // wraps ma_device_init + ma_device_start; the data_callback
        // invokes the C emitted from core.audio
```

It must declare: language/version (`c` / `c11`), target env, the typed
device config (a shared schema), `audio.*` effects, a sandbox grant for
device access, and miniaudio pinned by commit + checksum. As a `native`
risk-class capsule it requires ABI and memory-boundary review and is
reported `not_lowerable` until execution lands.

## Phased milestones

| Phase | Status | Scope | Runnable now? |
| --- | --- | --- | --- |
| `audio.1` | Done | Native DSP core v0 (`examples/audio`, module `audio.dsp`): i16<->f32 conversion, interleave/deinterleave, `apply_gain`, `mix_buffers`, peak/energy, **plus exact Q15 fixed-point `apply_gain_q15`/`mix_q15`**. Checks, borrow-checks, runs through the interpreter on real buffers, passes inline tests, and emits C (`fgx build --backend c`). | Yes |
| `audio.2` | Planned | `audio.*` effect category + `cpu.realtime` capability + realtime-safety checking (no alloc/io on the callback path). | Yes |
| `audio.3` | Planned | Filters and buffering: `biquad` EQ, one-pole smoother, fixed-capacity ring buffer; golden-buffer conformance tests. | Yes |
| `audio.4` | Planned | miniaudio C capsule contract: typed `AudioConfig` shared schema, `audio.output` effect, device sandbox grant, miniaudio pinned by commit+sha; reviewed by `fgx capsule`/`sandbox`/`capsule-audit`. Review-only. | Contract only |
| `audio.5` | Planned | Format and channel breadth: i24/u8/f64, channel mapping, linear then sinc resampler matching miniaudio conventions. | Yes |
| `audio.6` | Blocked | Live playback: miniaudio's callback invokes the C emitted from `core.audio`. Depends on native backend + C FFI + real capsule execution. | No (blocked) |

## Acceptance gates

- `audio.1`: a gain/mix pipeline runs through `fgx run` on a real i16
  buffer and produces the expected samples; `fgx build --backend c` emits
  compilable C for the same kernels.
- `audio.2`: a DSP function that calls an allocating helper fails
  `fgx check` with an `audio`/`cpu.realtime` safety diagnostic; a pure one
  passes.
- `audio.3`: a biquad lowpass on a golden input matches a golden output
  buffer within tolerance through the interpreter.
- `audio.4`: the miniaudio capsule reviews as a `native`, high-risk
  capsule with a pinned, checksummed dependency, declared `audio.output`
  effect covered by an `audio.*` sandbox grant, and `not_lowerable`
  backend status.
- `audio.6`: deferred until the native backend, FFI, and capsule execution
  foundations exist; explicitly tracked as blocked.

## Dependencies and ordering

- `audio.1`–`audio.3` and `audio.5` need only the current language,
  interpreter, and C source backend — buildable in the normal milestone
  cadence.
- `audio.4` builds on the v2.6 native-capsule review path.
- `audio.6` is the integration milestone and is gated on the three
  foundations (native backend, C FFI, capsule execution). It must not be
  marked startable until those exist.

Building the native DSP core first is deliberate: it is the part that is
truthfully in scope today, it is what a future miniaudio callback will
actually call, and it lets the effect system prove realtime-safety before
any OS code is involved.
