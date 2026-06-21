use crate::trust::run;

#[test]
fn full_trust_loop_succeeds_with_real_crypto() {
    let session = run("optimize.media.h264", "patch:add_fast_path");

    assert_eq!(session.schema, "forgentex.trust_session.v1");
    assert_eq!(session.status, "ok");
    assert!(
        session.steps.iter().all(|s| s.ok),
        "steps: {:?}",
        session.steps
    );
    assert_eq!(session.steps.len(), 10);

    // The security-relevant outcomes, explicitly.
    assert!(session.replay_rejected, "replay must be rejected");
    assert!(session.tamper_rejected, "tampered message must be rejected");

    // Real transport actually moved bytes (tcp, or memory fallback).
    assert!(session.transport == "tcp" || session.transport == "memory");
    assert!(session.transport_bytes > 0);

    // Content-addressed audit hashes are full SHA-256 digests.
    assert_eq!(session.message_hash.len(), 64);
    assert_eq!(session.receipt_hash.len(), 64);

    // The secret key is never exposed — only a short key id.
    assert_eq!(session.key_id.len(), 16);
}

#[test]
fn session_is_deterministic_for_the_same_inputs() {
    let a = run("cap.x", "payload");
    let b = run("cap.x", "payload");
    assert_eq!(a.message_hash, b.message_hash);
    assert_eq!(a.receipt_hash, b.receipt_hash);
    assert_eq!(a.key_id, b.key_id);
}

#[test]
fn different_capability_changes_the_message_hash() {
    let a = run("cap.a", "p");
    let b = run("cap.b", "p");
    assert_ne!(a.message_hash, b.message_hash);
}

#[test]
fn report_json_is_well_formed() {
    let json = run("cap.x", "p").to_json();
    assert!(json.contains("\"schema\": \"forgentex.trust_session.v1\""));
    assert!(json.contains("\"replay_rejected\": true"));
    assert!(json.contains("\"tamper_rejected\": true"));
    assert!(json.contains("\"message_hash\""));
}
