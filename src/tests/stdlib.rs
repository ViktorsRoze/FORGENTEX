use std::path::Path;

use crate::interp::{canonical_json, run_entry_json, MAX_SERIALIZED_BYTES};
use crate::parser::parse_file;
use crate::project::{check_project, generate_project_map};
use crate::stdlib::{standard_library_json, standard_library_text};
use crate::typecheck::check;

use super::manifest_path;

#[test]
fn standard_library_catalog_lists_core_modules() {
    let text = standard_library_text();
    assert!(text.contains("Forgentex standard library v0"));
    assert!(text.contains("core.mem"));
    assert!(text.contains("core.slice"));
    assert!(text.contains("core.math"));
    assert!(text.contains("core.music"));
    assert!(text.contains("core.result"));
    assert!(text.contains("core.option"));
    assert!(text.contains("core.bits"));
    assert!(text.contains("core.simd"));
    assert!(text.contains("core.target"));
    assert!(text.contains("core.fs"));
    assert!(text.contains("core.net"));
    assert!(text.contains("core.serialize"));
    assert!(text.contains("core.stream"));
    assert!(text.contains("core.crypto"));
    assert!(text.contains("core.agent"));

    let json = standard_library_json();
    assert!(json.contains("\"schema\": \"forgentex.stdlib.v1\""));
    assert!(json.contains("\"name\": \"core.target\""));
    assert!(json.contains("\"name\": \"core.fs\""));
    assert!(json.contains("\"name\": \"core.crypto\""));
    assert!(json.contains("\"name\": \"core.agent\""));
    assert!(json.contains("core_math_checked_add_i32"));
    assert!(json.contains("core_music_tempo_map_millis"));
    assert!(json.contains("core_music_conflict_winner_id"));
    assert!(json.contains("core_net_http_get"));
    assert!(json.contains("core_crypto_sign_ed25519"));
    assert!(json.contains("core_agent_payment_receipt"));
}

#[test]
fn standard_library_source_package_typechecks() {
    let diagnostics = check_project(&manifest_path("stdlib")).unwrap();
    assert_eq!(diagnostics, Vec::new());
}

