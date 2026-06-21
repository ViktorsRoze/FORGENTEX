use crate::compress::{compress, decompress};
use crate::format::format_agent;

use super::parse_fixture;

fn roundtrips(agent: &str) {
    let report = compress(agent);
    assert!(report.roundtrip_ok, "compression must be lossless");
    assert!(report.compressed_chars <= report.original_chars);
    let restored = decompress(&report.compressed).unwrap();
    assert_eq!(
        restored, agent,
        "decompress must reproduce the source exactly"
    );
}

#[test]
fn compression_is_lossless_across_the_corpus() {
    for fixture in [
        "examples/audio/src/dsp.fgx",
        "examples/capsule/web_session.fgx",
        "examples/capsule/normalize_python.fgx",
        "examples/capsule/saxpy_cuda.fgx",
        "examples/capsule/backup_bash.fgx",
        "examples/media/audio_mix.fgx",
    ] {
        let program = parse_fixture(fixture);
        roundtrips(&format_agent(&program));
    }
}

#[test]
fn deterministic_same_input_same_output() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let agent = format_agent(&program);
    assert_eq!(compress(&agent).compressed, compress(&agent).compressed);
}

#[test]
fn never_grows_the_artifact() {
    let program = parse_fixture("examples/capsule/web_session.fgx");
    let report = compress(&format_agent(&program));
    assert!(report.ratio() <= 1.0);
}

#[test]
fn decompress_passes_through_uncompressed_text() {
    // A non-compressed string is returned unchanged.
    assert_eq!(
        decompress("plain text\nno header").unwrap(),
        "plain text\nno header"
    );
}

#[test]
fn body_lines_starting_with_hash_d_survive_roundtrip() {
    // A foreign-code comment line like "#D ..." must not be mistaken for a
    // dictionary entry; the dict count in the header guards this.
    let source = "\
CAP demo python \"3.13\"
  IN value i32
  OUT value i32
  SBX \"fs.none\"
  CODE 2
#D this is a python comment that looks like a dict line
value = value + value + value + value
ENDCAP
";
    let report = compress(source);
    assert!(report.roundtrip_ok);
    assert_eq!(decompress(&report.compressed).unwrap(), source);
}

#[test]
fn minify_is_lossless_at_ast_level() {
    use crate::compress::minify_agent;
    use crate::parser::parse_agent;
    for fixture in [
        "examples/audio/src/dsp.fgx",
        "examples/media/audio_mix.fgx",
        "examples/capsule/web_session.fgx",
        "examples/capsule/normalize_python.fgx",
        "examples/smoke/inline_tests.fgx",
    ] {
        let program = parse_fixture(fixture);
        let minified = minify_agent(&format_agent(&program));
        let reparsed = parse_agent(&minified)
            .unwrap_or_else(|e| panic!("minified {fixture} should parse: {e}"));
        assert!(
            program.canonical_eq(&reparsed),
            "minify must preserve the canonical AST for {fixture}"
        );
    }
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let json = compress(&format_agent(&program)).to_json();
    assert!(json.contains("\"schema\": \"forgentex.capsule_compress.v1\""));
    assert!(json.contains("\"roundtrip_ok\": true"));
}
