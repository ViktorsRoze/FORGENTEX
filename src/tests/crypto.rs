use crate::crypto::{constant_time_eq, hmac_sha256_hex, self_test, sha256_hex};

use super::parse_fixture;

#[test]
fn sha256_matches_nist_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn hmac_matches_rfc4231_test_case_2() {
    assert_eq!(
        hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn constant_time_eq_checks_content_and_length() {
    assert!(constant_time_eq(b"forgentex", b"forgentex"));
    assert!(!constant_time_eq(b"forgentex", b"forgentey"));
    assert!(!constant_time_eq(b"abc", b"abcd"));
}

#[test]
fn self_test_passes_all_vectors() {
    let report = self_test();
    assert!(report.is_success());
    assert_eq!(report.passed(), report.total());
    assert!(report.to_json().contains("forgentex.crypto_selftest.v1"));
}

#[test]
fn interpreter_exposes_real_sha256() {
    use crate::ast::Expr;
    let program = parse_fixture("examples/crypto/hashing.fgx");
    let call = Expr::Call {
        name: "core_hash_sha256".to_string(),
        args: vec![Expr::String("abc".to_string())],
    };
    let value = crate::interp::eval_expr_for_test(&program, &call).unwrap();
    match value {
        crate::interp::Value::String(s) => assert_eq!(
            s,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ),
        other => panic!("expected string digest, got {other:?}"),
    }
}

#[test]
fn hashing_example_checks_clean() {
    let program = parse_fixture("examples/crypto/hashing.fgx");
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}
