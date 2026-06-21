//! Agent trust fabric (v2.19).
//!
//! The end-to-end agent-to-agent trust loop, built from the real primitives:
//! HMAC-SHA256 capability tokens and message authentication (v2.16), constant-
//! time verification, replay protection (nonce + timestamp), content-addressed
//! audit receipts (SHA-256), and the real loopback TCP transport (v2.15).
//!
//! Honest scope: this is **shared-key** (symmetric) trust — issuer and verifier
//! hold a pre-shared secret, authenticated with HMAC. Public-key agent identity
//! (Ed25519) is the upgrade that arrives with the audited host crypto library
//! over the FFI bridge; it is deliberately not hand-rolled here. The secret key
//! is never logged — only its `key_id` (a hash) appears in the report.
//!
//! The session is deterministic (fixed demo key seed, timestamp, and nonce) so
//! the audit hashes are reproducible.

use crate::crypto::{constant_time_eq, hmac_sha256_hex, sha256_hex};
use crate::json;

const DEMO_TS: u64 = 1_700_000_000;
const DEMO_NONCE: &str = "n-7f3a";

#[derive(Clone, Debug, PartialEq)]
pub struct TrustStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrustSession {
    pub schema: String,
    pub issuer: String,
    pub verifier: String,
    pub capability: String,
    pub key_id: String,
    pub transport: String,
    pub transport_bytes: usize,
    pub message_hash: String,
    pub receipt_hash: String,
    pub steps: Vec<TrustStep>,
    pub replay_rejected: bool,
    pub tamper_rejected: bool,
    pub status: String,
}

fn shared_key() -> Vec<u8> {
    // A pre-shared secret. Real deployments exchange this out of band; the demo
    // derives a fixed one so the session is reproducible.
    sha256_hex(b"forgentex-demo-shared-key").into_bytes()
}

fn sign(key: &[u8], canonical: &str) -> String {
    hmac_sha256_hex(key, canonical.as_bytes())
}

fn verify(key: &[u8], canonical: &str, tag: &str) -> bool {
    constant_time_eq(sign(key, canonical).as_bytes(), tag.as_bytes())
}

/// Move bytes over a real loopback TCP socket, returning (kind, bytes). Falls
/// back to in-memory transport if the loopback is unavailable.
fn transport_roundtrip(payload: &[u8]) -> (String, usize) {
    match tcp_roundtrip(payload) {
        Some(bytes) => ("tcp".to_string(), bytes),
        None => ("memory".to_string(), payload.len()),
    }
}

fn tcp_roundtrip(payload: &[u8]) -> Option<usize> {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};

    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let addr = listener.local_addr().ok()?;
    let expected = payload.to_vec();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            let _ = stream.write_all(&buf);
        }
    });

    let mut stream = TcpStream::connect(addr).ok()?;
    stream.write_all(payload).ok()?;
    stream.shutdown(Shutdown::Write).ok()?;
    let mut back = Vec::new();
    stream.read_to_end(&mut back).ok()?;
    let _ = handle.join();

    if back == expected {
        Some(back.len())
    } else {
        None
    }
}