#[test]
fn serialization_is_canonical_bounded_and_binary_exact() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/src/core/serialize.fgx");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse_file(&path, &source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let input = r#" { "b": 2.0, "a": [1e0, "\u00e9", "\ud83d\ude00"] } "#;
    let canonical = r#"{"a":[1,"é","😀"],"b":2}"#;
    let output = run_entry_json(
        &program,
        "core_serialize_json_canonical",
        &format!(
            r#"{{"args":{{"value":{}}},"expect":{{"return":{}}}}}"#,
            crate::json::string(input),
            crate::json::string(canonical)
        ),
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");

    for (value, expected) in [(canonical, true), (input, false)] {
        let output = run_entry_json(
            &program,
            "core_serialize_json_is_canonical",
            &format!(
                r#"{{"args":{{"value":{}}},"expect":{{"return":{expected}}}}}"#,
                crate::json::string(value)
            ),
        )
        .unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{output}");
    }

    for (entry, args, expected) in [
        (
            "core_serialize_utf8_encode",
            r#"{"value":"A€"}"#,
            "[65,226,130,172]",
        ),
        (
            "core_serialize_utf8_decode",
            r#"{"bytes":[65,226,130,172]}"#,
            r#""A€""#,
        ),
        ("core_serialize_u16_le", r#"{"value":4660}"#, "[52,18]"),
        (
            "core_serialize_u32_le",
            r#"{"value":305419896}"#,
            "[120,86,52,18]",
        ),
        (
            "core_serialize_i32_le",
            r#"{"value":-2}"#,
            "[254,255,255,255]",
        ),
        (
            "core_serialize_read_u16_le",
            r#"{"bytes":[0,52,18],"offset":1}"#,
            "4660",
        ),
        (
            "core_serialize_read_u32_le",
            r#"{"bytes":[0,120,86,52,18],"offset":1}"#,
            "305419896",
        ),
        (
            "core_serialize_read_i32_le",
            r#"{"bytes":[0,254,255,255,255],"offset":1}"#,
            "-2",
        ),
        (
            "core_serialize_concat",
            r#"{"left":[1,2],"right":[3,4]}"#,
            "[1,2,3,4]",
        ),
    ] {
        let output = run_entry_json(
            &program,
            entry,
            &format!(r#"{{"args":{args},"expect":{{"return":{expected}}}}}"#),
        )
        .unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{entry}: {output}");
    }

    for invalid in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":01}"#,
        r#"{"a":1,}"#,
        r#""\ud800""#,
    ] {
        let error = run_entry_json(
            &program,
            "core_serialize_json_canonical",
            &format!(r#"{{"args":{{"value":{}}}}}"#, crate::json::string(invalid)),
        )
        .unwrap_err();
        assert!(error.contains("invalid_json"), "{invalid}: {error}");
    }

    let invalid_utf8 = run_entry_json(
        &program,
        "core_serialize_utf8_decode",
        r#"{"args":{"bytes":[255]}}"#,
    )
    .unwrap_err();
    assert!(
        invalid_utf8.contains("serialize_invalid_utf8"),
        "{invalid_utf8}"
    );

    let out_of_bounds = run_entry_json(
        &program,
        "core_serialize_read_u32_le",
        r#"{"args":{"bytes":[1,2,3],"offset":0}}"#,
    )
    .unwrap_err();
    assert!(
        out_of_bounds.contains("serialize_bounds"),
        "{out_of_bounds}"
    );

    let nested = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    let too_deep = run_entry_json(
        &program,
        "core_serialize_json_canonical",
        &format!(r#"{{"args":{{"value":{}}}}}"#, crate::json::string(&nested)),
    )
    .unwrap_err();
    assert!(too_deep.contains("JSON nesting exceeds 128"), "{too_deep}");

    let oversized = " ".repeat(MAX_SERIALIZED_BYTES + 1);
    let too_large = canonical_json(&oversized).unwrap_err();
    assert!(too_large.contains("serialize_size_limit"), "{too_large}");
}

#[test]
fn media_project_resolves_imported_standard_modules() {
    let diagnostics = check_project(&manifest_path(".")).unwrap();
    assert_eq!(diagnostics, Vec::new());

    let map = generate_project_map(&manifest_path(".")).unwrap();
    assert!(map.contains("M core.math stdlib/src/core/math.fgx"));
    assert!(map.contains("M core.slice stdlib/src/core/slice.fgx"));
    assert!(map.contains("M core.bits stdlib/src/core/bits.fgx"));
    assert!(map.contains("M core.music stdlib/src/core/music.fgx"));
    assert!(map.contains("M music.timeline examples/project/music/timeline.fgx"));
    assert!(map.contains("imports=core.music"));
    assert!(map.contains("imports=core.math,core.slice,media.types"));
}

#[test]
fn music_timeline_converts_quantizes_and_applies_tempo_maps() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/src/core/music.fgx");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse_file(&path, &source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let position = r#"{"$struct":"MusicalTime","bar":5,"beat":1,"tick":0}"#;
    let tick = run_entry_json(
        &program,
        "core_music_to_tick",
        &format!(
            r#"{{"args":{{"position":{position},"beats_per_bar":4,"ticks_per_beat":480}},"expect":{{"return":7680}}}}"#
        ),
    )
    .unwrap();
    assert!(tick.contains("\"status\": \"pass\""), "{tick}");

    let duration = run_entry_json(
        &program,
        "core_music_tick_to_millis",
        r#"{"args":{"tick":30720,"bpm":120,"ticks_per_beat":480},"expect":{"return":32000}}"#,
    )
    .unwrap();
    assert!(duration.contains("\"status\": \"pass\""), "{duration}");

    let long_duration = run_entry_json(
        &program,
        "core_music_tick_to_millis",
        r#"{"args":{"tick":307200,"bpm":120,"ticks_per_beat":480},"expect":{"return":320000}}"#,
    )
    .unwrap();
    assert!(
        long_duration.contains("\"status\": \"pass\""),
        "{long_duration}"
    );

    for (raw, expected) in [(119, 0), (120, 240), (359, 240), (360, 480)] {
        let output = run_entry_json(
            &program,
            "core_music_quantize_tick",
            &format!(
                r#"{{"args":{{"tick":{raw},"grid_ticks":240}},"expect":{{"return":{expected}}}}}"#
            ),
        )
        .unwrap();
        assert!(output.contains("\"status\": \"pass\""), "{output}");
    }

    let tempo_map = r#"{"$struct":"TempoMap","change_tick":1920,"initial_bpm":120,"next_bpm":60}"#;
    let mapped = run_entry_json(
        &program,
        "core_music_tempo_map_millis",
        &format!(
            r#"{{"args":{{"tempo_map":{tempo_map},"tick":2880,"ticks_per_beat":480}},"expect":{{"return":4000}}}}"#
        ),
    )
    .unwrap();
    assert!(mapped.contains("\"status\": \"pass\""), "{mapped}");
}

#[test]
fn music_timeline_validation_rejects_invalid_positions_and_maps() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/src/core/music.fgx");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse_file(&path, &source).unwrap();

    let invalid_position = run_entry_json(
        &program,
        "core_music_position_valid",
        r#"{"args":{"position":{"$struct":"MusicalTime","bar":1,"beat":5,"tick":0},"beats_per_bar":4,"ticks_per_beat":480},"expect":{"return":false}}"#,
    )
    .unwrap();
    assert!(
        invalid_position.contains("\"status\": \"pass\""),
        "{invalid_position}"
    );

    let invalid_map = run_entry_json(
        &program,
        "core_music_tempo_map_valid",
        r#"{"args":{"tempo_map":{"$struct":"TempoMap","change_tick":0,"initial_bpm":120,"next_bpm":90},"ticks_per_beat":480},"expect":{"return":false}}"#,
    )
    .unwrap();
    assert!(
        invalid_map.contains("\"status\": \"pass\""),
        "{invalid_map}"
    );

    let zero_grid = run_entry_json(
        &program,
        "core_music_quantize_tick",
        r#"{"args":{"tick":120,"grid_ticks":0}}"#,
    )
    .unwrap_err();
    assert!(zero_grid.contains("trap: division_by_zero"), "{zero_grid}");

    let zero_tempo = run_entry_json(
        &program,
        "core_music_tick_to_millis",
        r#"{"args":{"tick":480,"bpm":0,"ticks_per_beat":480}}"#,
    )
    .unwrap_err();
    assert!(
        zero_tempo.contains("trap: division_by_zero"),
        "{zero_tempo}"
    );
}

#[test]
fn music_cue_schema_builds_quantized_bounded_markers() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/src/core/music.fgx");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse_file(&path, &source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let built = run_entry_json(
        &program,
        "core_music_make_cue",
        r#"{"args":{"cue_id":5,"action_id":1,"track":0,"position":{"$struct":"MusicalTime","bar":5,"beat":1,"tick":120},"duration_ticks":1920,"priority":100,"beats_per_bar":4,"ticks_per_beat":480,"grid_ticks":240},"expect":{"return":{"$struct":"MusicCue","version":1,"cue_id":5,"action_id":1,"track":0,"start_tick":7920,"duration_ticks":1920,"priority":100}}}"#,
    )
    .unwrap();
    assert!(built.contains("\"status\": \"pass\""), "{built}");

    let valid = run_entry_json(
        &program,
        "core_music_cue_valid",
        r#"{"args":{"cue":{"$struct":"MusicCue","version":1,"cue_id":5,"action_id":1,"track":0,"start_tick":7920,"duration_ticks":1920,"priority":100},"track_count":4,"timeline_ticks":30720},"expect":{"return":true}}"#,
    )
    .unwrap();
    assert!(valid.contains("\"status\": \"pass\""), "{valid}");

    for cue in [
        r#"{"$struct":"MusicCue","version":0,"cue_id":1,"action_id":1,"track":0,"start_tick":0,"duration_ticks":1,"priority":0}"#,
        r#"{"$struct":"MusicCue","version":1,"cue_id":0,"action_id":1,"track":0,"start_tick":0,"duration_ticks":1,"priority":0}"#,
        r#"{"$struct":"MusicCue","version":1,"cue_id":1,"action_id":0,"track":0,"start_tick":0,"duration_ticks":1,"priority":0}"#,
        r#"{"$struct":"MusicCue","version":1,"cue_id":1,"action_id":1,"track":4,"start_tick":0,"duration_ticks":1,"priority":0}"#,
        r#"{"$struct":"MusicCue","version":1,"cue_id":1,"action_id":1,"track":0,"start_tick":0,"duration_ticks":0,"priority":0}"#,
        r#"{"$struct":"MusicCue","version":1,"cue_id":1,"action_id":1,"track":0,"start_tick":30720,"duration_ticks":1,"priority":0}"#,
    ] {
        let invalid = run_entry_json(
            &program,
            "core_music_cue_valid",
            &format!(
                r#"{{"args":{{"cue":{cue},"track_count":4,"timeline_ticks":30720}},"expect":{{"return":false}}}}"#
            ),
        )
        .unwrap();
        assert!(invalid.contains("\"status\": \"pass\""), "{invalid}");
    }
}

