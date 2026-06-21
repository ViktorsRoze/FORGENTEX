# v3.8 — Latent Agent-Message Protocol

**Inspiration:** LatentMAS (Gen-Verse, ICML 2026) — multi-agent systems where
agents collaborate by passing **hidden states / KV-cache** ("latent thoughts")
instead of text, reporting 50–80% fewer tokens and 3–7× speedup, training-free
over HuggingFace/vLLM models.

## The fit, stated honestly

Forgentex does **not** compute latents and must not try to: that needs a model
runtime (tensors, KV-cache, GPU, vLLM) and dependencies the zero-dependency
core forbids, and it is the ML runtime's job. Forgentex's contribution is a
*different layer* — the one it is uniquely good at and just built across v2:

> Forgentex is the wrong layer to **produce** latents, but the right layer to
> be the **typed, signed, model-versioned envelope, transport contract, and
> audit** for them — and to run LatentMAS itself as a gated capsule.

LatentMAS passes hidden states informally. In a multi-agent economy those
payloads need governance, which is exactly the v2 machinery:

1. **Model-fingerprint typing (the key correctness contribution).** A latent is
   meaningless outside the exact model that produced it. The typed boundary
   carries a mandatory fingerprint `{model_id, revision, layer, dtype, shape}`,
   and consuming a latent whose fingerprint does not match the consumer's model
   is a **gate error** (`latent_model_mismatch`) — never a silent corruption.
   Nothing in LatentMAS enforces this; the type system does.
2. **Integrity, access control, audit** (reuses v2.16 hashing + v2.19 trust):
   the latent is content-addressed (SHA-256), signed/verified, capability-gated
   (who may consume this thought), replay-protected, and receipted. Forgentex
   secures the exchange; it never opens the tensor.
3. **Data plane** (v2.2 / v2.18): a latent flows between agent capsules **by
   name** through the shared namespace, its fingerprint as the type.
4. **LatentMAS as a capsule.** The actual hidden-state/KV-cache work runs in a
   `python` capsule (HuggingFace/vLLM) with `gpu`/`alloc` effects and a
   sandbox, executed and gated via the real Python bridge (v2.17). Forgentex
   orchestrates and secures; LatentMAS does the ML.

## Non-goals (kept honest)

- **No latent computation, no tensor library, no GPU in the core.** The
  zero-dependency core is preserved; the model runtime moves the bytes (shared
  memory / vLLM), Forgentex defines and validates the protocol.
- **No portability claim.** Latents are model+revision locked; the protocol's
  value is *preventing* incompatible/unsafe exchange and governing it, not
  making latents portable.
- **No metric conflation.** The 50–80% token / 3–7× speed wins are LatentMAS's
  *inference* metrics. Forgentex's own token metric only covers the *envelope*
  (a content-addressed latent reference + fingerprint vs an inlined text
  reasoning trace). The two are reported separately and never merged.

## Milestone shape (v3.8)

A self-contained, **zero-dependency** slice that stands on the existing trust
and Python machinery:

- A `latent` typed boundary value and a `LatentRef` fingerprint type
  `{model_id, revision, layer, dtype, shape, hash}`; a `latent_model_mismatch`
  check at `fgx check`.
- A signed, content-addressed **latent envelope** extending the trust session
  (v2.19): issue → sign → transmit (ref, not tensor) → verify → capability
  check → receipt, with `forgentex.latent_session.v1`.
- A **LatentMAS capsule example**: a `python` capsule declaring the typed
  latent boundary, `gpu`/`alloc` effects, and a pinned HuggingFace/vLLM
  dependency, executed/gated via the Python bridge (graceful
  `runtime_unavailable` when the model/GPU is absent, like every other bridge).
- A **transport comparison**: envelope tokens (latent ref + fingerprint) vs an
  inlined text reasoning trace, measured with the existing token tooling and
  clearly labeled as the envelope metric, not the inference metric.

## Why it belongs in v3

It is the natural convergence of three things v2 already delivered — the typed
boundary, the agent trust fabric, and the real Python bridge — pointed at a
live research direction (latent multi-agent collaboration). Forgentex becomes
the **governance and typing layer for latent agent communication**: it makes an
otherwise informal, model-fragile, unauthenticated exchange typed, versioned,
signed, and auditable, without ever leaving its dependency-free lane.
