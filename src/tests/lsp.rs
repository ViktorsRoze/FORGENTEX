use crate::lsp::{capabilities_json, server_info_json, LspServer};

use super::fixture;

#[test]
fn lsp_initialize_advertises_stage21_capabilities() {
    let mut server = LspServer::new();
    let responses = server.handle_message(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"initializationOptions":{"target":"aarch64-unknown-linux-gnu"}}}"#,
    );
    assert_eq!(responses.len(), 1);
    let response = &responses[0];
    assert!(response.contains("\"hoverProvider\":true"));
    assert!(response.contains("\"definitionProvider\":true"));
    assert!(response.contains("\"renameProvider\""));
    assert!(response.contains("\"documentFormattingProvider\":true"));
    assert!(response.contains("\"semanticTokensProvider\""));
    assert!(response.contains("\"semanticPatchCodeActions\":true"));
    assert_eq!(server.target(), Some("aarch64-unknown-linux-gnu"));

    let probe = server_info_json();
    assert!(probe.contains("forgentex.lsp.v1"));
    assert!(capabilities_json().contains("targetAwareDiagnostics"));
}

#[test]
fn lsp_serves_fgx_symbols_hover_formatting_semantic_tokens_definition_and_rename() {
    let source = fixture("examples/media/audio_mix.fgx");
    let uri = "file:///examples/media/audio_mix.fgx";
    let mut server = LspServer::new();

    let open = server.handle_message(&did_open(uri, "forgentex-fgx", source));
    assert_eq!(open.len(), 1);
    assert!(open[0].contains("textDocument/publishDiagnostics"));
    assert!(open[0].contains("\"diagnostics\":[]"));

    let symbols = server.handle_message(&request(2, "textDocument/documentSymbol", uri, ""));
    assert!(symbols[0].contains("@function.mix_samples"));
    assert!(symbols[0].contains("@struct.AudioBlock"));

    let line = line_of(source, "fn mix_samples");
    let character = source
        .lines()
        .nth(line)
        .unwrap()
        .find("mix_samples")
        .unwrap();
    let hover = server.handle_message(&position_request(
        3,
        "textDocument/hover",
        uri,
        line,
        character,
        "",
    ));
    assert!(hover[0].contains("Stable node ID"));
    assert!(hover[0].contains("@function.mix_samples"));

    let formatting = server.handle_message(&request(4, "textDocument/formatting", uri, ""));
    assert!(formatting[0].contains("\"newText\""));
    assert!(formatting[0].contains("fn mix_samples"));

    let tokens = server.handle_message(&request(5, "textDocument/semanticTokens/full", uri, ""));
    assert!(tokens[0].contains("\"data\":["));

    let definition = server.handle_message(&position_request(
        6,
        "textDocument/definition",
        uri,
        line,
        character,
        "",
    ));
    assert!(definition[0].contains("\"uri\""));
    assert!(definition[0].contains("\"range\""));

    let rename = server.handle_message(&position_request(
        7,
        "textDocument/rename",
        uri,
        line,
        character,
        r#","newName":"mix_samples_checked""#,
    ));
    assert!(rename[0].contains("\"changes\""));
    assert!(rename[0].contains("mix_samples_checked"));
}

#[test]
fn lsp_target_aware_diagnostics_cover_architecture_mismatch() {
    let source = fixture("examples/arch/x86_64_popcount.fgx");
    let uri = "file:///examples/arch/x86_64_popcount.fgx";
    let mut server = LspServer::with_target("aarch64-unknown-linux-gnu");
    let responses = server.handle_message(&did_open(uri, "forgentex-fgx", source));
    assert_eq!(responses.len(), 1);
    assert!(responses[0].contains("target_arch_mismatch"));
    assert!(responses[0].contains("x86_64"));
    assert!(responses[0].contains("aarch64-unknown-linux-gnu"));
}

#[test]
fn lsp_serves_fgp_patch_documents_with_hover_symbols_and_code_actions() {
    let patch = fixture("patches/signed_add_crypto_effect.fgp");
    let uri = "file:///patches/signed_add_crypto_effect.fgp";
    let mut server = LspServer::new();
    let open = server.handle_message(&did_open(uri, "forgentex-fgp", patch));
    assert_eq!(open.len(), 1);
    assert!(open[0].contains("textDocument/publishDiagnostics"));

    let symbols = server.handle_message(&request(8, "textDocument/documentSymbol", uri, ""));
    assert!(symbols[0].contains("@function.mix_samples"));

    let line = line_of(patch, "TARGET @function.mix_samples");
    let hover = server.handle_message(&position_request(
        9,
        "textDocument/hover",
        uri,
        line,
        patch.lines().nth(line).unwrap().find("@function").unwrap(),
        "",
    ));
    assert!(hover[0].contains("@function.mix_samples"));

    let actions = server.handle_message(&request(10, "textDocument/codeAction", uri, ""));
    assert!(actions[0].contains("forgentex.createSemanticPatch"));
    assert!(actions[0].contains("forgentex.runTargetDiagnostics"));
}

fn did_open(uri: &str, language_id: &str, text: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/didOpen\",\"params\":{{\"textDocument\":{{\"uri\":{},\"languageId\":{},\"version\":1,\"text\":{}}}}}}}",
        json_string(uri),
        json_string(language_id),
        json_string(text)
    )
}

fn request(id: usize, method: &str, uri: &str, extra: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{{\"textDocument\":{{\"uri\":{}}}{extra}}}}}",
        json_string(method),
        json_string(uri)
    )
}

fn position_request(
    id: usize,
    method: &str,
    uri: &str,
    line: usize,
    character: usize,
    extra: &str,
) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{{\"textDocument\":{{\"uri\":{}}},\"position\":{{\"line\":{line},\"character\":{character}}}{extra}}}}}",
        json_string(method),
        json_string(uri)
    )
}

fn line_of(source: &str, needle: &str) -> usize {
    source
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing line containing {needle}"))
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
