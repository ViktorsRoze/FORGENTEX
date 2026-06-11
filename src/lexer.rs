use crate::span::{escape, ParseDiagnostic, SourceSpan};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexedFile {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident,
    Keyword,
    Number,
    String,
    Symbol,
    Operator,
    Newline,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub kind: CommentKind,
    pub text: String,
    pub name: Option<String>,
    pub payload: Option<String>,
    pub target: Option<String>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommentKind {
    Line,
    Doc,
    ReviewAnnotation,
}

pub fn lex(file: &str, source: &str) -> LexedFile {
    let mut tokens = Vec::new();
    let mut comments = Vec::new();
    let mut diagnostics = Vec::new();

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line_no = line_idx + 1;
        let chars: Vec<char> = raw_line.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            let ch = chars[index];
            let col = index + 1;
            if ch.is_whitespace() {
                index += 1;
                continue;
            }
            if ch == '#' {
                let text: String = chars[index..].iter().collect();
                comments.push(parse_comment(file, line_no, col, raw_line.len() + 1, &text));
                break;
            }
            if ch.is_ascii_alphabetic() || ch == '_' {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                let lexeme: String = chars[start..index].iter().collect();
                let kind = if is_keyword(&lexeme) {
                    TokenKind::Keyword
                } else {
                    TokenKind::Ident
                };
                tokens.push(Token {
                    kind,
                    lexeme,
                    span: SourceSpan::new(file, line_no, start + 1, line_no, index + 1),
                });
                continue;
            }
            if ch.is_ascii_digit() {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_digit()
                        || (chars[index] == '.'
                            && index + 1 < chars.len()
                            && chars[index + 1] != '.'))
                {
                    index += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Number,
                    lexeme: chars[start..index].iter().collect(),
                    span: SourceSpan::new(file, line_no, start + 1, line_no, index + 1),
                });
                continue;
            }
            if ch == '"' {
                let start = index;
                index += 1;
                let mut escaped = false;
                while index < chars.len() {
                    let current = chars[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if current == '\\' {
                        escaped = true;
                        continue;
                    }
                    if current == '"' {
                        break;
                    }
                }
                let closed = chars.get(index.saturating_sub(1)) == Some(&'"');
                let span = SourceSpan::new(file, line_no, start + 1, line_no, index + 1);
                tokens.push(Token {
                    kind: if closed {
                        TokenKind::String
                    } else {
                        TokenKind::Unknown
                    },
                    lexeme: chars[start..index].iter().collect(),
                    span: span.clone(),
                });
                if !closed {
                    diagnostics.push(
                        ParseDiagnostic::new("invalid_token", "unterminated string literal")
                            .with_span(span)
                            .suggest("close the string literal"),
                    );
                }
                continue;
            }
            if index + 1 < chars.len() {
                let two: String = chars[index..=index + 1].iter().collect();
                if is_two_char_operator(&two) {
                    tokens.push(Token {
                        kind: TokenKind::Operator,
                        lexeme: two,
                        span: SourceSpan::new(file, line_no, col, line_no, col + 2),
                    });
                    index += 2;
                    continue;
                }
            }
            if is_symbol(ch) {
                tokens.push(Token {
                    kind: TokenKind::Symbol,
                    lexeme: ch.to_string(),
                    span: SourceSpan::new(file, line_no, col, line_no, col + 1),
                });
                index += 1;
                continue;
            }
            if is_single_char_operator(ch) {
                tokens.push(Token {
                    kind: TokenKind::Operator,
                    lexeme: ch.to_string(),
                    span: SourceSpan::new(file, line_no, col, line_no, col + 1),
                });
                index += 1;
                continue;
            }
            let span = SourceSpan::new(file, line_no, col, line_no, col + 1);
            tokens.push(Token {
                kind: TokenKind::Unknown,
                lexeme: ch.to_string(),
                span: span.clone(),
            });
            diagnostics.push(
                ParseDiagnostic::new("invalid_token", format!("invalid token '{ch}'"))
                    .with_span(span)
                    .suggest("remove or replace the invalid character"),
            );
            index += 1;
        }
        tokens.push(Token {
            kind: TokenKind::Newline,
            lexeme: "\\n".to_string(),
            span: SourceSpan::new(
                file,
                line_no,
                raw_line.len() + 1,
                line_no,
                raw_line.len() + 1,
            ),
        });
    }

    LexedFile {
        tokens,
        comments,
        diagnostics,
    }
}

