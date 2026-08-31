# Forgentex Standard Library v0

Stage 16 adds the first standard library surface. It is deliberately small,
allocation-free, and built out of source modules that the project loader can
resolve when a package imports `core.*`.

## Source Package

The standard library lives in:

```text
stdlib/Forgentex.toml
stdlib/src/core/*.fgx
```

Projects import modules with normal module imports:

```text
import core.math
import core.slice
```

`fgx check --project` automatically loads imported `core.*` modules from the
standard library package. Single-file examples can also call the `core_*` API
names directly; the checker and interpreter treat them as standard builtins for
the v0 demo.

## CLI

List the library catalog:

```powershell
fgx stdlib
fgx stdlib --json
```

`--json` emits the stable `forgentex.stdlib.v1` schema.

## Modules

`core.mem`:

- `core_mem_is_aligned_i16(values, alignment) -> bool`
- allocation-free alignment predicate
- currently deterministic and conservative in the interpreter and C backend

`core.slice`:

- `core_slice_len_i16(values) -> usize`
- `core_slice_len_u8(values) -> usize`
- `core_slice_len_mut_i16(values) -> usize`
- `core_slice_require_same_len_i16(left, right) -> void`
- `core_slice_require_mut_same_len_i16(dst, src) -> void`
- `core_slice_copy_i16(dst, src) -> void`

`core.math`:

- `core_math_clamp_i16(x) -> i16`
- `core_math_checked_add_i32(left, right) -> i32`
- `core_math_checked_sub_i32(left, right) -> i32`
- `core_math_checked_mul_i32(left, right) -> i32`
- `core_math_saturating_add_i16(left, right) -> i16`

`core.music`:

- `MusicalTime { bar, beat, tick }` with one-based bars/beats
- `TempoMap { change_tick, initial_bpm, next_bpm }` for one bounded tempo change
- `MusicCue { version, cue_id, action_id, track, start_tick, duration_ticks, priority }`
- `SceneAction { version, action_id, kind, target_id, parameter_q16, seed, max_instances }`
- `core_music_position_valid(position, beats_per_bar, ticks_per_beat) -> bool`
- `core_music_to_tick(position, beats_per_bar, ticks_per_beat) -> u32`
- `core_music_bar_start_tick(bar, beats_per_bar, ticks_per_beat) -> u32`
- `core_music_tick_to_millis(tick, bpm, ticks_per_beat) -> u32`
- `core_music_quantize_tick(tick, grid_ticks) -> u32`
- `core_music_make_cue(...) -> MusicCue`
- `core_music_make_action(...) -> SceneAction`
- `core_music_action_valid(action, instance_limit) -> bool`
- `core_music_cue_valid(cue, track_count, timeline_ticks) -> bool`
- `core_music_cue_action_valid(cue, action, track_count, timeline_ticks, instance_limit) -> bool`
- `core_music_cues_overlap(left, right) -> bool`
- `core_music_cues_conflict(left, right) -> bool`
- `core_music_cue_wins(left, right) -> bool`
- `core_music_conflict_winner_id(left, right) -> u32`
- `core_music_tempo_map_valid(tempo_map, ticks_per_beat) -> bool`
- `core_music_tempo_map_millis(tempo_map, tick, ticks_per_beat) -> u32`

All time conversion uses integer milliseconds rounded down. Cue quantization
uses nearest-grid rounding with exact half-grid ties moving forward. The
validator rejects a zero change tick, zero BPM, and zero tick rates; callers
validate before conversion, while zero divisors still produce deterministic
interpreter traps if they bypass validation. Millisecond intermediates widen
to `u64` so useful timelines do not overflow the public `u32` tick inputs.
`MusicCue` version 1 uses half-open intervals, same-track conflicts, higher-
priority wins, and lower cue ID as the stable tie-break. Its numeric action ID
must refer to an approved scene-action table; it is not executable code. See
[`music_timeline.md`](music_timeline.md).
`SceneAction` version 1 defines one bounded table entry: reveal, emit, camera
key, or material key. Emitters require an explicit seed; all actions obey the
caller's instance budget, and the cue/action validator requires matching IDs.

`core.option`:

- `core_option_some_u8(value) -> Option[u8]`
- `core_option_none_u8() -> Option[u8]`
- `core_option_is_some_u8(value) -> bool`
- `core_option_is_none_u8(value) -> bool`
- `core_option_unwrap_or_u8(value, fallback) -> u8`

`core.result`:

