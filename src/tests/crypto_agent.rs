use std::path::Path;

use crate::effects::{analyze, effects_to_json};
use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::patch::apply_patch;
use crate::typecheck::check;

use super::{fixture, parse_fixture};

#[test]
fn crypto_and_agent_examples_check_and_report_effects() {
    for path in [
        "examples/agent/signed_patch.fgx",
        "examples/agent/payment_receipt.fgx",
    ] {
        let program = parse_fixture(path);
        assert_eq!(check(&program), Vec::new(), "{path} should check");
        let json = effects_to_json(&analyze(&program));
        assert!(
            json.contains("crypto.") || json.contains("ledger.mock") || json.contains("agent.")
        );
    }
}

#[test]
fn approved_signing_and_mock_receipts_execute() {
    let signed_patch = parse_fixture("examples/agent/signed_patch.fgx");
    let signature = run_entry_json(
        &signed_patch,
        "sign_patch",
        include_str!("../../examples/run/signed_patch.json"),
    )
    .unwrap();
    assert!(signature.contains("\"status\": \"ok\""));
    assert!(signature.contains("sig:ed25519:"));

    let receipt_program = parse_fixture("examples/agent/payment_receipt.fgx");
    let receipt = run_entry_json(
        &receipt_program,
        "issue_receipt",
        include_str!("../../examples/run/payment_receipt.json"),
    )
    .unwrap();
    assert!(receipt.contains("\"status\": \"ok\""));
    assert!(receipt.contains("receipt:mock:"));
}

#[test]
fn secret_values_cannot_be_logged_serialized_or_cloned_in_safe_code() {
    let source = r#"
fn log_secret(secret: SecretKey[Ed25519]) -> void effects [crypto.secret, io.write]:
    core_agent_log_secret(secret)
    return

fn export_secret(secret: SecretKey[Ed25519]) -> str effects [crypto.secret, serialize]:
    return core_crypto_export_secret_key(secret)

fn copy_secret(secret: SecretKey[Ed25519]) -> SecretKey[Ed25519] effects [crypto.secret]:
    return core_crypto_clone_secret_key(secret)
"#;
    let program = parse_file(Path::new("secret_leaks.fgx"), source).unwrap();
    let diagnostics = check(&program);
    let text = format!("{diagnostics:?}");

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "secret_leak")
            .count(),
        3
    );
    assert!(text.contains("log or print secret material"));
    assert!(text.contains("serialize secret material"));
    assert!(text.contains("copy secret material"));
}

#[test]
fn approved_secret_crypto_api_passes_in_safe_code() {
    let source = r#"
fn sign(secret: SecretKey[Ed25519], message: str) -> Signature[Ed25519] effects [crypto.sign, crypto.secret]:
    return core_crypto_sign_ed25519(secret, message)
"#;
    let program = parse_file(Path::new("secret_ok.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());
}

#[test]
fn secret_signature_requires_explicit_crypto_secret_effect() {
    let source = r#"
fn receive(secret: SecretKey[Ed25519]) -> void:
    return
"#;
    let program = parse_file(Path::new("secret_signature_effect.fgx"), source).unwrap();
    let diagnostics = check(&program);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "undeclared_effect" && diagnostic.message.contains("crypto.secret")
    }));
}

#[test]
fn signed_patch_envelope_is_verified_by_patch_workflow() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let result = apply_patch(program, fixture("patches/signed_add_crypto_effect.fgp")).unwrap();
    let review = result.review.render();
    let json = result.review.render_json(&check(&result.program));

    assert_eq!(result.review.signature_status, "signed-mock-accepted");
    assert!(review.contains("Verified signed semantic patch envelope"));
    assert!(review.contains("Signature status: signed-mock-accepted"));
    assert!(json.contains("\"signed_by\": \"agent-public-demo\""));
    assert!(json.contains("\"signature_algorithm\": \"Ed25519\""));
}

#[test]
fn invalid_signed_patch_envelope_fails_before_application() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let patch = "VERSION 1\n\
                 SIGNED_BY agent-public-demo\n\
                 SIGNATURE_ALGORITHM Ed25519\n\
                 SIGNATURE not-real\n\
                 TARGET @function.mix_samples\n\
                 OP ADD_EFFECT effect=crypto.verify\n";

    let err = apply_patch(program, patch).unwrap_err();
    assert!(err.contains("signed patch verification failed"));
}
