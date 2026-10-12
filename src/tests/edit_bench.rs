use crate::edit_bench::run;

use super::parse_fixture;

#[test]
fn semantic_patching_beats_resending_the_whole_file_and_scales() {
    // True, but against a deliberately weak baseline: no competent agent
    // re-emits the whole file per edit. The comparison that matters is the
    // text-patch baseline, covered below.
    let small = parse_fixture("examples/media/audio_mix.fgx");
    let large = parse_fixture("examples/audio/src/dsp.fgx");

    let small_bench = run("audio_mix.fgx", &small, 8).unwrap();
    let large_bench = run("dsp.fgx", &large, 8).unwrap();

    assert!(small_bench.ratio() < 1.0);
    assert!(large_bench.ratio() < 1.0);
    assert!(
        large_bench.ratio() < small_bench.ratio(),
        "more functions should widen the gap over whole-file resend"
    );
}

#[test]
fn every_result_is_compared_against_the_conventional_baseline_too() {
    // Reporting only the whole-file baseline is what made "8x cheaper" look
    // like a result. The text-patch baseline — read the file once, then send a
    // small patch per edit — has to be in every report beside it.
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let bench = run("dsp.fgx", &program, 8).unwrap();
    assert_eq!(
        bench.text_patch_total,
        bench.whole_file_tokens + 8 * bench.patch_tokens
    );
    assert!(bench.text_patch_total < bench.whole_file_total);

    let json = bench.to_json();
    assert!(json.contains("\"text_patch_total\""), "{json}");
    assert!(json.contains("\"patch_over_text_patch\""), "{json}");
    let text = bench.to_text();
    assert!(text.contains("semantic/text_patch="), "{text}");
}

#[test]
fn the_report_says_it_is_a_cost_model_and_states_its_assumptions() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let bench = run("audio_mix.fgx", &program, 8).unwrap();
    let json = bench.to_json();
    assert!(json.contains("\"kind\": \"cost_model\""), "{json}");
    assert!(json.contains("\"tokenizer\": \"heuristic-v1\""), "{json}");
    assert!(json.contains("not a measured coding task"), "{json}");
    let text = bench.to_text();
    assert!(text.contains("COST MODEL"), "{text}");
    for assumption in crate::edit_bench::ASSUMPTIONS {
        assert!(
            text.contains(assumption),
            "missing assumption: {assumption}"
        );
    }
}

#[test]
fn a_file_without_functions_is_rejected_rather_than_estimated() {
    // web_session.fgx has no functions. The old model charged zero for reading
    // the edit target and still invented a function patch, reporting a 94%
    // saving on edits that cannot happen.
    let program = parse_fixture("examples/capsule/web_session.fgx");
    assert_eq!(program.functions().count(), 0);
    let err = run("web_session.fgx", &program, 8).unwrap_err();
    assert!(err.contains("edit_bench_unsupported"), "{err}");
    assert!(err.contains("no functions"), "{err}");
}

#[test]
fn zero_edits_is_rejected() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let err = run("audio_mix.fgx", &program, 0).unwrap_err();
    assert!(err.contains("at least 1"), "{err}");
}

#[test]
fn the_representative_patch_is_a_real_patch_that_applies() {
    // The previous fixed text used a REPLACE_STATEMENT operation the protocol
    // does not have. The model now refuses to run unless its patch is accepted
    // by the real patch engine, so a successful run is the proof — checked here
    // for a function with a return value and for a void one.
    for fixture in ["examples/hello.fgx", "examples/media/audio_mix.fgx"] {
        let program = parse_fixture(fixture);
        let bench = run(fixture, &program, 3)
            .unwrap_or_else(|err| panic!("{fixture}: representative patch should apply: {err}"));
        assert!(bench.patch_tokens > 0);
    }
}

#[test]
fn whole_file_cost_grows_per_edit() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let one = run("f", &program, 1).unwrap().whole_file_total;
    let ten = run("f", &program, 10).unwrap().whole_file_total;
    assert!(
        ten > one * 5,
        "whole-file cost should grow roughly linearly with edits"
    );
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let json = run("dsp.fgx", &program, 8).unwrap().to_json();
    assert!(json.contains("\"schema\": \"forgentex.edit_bench.v1\""));
    assert!(json.contains("\"patch_over_whole\""));
    assert!(json.contains("\"breakeven_edits\""));
}