- `core_result_ok_u8(value) -> Result[u8,u8]`
- `core_result_err_u8(value) -> Result[u8,u8]`
- `core_result_is_ok_u8(value) -> bool`
- `core_result_is_err_u8(value) -> bool`
- `core_result_unwrap_or_u8(value, fallback) -> u8`

`core.bits`:

- `core_bits_extract_u8(value, shift, mask) -> u8`
- `core_bits_popcount_u32(value) -> u32`
- `core_bits_clz_u32(value) -> u32`
- `core_bits_ctz_u32(value) -> u32`
- `core_bits_rotl_u32(value, amount) -> u32`

`core.simd`:

- `core_simd_load_i16x8(src, index) -> vec[i16,8]`
- `core_simd_store_i16x8(dst, index, value) -> void`
- `core_simd_vec_i16x8(a,b,c,d,e,f,g,h) -> vec[i16,8]`
- `core_simd_splat_i16x8(value) -> vec[i16,8]`
- `core_simd_saturating_add(left, right) -> vec[i16,8]`
- `core_simd_eq(left, right) -> mask[8]`
- `core_simd_select(mask, left, right) -> vec[i16,8]`
- `core_simd_reduce_add(value) -> i16`

`core.target`:

- `core_target_has_feature_neon() -> bool`
- `core_target_has_feature_avx2() -> bool`
- `core_target_has_feature_riscv_v() -> bool`
- `core_target_feature_enabled_neon() -> bool`
- `core_target_feature_enabled_avx2() -> bool`
- `core_target_feature_enabled_riscv_v() -> bool`
- `core_target_is_x86_64() -> bool`
- `core_target_is_aarch64() -> bool`
- `core_target_is_riscv64() -> bool`

`core.fs`:

- `core_fs_scope_read(scope) -> bool`
- `core_fs_scope_write(scope) -> bool`
- `core_fs_read_text(scope, path) -> Result[str,str]`
- `core_fs_write_text(scope, path, contents) -> Result[usize,str]`
- `core_fs_copy_text(scope, source, dest) -> Result[usize,str]`

`core.path`:

- `core_path_parse(value) -> Path`
- `core_path_join(base, child) -> Path`
- `core_path_parent(path) -> Option[Path]`
- `core_path_file_name(path) -> str`
- `core_path_extension(path) -> str`
- `core_path_to_str(path) -> str`
- `core_path_starts_with(path, base) -> bool`

`Path` is a statically distinct immutable portable path. `DirEntry` contains a
typed path, kind, and regular-file size for collected directory listings.

`core.io` (hosted by `io-run` or the manifest-backed application runtime):

- `core_io_read_file(path) -> str`
- `core_io_read_bytes(path) -> Vec[u8]`
- `core_io_write_file(path, contents) -> usize`
- `core_io_write_file_atomic(path, contents) -> usize`
- `core_io_write_bytes_atomic(path, bytes) -> usize`
- `core_io_publish_text_artifact(name, path: Path, contents) -> usize`
- `core_io_publish_bytes_artifact(name, path: Path, bytes) -> usize`
- `core_io_copy_file(source, destination) -> usize`
- `core_io_copy_file_atomic(source, destination) -> usize`
- `core_io_list_dir(path: Path) -> Vec[DirEntry]`

The application host constrains these calls with canonical permission roots
and cumulative byte/entry budgets. Directory listings are sorted and
all-or-error. Atomic variants publish only new destinations and refuse to
replace an existing path.
Artifact variants additionally require a matching manifest declaration,
enforce its narrower root and byte ceiling, and return versioned
size/hash/media evidence through `fgx app-run`; ordinary writes never become
artifact receipts.

`core.net`:

- `core_net_http_get(scope, url) -> Result[str,str]`
- `core_net_http_post(scope, url, body) -> Result[str,str]`
- `core_net_tcp_send(scope, endpoint, payload) -> Result[usize,str]`
- `core_net_udp_send(scope, endpoint, payload) -> Result[usize,str]`
- `core_net_websocket_send(scope, endpoint, payload) -> Result[usize,str]`

`core.serialize`:

- `core_serialize_text_len(value) -> usize`
- `core_serialize_json_canonical(value) -> str`
- `core_serialize_json_is_canonical(value) -> bool`
- `core_serialize_utf8_encode(value) -> Vec[u8]`
- `core_serialize_utf8_decode(bytes) -> str`
- `core_serialize_u16_le(value) -> Vec[u8]`
- `core_serialize_u32_le(value) -> Vec[u8]`
- `core_serialize_i32_le(value) -> Vec[u8]`
- `core_serialize_read_u16_le(bytes, offset) -> u16`
- `core_serialize_read_u32_le(bytes, offset) -> u32`
- `core_serialize_read_i32_le(bytes, offset) -> i32`
- `core_serialize_concat(left, right) -> Vec[u8]`

