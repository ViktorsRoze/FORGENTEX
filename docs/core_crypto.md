# core.hash — Real, Verified Cryptographic Hashing (v2.16)

The symmetric trust backbone: real, standard, **test-vector-verified**
hashing and message authentication, with zero dependencies.

## The principle: borrow the dangerous primitives, implement the safe ones

Forgentex's crypto rule is **borrow, never roll your own** — for the
*dangerous* primitives. Asymmetric signatures (Ed25519), AEAD, and key
generation are subtle (constant-time field arithmetic, nonce handling) and
must come from an **audited host crypto library over the FFI bridge** (v2.13);
they are deliberately **not** hand-rolled and are out of this slice.

But the *standard, fully specified, side-channel-safe* primitives are safe to
implement to spec and **verify against official known-answer vectors**, which
is exactly what `core.hash` does:

- **SHA-256** (FIPS 180-4)
- **HMAC-SHA256** (RFC 2104), verified against RFC 4231 vectors
- **constant-time comparison** for safe tag/MAC verification

These give content-addressing, integrity, shared-key authentication, and
safe verification — enough for capability tokens, signed-by-shared-key
messages, and content-addressed patches.

## Builtins

| Builtin | Effect | Returns |
| --- | --- | --- |
| `core_hash_sha256(message)` | `crypto.hash` | hex digest (`str`) |
| `core_hash_hmac_sha256(key, message)` | `crypto.hash` | hex MAC (`str`) |
| `core_hash_ct_eq(a, b)` | none (pure) | `bool` |

They are deterministic pure computation, so they work in ordinary `fgx run`
and need no runtime host — unlike `core.io`/`core.socket`.

## Example

```text
import core.hash

fn verify_tag(key: str, message: str, tag: str) -> bool effects [crypto.hash]:
    return core_hash_ct_eq(core_hash_hmac_sha256(key, message), tag)
```

Verifying a MAC with `core_hash_ct_eq` rather than `==` means the comparison
takes the same time regardless of where the bytes differ, so it does not leak
the expected tag via timing.

## Verification

`core.hash` is correct by **known-answer test**, not by assertion:

```text
$ fgx crypto-selftest
Forgentex crypto self-test (known-answer vectors)
  pass sha256_empty        # e3b0c442...855
  pass sha256_abc          # ba7816bf...ad
  pass hmac_rfc4231_tc2    # 5bdcc146...3843
  pass ct_eq_true
  pass ct_eq_false
  pass ct_eq_len
  6/6 passed
```

`SHA-256("abc")` matches the NIST vector and `python -c hashlib.sha256`
byte-for-byte. Report schema: `forgentex.crypto_selftest.v1`.

## Scope and honesty

- **In:** real SHA-256, HMAC-SHA256, constant-time compare — implemented to
  spec, zero-dependency, test-vector-verified, deterministic.
- **Deferred (on purpose):** Ed25519 sign/verify, AEAD, KDF, secure random
  key generation — these come from an audited host crypto library over FFI,
  never hand-rolled. The mock `core.crypto` family remains the typed
  review/demo placeholder for those until the FFI path lands.
- The v2.19 agent trust fabric will combine this (integrity + shared-key
  auth) with the audited asymmetric primitives for full agent-to-agent trust.
