use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::diag::Diagnostic;
use crate::format::{format_agent, format_human};
use crate::lexer::lex;
use crate::parser::{parse_file, parse_file_detailed};
use crate::patch::diagnose_patch_source;
use crate::simd::check_simd_fallbacks;
use crate::span::{ParseDiagnostic, SourceMap, SourceSpan};
use crate::targets::check_program_target;
use crate::typecheck::check;

#[derive(Clone, Debug, Default)]
pub struct LspServer {
    documents: BTreeMap<String, LspDocument>,
    target: Option<String>,
    shutdown_requested: bool,
    exit_requested: bool,
}

#[derive(Clone, Debug)]
struct LspDocument {
    uri: String,
    language_id: String,
    text: String,
    version: Option<i64>,
}

#[derive(Clone, Debug)]
struct LspDiagnostic {
    range: LspRange,
    code: String,
    message: String,
    node: Option<String>,
    suggested_patches: Vec<String>,
}

#[derive(Clone, Debug)]
struct LspSymbol {
    name: String,
    node_id: String,
    kind: u8,
    range: LspRange,
    selection_range: LspRange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LspPosition {
    line: usize,
    character: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Clone, Debug)]
struct SemanticToken {
    line: usize,
    start: usize,
    len: usize,
    kind: usize,
}

impl LspServer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_target(target: impl Into<String>) -> Self {
        Self {
            target: Some(target.into()),
            ..Self::default()
        }
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn should_exit(&self) -> bool {
        self.exit_requested
    }

    pub fn handle_message(&mut self, message: &str) -> Vec<String> {
        let method = json_string_field(message, "method");
        let id = json_raw_field(message, "id");
        match method.as_deref() {
            Some("initialize") => {
                if let Some(target) = json_string_field(message, "target") {
                    self.target = Some(target);
                }
                vec![response(
                    id.as_deref().unwrap_or("null"),
                    &initialize_result_json(),
                )]
            }
            Some("initialized") => Vec::new(),
            Some("shutdown") => {
                self.shutdown_requested = true;
                vec![response(id.as_deref().unwrap_or("null"), "null")]
            }
            Some("exit") => {
                self.exit_requested = true;
                Vec::new()
            }
            Some("workspace/didChangeConfiguration") | Some("forgentex/setTarget") => {
                if let Some(target) = json_string_field(message, "target") {
                    self.target = Some(target);
                    return self.publish_all_diagnostics();
                }
                Vec::new()
            }
            Some("textDocument/didOpen") => self.did_open(message),
            Some("textDocument/didChange") => self.did_change(message),
            Some("textDocument/hover") => self.request_hover(id.as_deref(), message),
            Some("textDocument/documentSymbol") => {
                self.request_document_symbols(id.as_deref(), message)
            }
            Some("textDocument/definition") => self.request_definition(id.as_deref(), message),
            Some("textDocument/rename") => self.request_rename(id.as_deref(), message),
            Some("textDocument/formatting") => self.request_formatting(id.as_deref(), message),
            Some("textDocument/codeAction") => self.request_code_actions(id.as_deref(), message),
            Some("textDocument/semanticTokens/full") => {
                self.request_semantic_tokens(id.as_deref(), message)
            }
            Some(other) if id.is_some() => vec![error_response(
                id.as_deref().unwrap_or("null"),
                -32601,
                &format!("method not found: {other}"),
            )],
            _ => Vec::new(),
        }
    }

    fn did_open(&mut self, message: &str) -> Vec<String> {
        let Some(uri) = json_string_field(message, "uri") else {
            return Vec::new();
        };
        let text = json_last_string_field(message, "text").unwrap_or_default();
        let language_id = json_string_field(message, "languageId")
            .unwrap_or_else(|| language_id_for_uri(&uri).to_string());
        let version = json_number_field(message, "version");
        self.documents.insert(
            uri.clone(),
            LspDocument {
                uri: uri.clone(),
                language_id,
                text,
                version,
            },
        );
        vec![publish_diagnostics(
            &uri,
            &self.analyze_uri(&uri).diagnostics,
        )]
    }

