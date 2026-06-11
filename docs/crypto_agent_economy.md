# Forgentex Crypto and Agent Economy Model

Stage 18 adds typed, deterministic mock crypto and agent-economy APIs. The goal
is not real cryptography yet; the goal is to make signatures, secrets, patch
trust, and payment receipts visible to the checker, interpreter, and review
reports.

## Typed Values

The v0 type surface includes:

```text
Hash[Blake3]
Signature[Ed25519]
SecretKey[Ed25519]
PublicKey[Ed25519]
Nonce
TokenAmount[MockLedger]
Receipt
```

These are named or generic AST types. They can be parsed, formatted, checked,
returned, and passed through functions like other v0 value types. Runtime values
are opaque deterministic strings in the interpreter.

## Standard APIs

`core.crypto` exposes deterministic mock crypto helpers:

- `core_crypto_mock_secret_key(label) -> SecretKey[Ed25519]`
- `core_crypto_public_key_ed25519(secret) -> PublicKey[Ed25519]`
- `core_crypto_hash_blake3(message) -> Hash[Blake3]`
- `core_crypto_sign_ed25519(secret, message) -> Signature[Ed25519]`
- `core_crypto_verify_ed25519(public_key, message, signature) -> bool`
- `core_crypto_nonce(label) -> Nonce`
- `unsafe core_crypto_export_secret_key(secret) -> str`
- `unsafe core_crypto_clone_secret_key(secret) -> SecretKey[Ed25519]`

`core.agent` exposes mock agent-economy helpers:

- `core_agent_token_amount(symbol, amount) -> TokenAmount[MockLedger]`
- `core_agent_payment_receipt(payer, payee, amount, memo_hash) -> Receipt`
- `core_agent_receipt_ok(receipt) -> bool`
- `core_agent_signed_patch_hash(patch_text) -> Hash[Blake3]`
- `unsafe core_agent_log_secret(secret) -> void`

Single-file demos can call these flat `core_*` names directly. Manifest projects
can import `core.crypto` and `core.agent` through the standard library package.

## Secret Handling

`SecretKey[...]` values are non-copy owned values. Safe code may pass a secret
to approved crypto APIs for signing or public-key derivation:

```forgentex
fn sign(secret: SecretKey[Ed25519], message: str) -> Signature[Ed25519] effects [crypto.sign, crypto.secret]:
    return core_crypto_sign_ed25519(secret, message)
```

Safe code cannot accidentally pass secrets to logging, printing, serialization,
export, clone, or copy-style sinks. `fgx check` reports `secret_leak` with repair
intents:

```text
use_approved_crypto_api
mark_function_unsafe_for_secret_export
```

Unsafe escape hatches exist for review-only demos, but the unsafe marker is the
human audit boundary.

Functions that accept or return `SecretKey[...]` also infer `crypto.secret`, so
wrappers must declare the capability even when their body only returns or
forwards secret material.

## Effects

Stage 18 APIs infer and require explicit effects:

- `crypto.secret` for APIs touching secret material
- `crypto.derive` for public-key derivation
- `crypto.hash` for hashing
- `crypto.sign` and `crypto.verify` for signatures
- `crypto.nonce` for nonce creation
- `ledger.mock` for mock ledger values
- `agent.payment` for receipt creation
- `agent.patch` for signed patch workflows

The C backend rejects these runtime-only effects. The interpreter implements
deterministic mocks so tests and demos remain reproducible.

## Signed Patch Envelopes

`.fgp` v1 patches can now include signed mock envelope fields:

```text
VERSION 1
AUTHOR agent.demo
SIGNED_BY agent-public-demo
SIGNATURE_ALGORITHM Ed25519
SIGNATURE mock-sig-stage18-demo
TARGET @function.mix_samples
PRECONDITION NODE_EXISTS @function.mix_samples
OP ADD_EFFECT effect=crypto.verify
```

Patch application verifies the signed envelope before preconditions and
operations run. Accepted mock signatures report:

```text
signed-mock-accepted
```

Invalid signed envelopes fail before the AST preview is mutated.

## Example Flow

```powershell
fgx check examples/agent/signed_patch.fgx --json
fgx effects examples/agent/signed_patch.fgx --json
fgx run examples/agent/signed_patch.fgx --entry sign_patch --json-input examples/run/signed_patch.json
fgx run examples/agent/payment_receipt.fgx --entry issue_receipt --json-input examples/run/payment_receipt.json
fgx patch examples/media/audio_mix.fgx patches/signed_add_crypto_effect.fgp --emit-review-json
```

## Deferred Work

- real Blake3 and Ed25519 implementations
- secure key generation and storage
- constant-time guarantees
- signature verification against real public keys
- nonce uniqueness across real processes
- ledger consensus, wallet integration, and chain-specific token APIs
- permission manifests for crypto and payment capabilities
