// Every diagnostic a check can emit must be documented.
//
// The catalog freezes the public `TYP004`-style codes, but most checks emit a
// rule code as a plain string, and those had drifted: 125 of 194 were absent
// from `docs/diagnostics.md`, including every carried-section check. A code an
// agent can match on but cannot look up is a poor contract, so the source is
// the authority here and the document is required to keep up with it.

use std::path::{Path, PathBuf};

const DOC: &str = include_str!("../../docs/diagnostics.md");

/// Every `Diagnostic::new("<code>"` literal in the crate.
///
/// Scanned rather than registered because a registry is exactly the thing that
/// would fall out of date: a new call site is caught even when its author does
/// not know this test exists.
fn emitted_codes() -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("source directory should be readable") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source file should be readable");
            for code in codes_in(&text) {
                found.push((code, path.clone()));
            }
        }
    }
    found
}

/// The codes named in one source file. Hand-rolled because the compiler has no
/// dependencies, including no regex crate.
fn codes_in(text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("Diagnostic::new(") {
        rest = &rest[at + "Diagnostic::new(".len()..];
        let argument = rest.trim_start();
        let Some(literal) = argument.strip_prefix('"') else {
            // A computed code, for example one forwarded from another
            // diagnostic; there is no literal here to document.
            continue;
        };
        if let Some(end) = literal.find('"') {
            let code = &literal[..end];
            if !code.is_empty()
                && code
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            {
                codes.push(code.to_string());
            }
        }
    }
    codes
}

#[test]
fn every_emitted_diagnostic_code_is_documented() {
    let emitted = emitted_codes();
    assert!(
        emitted.len() > 150,
        "the scan found only {} codes, so it is probably broken rather than the tree being small",
        emitted.len()
    );

    let mut undocumented: Vec<String> = Vec::new();
    for (code, path) in &emitted {
        if !DOC.contains(&format!("`{code}`")) {
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            let entry = format!("{code} (emitted in {file})");
            if !undocumented.contains(&entry) {
                undocumented.push(entry);
            }
        }
    }
    undocumented.sort();
    assert!(
        undocumented.is_empty(),
        "these diagnostic codes have no entry in docs/diagnostics.md: {}",
        undocumented.join(", ")
    );
}

#[test]
fn the_index_explains_the_two_tiers_and_documents_the_section_checks() {
    // The distinction an agent needs: a catalogued code can be explained and
    // repaired through `fgx explain` / `fgx fix`, a rule code cannot.
    assert!(DOC.contains("Catalogued codes"), "the tiers must be named");
    assert!(DOC.contains("Rule codes"));
    assert!(DOC.contains("fgx diagnostics list"));

    // A catalogued code stays looked-up through the catalog, not this index.
    let catalogued = crate::diagnostic_catalog::catalog();
    assert!(!catalogued.is_empty());

    for code in [
        "blob_hash_mismatch",
        "note_target_missing",
        "section_timestamp_invalid",
        "sandbox_effect_uncovered",
        "exact_arithmetic_calls_inexact",
    ] {
        assert!(DOC.contains(&format!("`{code}`")), "{code}");
    }
}