    fn did_change(&mut self, message: &str) -> Vec<String> {
        let Some(uri) = json_string_field(message, "uri") else {
            return Vec::new();
        };
        let Some(text) = json_last_string_field(message, "text") else {
            return Vec::new();
        };
        let version = json_number_field(message, "version");
        let language_id = language_id_for_uri(&uri).to_string();
        self.documents
            .entry(uri.clone())
            .and_modify(|document| {
                document.text = text.clone();
                document.version = version;
            })
            .or_insert(LspDocument {
                uri: uri.clone(),
                language_id,
                text,
                version,
            });
        vec![publish_diagnostics(
            &uri,
            &self.analyze_uri(&uri).diagnostics,
        )]
    }

    fn request_hover(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some((uri, position)) = uri_and_position(message) else {
            return vec![response(id, "null")];
        };
        let Some(document) = self.documents.get(&uri) else {
            return vec![response(id, "null")];
        };
        let symbols = symbols_for_text(&document.text);
        let hover = hover_json(document, position, &symbols).unwrap_or_else(|| "null".to_string());
        vec![response(id, &hover)]
    }

    fn request_document_symbols(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some(uri) = json_string_field(message, "uri") else {
            return vec![response(id, "[]")];
        };
        let symbols = self
            .documents
            .get(&uri)
            .map(|document| symbols_for_text(&document.text))
            .unwrap_or_default();
        vec![response(id, &document_symbols_json(&symbols))]
    }

    fn request_definition(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some((uri, position)) = uri_and_position(message) else {
            return vec![response(id, "null")];
        };
        let Some(document) = self.documents.get(&uri) else {
            return vec![response(id, "null")];
        };
        let symbols = symbols_for_text(&document.text);
        let Some(word) = word_at_position(&document.text, position) else {
            return vec![response(id, "null")];
        };
        let target = normalize_symbol_word(&word);
        let location = symbols
            .iter()
            .find(|symbol| symbol.name == target || symbol.node_id == target)
            .map(|symbol| location_json(&document.uri, symbol.selection_range))
            .unwrap_or_else(|| "null".to_string());
        vec![response(id, &location)]
    }

    fn request_rename(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some((uri, position)) = uri_and_position(message) else {
            return vec![response(id, "null")];
        };
        let Some(new_name) = json_string_field(message, "newName") else {
            return vec![response(id, "null")];
        };
        let Some(document) = self.documents.get(&uri) else {
            return vec![response(id, "null")];
        };
        let Some(old_name) = word_at_position(&document.text, position) else {
            return vec![response(id, "null")];
        };
        let ranges = exact_word_ranges(&document.text, &old_name);
        vec![response(
            id,
            &workspace_edit_json(&document.uri, &ranges, &new_name),
        )]
    }

    fn request_formatting(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some(uri) = json_string_field(message, "uri") else {
            return vec![response(id, "[]")];
        };
        let Some(document) = self.documents.get(&uri) else {
            return vec![response(id, "[]")];
        };
        if document_kind(document) == DocumentKind::Patch {
            return vec![response(id, "[]")];
        }
        let path = uri_to_path(&document.uri);
        let Ok(program) = parse_file(Path::new(&path), &document.text) else {
            return vec![response(id, "[]")];
        };
        let rendered = if document_kind(document) == DocumentKind::Agent {
            format_agent(&program)
        } else {
            format_human(&program)
        };
        let edit = text_edit_json(full_document_range(&document.text), &rendered);
        vec![response(id, &format!("[{edit}]"))]
    }

    fn request_code_actions(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some(uri) = json_string_field(message, "uri") else {
            return vec![response(id, "[]")];
        };
        let analysis = self.analyze_uri(&uri);
        vec![response(
            id,
            &code_actions_json(&uri, &analysis.diagnostics),
        )]
    }