#[test]
fn music_cue_conflicts_use_half_open_intervals_priority_and_stable_ids() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/src/core/music.fgx");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse_file(&path, &source).unwrap();
    let first = r#"{"$struct":"MusicCue","version":1,"cue_id":7,"action_id":1,"track":0,"start_tick":100,"duration_ticks":100,"priority":10}"#;

    for (other, expected) in [
        (
            r#"{"$struct":"MusicCue","version":1,"cue_id":8,"action_id":2,"track":0,"start_tick":199,"duration_ticks":10,"priority":20}"#,
            true,
        ),
        (
            r#"{"$struct":"MusicCue","version":1,"cue_id":8,"action_id":2,"track":0,"start_tick":200,"duration_ticks":10,"priority":20}"#,
            false,
        ),
        (
            r#"{"$struct":"MusicCue","version":1,"cue_id":8,"action_id":2,"track":1,"start_tick":199,"duration_ticks":10,"priority":20}"#,
            false,
        ),
    ] {
        let conflict = run_entry_json(
            &program,
            "core_music_cues_conflict",
            &format!(
                r#"{{"args":{{"left":{first},"right":{other}}},"expect":{{"return":{expected}}}}}"#
            ),
        )
        .unwrap();
        assert!(conflict.contains("\"status\": \"pass\""), "{conflict}");
    }

    for (left, right, expected) in [
        (
            r#"{"$struct":"MusicCue","version":1,"cue_id":9,"action_id":1,"track":0,"start_tick":100,"duration_ticks":100,"priority":20}"#,
            first,
            true,
        ),
        (
            first,
            r#"{"$struct":"MusicCue","version":1,"cue_id":9,"action_id":1,"track":0,"start_tick":100,"duration_ticks":100,"priority":20}"#,
            false,
        ),
        (
            first,
            r#"{"$struct":"MusicCue","version":1,"cue_id":8,"action_id":1,"track":0,"start_tick":100,"duration_ticks":100,"priority":10}"#,
            true,
        ),
    ] {
        let winner = run_entry_json(
            &program,
            "core_music_cue_wins",
            &format!(
                r#"{{"args":{{"left":{left},"right":{right}}},"expect":{{"return":{expected}}}}}"#
            ),
        )
        .unwrap();
        assert!(winner.contains("\"status\": \"pass\""), "{winner}");
    }

    let winner_id = run_entry_json(
        &program,
        "core_music_conflict_winner_id",
        &format!(
            r#"{{"args":{{"left":{first},"right":{{"$struct":"MusicCue","version":1,"cue_id":8,"action_id":2,"track":0,"start_tick":199,"duration_ticks":10,"priority":20}}}},"expect":{{"return":8}}}}"#
        ),
    )
    .unwrap();
    assert!(winner_id.contains("\"status\": \"pass\""), "{winner_id}");

    let unknown_version = run_entry_json(
        &program,
        "core_music_conflict_winner_id",
        &format!(
            r#"{{"args":{{"left":{first},"right":{{"$struct":"MusicCue","version":2,"cue_id":8,"action_id":2,"track":0,"start_tick":199,"duration_ticks":10,"priority":20}}}},"expect":{{"return":0}}}}"#
        ),
    )
    .unwrap();
    assert!(
        unknown_version.contains("\"status\": \"pass\""),
        "{unknown_version}"
    );
}

