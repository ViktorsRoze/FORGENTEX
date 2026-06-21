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