    fn request_semantic_tokens(&self, id: Option<&str>, message: &str) -> Vec<String> {
        let Some(id) = id else {
            return Vec::new();
        };
        let Some(uri) = json_string_field(message, "uri") else {
            return vec![response(id, "{\"data\":[]}")];
        };
        let data = self
            .documents
            .get(&uri)
            .map(|document| semantic_tokens_json(&document.text))
            .unwrap_or_else(|| "{\"data\":[]}".to_string());
        vec![response(id, &data)]
    }

    fn publish_all_diagnostics(&self) -> Vec<String> {
        self.documents
            .keys()
            .map(|uri| publish_diagnostics(uri, &self.analyze_uri(uri).diagnostics))
            .collect()
    }

    fn analyze_uri(&self, uri: &str) -> Analysis {
        self.documents
            .get(uri)
            .map(|document| self.analyze_document(document))
            .unwrap_or_default()
    }

    fn analyze_document(&self, document: &LspDocument) -> Analysis {
        match document_kind(document) {
            DocumentKind::Patch => analyze_patch_document(document),
            DocumentKind::Human | DocumentKind::Agent | DocumentKind::Map => {
                analyze_source_document(document, self.target.as_deref())
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Analysis {
    diagnostics: Vec<LspDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentKind {
    Human,
    Agent,
    Patch,
    Map,
}

pub fn run_stdio() -> Result<(), String> {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut server = LspServer::new();

    while let Some(message) = read_lsp_message(&mut reader)? {
        for response in server.handle_message(&message) {
            write_lsp_message(&mut writer, &response)?;
        }
        writer.flush().map_err(|err| err.to_string())?;
        if server.should_exit() {
            break;
        }
    }

    Ok(())
}

pub fn server_info_json() -> String {
    format!(
        "{{\n  \"name\": \"forgentex-lsp\",\n  \"schema\": \"forgentex.lsp.v1\",\n  \"transport\": \"stdio\",\n  \"capabilities\": {}\n}}",
        capabilities_json()
    )
}

pub fn capabilities_json() -> String {
    "{\
\"textDocumentSync\":2,\
\"hoverProvider\":true,\
\"definitionProvider\":true,\
\"renameProvider\":{\"prepareProvider\":false},\
\"documentFormattingProvider\":true,\
\"documentSymbolProvider\":true,\
\"codeActionProvider\":{\"codeActionKinds\":[\"quickfix\",\"refactor.rewrite\",\"source\"]},\
\"semanticTokensProvider\":{\"legend\":{\"tokenTypes\":[\"keyword\",\"function\",\"type\",\"parameter\",\"variable\",\"property\",\"string\",\"number\",\"operator\",\"comment\",\"macro\"],\"tokenModifiers\":[]},\"full\":true},\
\"experimental\":{\"nodeIdNavigation\":true,\"semanticPatchCodeActions\":true,\"targetAwareDiagnostics\":true}\
}"
    .to_string()
}

fn initialize_result_json() -> String {
    format!(
        "{{\"serverInfo\":{{\"name\":\"forgentex-lsp\",\"version\":\"0.1.0\"}},\"capabilities\":{}}}",
        capabilities_json()
    )
}

fn analyze_source_document(document: &LspDocument, target: Option<&str>) -> Analysis {
    let path = uri_to_path(&document.uri);
    let output = parse_file_detailed(Path::new(&path), &document.text);
    let mut diagnostics = Vec::new();
    for diagnostic in output.diagnostics {
        diagnostics.push(parse_diagnostic_to_lsp(&diagnostic));
    }
    let Some(program) = output.program else {
        return Analysis { diagnostics };
    };

    for diagnostic in check(&program) {
        diagnostics.push(type_diagnostic_to_lsp(&diagnostic, &output.source_map));
    }
    if let Some(triple) = target {
        for diagnostic in check_program_target(&program, triple)
            .into_iter()
            .chain(check_simd_fallbacks(&program, triple))
        {
            diagnostics.push(type_diagnostic_to_lsp(&diagnostic, &output.source_map));
        }
    }
    Analysis { diagnostics }
}

fn analyze_patch_document(document: &LspDocument) -> Analysis {
    let path = uri_to_path(&document.uri);
    let lexed = lex(&path, &document.text);
    let mut diagnostics = Vec::new();
    for diagnostic in lexed
        .diagnostics
        .into_iter()
        .chain(diagnose_patch_source(&path, &document.text))
    {
        diagnostics.push(parse_diagnostic_to_lsp(&diagnostic));
    }
    Analysis { diagnostics }
}

fn parse_diagnostic_to_lsp(diagnostic: &ParseDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: diagnostic
            .span
            .as_ref()
            .map(span_to_range)
            .unwrap_or_else(|| LspRange::single_line(0, 0, diagnostic.message.len().max(1))),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        node: None,
        suggested_patches: diagnostic.suggested_patches.clone(),
    }
}

fn type_diagnostic_to_lsp(diagnostic: &Diagnostic, source_map: &SourceMap) -> LspDiagnostic {
    LspDiagnostic {
        range: range_for_node(source_map, &diagnostic.node)
            .unwrap_or_else(|| LspRange::single_line(0, 0, 1)),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        node: Some(diagnostic.node.clone()),
        suggested_patches: diagnostic.suggested_patches.clone(),
    }
}

fn range_for_node(source_map: &SourceMap, node: &str) -> Option<LspRange> {
    source_map
        .nodes
        .iter()
        .find(|entry| entry.node == node)
        .map(|entry| span_to_range(&entry.span))
}

fn span_to_range(span: &SourceSpan) -> LspRange {
    LspRange {
        start: LspPosition {
            line: span.start_line.saturating_sub(1),
            character: span.start_col.saturating_sub(1),
        },
        end: LspPosition {
            line: span.end_line.saturating_sub(1),
            character: span.end_col.saturating_sub(1),
        },
    }
}

impl LspRange {
    fn single_line(line: usize, start: usize, end: usize) -> Self {
        Self {
            start: LspPosition {
                line,
                character: start,
            },
            end: LspPosition {
                line,
                character: end,
            },
        }
    }
}

fn document_kind(document: &LspDocument) -> DocumentKind {
    if document.language_id.contains("fgp") || document.uri.ends_with(".fgp") {
        DocumentKind::Patch
    } else if document.language_id.contains("fgai") || document.uri.ends_with(".fgai") {
        DocumentKind::Agent
    } else if document.language_id.contains("fgmap") || document.uri.ends_with(".fgmap") {
        DocumentKind::Map
    } else {
        DocumentKind::Human
    }
}

fn language_id_for_uri(uri: &str) -> &'static str {
    if uri.ends_with(".fgai") {
        "forgentex-fgai"
    } else if uri.ends_with(".fgp") {
        "forgentex-fgp"
    } else if uri.ends_with(".fgmap") {
        "forgentex-fgmap"
    } else {
        "forgentex-fgx"
    }
}

fn hover_json(
    document: &LspDocument,
    position: LspPosition,
    symbols: &[LspSymbol],
) -> Option<String> {
    let line = document.text.lines().nth(position.line).unwrap_or_default();
    if let Some(symbol) = symbols
        .iter()
        .find(|symbol| range_contains(symbol.selection_range, position))
    {
        return Some(markdown_hover_json(&format!(
            "**{}**\n\nStable node ID: `{}`",
            symbol.name, symbol.node_id
        )));
    }
    if let Some(word) = word_at_position(&document.text, position) {
        if word.starts_with("@function.") || word.starts_with("@struct.") {
            return Some(markdown_hover_json(&format!(
                "Forgentex semantic target: `{word}`"
            )));
        }
        if let Some(symbol) = symbols.iter().find(|symbol| symbol.name == word) {
            return Some(markdown_hover_json(&format!(
                "**{}**\n\nStable node ID: `{}`",
                symbol.name, symbol.node_id
            )));
        }
    }
    line.find("TARGET @").map(|index| {
        let target = line[index + "TARGET ".len()..].trim();
        markdown_hover_json(&format!("Semantic patch target: `{target}`"))
    })
}

fn markdown_hover_json(contents: &str) -> String {
    format!(
        "{{\"contents\":{{\"kind\":\"markdown\",\"value\":{}}}}}",
        json_string(contents)
    )
}

fn range_contains(range: LspRange, position: LspPosition) -> bool {
    position.line == range.start.line
        && position.character >= range.start.character
        && position.character <= range.end.character
}

fn symbols_for_text(text: &str) -> Vec<LspSymbol> {
    let mut symbols = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        if let Some(symbol) = symbol_for_line(line_index, line) {
            symbols.push(symbol);
        }
    }
    symbols
}

fn symbol_for_line(line_index: usize, line: &str) -> Option<LspSymbol> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if let Some(rest) = trimmed.strip_prefix("unsafe fn ") {
        return function_symbol(line_index, indent + "unsafe fn ".len(), rest);
    }
    if let Some(rest) = trimmed.strip_prefix("fn ") {
        return function_symbol(line_index, indent + "fn ".len(), rest);
    }
    if let Some(rest) = trimmed.strip_prefix("UF ") {
        return function_symbol(line_index, indent + "UF ".len(), rest);
    }
    if let Some(rest) = trimmed.strip_prefix("F ") {
        return function_symbol(line_index, indent + "F ".len(), rest);
    }
    if let Some(rest) = trimmed.strip_prefix("struct ") {
        return struct_symbol(line_index, indent + "struct ".len(), rest);
    }
    if let Some(rest) = trimmed.strip_prefix("S ") {
        return struct_symbol(line_index, indent + "S ".len(), rest);
    }
    if let Some(rest) = trimmed.strip_prefix("TARGET ") {
        let target = rest.split_whitespace().next().unwrap_or(rest);
        let range = LspRange::single_line(
            line_index,
            indent + "TARGET ".len(),
            indent + "TARGET ".len() + target.len(),
        );
        return Some(LspSymbol {
            name: target.to_string(),
            node_id: target.to_string(),
            kind: 19,
            range,
            selection_range: range,
        });
    }
    None
}

fn function_symbol(line_index: usize, start: usize, rest: &str) -> Option<LspSymbol> {
    let name = identifier_prefix(rest)?;
    let range = LspRange::single_line(line_index, start, start + name.len());
    Some(LspSymbol {
        name: name.to_string(),
        node_id: format!("@function.{name}"),
        kind: 12,
        range,
        selection_range: range,
    })
}

fn struct_symbol(line_index: usize, start: usize, rest: &str) -> Option<LspSymbol> {
    let name = identifier_prefix(rest)?;
    let range = LspRange::single_line(line_index, start, start + name.len());
    Some(LspSymbol {
        name: name.to_string(),
        node_id: format!("@struct.{name}"),
        kind: 23,
        range,
        selection_range: range,
    })
}

fn identifier_prefix(value: &str) -> Option<&str> {
    let len = value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_')
        .map(|(index, ch)| index + ch.len_utf8())
        .last()?;
    Some(&value[..len])
}

fn normalize_symbol_word(word: &str) -> String {
    word.strip_prefix("@function.")
        .or_else(|| word.strip_prefix("@struct."))
        .unwrap_or(word)
        .to_string()
}

fn document_symbols_json(symbols: &[LspSymbol]) -> String {
    let mut out = String::from("[");
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":{},\"detail\":{},\"kind\":{},\"range\":{},\"selectionRange\":{}}}",
            json_string(&symbol.name),
            json_string(&symbol.node_id),
            symbol.kind,
            range_json(symbol.range),
            range_json(symbol.selection_range)
        ));
    }
    out.push(']');
    out
}

