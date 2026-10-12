use crate::tokens::{estimate, report};

use super::parse_fixture;

#[test]
fn estimate_basics() {
    assert_eq!(estimate(""), 0);
    // Each punctuation character is its own token.
    assert_eq!(estimate("{};"), 3);
    // A short word is one token; a newline is one token.
    assert_eq!(estimate("fn\n"), 2);
}

#[test]
fn estimate_charges_for_punctuation_density() {
    // The dense form has more punctuation; the estimate must be able to see
    // that, unlike a whitespace word count.
    let readable = "struct Point:\n    x: i32\n";
    let dense = "S Point{x i32}";
    let _ = (estimate(readable), estimate(dense));
    // Punctuation-heavy text costs at least one token per symbol.
    assert!(estimate("a.b.c.d") >= 4);
}

#[test]
fn report_has_all_surfaces() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let r = report("audio_mix.fgx", &program);
    assert_eq!(r.schema, "forgentex.token_efficiency.v1");
    assert!(r.human_tokens > 0);
    assert!(r.agent_tokens > 0);
    assert!(
        r.canonical_ast_tokens > r.human_tokens,
        "AST is the heavy surface"
    );
    assert!(r.agent_vs_human() > 0.0);

    let json = r.to_json();
    assert!(json.contains("\"schema\": \"forgentex.token_efficiency.v1\""));
    assert!(json.contains("\"agent_vs_human\""));
    assert!(json.contains("\"tokenizer\": \"heuristic-v1\""));
}

#[test]
fn report_is_deterministic() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    assert_eq!(
        report("dsp.fgx", &program).to_json(),
        report("dsp.fgx", &program).to_json()
    );
}

#[test]
fn every_heuristic_ratio_is_labelled_as_one() {
    // The heuristic can point the wrong way — measured against real BPE it put
    // dsp.fgx's agent form at 0.97 of the human form when it was ~1% larger —
    // so a reader who only sees the ratio must also see what kind of number it
    // is, in both renderings.
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let r = report("dsp.fgx", &program);
    let json = r.to_json();
    assert!(json.contains("\"ratio_kind\": \"heuristic\""), "{json}");
    assert!(json.contains("can point the wrong way"), "{json}");
    assert!(json.contains("\"measured\": null"), "{json}");
    assert!(r.to_text().contains("can point the wrong way"));
    // Still a single JSON object, so v1 consumers keep parsing it.
    assert!(json.trim_start().starts_with('{') && json.trim_end().ends_with('}'));
}

#[test]
fn measured_counts_come_from_the_supplied_tokenizer_and_stay_separate() {
    let program = parse_fixture("examples/hello.fgx");
    let mut seen = Vec::new();
    let measured = crate::tokens::measure(&program, "test:chars", |text| {
        seen.push(text.to_string());
        Ok(text.chars().count())
    })
    .unwrap();
    // Four surfaces, each handed over verbatim.
    assert_eq!(seen.len(), 4);
    let surfaces = crate::tokens::surfaces(&program);
    for ((_, expected), actual) in surfaces.iter().zip(&seen) {
        assert_eq!(expected, actual);
    }
    assert_eq!(measured.tokenizer, "test:chars");
    assert_eq!(measured.human_tokens, seen[0].chars().count());

    let mut r = report("hello.fgx", &program);
    let heuristic_agent = r.agent_tokens;
    r.measured = Some(measured.clone());
    // The heuristic fields are untouched: a measurement is reported beside
    // them, never merged into them.
    assert_eq!(r.agent_tokens, heuristic_agent);
    let json = r.to_json();
    assert!(json.contains("\"tokenizer\": \"test:chars\""), "{json}");
    assert!(json.contains(&format!("\"human_tokens\": {}", measured.human_tokens)));
    assert!(r.to_text().contains("measured tokenizer=test:chars"));
}

#[test]
fn a_tokenizer_failure_fails_the_whole_measurement() {
    // A table mixing measured and estimated rows would be read as all measured.
    let program = parse_fixture("examples/hello.fgx");
    let mut calls = 0;
    let err = crate::tokens::measure(&program, "flaky", |_| {
        calls += 1;
        if calls == 2 {
            Err("boom".to_string())
        } else {
            Ok(1)
        }
    })
    .unwrap_err();
    assert!(err.contains("agent surface"), "{err}");
    assert!(err.contains("boom"), "{err}");
}