impl Token {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"kind\":\"{}\",\"lexeme\":\"{}\",\"span\":{}}}",
            self.kind.as_str(),
            escape(&self.lexeme),
            self.span.to_json()
        )
    }
}

impl TokenKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TokenKind::Ident => "ident",
            TokenKind::Keyword => "keyword",
            TokenKind::Number => "number",
            TokenKind::String => "string",
            TokenKind::Symbol => "symbol",
            TokenKind::Operator => "operator",
            TokenKind::Newline => "newline",
            TokenKind::Unknown => "unknown",
        }
    }
}

impl Comment {
    pub fn to_json(&self) -> String {
        let name = self
            .name
            .as_ref()
            .map(|value| format!("\"{}\"", escape(value)))
            .unwrap_or_else(|| "null".to_string());
        let payload = self
            .payload
            .as_ref()
            .map(|value| format!("\"{}\"", escape(value)))
            .unwrap_or_else(|| "null".to_string());
        let target = self
            .target
            .as_ref()
            .map(|value| format!("\"{}\"", escape(value)))
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"kind\":\"{}\",\"text\":\"{}\",\"name\":{},\"payload\":{},\"target\":{},\"span\":{}}}",
            self.kind.as_str(),
            escape(&self.text),
            name,
            payload,
            target,
            self.span.to_json()
        )
    }
}

impl CommentKind {
    fn as_str(&self) -> &'static str {
        match self {
            CommentKind::Line => "line",
            CommentKind::Doc => "doc",
            CommentKind::ReviewAnnotation => "review_annotation",
        }
    }
}

fn parse_comment(
    file: &str,
    line_no: usize,
    start_col: usize,
    end_col: usize,
    text: &str,
) -> Comment {
    let span = SourceSpan::new(file, line_no, start_col, line_no, end_col);
    if let Some(rest) = text.strip_prefix("#@") {
        let rest = rest.trim();
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts
            .next()
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let payload = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let target = payload
            .and_then(|value| value.split_whitespace().find(|part| part.starts_with('@')))
            .map(str::to_string);
        Comment {
            kind: CommentKind::ReviewAnnotation,
            text: text.to_string(),
            name,
            payload: payload.map(str::to_string),
            target,
            span,
        }
    } else if text.starts_with("##") {
        Comment {
            kind: CommentKind::Doc,
            text: text.to_string(),
            name: None,
            payload: Some(text.trim_start_matches('#').trim().to_string()),
            target: None,
            span,
        }
    } else {
        Comment {
            kind: CommentKind::Line,
            text: text.to_string(),
            name: None,
            payload: None,
            target: None,
            span,
        }
    }
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value,
        "struct"
            | "fn"
            | "unsafe"
            | "let"
            | "return"
            | "require"
            | "if"
            | "for"
            | "in"
            | "asm"
            | "S"
            | "F"
            | "UF"
            | "LET"
            | "R"
            | "REQ"
            | "IF"
            | "FOR"
            | "SET"
            | "ASM"
            | "PATCH"
            | "ADD_STRUCT_FIELD"
            | "REPLACE_RETURN_TYPE"
            | "RENAME_FUNCTION"
            | "INSERT_FAST_PATH"
            | "ADD_UNSAFE_ASM_STUB"
    )
}

fn is_symbol(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '.'
    )
}

fn is_single_char_operator(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '%' | '<' | '>' | '=' | '!' | '&' | '|'
    )
}

fn is_two_char_operator(value: &str) -> bool {
    matches!(
        value,
        "==" | "!=" | "<=" | ">=" | "&&" | "||" | ">>" | "<<" | ".." | "->"
    )
}