fn publish_diagnostics(uri: &str, diagnostics: &[LspDiagnostic]) -> String {
    let mut out = String::new();
    out.push_str(
        "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{",
    );
    out.push_str(&format!("\"uri\":{},\"diagnostics\":[", json_string(uri)));
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&diagnostic_json(diagnostic));
    }
    out.push_str("]}}");
    out
}

fn diagnostic_json(diagnostic: &LspDiagnostic) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{{\"range\":{},\"severity\":1,\"source\":\"fgx\",\"code\":{},\"message\":{}",
        range_json(diagnostic.range),
        json_string(&diagnostic.code),
        json_string(&diagnostic.message)
    ));
    if diagnostic.node.is_some() || !diagnostic.suggested_patches.is_empty() {
        out.push_str(",\"data\":{");
        let mut wrote = false;
        if let Some(node) = &diagnostic.node {
            out.push_str(&format!("\"node\":{}", json_string(node)));
            wrote = true;
        }
        if !diagnostic.suggested_patches.is_empty() {
            if wrote {
                out.push(',');
            }
            out.push_str("\"suggested_patches\":[");
            for (index, patch) in diagnostic.suggested_patches.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&json_string(patch));
            }
            out.push(']');
        }
        out.push('}');
    }
    out.push('}');
    out
}