#[test]
fn music_scene_actions_are_versioned_bounded_and_linked_to_cues() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib/src/core/music.fgx");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = parse_file(&path, &source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let built = run_entry_json(
        &program,
        "core_music_make_action",
        r#"{"args":{"action_id":1,"kind":2,"target_id":1,"parameter_q16":65536,"seed":7,"max_instances":256},"expect":{"return":{"$struct":"SceneAction","version":1,"action_id":1,"kind":2,"target_id":1,"parameter_q16":65536,"seed":7,"max_instances":256}}}"#,
    )
    .unwrap();
    assert!(built.contains("\"status\": \"pass\""), "{built}");

    let emitter = r#"{"$struct":"SceneAction","version":1,"action_id":1,"kind":2,"target_id":1,"parameter_q16":65536,"seed":7,"max_instances":256}"#;
    let valid = run_entry_json(
        &program,
        "core_music_action_valid",
        &format!(
            r#"{{"args":{{"action":{emitter},"instance_limit":1024}},"expect":{{"return":true}}}}"#
        ),
    )
    .unwrap();
    assert!(valid.contains("\"status\": \"pass\""), "{valid}");

    for action in [
        r#"{"$struct":"SceneAction","version":2,"action_id":1,"kind":2,"target_id":1,"parameter_q16":0,"seed":7,"max_instances":1}"#,
        r#"{"$struct":"SceneAction","version":1,"action_id":0,"kind":2,"target_id":1,"parameter_q16":0,"seed":7,"max_instances":1}"#,
        r#"{"$struct":"SceneAction","version":1,"action_id":1,"kind":5,"target_id":1,"parameter_q16":0,"seed":7,"max_instances":1}"#,
        r#"{"$struct":"SceneAction","version":1,"action_id":1,"kind":2,"target_id":0,"parameter_q16":0,"seed":7,"max_instances":1}"#,
        r#"{"$struct":"SceneAction","version":1,"action_id":1,"kind":2,"target_id":1,"parameter_q16":0,"seed":0,"max_instances":1}"#,
        r#"{"$struct":"SceneAction","version":1,"action_id":1,"kind":2,"target_id":1,"parameter_q16":0,"seed":7,"max_instances":1025}"#,
        r#"{"$struct":"SceneAction","version":1,"action_id":1,"kind":1,"target_id":1,"parameter_q16":0,"seed":0,"max_instances":2}"#,
    ] {
        let invalid = run_entry_json(
            &program,
            "core_music_action_valid",
            &format!(
                r#"{{"args":{{"action":{action},"instance_limit":1024}},"expect":{{"return":false}}}}"#
            ),
        )
        .unwrap();
        assert!(invalid.contains("\"status\": \"pass\""), "{invalid}");
    }

    let cue = r#"{"$struct":"MusicCue","version":1,"cue_id":5,"action_id":1,"track":0,"start_tick":7680,"duration_ticks":1920,"priority":100}"#;
    let linked = run_entry_json(
        &program,
        "core_music_cue_action_valid",
        &format!(
            r#"{{"args":{{"cue":{cue},"action":{emitter},"track_count":4,"timeline_ticks":30720,"instance_limit":1024}},"expect":{{"return":true}}}}"#
        ),
    )
    .unwrap();
    assert!(linked.contains("\"status\": \"pass\""), "{linked}");

    let mismatched = run_entry_json(
        &program,
        "core_music_cue_action_valid",
        &format!(
            r#"{{"args":{{"cue":{cue},"action":{{"$struct":"SceneAction","version":1,"action_id":2,"kind":2,"target_id":1,"parameter_q16":65536,"seed":7,"max_instances":256}},"track_count":4,"timeline_ticks":30720,"instance_limit":1024}},"expect":{{"return":false}}}}"#
        ),
    )
    .unwrap();
    assert!(mismatched.contains("\"status\": \"pass\""), "{mismatched}");
}

