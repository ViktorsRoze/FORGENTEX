use std::fs;
use std::path::PathBuf;

use crate::graph::{
    diff_graphs, dump_graph_text, graph_context_json, patch_graph, roundtrip_graph, validate_graph,
    view_graph, GraphArtifact,
};

use super::manifest_path;

#[test]
fn graph_dump_validate_context_and_roundtrip_are_deterministic() {
    let source = manifest_path("examples/smoke/inline_tests.fgx");
    let graph_text = dump_graph_text(&source).unwrap();
    assert!(graph_text.contains("FGGRAPH schema=\"forgentex.graph.v1\""));
    assert!(graph_text.contains("SYMBOL kind=\"function\" name=\"clamp_byte\""));
    assert!(graph_text
        .contains("tests=\"@test.clamp_high,@test.clamp_low,@test.safe_assembly_rejected\""));
    assert!(graph_text.contains("DIAGNOSTIC code=\"unsafe_required\""));

    let parsed = GraphArtifact::from_text(&graph_text).unwrap();
    assert_eq!(parsed.schema, "forgentex.graph.v1");
    assert_eq!(parsed.modules.len(), 1);
    assert!(parsed
        .symbols
        .iter()
        .any(|symbol| symbol.id == "@test.safe_assembly_rejected"));

    let graph_path = temp_file("stage31_inline.fggraph");
    fs::write(&graph_path, graph_text).unwrap();
    let validation = validate_graph(&graph_path);
    assert_eq!(validation.status, "pass");
    assert_eq!(validation.modules, 1);

    let context =
        graph_context_json(&graph_path, "@function.clamp_byte", Some("effects,tests")).unwrap();
    assert!(context.contains("\"schema\": \"forgentex.graph_context.v1\""));
    assert!(context.contains("\"id\": \"@function.clamp_byte\""));
    assert!(context.contains("@test.clamp_high"));

    let agent = view_graph(&graph_path, "fgai").unwrap();
    assert!(agent.contains("TEST \"clamp high\""));

    let roundtrip = roundtrip_graph(&source).unwrap();
    assert_eq!(roundtrip.status, "pass");
    assert_eq!(roundtrip.modules, 1);
    let _ = fs::remove_file(graph_path);
}

#[test]
fn graph_patch_and_diff_report_review_consequences() {
    let graph_path = temp_file("stage31_audio.fggraph");
    let patched_path = temp_file("stage31_audio_patched.fggraph");
    let graph_text = dump_graph_text(&manifest_path("examples/media/audio_mix.fgx")).unwrap();
    fs::write(&graph_path, graph_text).unwrap();

    let report = patch_graph(
        &graph_path,
        &manifest_path("patches/add_fast_path.fgp"),
        Some(&patched_path),
    )
    .unwrap();
    assert_eq!(report.status, "pass");
    assert!(report.canonical_ast_equal_source_patch);
    assert_eq!(report.patched_graph, Some(display_path(&patched_path)));

    let validation = validate_graph(&patched_path);
    assert_eq!(validation.status, "pass");

    let diff = diff_graphs(&graph_path, &patched_path).unwrap();
    assert_eq!(diff.status, "different");
    assert!(diff
        .added_symbols
        .iter()
        .any(|symbol| symbol == "@function.mix_samples_avx2"));
    assert!(diff
        .review_consequences
        .iter()
        .any(|consequence| consequence == "unsafe_changed"));
    assert!(diff
        .review_consequences
        .iter()
        .any(|consequence| consequence == "capability_changed"));

    let _ = fs::remove_file(graph_path);
    let _ = fs::remove_file(patched_path);
}

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_{}", std::process::id(), name))
}

fn display_path(path: &std::path::Path) -> String {
    path.display().to_string().replace('\\', "/")
}