The JSON codec emits the Forgentex canonical JSON profile: compact output,
sorted object keys, decoded/re-encoded Unicode, strict duplicate/malformed
input rejection, and deterministic normalization through the current
`i64`/`f64` value model. JSON and variable-size byte transforms have a 16 MiB
per-value ceiling; JSON nesting is capped at 128 containers. The new codec
operations are deterministic memory transforms and declare no effect.

`core.stream`:

- `core_stream_count_bytes(chunk) -> usize`

`core.crypto`:

- `core_crypto_mock_secret_key(label) -> SecretKey[Ed25519]`
- `core_crypto_public_key_ed25519(secret) -> PublicKey[Ed25519]`
- `core_crypto_hash_blake3(message) -> Hash[Blake3]`
- `core_crypto_sign_ed25519(secret, message) -> Signature[Ed25519]`
- `core_crypto_verify_ed25519(public_key, message, signature) -> bool`
- `core_crypto_nonce(label) -> Nonce`
- `unsafe core_crypto_export_secret_key(secret) -> str`
- `unsafe core_crypto_clone_secret_key(secret) -> SecretKey[Ed25519]`

`core.agent`:

- `core_agent_token_amount(symbol, amount) -> TokenAmount[MockLedger]`
- `core_agent_payment_receipt(payer, payee, amount, memo_hash) -> Receipt`
- `core_agent_receipt_ok(receipt) -> bool`
- `core_agent_signed_patch_hash(patch_text) -> Hash[Blake3]`
- `unsafe core_agent_log_secret(secret) -> void`

## Compatibility Aliases

Earlier demo stages exposed some compiler-known intrinsic names directly:

- `Some`, `None`, `Ok`, `Err`, `is_some`, `is_none`, `is_ok`, `is_err`, `unwrap_or`
- `simd_*`
- `cpu_has_*`
- `aligned`

Those names still work in v0. Stage 16 adds standard-library names on top:

- `core_option_*`
- `core_result_*`
- `core_simd_*`
- `core_target_*`
- `core_mem_*`
- `core_fs_*`, `core_net_*`, `core_serialize_*`, and `core_stream_*`
- `core_crypto_*` and `core_agent_*`
- `core_music_*`

New examples should prefer the `core_*` names.

## Media/DSP Migration

The media examples now use standard library APIs for reusable behavior:

- audio mixing uses `core.math` and `core.slice`
- H.264 NAL parsing uses `core.bits`
- IDCT copying uses `core.slice`
- SIMD review uses `core.simd` and `core.target`
- Stage 17 I/O examples use `core.fs`, `core.net`, `core.serialize`, and
  `core.stream`
- Stage 18 agent examples use `core.crypto` and `core.agent` for signed patch
  and mock payment receipt flows
- the music visualizer uses `core.music` for authored marker positions,
  deterministic cue grids, and tempo-map conversion

This keeps demo code from depending on ad hoc compiler magic while preserving a
small implementation surface.

## Small Library Roadmap

Forgentex should keep the standard library small, explicit, and reviewable, but
the demo needs a few tiny libraries that make serious examples concise. These
libraries are planned additions; the stable v0 catalog above remains the current
implemented surface.

Near-term libraries for v0.20-v0.26:

| Planned module | Purpose | Important demo use |
| --- | --- | --- |
| `core.bytes` | Byte-slice helpers, endian reads/writes, varint-style helpers, and small checksums. | Bitstream parsers, packet headers, file/network demos. |
| `core.fixed` | Deterministic fixed-point arithmetic, scaling, rounding, min/max, and safe multiply/divide helpers. | Butterfly ray-grid simulation, geometry demos, reproducible benchmarks. |
| `core.stats` | Precise allocation-free statistics over integer and fixed-point values: counts, checked sums, min/max, mean with explicit rounding, variance, histograms, and stable summary records. | Benchmark summaries, signal analysis, simulation sweeps, token-efficiency reports. |
| `core.geom` | Tiny 2D vector, ray, disc, AABB, reflection, and intersection helpers built on fixed-point values. | Ray/ball butterfly benchmark, collision demos, path tracing micro-tests. |
| `core.grid` | 2D grid indexing, bounds checks, row/column helpers, and deterministic cell stepping. | Butterfly ray-grid simulation, cellular automata, image/filter demos. |
| `core.hash` | Non-cryptographic stable hashes and hash-combine helpers for deterministic path fingerprints. | Simulation path hashes, parser golden hashes, benchmark outputs. |
| `core.rng` | Seeded deterministic PRNG for simulations and fuzz-like demos, explicitly not cryptographic. | Butterfly perturbations, randomized-but-reproducible tests. |
| `core.text` | Tiny ASCII/UTF-8 length, prefix/suffix, split, and parse helpers without heap allocation. | Diagnostics, config-like inputs, protocol examples. |
| `core.json` | Minimal deterministic JSON field extraction and scalar encoding for demo inputs/outputs. | Benchmark inputs, agent packets, golden run fixtures. |
| `core.test` | Small assertion, equality, range, and golden-hash helpers for inline specs. | Inline tests, conformance examples, token-efficiency demos. |
| `core.bench` | Deterministic counters, run summaries, and benchmark-output helpers. | `fgx bench`, token-efficiency reports, simulation demos. |

