use crate::project_map::{generate_map, generate_map_with_options, get_symbol, ProjectMapOptions};

use super::manifest_path;

#[test]
fn project_map_deduplicates_human_and_agent_views() {
    let map = generate_map(&manifest_path("examples")).unwrap();
    assert!(map.contains("M hello.fgx"));
    assert!(!map.contains("M hello.fgai"));
    assert!(map.contains("M media/audio_mix.fgx"));
    assert!(!map.contains("M media/audio_mix.fgai"));
    assert!(map.contains("F popcount_x86 @function.popcount_x86 unsafe asm"));
}

#[test]
fn project_map_snapshot_contains_stable_symbols() {
    let map = generate_map(&manifest_path("examples")).unwrap();
    assert!(map.contains("S Point @struct.Point fields=2"));
    assert!(map.contains("F mix_samples @function.mix_samples params=4"));
    assert!(map.contains("S H264NalHeader @struct.H264NalHeader fields=3"));
}

#[test]
fn project_map_reports_per_symbol_heuristic_token_costs() {
    let map = generate_map(&manifest_path("examples/media/audio_mix.fgx")).unwrap();
    let clamp = map
        .lines()
        .find(|line| line.contains("F clamp_i16 "))
        .expect("clamp symbol");
    let mix = map
        .lines()
        .find(|line| line.contains("F mix_samples "))
        .expect("mix symbol");
    let clamp_tokens = suffix_usize(clamp, "tokens~");
    let mix_tokens = suffix_usize(mix, "tokens~");
    assert!(clamp_tokens > 0);
    assert!(mix_tokens > clamp_tokens);
}

fn suffix_usize(line: &str, marker: &str) -> usize {
    line.rsplit_once(marker)
        .expect("token marker")
        .1
        .parse()
        .expect("token count")
}

#[test]
fn project_map_can_include_target_metadata() {
    let map = generate_map_with_options(
        &manifest_path("examples"),
        ProjectMapOptions {
            include_targets: true,
        },
    )
    .unwrap();
    assert!(map.contains("F popcount_x86 @function.popcount_x86 unsafe asm"));
    assert!(map.contains("cpu.popcnt,target.x86_64"));
}

#[test]
fn get_symbol_deduplicates_type_dependencies() {
    let context = get_symbol(
        &manifest_path("examples"),
        "@function.mix_samples",
        Some("types"),
    )
    .unwrap();
    assert_eq!(context.matches("struct Point:").count(), 1);
    assert_eq!(context.matches("struct AudioBlock:").count(), 1);
    assert!(context.contains("fn mix_samples"));
}