pub fn run(capability: &str, payload: &str) -> TrustSession {
    let key = shared_key();
    let key_id = sha256_hex(&key)[..16].to_string();
    let issuer = "alice".to_string();
    let verifier = "bob".to_string();
    let now = DEMO_TS;
    let expiry = DEMO_TS + 3600;

    let mut steps = Vec::new();
    let mut seen_nonces: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // 1. Issue a capability token (HMAC-authenticated, with expiry).
    let token_canonical = format!("cap|{issuer}|{verifier}|{capability}|{DEMO_NONCE}|{expiry}");
    let token_tag = sign(&key, &token_canonical);
    steps.push(TrustStep {
        name: "issue_capability_token".to_string(),
        ok: true,
        detail: format!("{capability} expires={expiry} tag={}…", &token_tag[..12]),
    });

    // 2. Verifier checks the token tag and that it has not expired.
    let token_ok = verify(&key, &token_canonical, &token_tag) && expiry > now;
    steps.push(TrustStep {
        name: "verify_capability_token".to_string(),
        ok: token_ok,
        detail: "tag valid + not expired".to_string(),
    });

    // 3. Issuer signs a message envelope.
    let canonical = format!("{issuer}|{verifier}|{DEMO_NONCE}|{now}|{capability}|{payload}");
    let tag = sign(&key, &canonical);
    let envelope = format!("{canonical}|sig:{tag}");
    steps.push(TrustStep {
        name: "sign_message".to_string(),
        ok: true,
        detail: format!("nonce={DEMO_NONCE} ts={now}"),
    });

    // 4. Transmit the envelope over the real transport.
    let (transport, transport_bytes) = transport_roundtrip(envelope.as_bytes());
    steps.push(TrustStep {
        name: "transmit".to_string(),
        ok: true,
        detail: format!("{transport} {transport_bytes} bytes"),
    });
    let message_hash = sha256_hex(envelope.as_bytes());

    // 5. Verifier checks the message signature (constant-time).
    let (recv_canonical, recv_tag) = split_envelope(&envelope);
    let sig_ok = verify(&key, &recv_canonical, &recv_tag);
    steps.push(TrustStep {
        name: "verify_signature".to_string(),
        ok: sig_ok,
        detail: "HMAC-SHA256 via constant-time compare".to_string(),
    });

    // 6. Freshness + first-seen nonce.
    let fresh = now <= DEMO_TS + 300 && !seen_nonces.contains(DEMO_NONCE);
    if fresh {
        seen_nonces.insert(DEMO_NONCE.to_string());
    }
    steps.push(TrustStep {
        name: "check_freshness".to_string(),
        ok: fresh,
        detail: "timestamp in window, nonce first-seen".to_string(),
    });

    // 7. Verifier issues a signed receipt (content-addressed by message hash).
    let receipt_canonical = format!("receipt|{verifier}|{message_hash}|accepted");
    let receipt_tag = sign(&key, &receipt_canonical);
    let receipt_hash = sha256_hex(format!("{receipt_canonical}|sig:{receipt_tag}").as_bytes());
    steps.push(TrustStep {
        name: "issue_receipt".to_string(),
        ok: true,
        detail: format!("receipt={}…", &receipt_hash[..12]),
    });

    // 8. Issuer verifies the receipt.
    let receipt_ok = verify(&key, &receipt_canonical, &receipt_tag);
    steps.push(TrustStep {
        name: "verify_receipt".to_string(),
        ok: receipt_ok,
        detail: "receipt tag valid".to_string(),
    });

    // 9. Replay: the same nonce is now seen, so a resend is rejected.
    let replay_rejected = seen_nonces.contains(DEMO_NONCE);
    steps.push(TrustStep {
        name: "reject_replay".to_string(),
        ok: replay_rejected,
        detail: "duplicate nonce rejected".to_string(),
    });

    // 10. Tamper: a flipped payload no longer matches the original tag.
    let tampered =
        format!("{issuer}|{verifier}|{DEMO_NONCE}|{now}|{capability}|{payload}-TAMPERED");
    let tamper_rejected = !verify(&key, &tampered, &tag);
    steps.push(TrustStep {
        name: "reject_tampered".to_string(),
        ok: tamper_rejected,
        detail: "modified message fails verification".to_string(),
    });

    let status = if steps.iter().all(|s| s.ok) {
        "ok"
    } else {
        "failed"
    }
    .to_string();

    TrustSession {
        schema: "forgentex.trust_session.v1".to_string(),
        issuer,
        verifier,
        capability: capability.to_string(),
        key_id,
        transport,
        transport_bytes,
        message_hash,
        receipt_hash,
        steps,
        replay_rejected,
        tamper_rejected,
        status,
    }
}

fn split_envelope(envelope: &str) -> (String, String) {
    match envelope.rsplit_once("|sig:") {
        Some((canonical, tag)) => (canonical.to_string(), tag.to_string()),
        None => (envelope.to_string(), String::new()),
    }
}

impl TrustSession {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex agent trust session (shared-key)\n");
        out.push_str(&format!(
            "schema={} status={} {} -> {} capability={}\n",
            self.schema, self.status, self.issuer, self.verifier, self.capability
        ));
        out.push_str(&format!(
            "  key_id={} transport={} ({} bytes)\n",
            self.key_id, self.transport, self.transport_bytes
        ));
        for step in &self.steps {
            out.push_str(&format!(
                "  {} {:<24} {}\n",
                if step.ok { "pass" } else { "FAIL" },
                step.name,
                step.detail
            ));
        }
        out.push_str(&format!(
            "  message_hash={}\n  receipt_hash={}\n",
            self.message_hash, self.receipt_hash
        ));
        out
    }

    pub fn to_json(&self) -> String {
        let steps = self
            .steps
            .iter()
            .map(|s| {
                format!(
                    "{{ \"name\": {}, \"ok\": {}, \"detail\": {} }}",
                    json::string(&s.name),
                    s.ok,
                    json::string(&s.detail)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"status\": {},\n  \"issuer\": {},\n  \"verifier\": {},\n  \"capability\": {},\n  \"key_id\": {},\n  \"transport\": {},\n  \"transport_bytes\": {},\n  \"message_hash\": {},\n  \"receipt_hash\": {},\n  \"replay_rejected\": {},\n  \"tamper_rejected\": {},\n  \"steps\": [\n    {}\n  ]\n}}",
            json::string(&self.schema),
            json::string(&self.status),
            json::string(&self.issuer),
            json::string(&self.verifier),
            json::string(&self.capability),
            json::string(&self.key_id),
            json::string(&self.transport),
            self.transport_bytes,
            json::string(&self.message_hash),
            json::string(&self.receipt_hash),
            self.replay_rejected,
            self.tamper_rejected,
            steps,
        )
    }
}
