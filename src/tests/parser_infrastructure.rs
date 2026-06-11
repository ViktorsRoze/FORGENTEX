use std::path::Path;

use crate::lexer::{lex, CommentKind, TokenKind};
use crate::parser::parse_file_detailed;
use crate::patch::diagnose_patch_source;

#[test]
fn lexer_emits_tokens_with_spans() {
    let lexed = lex(
        "inline.fgx",
        "fn area(width: i32) -> i32:\n    return width\n",
    );
    let fn_token = lexed
        .tokens
        .iter()
        .find(|token| token.lexeme == "fn")
        .expect("fn token");
    assert_eq!(fn_token.kind, TokenKind::Keyword);
    assert_eq!(fn_token.span.start_line, 1);
    assert_eq!(fn_token.span.start_col, 1);
    assert_eq!(fn_token.span.end_col, 3);
}

#[test]
fn comments_docs_and_review_annotations_are_preserved() {
    let source = "## demo docs\n#@ risk @function.area needs-review\nfn area(width: i32, height: i32) -> i32:\n    return width * height\n";
    let output = parse_file_detailed(Path::new("annotated.fgx"), source);
    assert!(output.diagnostics.is_empty());
    assert_eq!(output.source_map.comments.len(), 2);
    assert_eq!(output.source_map.comments[0].kind, CommentKind::Doc);
    assert_eq!(
        output.source_map.comments[1].kind,
        CommentKind::ReviewAnnotation
    );
    assert_eq!(
        output.source_map.comments[1].target.as_deref(),
        Some("@function.area")
    );
}

#[test]
fn source_map_preserves_declaration_statement_and_expression_spans() {
    let source = "fn area(width: i32, height: i32) -> i32:\n    return width * height\n";
    let output = parse_file_detailed(Path::new("area.fgx"), source);
    assert!(output.diagnostics.is_empty());
    let nodes = output
        .source_map
        .nodes
        .iter()
        .map(|node| node.node.as_str())
        .collect::<Vec<_>>();
    assert!(nodes.contains(&"@function.area"));
    assert!(nodes.contains(&"@function.area.param.width"));
    assert!(nodes.contains(&"@function.area.stmt.0"));
    assert!(nodes.contains(&"@function.area.stmt.0.expr.left"));
}

#[test]
fn parse_errors_are_structured_with_repair_hints() {
    let source = "fn bad(x: i32) -> i32:\n    if x > 0\n        return x\n";
    let output = parse_file_detailed(Path::new("bad.fgx"), source);
    assert!(output.program.is_none());
    let diagnostic = output.diagnostics.last().expect("parse diagnostic");
    assert_eq!(diagnostic.code, "missing_colon");
    assert!(diagnostic
        .suggested_patches
        .iter()
        .any(|patch| patch.contains("insert ':'")));
    assert!(diagnostic.span.is_some());
}

#[test]
fn fgp_patch_diagnostics_are_structured() {
    let diagnostics = diagnose_patch_source("bad.fgp", "INSERT_FAST_PATH target=x86_64\n");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "patch_missing_target");
    assert!(diagnostics[0].span.is_some());
}
