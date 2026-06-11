use crate::lexer::{Comment, Token};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub file: String,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeSpan {
    pub node: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMap {
    pub file: String,
    pub nodes: Vec<NodeSpan>,
    pub comments: Vec<Comment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub suggested_patches: Vec<String>,
}

impl SourceSpan {
    pub fn new(
        file: impl Into<String>,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Self {
        Self {
            file: file.into(),
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    pub fn point(file: impl Into<String>, line: usize, col: usize) -> Self {
        Self::new(file, line, col, line, col)
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"file\":\"{}\",\"start_line\":{},\"start_col\":{},\"end_line\":{},\"end_col\":{}}}",
            escape(&self.file),
            self.start_line,
            self.start_col,
            self.end_line,
            self.end_col
        )
    }
}

impl SourceMap {
    pub fn new(file: impl Into<String>, comments: Vec<Comment>) -> Self {
        Self {
            file: file.into(),
            nodes: Vec::new(),
            comments,
        }
    }

    pub fn add(&mut self, node: impl Into<String>, span: SourceSpan) {
        self.nodes.push(NodeSpan {
            node: node.into(),
            span,
        });
    }

    pub fn sort_and_dedup(&mut self) {
        self.nodes.sort_by(|left, right| {
            left.node
                .cmp(&right.node)
                .then(left.span.start_line.cmp(&right.span.start_line))
                .then(left.span.start_col.cmp(&right.span.start_col))
        });
        self.nodes.dedup_by(|left, right| left.node == right.node);
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"file\": \"{}\",\n", escape(&self.file)));
        out.push_str("  \"nodes\": [");
        for (idx, node) in self.nodes.iter().enumerate() {
            if idx == 0 {
                out.push('\n');
            } else {
                out.push_str(",\n");
            }
            out.push_str(&format!(
                "    {{\"node\":\"{}\",\"span\":{}}}",
                escape(&node.node),
                node.span.to_json()
            ));
        }
        if !self.nodes.is_empty() {
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str("  \"comments\": [");
        for (idx, comment) in self.comments.iter().enumerate() {
            if idx == 0 {
                out.push('\n');
            } else {
                out.push_str(",\n");
            }
            out.push_str(&format!("    {}", comment.to_json()));
        }
        if !self.comments.is_empty() {
            out.push('\n');
        }
        out.push_str("  ]\n}");
        out
    }
}

impl ParseDiagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            span: None,
            suggested_patches: Vec::new(),
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn suggest(mut self, patch: impl Into<String>) -> Self {
        self.suggested_patches.push(patch.into());
        self
    }

    pub fn to_json(&self) -> String {
        let span = self
            .span
            .as_ref()
            .map(SourceSpan::to_json)
            .unwrap_or_else(|| "null".to_string());
        let suggestions = self
            .suggested_patches
            .iter()
            .map(|suggestion| format!("\"{}\"", escape(suggestion)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{{\"error\":\"{}\",\"message\":\"{}\",\"span\":{},\"suggested_patches\":[{}]}}",
            escape(&self.code),
            escape(&self.message),
            span,
            suggestions
        )
    }
}

pub fn diagnostics_to_json(diagnostics: &[ParseDiagnostic]) -> String {
    let mut out = String::new();
    out.push('[');
    for (idx, diagnostic) in diagnostics.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&diagnostic.to_json());
    }
    out.push(']');
    out
}

pub fn tokens_to_json(tokens: &[Token]) -> String {
    let mut out = String::new();
    out.push('[');
    for (idx, token) in tokens.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&token.to_json());
    }
    out.push(']');
    out
}

pub use crate::json::escape;