fn code_actions_json(uri: &str, diagnostics: &[LspDiagnostic]) -> String {
    let mut actions = Vec::new();
    for diagnostic in diagnostics {
        for suggestion in &diagnostic.suggested_patches {
            actions.push(format!(
                "{{\"title\":{},\"kind\":\"quickfix\",\"command\":{{\"title\":\"Create semantic patch\",\"command\":\"forgentex.createSemanticPatch\",\"arguments\":[{},{},{}]}}}}",
                json_string(&format!("Forgentex: apply suggestion {suggestion}")),
                json_string(uri),
                json_string(diagnostic.node.as_deref().unwrap_or("@program")),
                json_string(suggestion)
            ));
        }
    }
    actions.push(format!(
        "{{\"title\":\"Forgentex: create semantic patch for selected node\",\"kind\":\"refactor.rewrite\",\"command\":{{\"title\":\"Create semantic patch\",\"command\":\"forgentex.createSemanticPatch\",\"arguments\":[{}]}}}}",
        json_string(uri)
    ));
    actions.push(format!(
        "{{\"title\":\"Forgentex: run target-aware diagnostics\",\"kind\":\"source\",\"command\":{{\"title\":\"Run target diagnostics\",\"command\":\"forgentex.runTargetDiagnostics\",\"arguments\":[{}]}}}}",
        json_string(uri)
    ));
    format!("[{}]", actions.join(","))
}