Precision rule: `core.math` remains the small stable checked-arithmetic surface,
`core.fixed` should provide deterministic scaled numeric values, and
`core.stats` should provide statistical summaries that make rounding,
overflow, sample size, and accumulator width explicit. Statistics APIs should
prefer exact integer counts, checked or widened accumulators, and deterministic
fixed-point rounding over platform-dependent floating behavior.

Later libraries, after the ownership and allocation story is stronger:

| Planned module | Purpose | Deferred reason |
| --- | --- | --- |
| `core.alloc` | Explicit allocator and fixed-buffer allocation policies. | Needs mature ownership, lifetimes, and allocator effects. |
| `core.collections` | Small vectors, maps, ring buffers, and sets. | Needs allocation/fixed-buffer policy and generic APIs. |
| `core.time` | Copyable shared deterministic `AsyncClock`, bounded one-shot logical timers, stable deadline ordering, wake-token integration, waiter-safe cancellation/rescheduling, fixed-rate `IntervalTimer` cadence, and immediate losing-timer reclamation through deterministic token selection in the interpreter. | Real monotonic/wall time and host reactor integration remain platform/runtime dependent. |
| `core.log` | Structured logging and trace events. | Needs stable effect and output policy for production use. |
| `core.concurrent` | Deterministic cooperative `Task[T]` with explicit parked/woken state, retry-safe nonblocking `task_poll`, logical-timer-aware blocking `task_wait`, bounded consuming `task_race`, consuming logical `task_timeout`, atomic wake-source rebinding through `task_wait_on`, and bounded typed `task_then` continuation chains; bounded shared one-shot `WakeToken`, bounded nonblocking token selection with stable winner/loser cancellation, heterogeneous source-preserving timeout waits, borrowed channel read/send readiness, hosted capability-confined regular-file read and no-clobber create availability, logical timer wake integration, bounded affine `TaskGroup[T]`, and bounded `Channel[T]` interpreter primitives; full or incremental ordered joins, capacity release, cooperative FIFO backpressure, cancellation propagation, and explicit concurrency effects. | Partial: real parallel scheduling, nested scopes, statement-frame suspension, open-stream/socket readiness, OS/reactor wake sources, atomics, wall-clock timers, and async I/O remain deferred. |

The butterfly ray-grid benchmark described in
[`docs/token_efficiency_benchmarks.md`](token_efficiency_benchmarks.md) should be
the first consumer for `core.fixed`, `core.geom`, `core.grid`, `core.hash`,
`core.rng`, and summary uses from `core.stats`. That benchmark should prove
these libraries reduce source and agent-context tokens without hiding safety
checks, precise numeric rules, or deterministic behavior.

Each planned library should ship with:

- a tiny source module under `stdlib/src/core/`
- examples that use the public `core_*` API names
- `fgx stdlib --json` catalog entries
- type-checker and interpreter coverage
- at least one token-efficiency or demo-scope use case
- compatibility notes when an API becomes part of the stable v0 surface

## Current Limits

- only a small typed subset is available, mostly `u8`, `i16`, `i32`, and fixed SIMD shapes
- no allocation APIs yet
- `str` is available for deterministic I/O and networking demos, but string
  operations are intentionally tiny
- no generic standard-library functions yet
- no module-qualified call syntax yet; v0 uses flat `core_*` function names
- target feature queries are deterministic in the interpreter and conservative in generated C
- filesystem and network APIs are mocked in the interpreter and rejected by the
  C backend
- crypto and agent-economy APIs are deterministic mocks; real cryptography,
  secure key storage, and real ledgers are deferred
