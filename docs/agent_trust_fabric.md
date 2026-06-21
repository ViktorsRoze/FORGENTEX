# Agent Trust Fabric (v2.19)

The end-to-end agent-to-agent trust loop, built from the **real** primitives:
HMAC-SHA256 (v2.16) for capability tokens and message authentication,
constant-time verification, replay protection, content-addressed audit
receipts (SHA-256), and the real loopback TCP transport (v2.15).

## The loop

```text
$ fgx trust-session
Forgentex agent trust session (shared-key)
schema=forgentex.trust_session.v1 status=ok alice -> bob capability=optimize.media.h264
  key_id=1f180b059d5df312 transport=tcp (136 bytes)
  pass issue_capability_token   optimize.media.h264 expires=1700003600 tag=64652cf17a35…
  pass verify_capability_token  tag valid + not expired
  pass sign_message             nonce=n-7f3a ts=1700000000
  pass transmit                 tcp 136 bytes
  pass verify_signature         HMAC-SHA256 via constant-time compare
  pass check_freshness          timestamp in window, nonce first-seen
  pass issue_receipt            receipt=6309fdc95822…
  pass verify_receipt           receipt tag valid
  pass reject_replay            duplicate nonce rejected
  pass reject_tampered          modified message fails verification
  message_hash=103e2087…
  receipt_hash=6309fdc9…
```

Two agents (alice → bob) holding a pre-shared key:

1. **Capability token** — alice issues bob an HMAC-authenticated token for a
   capability, with an expiry.
2. **Signed message** — alice signs an envelope `{from, to, nonce, timestamp,
   capability, payload}` with HMAC-SHA256.
3. **Real transport** — the envelope is sent over a real loopback TCP socket
   (falling back to in-memory only if loopback is unavailable).
4. **Verification** — bob verifies the tag with **constant-time compare**, so
   a bad signature leaks nothing via timing.
5. **Freshness + replay** — the timestamp must be in window and the nonce
   first-seen; a resent nonce is **rejected**.
6. **Audit receipt** — bob returns an HMAC receipt content-addressed by the
   message hash; alice verifies it.
7. **Tamper resistance** — a modified message no longer matches its tag and is
   **rejected**.

## The report: `forgentex.trust_session.v1`

Every step with its `ok`/detail, the transport kind and bytes, the
`message_hash` and `receipt_hash` (full SHA-256 audit anchors), and the
`replay_rejected` / `tamper_rejected` outcomes. The session is deterministic
(fixed demo key seed, timestamp, nonce) so the audit hashes are reproducible.

## Secret hygiene

The shared secret is **never logged** — only its `key_id` (a hash prefix)
appears in the report. This is the runtime side of the secret-safety rule
from v2.16.

## Honest scope

- This is **shared-key (symmetric)** trust: issuer and verifier hold a
  pre-shared secret, authenticated with HMAC. It is real and sufficient for a
  pre-shared-key agent mesh (capability tokens, replay protection, receipts).
- **Public-key agent identity (Ed25519)** — so two agents who have *never*
  shared a secret can establish trust — is the upgrade that arrives with the
  audited host crypto library over the FFI bridge (v2.16's deferred half). It
  is deliberately not hand-rolled.
- Replaces the mock agent-economy demos (v2.8 / Phase 9.5) with a real loop.