fn location_json(uri: &str, range: LspRange) -> String {
    format!(
        "{{\"uri\":{},\"range\":{}}}",
        json_string(uri),
        range_json(range)
    )
}

fn workspace_edit_json(uri: &str, ranges: &[LspRange], new_name: &str) -> String {
    let mut out = format!("{{\"changes\":{{{}:[", json_string(uri));
    for (index, range) in ranges.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&text_edit_json(*range, new_name));
    }
    out.push_str("]}}}");
    out
}

fn text_edit_json(range: LspRange, new_text: &str) -> String {
    format!(
        "{{\"range\":{},\"newText\":{}}}",
        range_json(range),
        json_string(new_text)
    )
}

fn range_json(range: LspRange) -> String {
    format!(
        "{{\"start\":{},\"end\":{}}}",
        position_json(range.start),
        position_json(range.end)
    )
}

fn position_json(position: LspPosition) -> String {
    format!(
        "{{\"line\":{},\"character\":{}}}",
        position.line, position.character
    )
}

fn full_document_range(text: &str) -> LspRange {
    let mut line_count = 0;
    let mut last_len = 0;
    for line in text.lines() {
        line_count += 1;
        last_len = line.len();
    }
    if line_count == 0 {
        LspRange::single_line(0, 0, 0)
    } else {
        LspRange {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: line_count - 1,
                character: last_len,
            },
        }
    }
}

fn semantic_tokens_json(text: &str) -> String {
    let mut tokens = semantic_tokens(text);
    tokens.sort_by_key(|token| (token.line, token.start));
    let mut previous_line = 0;
    let mut previous_start = 0;
    let mut data = Vec::new();
    for token in tokens {
        let delta_line = token.line.saturating_sub(previous_line);
        let delta_start = if delta_line == 0 {
            token.start.saturating_sub(previous_start)
        } else {
            token.start
        };
        data.push(format!(
            "{delta_line},{delta_start},{},{},0",
            token.len, token.kind
        ));
        previous_line = token.line;
        previous_start = token.start;
    }
    format!("{{\"data\":[{}]}}", data.join(","))
}