#[test]
fn standard_runtime_intrinsics_conform() {
    let source = r#"
fn add_ok() -> i32:
    return core_math_checked_add_i32(20, 22)

fn header_bits(byte: u8) -> u8:
    return core_bits_extract_u8(byte, 5, 3)

fn choose(flag: bool, fallback: u8) -> u8:
    let value: Option[u8] = core_option_some_u8(9)
    if flag:
        return core_option_unwrap_or_u8(value, fallback)
    return core_option_unwrap_or_u8(core_option_none_u8(), fallback)

fn choose_result(flag: bool, fallback: u8) -> u8:
    let value: Result[u8,u8] = core_result_ok_u8(11)
    if flag:
        return core_result_unwrap_or_u8(value, fallback)
    return core_result_unwrap_or_u8(core_result_err_u8(1), fallback)
"#;
    let program = parse_file(Path::new("stdlib_conformance.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let add = run_entry_json(
        &program,
        "add_ok",
        r#"{ "args": {}, "expect": { "return": 42 } }"#,
    )
    .unwrap();
    let bits = run_entry_json(
        &program,
        "header_bits",
        r#"{ "args": { "byte": 97 }, "expect": { "return": 3 } }"#,
    )
    .unwrap();
    let option = run_entry_json(
        &program,
        "choose",
        r#"{ "args": { "flag": false, "fallback": 4 }, "expect": { "return": 4 } }"#,
    )
    .unwrap();
    let result = run_entry_json(
        &program,
        "choose_result",
        r#"{ "args": { "flag": true, "fallback": 4 }, "expect": { "return": 11 } }"#,
    )
    .unwrap();

    assert!(add.contains("\"status\": \"pass\""));
    assert!(bits.contains("\"status\": \"pass\""));
    assert!(option.contains("\"status\": \"pass\""));
    assert!(result.contains("\"status\": \"pass\""));
}

#[test]
fn checked_arithmetic_reports_deterministic_overflow_traps() {
    let source = r#"
fn overflow() -> i32:
    return core_math_checked_add_i32(2147483647, 1)
"#;
    let program = parse_file(Path::new("stdlib_overflow.fgx"), source).unwrap();
    assert_eq!(check(&program), Vec::new());

    let err = run_entry_json(&program, "overflow", r#"{ "args": {} }"#).unwrap_err();
    assert!(err.contains("trap: integer_overflow"));
}
