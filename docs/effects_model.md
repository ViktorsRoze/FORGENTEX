# Forgentex Effects Model

Stage 6 adds explicit effects and capabilities. Effects make side behavior
visible to agents and human reviewers before Forgentex has a full module system,
package manifest, or runtime permission model.

## Syntax

Human `.fgx` functions can declare effects after the return type:

```forgentex
fn read_byte() -> u8 effects [io.read]:
    return io_read()
```

Agent `.fgai` uses the compact canonical form:

```text
F read_byte() -> u8 effects[io.read] {
  R io_read()
}
```

Unsafe functions use the same effect syntax:

```forgentex
unsafe fn popcount_x86(x: u64) -> u32 effects [cpu.avx2]:
    asm x86_64:
        in rax: x
        out rax: y
        op popcnt rax rax
    return u32(y)
```

## Effect Vocabulary

Stable v1 effect names:

```text
alloc
io.read
io.write
fs.read
fs.write
network.connect
network.read
network.write
network.http
network.udp
network.websocket
serialize
stream
crypto.sign
crypto.verify
crypto.hash
crypto.secret
crypto.derive
crypto.nonce
ledger.mock
agent.payment
agent.patch
gpu.render
media.encode
memory.raw
cpu.simd
cpu.asm
cpu.<feature>
target.<architecture>
```

`cpu.<feature>` covers target-specific features such as `cpu.avx2`,
`cpu.neon`, `cpu.sve2`, and future RISC-V vector or crypto features.
`target.<architecture>` is inferred from structured assembly blocks, for
example `target.x86_64`, `target.aarch64`, and `target.riscv64`.

## Inference

The checker infers effects from:

- calls to functions with declared effects.
- builtin effectful calls such as `io_read`, `io_write`, `network_read`,
  `network_write`, `crypto_sign`, `crypto_verify`, `crypto_hash`, and
  `alloc_buffer`.
- Stage 17 standard runtime calls such as `core_fs_read_text`,
  `core_fs_copy_text`, `core_net_http_get`, `core_net_tcp_send`,
  `core_serialize_text_len`, and `core_stream_count_bytes`.
- Stage 18 mock crypto and agent-economy calls such as
  `core_crypto_sign_ed25519`, `core_crypto_hash_blake3`,
  `core_agent_payment_receipt`, and `core_agent_signed_patch_hash`.
- structured assembly blocks, which infer `cpu.asm`, `memory.raw`, and a
  `target.<architecture>` effect.
- raw pointer types and raw pointer indexing, which infer `memory.raw`.
- portable SIMD intrinsics named `simd_*` or standard-library aliases named
  `core_simd_*`, which infer `cpu.simd`.

If a safe function calls an effectful function or builtin without declaring the
effect, `fgx check` reports `undeclared_effect`.

```forgentex
fn bad() -> u8:
    return io_read()     # undeclared_effect: io.read
```

Adding the declaration makes the capability boundary explicit:

```forgentex
fn ok() -> u8 effects [io.read]:
    return io_read()
```

First-class function values carry the same boundary in their type. A bare
`Fn[P…, R]` accepts only pure callables; an effectful value uses a final
`effects[...]` marker:

```forgentex
fn apply_read(f: Fn[i32, i32, effects[io.read]], value: i32) -> i32 effects [io.read]:
    return f(value)
```

Named functions contribute their declared effects and closures contribute the
effects inferred from their bodies. Assigning a callable is allowed when its
effect set is a subset of the annotated contract, so a wider contract remains
conservative. Calling through the value infers the entire annotated set at the
call site; indirect calls therefore cannot hide capabilities. Unknown,
duplicate, and misplaced `effects[...]` markers are type errors.

Generic callable contracts place `forall[...]` first and keep `effects[...]`
last, for example `Fn[forall[T], T, T, effects[io.read]]`. Every indirect call
gets a fresh type substitution while still contributing the complete annotated
effect set.

## Unsafe Effects

`memory.raw` and `cpu.asm` require an `unsafe fn` boundary. Declaring those
effects in a safe function reports `unsafe_effect_requires_unsafe`.

Unsafe functions may contain structured assembly without explicitly declaring
`cpu.asm`, `memory.raw`, or `target.<architecture>`; the unsafe boundary itself
is the review boundary for those structural effects. They still appear in
`fgx effects` output.

## Effects Command

Effect analysis is available through:

```powershell
fgx effects examples/media/popcount_x86.fgx --json
```

The output shape is:

```json
{
  "schema": "forgentex.effects.v1",
  "functions": [
    {
      "id": "@function.read_byte",
      "name": "read_byte",
      "unsafe": false,
      "declared": ["io.read"],
      "inferred": ["io.read"],
      "missing": [],
      "invalid": [],
      "capabilities": ["io.read"]
    }
  ]
}
```

`declared` is the source-level capability list. `inferred` is what the checker
found in the body/signature. `missing` lists inferred effects that should be
declared. `capabilities` is the function boundary summary for agents and review
tools.

## I/O and Network Effects

Stage 17 adds capability-scoped filesystem and networking effects. Functions
that call `core.fs` APIs must declare `fs.read` or `fs.write`. Functions that
call `core.net` APIs must declare the relevant `network.*` effects, including
`network.connect` for connection-oriented APIs and protocol markers such as
`network.http`, `network.udp`, or `network.websocket`.

Patch review reports now include separate filesystem and network access flags:

```text
  - Filesystem access changed: yes
  - Network access changed: yes
```

The interpreter uses deterministic mocks for these APIs; real permission
manifests and OS/network enforcement are deferred.

## Crypto and Agent-Economy Effects

Stage 18 adds typed mock crypto and payment effects for agent workflows.
`crypto.secret` is inferred from APIs and function signatures that touch
`SecretKey[...]` values. Safe functions may sign or derive public keys through
approved APIs, but `fgx check` rejects accidental logging, serialization,
exporting, or copying of secret values with the `secret_leak` diagnostic.
Explicit unsafe wrappers remain available for review-only escape hatches.

`ledger.mock`, `agent.payment`, and `agent.patch` describe deterministic mock
ledger receipts and signed semantic patch helpers. These effects are rejected by
the C backend along with other runtime-only effects.

## Semantic Patches

The v0 patch protocol can add an effect directly:

```text
PATCH @function.read_byte
ADD_EFFECT io.read
```

Fast-path patches also add CPU feature effects to the modified function when
they insert target-specific calls. Review reports include whether an effect or
capability changed. Stage 11 target checks validate concrete `cpu.<feature>` and
`target.<family>` names when `fgx check --target <triple>` is used.

## Deferred Work

Stage 6 does not yet include:

- project manifests or module-level capability declarations.
- effect polymorphism.
- effect subtraction or sandbox policies.
- real runtime enforcement.