fn semantic_tokens(text: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let words = words_with_positions(line);
        for (word_index, (start, word)) in words.iter().enumerate() {
            let kind = if is_keyword(word) {
                Some(0)
            } else if is_builtin_type(word) || word.chars().next().is_some_and(char::is_uppercase) {
                Some(2)
            } else if word.starts_with("@")
                || word.contains('.')
                || is_architecture_label(word)
                || is_effect_label(word)
            {
                Some(10)
            } else if word_index > 0
                && matches!(words[word_index - 1].1.as_str(), "fn" | "F" | "UF")
            {
                Some(1)
            } else if word_index > 0 && words[word_index - 1].1 == "struct" {
                Some(2)
            } else {
                None
            };
            if let Some(kind) = kind {
                tokens.push(SemanticToken {
                    line: line_index,
                    start: *start,
                    len: word.len(),
                    kind,
                });
            }
        }
    }
    tokens
}

fn words_with_positions(line: &str) -> Vec<(usize, String)> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, ch) in line.char_indices() {
        if is_symbol_char(ch) {
            start.get_or_insert(index);
        } else if let Some(word_start) = start.take() {
            words.push((word_start, line[word_start..index].to_string()));
        }
    }
    if let Some(word_start) = start {
        words.push((word_start, line[word_start..].to_string()));
    }
    words
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "struct"
            | "fn"
            | "unsafe"
            | "let"
            | "return"
            | "require"
            | "if"
            | "else"
            | "for"
            | "in"
            | "effects"
            | "asm"
            | "S"
            | "F"
            | "UF"
            | "LET"
            | "R"
            | "REQ"
            | "IF"
            | "FOR"
            | "ASM"
            | "VERSION"
            | "AUTHOR"
            | "TARGET"
            | "OP"
            | "PRECONDITION"
            | "SIGNED_BY"
            | "SIGNATURE"
    )
}

fn is_builtin_type(word: &str) -> bool {
    matches!(
        word,
        "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "f32"
            | "f64"
            | "void"
    )
}

fn is_architecture_label(word: &str) -> bool {
    matches!(
        word,
        "x86_64" | "arm" | "armv7" | "aarch64" | "riscv32" | "riscv64" | "risc-v"
    )
}

fn is_effect_label(word: &str) -> bool {
    word.starts_with("cpu.")
        || word.starts_with("target.")
        || word.starts_with("crypto.")
        || word.starts_with("network.")
        || word.starts_with("fs.")
        || word.starts_with("agent.")
}

fn uri_and_position(message: &str) -> Option<(String, LspPosition)> {
    Some((json_string_field(message, "uri")?, position_field(message)?))
}

fn position_field(message: &str) -> Option<LspPosition> {
    let position_index = message.find("\"position\"")?;
    Some(LspPosition {
        line: json_number_field_after(message, "line", position_index)? as usize,
        character: json_number_field_after(message, "character", position_index)? as usize,
    })
}

fn word_at_position(text: &str, position: LspPosition) -> Option<String> {
    let line = text.lines().nth(position.line)?;
    let mut start = position.character.min(line.len());
    while start > 0 {
        let previous = line[..start].chars().next_back()?;
        if !is_symbol_char(previous) {
            break;
        }
        start -= previous.len_utf8();
    }
    let mut end = position.character.min(line.len());
    while end < line.len() {
        let ch = line[end..].chars().next()?;
        if !is_symbol_char(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    (start < end).then(|| line[start..end].to_string())
}

fn exact_word_ranges(text: &str, word: &str) -> Vec<LspRange> {
    let mut ranges = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let mut offset = 0;
        while let Some(relative) = line[offset..].find(word) {
            let start = offset + relative;
            let end = start + word.len();
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            if before.is_none_or(|ch| !is_symbol_char(ch))
                && after.is_none_or(|ch| !is_symbol_char(ch))
            {
                ranges.push(LspRange::single_line(line_index, start, end));
            }
            offset = end;
        }
    }
    ranges
}

fn is_symbol_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '@' | '.')
}

