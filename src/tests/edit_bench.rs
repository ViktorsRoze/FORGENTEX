use crate::edit_bench::run;

use super::parse_fixture;

#[test]
fn patch_strategy_beats_whole_file_and_scales() {
    let small = parse_fixture("examples/media/audio_mix.fgx");
    let large = parse_fixture("examples/audio/src/dsp.fgx");

    let small_bench = run("audio_mix.fgx", &small, 8);
    let large_bench = run("dsp.fgx", &large, 8);

    // The patch strategy is cheaper on both.
    assert!(small_bench.ratio() < 1.0);
    assert!(large_bench.ratio() < 1.0);
    // And the advantage grows with file size / function count.
    assert!(
        large_bench.ratio() < small_bench.ratio(),
        "more functions should widen the patch advantage"
    );
}

#[test]
fn patch_wins_from_the_first_edit() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let bench = run("dsp.fgx", &program, 8);
    assert_eq!(bench.breakeven_edits, Some(1));
}

#[test]
fn whole_file_cost_grows_per_edit() {
    let program = parse_fixture("examples/media/audio_mix.fgx");
    let one = run("f", &program, 1).whole_file_total;
    let ten = run("f", &program, 10).whole_file_total;
    assert!(
        ten > one * 5,
        "whole-file cost should grow roughly linearly with edits"
    );
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    let json = run("dsp.fgx", &program, 8).to_json();
    assert!(json.contains("\"schema\": \"forgentex.edit_bench.v1\""));
    assert!(json.contains("\"patch_over_whole\""));
    assert!(json.contains("\"breakeven_edits\""));
}
