//! Shared JSON string encoding for every machine-readable report.
//!
//! Forgentex's contract with agents is that every emitted report is valid
//! JSON. All modules must escape strings through this function so the
//! encoding cannot drift between reports.

/// Escape a string for use inside a JSON string literal.
///
/// Handles the two mandatory escapes (`"` and `\`), the common control
/// shorthands (`\n`, `\r`, `\t`, `\b`, `\f`), and falls back to `\u00XX`
/// for every other control character, as required by RFC 8259.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out
}

/// Render a quoted JSON string literal.
pub fn string(value: &str) -> String {
    format!("\"{}\"", escape(value))
}

/// Render an optional value as a quoted JSON string literal or `null`.
pub fn opt_string(value: Option<&str>) -> String {
    match value {
        Some(value) => string(value),
        None => "null".to_string(),
    }
}

/// Render a slice of strings as a JSON array of string literals.
pub fn string_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&string(value));
    }
    out.push(']');
    out
}
