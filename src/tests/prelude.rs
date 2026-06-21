use crate::format::{format_agent, format_human};
use crate::parser::{parse_agent, parse_human};
use crate::prelude::{alias_to_canonical, apply_aliases, canonical_to_alias, validate, PRELUDE};

use super::parse_fixture;

#[test]
fn prelude_has_twelve_two_letter_aliases() {
    assert_eq!(PRELUDE.len(), 12);
    for (_, alias) in PRELUDE {
        assert_eq!(alias.len(), 2, "alias '{alias}' must be two letters");
    }
}

#[test]
fn alias_canonical_roundtrip() {
    assert_eq!(canonical_to_alias("core_math_clamp_i16"), Some("cl"));
    assert_eq!(alias_to_canonical("cl"), Some("core_math_clamp_i16"));
    assert_eq!(canonical_to_alias("not_a_builtin"), None);
    assert_eq!(alias_to_canonical("zz"), None);
}

#[test]
fn agent_renders_aliases_human_keeps_canonical() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let agent = format_agent(&program);
    let human = format_human(&program);

    // Agent uses the short alias; human keeps the readable canonical name.
    assert!(agent.contains("cl(x)"));
    assert!(agent.contains("lm(dst)"));
    assert!(!agent.contains("core_math_clamp_i16"));
    assert!(human.contains("core_math_clamp_i16"));
    assert!(!human.contains("cl(x)"));
}

#[test]
fn alias_expansion_roundtrips_to_canonical_ast() {
    let from_human = parse_fixture("examples/media/audio_mix.fgx");
    let from_agent = parse_agent(&format_agent(&from_human)).expect("agent parse");
    assert!(
        from_human.canonical_eq(&from_agent),
        "aliased agent view must share the canonical AST"
    );
}

#[test]
fn writing_an_alias_in_human_source_resolves_to_the_builtin() {
    // `cl(x)` is accepted anywhere and means the builtin; human form
    // normalizes it back to the canonical name.
    let program = parse_human("fn f(x: f32) -> i16:\n    return cl(x)\n").unwrap();
    let function = program.find_function("f").unwrap();
    let human = format_human(&program);
    assert!(human.contains("core_math_clamp_i16(x)"));
    let _ = function;
}

#[test]
fn apply_aliases_only_touches_whole_words() {
    assert_eq!(
        apply_aliases("R cl_value + core_math_clamp_i16(x)"),
        "R cl_value + cl(x)"
    );
}

#[test]
fn reserved_alias_function_name_is_rejected() {
    let program = parse_human("fn cl(x: i32) -> i32:\n    return x\n").unwrap();
    let codes: Vec<String> = validate(&program).into_iter().map(|d| d.code).collect();
    assert!(codes.contains(&"reserved_prelude_alias".to_string()));
}