fn read_lsp_message<R: BufRead>(reader: &mut R) -> Result<Option<String>, String> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|err| format!("invalid Content-Length: {err}"))?,
            );
        }
    }
    let Some(length) = content_length else {
        return Err("missing Content-Length header".to_string());
    };
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|err| format!("failed to read LSP body: {err}"))?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|err| err.to_string())
}

fn write_lsp_message<W: Write>(writer: &mut W, message: &str) -> Result<(), String> {
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        message.len(),
        message
    )
    .map_err(|err| err.to_string())
}

fn response(id: &str, result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}")
}

fn error_response(id: &str, code: i64, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":{}}}}}",
        json_string(message)
    )
}

fn json_raw_field(source: &str, field: &str) -> Option<String> {
    let field_index = find_field_index(source, field)?;
    let colon = source[field_index..].find(':')? + field_index;
    let mut index = colon + 1;
    skip_ws(source, &mut index);
    if source[index..].starts_with('"') {
        let (_, end) = parse_json_string_at(source, index)?;
        return Some(source[index..end].to_string());
    }
    let end = source[index..]
        .find([',', '}'])
        .map(|relative| index + relative)
        .unwrap_or(source.len());
    Some(source[index..end].trim().to_string())
}

fn json_string_field(source: &str, field: &str) -> Option<String> {
    json_string_field_after(source, field, 0)
}

fn json_last_string_field(source: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{field}\"");
    let mut cursor = 0;
    let mut found = None;
    while let Some(relative) = source[cursor..].find(&pattern) {
        let index = cursor + relative;
        found = json_string_field_after(source, field, index);
        cursor = index + pattern.len();
    }
    found
}

fn json_string_field_after(source: &str, field: &str, start: usize) -> Option<String> {
    let field_index = find_field_index(&source[start..], field)? + start;
    let colon = source[field_index..].find(':')? + field_index;
    let mut index = colon + 1;
    skip_ws(source, &mut index);
    if !source[index..].starts_with('"') {
        return None;
    }
    parse_json_string_at(source, index).map(|(value, _)| value)
}

fn json_number_field(source: &str, field: &str) -> Option<i64> {
    json_number_field_after(source, field, 0)
}

fn json_number_field_after(source: &str, field: &str, start: usize) -> Option<i64> {
    let field_index = find_field_index(&source[start..], field)? + start;
    let colon = source[field_index..].find(':')? + field_index;
    let mut index = colon + 1;
    skip_ws(source, &mut index);
    let end = source[index..]
        .find(|ch: char| !ch.is_ascii_digit() && ch != '-')
        .map(|relative| index + relative)
        .unwrap_or(source.len());
    source[index..end].parse().ok()
}

fn find_field_index(source: &str, field: &str) -> Option<usize> {
    source.find(&format!("\"{field}\""))
}

fn parse_json_string_at(source: &str, quote_index: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut escaped = false;
    let mut chars = source[quote_index + 1..].char_indices();
    while let Some((relative, ch)) = chars.next() {
        let absolute = quote_index + 1 + relative;
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex = source.get(absolute + 1..absolute + 5)?;
                    let value = u16::from_str_radix(hex, 16).ok()?;
                    out.push(char::from_u32(value as u32)?);
                    for _ in 0..4 {
                        chars.next();
                    }
                }
                other => out.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some((out, absolute + 1));
        } else {
            out.push(ch);
        }
    }
    None
}

fn skip_ws(source: &str, index: &mut usize) {
    while source[*index..]
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
    {
        *index += source[*index..].chars().next().unwrap().len_utf8();
    }
}

fn uri_to_path(uri: &str) -> String {
    let mut path = uri.strip_prefix("file://").unwrap_or(uri).to_string();
    if path.len() > 3 && path.as_bytes()[0] == b'/' && path.as_bytes()[2] == b':' {
        path.remove(0);
    }
    percent_decode(&path)
}

fn percent_decode(value: &str) -> String {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(hex as char);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
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
            other if other.is_control() => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
