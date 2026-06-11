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

`core.net`:

- `core_net_http_get(scope, url) -> Result[str,str]`
- `core_net_http_post(scope, url, body) -> Result[str,str]`
- `core_net_tcp_send(scope, endpoint, payload) -> Result[usize,str]`
- `core_net_udp_send(scope, endpoint, payload) -> Result[usize,str]`
- `core_net_websocket_send(scope, endpoint, payload) -> Result[usize,str]`

`core.serialize`:

- `core_serialize_text_len(value) -> usize`

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
| `core.time` | Mock clock, monotonic time, durations, and timers. | Real time is platform/runtime dependent; deterministic mock time comes first. |
| `core.log` | Structured logging and trace events. | Needs stable effect and output policy for production use. |
| `core.concurrent` | Atomics, tasks, channels, or async primitives. | Deferred until the memory model and runtime story are mature. |

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
