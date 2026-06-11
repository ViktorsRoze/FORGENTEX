use crate::json::{escape, opt_string, string, string_array};

#[test]
fn escape_handles_quotes_backslashes_and_common_controls() {
    assert_eq!(escape("plain"), "plain");
    assert_eq!(escape("say \"hi\""), "say \\\"hi\\\"");
    assert_eq!(escape("a\\b"), "a\\\\b");
    assert_eq!(escape("line1\nline2"), "line1\\nline2");
    assert_eq!(escape("a\rb"), "a\\rb");
    assert_eq!(escape("a\tb"), "a\\tb");
}

#[test]
fn escape_handles_rare_control_characters() {
    assert_eq!(escape("a\u{08}b"), "a\\bb");
    assert_eq!(escape("a\u{0C}b"), "a\\fb");
    assert_eq!(escape("a\u{00}b"), "a\\u0000b");
    assert_eq!(escape("a\u{1F}b"), "a\\u001fb");
}

#[test]
fn escape_preserves_non_ascii_text() {
    assert_eq!(escape("héllo → wörld"), "héllo → wörld");
}

#[test]
fn escaped_output_parses_as_valid_json_string_content() {
    // Every byte below 0x20 must be escaped per RFC 8259.
    for code in 0u32..0x20 {
        let ch = char::from_u32(code).unwrap();
        let escaped = escape(&ch.to_string());
        assert!(
            !escaped.chars().any(|c| (c as u32) < 0x20),
            "control character {code:#x} leaked into JSON output: {escaped:?}"
        );
    }
}

#[test]
fn string_helpers_render_quoted_literals() {
    assert_eq!(string("a\"b"), "\"a\\\"b\"");
    assert_eq!(opt_string(Some("x")), "\"x\"");
    assert_eq!(opt_string(None), "null");
    assert_eq!(
        string_array(&["a".to_string(), "b\n".to_string()]),
        "[\"a\", \"b\\n\"]"
    );
}
