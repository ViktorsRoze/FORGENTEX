use std::path::Path;

use crate::ast::{
    is_primitive_name, AsmBinding, AsmBlock, Attribute, Capsule, CapsuleBinding, CapsuleDep,
    CapsuleFixture, CapsuleShareBinding, CapsuleValue, Expr, Field, FunctionDecl, Item, Param,
    Program, ShareDecl, ShareMode, Stmt, StructDecl, TestDecl, TestExpectation, Type,
};
use crate::lexer::{lex, Comment, Token};
use crate::span::{ParseDiagnostic, SourceMap, SourceSpan};

type FunctionHeader = (String, bool, Vec<Param>, Type, Vec<String>);

#[derive(Clone, Debug, PartialEq)]
pub struct ParseOutput {
    pub program: Option<Program>,
    pub source_map: SourceMap,
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

pub fn parse_file(path: &Path, source: &str) -> Result<Program, String> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("fgai") => parse_agent(source),
        Some("fgx") => parse_human(source),
        _ => parse_human(source),
    }
}

pub fn parse_file_detailed(path: &Path, source: &str) -> ParseOutput {
    let file = path.display().to_string();
    let lexed = lex(&file, source);
    let mut diagnostics = lexed.diagnostics.clone();
    match parse_file(path, source) {
        Ok(program) => {
            let source_map = build_source_map(path, source, &program, lexed.comments.clone());
            ParseOutput {
                program: Some(program),
                source_map,
                tokens: lexed.tokens,
                diagnostics,
            }
        }
        Err(message) => {
            diagnostics.push(parse_error_to_diagnostic(&file, source, &message));
            ParseOutput {
                program: None,
                source_map: SourceMap::new(file, lexed.comments.clone()),
                tokens: lexed.tokens,
                diagnostics,
            }
        }
    }
}

pub fn parse_human(source: &str) -> Result<Program, String> {
    let lines = source_lines(source);
    let mut program = Program::empty();
    let mut index = 0;
    let mut pending_attrs = Vec::new();
    while index < lines.len() {
        let line = &lines[index];
        if line.text.is_empty() {
            index += 1;
            continue;
        }
        if line.indent != 0 {
            return Err(format!(
                "top-level item must start at column 0: {}",
                line.text
            ));
        }
        if is_human_module_marker(&line.text) {
            index += 1;
            continue;
        }
        if line.text.starts_with('@') {
            pending_attrs.push(parse_attribute_line(&line.text)?);
            index += 1;
            continue;
        }
        let item_text = strip_visibility_marker(&line.text);
        if let Some(name) = item_text
            .strip_prefix("struct ")
            .and_then(|s| s.strip_suffix(':'))
        {
            index += 1;
            let mut fields = Vec::new();
            let mut pending_field_attrs = Vec::new();
            while index < lines.len() && lines[index].indent > 0 {
                let field_line = lines[index].text.trim();
                if !field_line.is_empty() {
                    if field_line.starts_with('@') {
                        pending_field_attrs.push(parse_attribute_line(field_line)?);
                    } else {
                        let (name, ty) = split_once(field_line, ":")
                            .ok_or_else(|| format!("invalid struct field: {field_line}"))?;
                        fields.push(Field {
                            name: name.trim().to_string(),
                            ty: Type::parse(ty),
                            attrs: std::mem::take(&mut pending_field_attrs),
                        });
                    }
                }
                index += 1;
            }
            program.items.push(Item::Struct(StructDecl::with_attrs(
                name.trim().to_string(),
                std::mem::take(&mut pending_attrs),
                fields,
            )));
            continue;
        }
        if item_text.starts_with("struct ") {
            return Err(format!(
                "struct declaration must end with ':' and contain indented fields: {}",
                item_text
            ));
        }
        if item_text.starts_with("fn ") || item_text.starts_with("unsafe fn ") {
            let (name, unsafe_fn, params, ret, effects) = parse_human_function_header(item_text)?;
            index += 1;
            let body = parse_human_block(&lines, &mut index, 4)?;
            let mut function = FunctionDecl::new(name, unsafe_fn, params, ret, body);
            function.effects = effects;
            function.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Function(function));
            continue;
        }
        if item_text.starts_with("test ") || item_text.starts_with("xfail test ") {
            let (name, xfail) = parse_human_test_header(item_text)?;
            index += 1;
            let expectations = parse_human_test_block(&lines, &mut index, 4)?;
            let _ = std::mem::take(&mut pending_attrs);
            program
                .items
                .push(Item::Test(TestDecl::new(name, xfail, expectations)));
            continue;
        }
        if item_text.starts_with("foreign ") {
            let mut capsule = parse_capsule_human(item_text, &lines, &mut index)?;
            capsule.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Capsule(capsule));
            continue;
        }
        if let Some(rest) = item_text.strip_prefix("share ") {
            let mut share = parse_share_decl(rest, false)?;
            share.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Share(share));
            index += 1;
            continue;
        }
        return Err(format!("unknown top-level item: {}", item_text));
    }
    Ok(program)
}

// ---- Foreign capsule parsing (v2.0) -------------------------------------
//
// Capsule contract lines are read from raw source because dependency
// references carry `#` (for example `#sha256:...`) and foreign code bodies
// may contain `#` comments, both of which the line comment-stripper would
// otherwise truncate.

fn parse_capsule_human(
    header: &str,
    lines: &[SourceLine],
    index: &mut usize,
) -> Result<Capsule, String> {
    let header = header
        .trim()
        .strip_suffix(':')
        .ok_or_else(|| format!("foreign capsule header must end with ':': {header}"))?;
    let rest = header
        .strip_prefix("foreign ")
        .ok_or_else(|| format!("capsule header must start with 'foreign ': {header}"))?;
    let mut capsule = parse_capsule_header(rest, true)?;
    *index += 1;
    while *index < lines.len() {
        let line = &lines[*index];
        if line.raw.trim().is_empty() {
            *index += 1;
            continue;
        }
        if line.indent == 0 {
            break;
        }
        let content = line.raw.trim();
        if let Some(binding) = content.strip_prefix("input ") {
            capsule.inputs.push(parse_capsule_binding(binding, false)?);
        } else if let Some(binding) = content.strip_prefix("output ") {
            capsule.outputs.push(parse_capsule_binding(binding, false)?);
        } else if let Some(dep) = content.strip_prefix("dep ") {
            let (source, _) = take_quoted(dep.trim())?;
            capsule.deps.push(CapsuleDep { source });
        } else if let Some(rule) = content.strip_prefix("sandbox ") {
            let (rule, _) = take_quoted(rule.trim())?;
            capsule.sandbox.push(rule);
        } else if let Some(binding) = content.strip_prefix("bind ") {
            capsule.bindings.push(parse_capsule_share_binding(binding)?);
        } else if let Some(fixture) = content.strip_prefix("mock ") {
            capsule.fixtures.push(parse_capsule_fixture(fixture)?);
        } else if content == "code:" {
            *index += 1;
            let mut code = Vec::new();
            while *index < lines.len() {
                let code_line = &lines[*index];
                if code_line.raw.trim().is_empty() {
                    code.push(String::new());
                    *index += 1;
                    continue;
                }
                if code_line.indent < 8 {
                    break;
                }
                code.push(strip_leading_spaces(&code_line.raw, 8));
                *index += 1;
            }
            while code.last().map(|line| line.is_empty()).unwrap_or(false) {
                code.pop();
            }
            capsule.code = code;
            continue;
        } else {
            return Err(format!("unknown capsule line: {content}"));
        }
        *index += 1;
    }
    Ok(capsule)
}

/// Pull every `CAP ... ENDCAP` block out of agent source before the normal
/// line-oriented agent parser runs, replacing each block with a single
/// `CAPREF <index>` marker so ordering is preserved.
fn extract_agent_capsules(source: &str) -> Result<(String, Vec<Capsule>), String> {
    let raw: Vec<&str> = source.lines().collect();
    let mut out_lines: Vec<String> = Vec::new();
    let mut capsules: Vec<Capsule> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let trimmed = raw[i].trim();
        let Some(header) = trimmed.strip_prefix("CAP ") else {
            out_lines.push(raw[i].to_string());
            i += 1;
            continue;
        };
        let mut capsule = parse_capsule_header(header, false)?;
        i += 1;
        let mut saw_code = false;
        while i < raw.len() {
            let line = raw[i].trim();
            if line.is_empty() {
                i += 1;
                continue;
            }
            if let Some(binding) = line.strip_prefix("IN ") {
                capsule.inputs.push(parse_capsule_binding(binding, true)?);
            } else if let Some(binding) = line.strip_prefix("OUT ") {
                capsule.outputs.push(parse_capsule_binding(binding, true)?);
            } else if let Some(dep) = line.strip_prefix("DEP ") {
                let (source, _) = take_quoted(dep.trim())?;
                capsule.deps.push(CapsuleDep { source });
            } else if let Some(rule) = line.strip_prefix("SBX ") {
                let (rule, _) = take_quoted(rule.trim())?;
                capsule.sandbox.push(rule);
            } else if let Some(binding) = line.strip_prefix("BIND ") {
                capsule.bindings.push(parse_capsule_share_binding(binding)?);
            } else if let Some(fixture) = line.strip_prefix("MOCK ") {
                capsule.fixtures.push(parse_capsule_fixture(fixture)?);
            } else if let Some(count) = line.strip_prefix("CODE ") {
                let count: usize = count
                    .trim()
                    .parse()
                    .map_err(|_| format!("invalid CODE line count: {count}"))?;
                i += 1;
                let mut code = Vec::new();
                for _ in 0..count {
                    if i >= raw.len() {
                        return Err("capsule CODE block ends before its line count".to_string());
                    }
                    code.push(raw[i].to_string());
                    i += 1;
                }
                capsule.code = code;
                while i < raw.len() && raw[i].trim().is_empty() {
                    i += 1;
                }
                if i >= raw.len() || raw[i].trim() != "ENDCAP" {
                    return Err("capsule is missing 'ENDCAP'".to_string());
                }
                i += 1;
                saw_code = true;
                break;
            } else {
                return Err(format!("unknown capsule line: {line}"));
            }
            i += 1;
        }
        if !saw_code {
            return Err("capsule is missing a 'CODE <n>' block".to_string());
        }
        out_lines.push(format!("CAPREF {}", capsules.len()));
        capsules.push(capsule);
    }
    Ok((out_lines.join("\n"), capsules))
}

fn parse_capsule_header(rest: &str, expect_version_keyword: bool) -> Result<Capsule, String> {
    let (name, rest) = take_word(rest)?;
    let (language, rest) = take_word(rest)?;
    let mut rest = rest.trim_start();
    if expect_version_keyword {
        rest = rest
            .strip_prefix("version ")
            .ok_or_else(|| "capsule header is missing 'version'".to_string())?
            .trim_start();
    }
    let (version, mut rest) = take_quoted(rest.trim_start())?;
    let mut capsule = Capsule::new(name, language, version);
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(after) = rest.strip_prefix("env ") {
            let (env, after) = take_quoted(after.trim_start())?;
            capsule.env = Some(env);
            rest = after;
        } else if let Some(after) = rest.strip_prefix("effects ") {
            let (list, after) = take_bracketed(after.trim_start())?;
            capsule.effects = split_comma_list(&list);
            rest = after;
        } else {
            return Err(format!("unexpected capsule header token near: {rest}"));
        }
    }
    Ok(capsule)
}

fn parse_capsule_binding(content: &str, agent: bool) -> Result<CapsuleBinding, String> {
    let content = content.trim();
    let (name, ty) = if agent {
        content
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("capsule binding needs a name and type: {content}"))?
    } else {
        content
            .split_once(':')
            .ok_or_else(|| format!("capsule binding needs 'name: type': {content}"))?
    };
    Ok(CapsuleBinding {
        name: name.trim().to_string(),
        ty: Type::parse(ty.trim()),
    })
}

fn parse_share_decl(rest: &str, agent: bool) -> Result<ShareDecl, String> {
    let rest = rest.trim();
    let (decl_part, default_part) = match rest.split_once('=') {
        Some((decl, value)) => (decl.trim(), Some(value.trim())),
        None => (rest, None),
    };
    let (name, ty_text) = if agent {
        decl_part
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("share needs a name and type: {decl_part}"))?
    } else {
        decl_part
            .split_once(':')
            .ok_or_else(|| format!("share needs 'name: type': {decl_part}"))?
    };
    let default = match default_part {
        Some(value) => Some(expr_to_capsule_value(&parse_expr(value)?)?),
        None => None,
    };
    Ok(ShareDecl::new(
        name.trim().to_string(),
        Type::parse(ty_text.trim()),
        default,
    ))
}

fn parse_capsule_share_binding(content: &str) -> Result<CapsuleShareBinding, String> {
    let mut parts = content.split_whitespace();
    let name = parts
        .next()
        .ok_or_else(|| format!("capsule bind needs a name: {content}"))?
        .to_string();
    let mode_text = parts
        .next()
        .ok_or_else(|| format!("capsule bind needs a mode (read|rw): {content}"))?;
    let mode = ShareMode::parse(mode_text)
        .ok_or_else(|| format!("unknown capsule bind mode '{mode_text}'"))?;
    Ok(CapsuleShareBinding { name, mode })
}

/// Parse `<in>, <in> -> <out>, <out>` literal lists into a fixture.
fn parse_capsule_fixture(content: &str) -> Result<CapsuleFixture, String> {
    let (inputs_text, outputs_text) = content
        .split_once("->")
        .ok_or_else(|| format!("capsule mock fixture needs '->': {content}"))?;
    Ok(CapsuleFixture {
        inputs: parse_capsule_value_list(inputs_text)?,
        outputs: parse_capsule_value_list(outputs_text)?,
    })
}

/// Parse a comma-separated list of capsule literal values, used by the
/// `fgx capsule-run --input` execution hook.
pub fn parse_capsule_values(text: &str) -> Result<Vec<CapsuleValue>, String> {
    parse_capsule_value_list(text)
}

fn parse_capsule_value_list(text: &str) -> Result<Vec<CapsuleValue>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|item| {
            let expr = parse_expr(item.trim())?;
            expr_to_capsule_value(&expr)
        })
        .collect()
}

fn expr_to_capsule_value(expr: &Expr) -> Result<CapsuleValue, String> {
    match expr {
        Expr::Int(value) => Ok(CapsuleValue::Int(*value)),
        Expr::Float(value) => Ok(CapsuleValue::Float(*value)),
        Expr::Bool(value) => Ok(CapsuleValue::Bool(*value)),
        Expr::String(value) => Ok(CapsuleValue::Str(value.clone())),
        Expr::Unary { op, expr } if op == "-" => match expr_to_capsule_value(expr)? {
            CapsuleValue::Int(value) => Ok(CapsuleValue::Int(-value)),
            CapsuleValue::Float(value) => Ok(CapsuleValue::Float(-value)),
            _ => Err("capsule fixture negation requires a numeric literal".to_string()),
        },
        _ => Err("capsule fixture values must be literals".to_string()),
    }
}

fn take_word(input: &str) -> Result<(String, &str), String> {
    let input = input.trim_start();
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    if end == 0 {
        return Err("expected an identifier in capsule header".to_string());
    }
    Ok((input[..end].to_string(), &input[end..]))
}

fn take_quoted(input: &str) -> Result<(String, &str), String> {
    let input = input.trim_start();
    let mut chars = input.char_indices();
    if chars.next().map(|(_, ch)| ch) != Some('"') {
        return Err(format!("expected a quoted value: {input}"));
    }
    let mut value = String::new();
    let mut escaped = false;
    for (idx, ch) in chars {
        if escaped {
            match ch {
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                other => value.push(other),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let rest = &input[idx + 1..];
                return Ok((value, rest));
            }
            other => value.push(other),
        }
    }
    Err(format!("unterminated quoted value: {input}"))
}

fn take_bracketed(input: &str) -> Result<(String, &str), String> {
    let input = input.trim_start();
    let inner = input
        .strip_prefix('[')
        .ok_or_else(|| format!("expected '[': {input}"))?;
    let end = inner
        .find(']')
        .ok_or_else(|| format!("expected ']': {input}"))?;
    Ok((inner[..end].to_string(), &inner[end + 1..]))
}

fn split_comma_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn strip_leading_spaces(line: &str, count: usize) -> String {
    let mut removed = 0;
    let mut chars = line.chars();
    let mut prefix_len = 0;
    for ch in chars.by_ref() {
        if ch == ' ' && removed < count {
            removed += 1;
            prefix_len += 1;
        } else {
            break;
        }
    }
    line[prefix_len..].to_string()
}

pub fn parse_agent(source: &str) -> Result<Program, String> {
    let (clean_source, capsules) = extract_agent_capsules(source)?;
    let source = clean_source.as_str();
    let lines: Vec<String> = source
        .lines()
        .map(|line| strip_comment(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let mut program = Program::empty();
    let mut index = 0;
    let mut pending_attrs = Vec::new();
    while index < lines.len() {
        let line = strip_agent_visibility_marker(&lines[index]);
        if is_agent_module_marker(line) {
            index += 1;
            continue;
        }
        if line.starts_with("A ") {
            pending_attrs.push(parse_agent_attribute_line(line)?);
            index += 1;
            continue;
        }
        if line.starts_with("S ") {
            let mut decl = parse_agent_struct(line)?;
            decl.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Struct(decl));
            index += 1;
            continue;
        }
        if line.starts_with("F ") || line.starts_with("UF ") || line.starts_with("unsafe F ") {
            let (name, unsafe_fn, params, ret, effects) = parse_agent_function_header(line)?;
            index += 1;
            let body = parse_agent_block(&lines, &mut index)?;
            let mut function = FunctionDecl::new(name, unsafe_fn, params, ret, body);
            function.effects = effects;
            function.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Function(function));
            continue;
        }
        if line.starts_with("TEST ") || line.starts_with("XTEST ") {
            let (name, xfail) = parse_agent_test_header(line)?;
            index += 1;
            let expectations = parse_agent_test_block(&lines, &mut index)?;
            let _ = std::mem::take(&mut pending_attrs);
            program
                .items
                .push(Item::Test(TestDecl::new(name, xfail, expectations)));
            continue;
        }
        if let Some(reference) = line.strip_prefix("CAPREF ") {
            let capsule_index: usize = reference
                .trim()
                .parse()
                .map_err(|_| format!("invalid capsule reference: {line}"))?;
            let mut capsule = capsules
                .get(capsule_index)
                .cloned()
                .ok_or_else(|| format!("capsule reference out of range: {line}"))?;
            capsule.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Capsule(capsule));
            index += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix("SHARE ") {
            let mut share = parse_share_decl(rest, true)?;
            share.attrs = std::mem::take(&mut pending_attrs);
            program.items.push(Item::Share(share));
            index += 1;
            continue;
        }
        return Err(format!("unknown agent item: {line}"));
    }
    Ok(program)
}

pub fn parse_expr(source: &str) -> Result<Expr, String> {
    let tokens = tokenize_expr(source)?;
    let mut parser = ExprParser { tokens, pos: 0 };
    let expr = parser.parse_min_prec(0)?;
    if parser.pos != parser.tokens.len() {
        return Err(format!(
            "unexpected token '{}' in expression '{}'",
            parser.tokens[parser.pos], source
        ));
    }
    Ok(expr)
}

fn build_source_map(
    path: &Path,
    source: &str,
    program: &Program,
    comments: Vec<Comment>,
) -> SourceMap {
    let file = path.display().to_string();
    let mut map = SourceMap::new(file.clone(), comments);
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("fgai") => map_agent_source(&file, source, program, &mut map),
        _ => map_human_source(&file, source, program, &mut map),
    }
    map.sort_and_dedup();
    map
}

fn map_human_source(file: &str, source: &str, program: &Program, map: &mut SourceMap) {
    let lines = source_lines(source);
    let mut index = 0;
    for item in &program.items {
        match item {
            Item::Struct(decl) => {
                let Some(header_idx) = find_line_from(&lines, index, |line| {
                    strip_visibility_marker(&line.text) == format!("struct {}:", decl.name)
                }) else {
                    continue;
                };
                index = header_idx + 1;
                let header_span = line_span(file, &lines[header_idx]);
                map.add(&decl.id, header_span.clone());
                for field in &decl.fields {
                    if let Some(field_idx) = find_line_from(&lines, index, |line| {
                        line.indent > 0 && line.text.starts_with(&format!("{}:", field.name))
                    }) {
                        map.add(
                            format!("{}.field.{}", decl.id, field.name),
                            line_span(file, &lines[field_idx]),
                        );
                        index = field_idx + 1;
                    }
                }
            }
            Item::Function(function) => {
                let Some(header_idx) = find_line_from(&lines, index, |line| {
                    let text = strip_visibility_marker(&line.text);
                    text.starts_with(&format!("fn {}(", function.name))
                        || text.starts_with(&format!("unsafe fn {}(", function.name))
                }) else {
                    continue;
                };
                index = header_idx + 1;
                let header_span = line_span(file, &lines[header_idx]);
                map.add(&function.id, header_span.clone());
                if function.unsafe_fn {
                    map.add(format!("{}.unsafe", function.id), header_span.clone());
                }
                for param in &function.params {
                    map.add(
                        format!("{}.param.{}", function.id, param.name),
                        header_span.clone(),
                    );
                }
                map_human_stmt_spans(
                    file,
                    &lines,
                    &mut index,
                    4,
                    &function.body,
                    &function.id,
                    map,
                );
            }
            Item::Test(test) => {
                let Some(header_idx) = find_line_from(&lines, index, |line| {
                    let text = strip_visibility_marker(&line.text);
                    text == format!("test \"{}\":", test.name)
                        || text == format!("xfail test \"{}\":", test.name)
                }) else {
                    continue;
                };
                index = header_idx + 1;
                let header_span = line_span(file, &lines[header_idx]);
                map.add(&test.id, header_span.clone());
                for idx in 0..test.expectations.len() {
                    if let Some(line_idx) = find_line_from(&lines, index, |line| line.indent == 4) {
                        map.add(
                            format!("{}.expect.{idx}", test.id),
                            line_span(file, &lines[line_idx]),
                        );
                        index = line_idx + 1;
                    }
                }
            }
            Item::Capsule(capsule) => {
                if let Some(header_idx) = find_line_from(&lines, index, |line| {
                    line.text.starts_with(&format!("foreign {} ", capsule.name))
                }) {
                    index = header_idx + 1;
                    map.add(&capsule.id, line_span(file, &lines[header_idx]));
                }
            }
            Item::Share(share) => {
                if let Some(line_idx) = find_line_from(&lines, index, |line| {
                    line.text.starts_with(&format!("share {}:", share.name))
                }) {
                    index = line_idx + 1;
                    map.add(&share.id, line_span(file, &lines[line_idx]));
                }
            }
        }
    }
}

fn map_human_stmt_spans(
    file: &str,
    lines: &[SourceLine],
    index: &mut usize,
    indent: usize,
    body: &[Stmt],
    prefix: &str,
    map: &mut SourceMap,
) {
    for (stmt_idx, stmt) in body.iter().enumerate() {
        let Some(line_idx) = next_stmt_line(lines, index, indent) else {
            return;
        };
        *index = line_idx + 1;
        let id = format!("{prefix}.stmt.{stmt_idx}");
        let span = line_span(file, &lines[line_idx]);
        map_stmt_and_expr_spans(stmt, &id, span.clone(), map);
        match stmt {
            Stmt::If { then_body, .. } => {
                map_human_nested_stmt_spans(
                    file,
                    lines,
                    index,
                    indent + 4,
                    then_body,
                    &format!("{id}.then"),
                    map,
                );
            }
            Stmt::For { body, .. } => {
                map_human_nested_stmt_spans(
                    file,
                    lines,
                    index,
                    indent + 4,
                    body,
                    &format!("{id}.body"),
                    map,
                );
            }
            Stmt::Asm(_) => {
                while *index < lines.len()
                    && (lines[*index].text.is_empty()
                        || (lines[*index].indent >= indent + 4 && !lines[*index].text.is_empty()))
                {
                    if !lines[*index].text.is_empty() && lines[*index].indent < indent + 4 {
                        break;
                    }
                    *index += 1;
                }
            }
            _ => {}
        }
    }
}

fn map_human_nested_stmt_spans(
    file: &str,
    lines: &[SourceLine],
    index: &mut usize,
    indent: usize,
    body: &[Stmt],
    prefix: &str,
    map: &mut SourceMap,
) {
    for (stmt_idx, stmt) in body.iter().enumerate() {
        let Some(line_idx) = next_stmt_line(lines, index, indent) else {
            return;
        };
        *index = line_idx + 1;
        let id = format!("{prefix}.{stmt_idx}");
        let span = line_span(file, &lines[line_idx]);
        map_stmt_and_expr_spans(stmt, &id, span.clone(), map);
        match stmt {
            Stmt::If { then_body, .. } => map_human_nested_stmt_spans(
                file,
                lines,
                index,
                indent + 4,
                then_body,
                &format!("{id}.then"),
                map,
            ),
            Stmt::For { body, .. } => map_human_nested_stmt_spans(
                file,
                lines,
                index,
                indent + 4,
                body,
                &format!("{id}.body"),
                map,
            ),
            _ => {}
        }
    }
}

fn map_agent_source(file: &str, source: &str, program: &Program, map: &mut SourceMap) {
    let lines = source_lines(source);
    let mut index = 0;
    for item in &program.items {
        match item {
            Item::Struct(decl) => {
                let Some(line_idx) = find_line_from(&lines, index, |line| {
                    strip_agent_visibility_marker(&line.text)
                        .starts_with(&format!("S {} ", decl.name))
                }) else {
                    continue;
                };
                index = line_idx + 1;
                let span = line_span(file, &lines[line_idx]);
                map.add(&decl.id, span.clone());
                for field in &decl.fields {
                    map.add(format!("{}.field.{}", decl.id, field.name), span.clone());
                }
            }
            Item::Function(function) => {
                let Some(line_idx) = find_line_from(&lines, index, |line| {
                    let text = strip_agent_visibility_marker(&line.text);
                    text.starts_with(&format!("F {}(", function.name))
                        || text.starts_with(&format!("UF {}(", function.name))
                        || text.starts_with(&format!("unsafe F {}(", function.name))
                }) else {
                    continue;
                };
                index = line_idx + 1;
                let span = line_span(file, &lines[line_idx]);
                map.add(&function.id, span.clone());
                if function.unsafe_fn {
                    map.add(format!("{}.unsafe", function.id), span.clone());
                }
                for param in &function.params {
                    map.add(
                        format!("{}.param.{}", function.id, param.name),
                        span.clone(),
                    );
                }
                map_agent_stmt_spans(file, &lines, &mut index, &function.body, &function.id, map);
            }
            Item::Test(test) => {
                let Some(line_idx) = find_line_from(&lines, index, |line| {
                    let text = strip_agent_visibility_marker(&line.text);
                    text == format!("TEST \"{}\" {{", test.name)
                        || text == format!("XTEST \"{}\" {{", test.name)
                }) else {
                    continue;
                };
                index = line_idx + 1;
                let span = line_span(file, &lines[line_idx]);
                map.add(&test.id, span);
                for idx in 0..test.expectations.len() {
                    if let Some(expect_idx) = next_agent_stmt_line(&lines, &mut index) {
                        map.add(
                            format!("{}.expect.{idx}", test.id),
                            line_span(file, &lines[expect_idx]),
                        );
                        index = expect_idx + 1;
                    }
                }
            }
            Item::Capsule(capsule) => {
                if let Some(line_idx) = find_line_from(&lines, index, |line| {
                    line.text.starts_with(&format!("CAP {} ", capsule.name))
                }) {
                    index = line_idx + 1;
                    map.add(&capsule.id, line_span(file, &lines[line_idx]));
                }
            }
            Item::Share(share) => {
                if let Some(line_idx) = find_line_from(&lines, index, |line| {
                    line.text.starts_with(&format!("SHARE {} ", share.name))
                }) {
                    index = line_idx + 1;
                    map.add(&share.id, line_span(file, &lines[line_idx]));
                }
            }
        }
    }
}

fn map_agent_stmt_spans(
    file: &str,
    lines: &[SourceLine],
    index: &mut usize,
    body: &[Stmt],
    prefix: &str,
    map: &mut SourceMap,
) {
    for (stmt_idx, stmt) in body.iter().enumerate() {
        let Some(line_idx) = next_agent_stmt_line(lines, index) else {
            return;
        };
        *index = line_idx + 1;
        let id = format!("{prefix}.stmt.{stmt_idx}");
        let span = line_span(file, &lines[line_idx]);
        map_stmt_and_expr_spans(stmt, &id, span.clone(), map);
        match stmt {
            Stmt::If { then_body, .. } => map_agent_nested_stmt_spans(
                file,
                lines,
                index,
                then_body,
                &format!("{id}.then"),
                map,
            ),
            Stmt::For { body, .. } => {
                map_agent_nested_stmt_spans(file, lines, index, body, &format!("{id}.body"), map)
            }
            Stmt::Asm(_) => {
                while *index < lines.len() && lines[*index].text != "}" {
                    *index += 1;
                }
                if *index < lines.len() {
                    *index += 1;
                }
            }
            _ => {}
        }
    }
    if *index < lines.len() && lines[*index].text == "}" {
        *index += 1;
    }
}

fn map_agent_nested_stmt_spans(
    file: &str,
    lines: &[SourceLine],
    index: &mut usize,
    body: &[Stmt],
    prefix: &str,
    map: &mut SourceMap,
) {
    for (stmt_idx, stmt) in body.iter().enumerate() {
        let Some(line_idx) = next_agent_stmt_line(lines, index) else {
            return;
        };
        *index = line_idx + 1;
        let id = format!("{prefix}.{stmt_idx}");
        let span = line_span(file, &lines[line_idx]);
        map_stmt_and_expr_spans(stmt, &id, span.clone(), map);
        match stmt {
            Stmt::If { then_body, .. } => map_agent_nested_stmt_spans(
                file,
                lines,
                index,
                then_body,
                &format!("{id}.then"),
                map,
            ),
            Stmt::For { body, .. } => {
                map_agent_nested_stmt_spans(file, lines, index, body, &format!("{id}.body"), map)
            }
            _ => {}
        }
    }
    if *index < lines.len() && lines[*index].text == "}" {
        *index += 1;
    }
}

fn map_stmt_and_expr_spans(stmt: &Stmt, id: &str, span: SourceSpan, map: &mut SourceMap) {
    map.add(id, span.clone());
    match stmt {
        Stmt::Let { expr, .. } => map_expr_spans(expr, &format!("{id}.expr"), span, map),
        Stmt::Return(Some(expr)) => map_expr_spans(expr, &format!("{id}.expr"), span, map),
        Stmt::Return(None) => {}
        Stmt::Require(expr) => map_expr_spans(expr, &format!("{id}.expr"), span, map),
        Stmt::If { cond, .. } => map_expr_spans(cond, &format!("{id}.cond"), span, map),
        Stmt::For { start, end, .. } => {
            map.add(format!("{id}.loop"), span.clone());
            map_expr_spans(start, &format!("{id}.start"), span.clone(), map);
            map_expr_spans(end, &format!("{id}.end"), span, map);
        }
        Stmt::Assign { target, expr } => {
            map_expr_spans(target, &format!("{id}.target"), span.clone(), map);
            map_expr_spans(expr, &format!("{id}.expr"), span, map);
        }
        Stmt::Expr(expr) => map_expr_spans(expr, &format!("{id}.expr"), span, map),
        Stmt::Asm(_) => map.add(format!("{id}.asm"), span),
    }
}

fn map_expr_spans(expr: &Expr, id: &str, span: SourceSpan, map: &mut SourceMap) {
    map.add(id, span.clone());
    match expr {
        Expr::Unary { expr, .. } => map_expr_spans(expr, &format!("{id}.expr"), span, map),
        Expr::Binary { left, right, .. } => {
            map_expr_spans(left, &format!("{id}.left"), span.clone(), map);
            map_expr_spans(right, &format!("{id}.right"), span, map);
        }
        Expr::Call { args, .. } => {
            for (idx, arg) in args.iter().enumerate() {
                map_expr_spans(arg, &format!("{id}.arg.{idx}"), span.clone(), map);
            }
        }
        Expr::Cast { expr, .. } => map_expr_spans(expr, &format!("{id}.expr"), span, map),
        Expr::Field { base, .. } => map_expr_spans(base, &format!("{id}.base"), span, map),
        Expr::Index { base, index } => {
            map_expr_spans(base, &format!("{id}.base"), span.clone(), map);
            map_expr_spans(index, &format!("{id}.index"), span, map);
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => {}
    }
}

fn parse_error_to_diagnostic(file: &str, source: &str, message: &str) -> ParseDiagnostic {
    let (code, suggestion) = if message.contains("must end with ':'") {
        ("missing_colon", "insert ':' at the end of the block header")
    } else if message.contains("missing closing '}'") {
        (
            "missing_closing_brace",
            "insert a matching '}' before the next declaration or end of file",
        )
    } else if message.contains("unexpected character") || message.contains("invalid float literal")
    {
        ("invalid_token", "replace the invalid token")
    } else if message.contains("unknown top-level item") || message.contains("unknown agent item") {
        ("unknown_item", "use a supported declaration form")
    } else if message.contains("expected token") {
        (
            "unexpected_token",
            "insert the expected token or rewrite the expression",
        )
    } else {
        ("parse_error", "rewrite this syntax using the v1 grammar")
    };
    let span = approximate_error_span(file, source, message);
    let mut diagnostic = ParseDiagnostic::new(code, message.to_string()).suggest(suggestion);
    if let Some(span) = span {
        diagnostic = diagnostic.with_span(span);
    }
    diagnostic
}

fn approximate_error_span(file: &str, source: &str, message: &str) -> Option<SourceSpan> {
    let fragment = message
        .rsplit_once(':')
        .map(|(_, after)| after.trim())
        .filter(|value| !value.is_empty());
    for (idx, line) in source.lines().enumerate() {
        let trimmed = strip_comment(line).trim();
        if let Some(fragment) = fragment {
            if !fragment.is_empty() && trimmed.contains(fragment) {
                return Some(SourceSpan::new(
                    file,
                    idx + 1,
                    line.find(fragment).unwrap_or(0) + 1,
                    idx + 1,
                    line.len() + 1,
                ));
            }
        }
        if !trimmed.is_empty() {
            return Some(SourceSpan::new(file, idx + 1, 1, idx + 1, line.len() + 1));
        }
    }
    None
}

fn find_line_from(
    lines: &[SourceLine],
    start: usize,
    predicate: impl Fn(&SourceLine) -> bool,
) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| predicate(line))
        .map(|(idx, _)| idx)
}

fn next_stmt_line(lines: &[SourceLine], index: &mut usize, indent: usize) -> Option<usize> {
    while *index < lines.len() {
        let line = &lines[*index];
        if line.text.is_empty() {
            *index += 1;
            continue;
        }
        if line.indent < indent {
            return None;
        }
        if line.indent == indent {
            return Some(*index);
        }
        *index += 1;
    }
    None
}

fn next_agent_stmt_line(lines: &[SourceLine], index: &mut usize) -> Option<usize> {
    while *index < lines.len() {
        if lines[*index].text.is_empty() {
            *index += 1;
            continue;
        }
        if lines[*index].text == "}" {
            return None;
        }
        return Some(*index);
    }
    None
}

fn line_span(file: &str, line: &SourceLine) -> SourceSpan {
    let start_col = line.indent + 1;
    SourceSpan::new(
        file,
        line.line_no,
        start_col,
        line.line_no,
        line.raw.len() + 1,
    )
}

#[derive(Clone, Debug)]
struct SourceLine {
    line_no: usize,
    indent: usize,
    text: String,
    raw: String,
}

fn source_lines(source: &str) -> Vec<SourceLine> {
    source
        .lines()
        .enumerate()
        .map(|(idx, line)| {
            let line = line.replace('\t', "    ");
            let indent = line.chars().take_while(|ch| *ch == ' ').count();
            SourceLine {
                line_no: idx + 1,
                indent,
                text: strip_comment(&line).trim().to_string(),
                raw: line,
            }
        })
        .collect()
}

fn strip_comment(line: &str) -> &str {
    if let Some((before, _)) = line.split_once('#') {
        before
    } else {
        line
    }
}

fn parse_quoted_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(format!("test name must be quoted: {value}"));
    }
    Ok(value[1..value.len() - 1]
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\\\", "\\"))
}

fn is_human_module_marker(text: &str) -> bool {
    text.starts_with("module ") || text.starts_with("import ")
}

fn strip_visibility_marker(text: &str) -> &str {
    text.strip_prefix("export ")
        .or_else(|| text.strip_prefix("pub "))
        .unwrap_or(text)
}

fn is_agent_module_marker(text: &str) -> bool {
    text.starts_with("M ") || text.starts_with("I ") || text.starts_with("IMPORT ")
}

fn strip_agent_visibility_marker(text: &str) -> &str {
    text.strip_prefix("EXPORT ")
        .or_else(|| text.strip_prefix("PUB "))
        .unwrap_or(text)
}

fn parse_attribute_line(text: &str) -> Result<Attribute, String> {
    let body = text
        .strip_prefix('@')
        .ok_or_else(|| format!("invalid attribute: {text}"))?;
    parse_attribute_body(body)
}

fn parse_agent_attribute_line(text: &str) -> Result<Attribute, String> {
    let body = text
        .strip_prefix("A ")
        .ok_or_else(|| format!("invalid agent attribute: {text}"))?;
    parse_attribute_body(body.trim())
}

fn parse_attribute_body(body: &str) -> Result<Attribute, String> {
    let body = body.trim();
    if body.is_empty() {
        return Err("empty attribute".to_string());
    }
    if let Some(open) = body.find('(') {
        let close = body
            .rfind(')')
            .ok_or_else(|| format!("attribute missing ')': {body}"))?;
        if close != body.len() - 1 {
            return Err(format!("unexpected text after attribute: {body}"));
        }
        let name = body[..open].trim();
        if name.is_empty() {
            return Err(format!("attribute missing name: {body}"));
        }
        let args = split_top_level(&body[open + 1..close], ',')
            .into_iter()
            .map(str::trim)
            .filter(|arg| !arg.is_empty())
            .map(|arg| arg.trim_matches('"').to_string())
            .collect();
        Ok(Attribute {
            name: name.to_string(),
            args,
        })
    } else {
        Ok(Attribute {
            name: body.to_string(),
            args: Vec::new(),
        })
    }
}

fn parse_inline_attributes(text: &str) -> Result<Vec<Attribute>, String> {
    let mut attrs = Vec::new();
    for token in text.split_whitespace() {
        attrs.push(parse_attribute_line(token)?);
    }
    Ok(attrs)
}

fn split_type_and_inline_attrs(text: &str) -> Result<(&str, Vec<Attribute>), String> {
    if let Some(index) = text.find(" @") {
        let ty = text[..index].trim();
        let attrs = parse_inline_attributes(text[index + 1..].trim())?;
        Ok((ty, attrs))
    } else {
        Ok((text.trim(), Vec::new()))
    }
}

fn parse_human_function_header(text: &str) -> Result<FunctionHeader, String> {
    let header = text
        .strip_suffix(':')
        .ok_or_else(|| format!("function header must end with ':': {text}"))?;
    let (unsafe_fn, rest) = if let Some(rest) = header.strip_prefix("unsafe fn ") {
        (true, rest)
    } else if let Some(rest) = header.strip_prefix("fn ") {
        (false, rest)
    } else {
        return Err(format!("invalid function header: {text}"));
    };
    let open = rest
        .find('(')
        .ok_or_else(|| format!("function header missing '(': {text}"))?;
    let close = rest
        .rfind(')')
        .ok_or_else(|| format!("function header missing ')': {text}"))?;
    let name = rest[..open].trim().to_string();
    let params = parse_human_params(&rest[open + 1..close])?;
    let after = rest[close + 1..].trim();
    let (ret, effects) = parse_return_and_effects(after)?;
    Ok((name, unsafe_fn, params, ret, effects))
}

fn parse_return_and_effects(after: &str) -> Result<(Type, Vec<String>), String> {
    let after = after.trim();
    let (return_text, effect_text) = split_effects_clause(after)?;
    let ret = if let Some(ret) = return_text.trim().strip_prefix("->") {
        Type::parse(ret)
    } else if return_text.trim().is_empty() {
        Type::Void
    } else {
        return Err(format!("invalid function suffix: {after}"));
    };
    Ok((ret, effect_text))
}

fn split_effects_clause(after: &str) -> Result<(&str, Vec<String>), String> {
    let Some(index) = effect_clause_start(after) else {
        return Ok((after, Vec::new()));
    };
    let before = &after[..index];
    let clause = after[index..].trim();
    let rest = clause
        .strip_prefix("effects")
        .ok_or_else(|| format!("invalid effects clause: {clause}"))?
        .trim();
    let inner = rest
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("effects clause must use brackets: {clause}"))?;
    let effects = split_top_level(inner, ',')
        .into_iter()
        .map(str::trim)
        .filter(|effect| !effect.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    Ok((before, effects))
}

fn effect_clause_start(after: &str) -> Option<usize> {
    if after.trim_start().starts_with("effects") {
        return Some(after.len() - after.trim_start().len());
    }
    after
        .find(" effects")
        .map(|index| index + 1)
        .filter(|index| after[*index..].starts_with("effects"))
}

fn parse_human_params(raw: &str) -> Result<Vec<Param>, String> {
    let mut params = Vec::new();
    for part in split_top_level(raw, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, ty) = split_once(part, ":")
            .ok_or_else(|| format!("human parameter must be name: Type, got {part}"))?;
        params.push(Param {
            name: name.trim().to_string(),
            ty: Type::parse(ty),
        });
    }
    Ok(params)
}

fn parse_human_test_header(text: &str) -> Result<(String, bool), String> {
    let (xfail, rest) = if let Some(rest) = text.strip_prefix("xfail test ") {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix("test ") {
        (false, rest)
    } else {
        return Err(format!("invalid test header: {text}"));
    };
    let name = rest
        .strip_suffix(':')
        .ok_or_else(|| format!("test declaration must end with ':': {text}"))?
        .trim();
    Ok((parse_quoted_name(name)?, xfail))
}

fn parse_human_test_block(
    lines: &[SourceLine],
    index: &mut usize,
    indent: usize,
) -> Result<Vec<TestExpectation>, String> {
    let mut expectations = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.text.is_empty() {
            *index += 1;
            continue;
        }
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(format!("unexpected indentation before '{}'", line.text));
        }
        expectations.push(parse_human_test_expectation(&line.text)?);
        *index += 1;
    }
    if expectations.is_empty() {
        return Err("test block must contain at least one expectation".to_string());
    }
    Ok(expectations)
}

fn parse_human_test_expectation(text: &str) -> Result<TestExpectation, String> {
    if let Some(code) = text.strip_prefix("expect_diagnostic ") {
        return Ok(TestExpectation::Diagnostic {
            code: code.trim().to_string(),
        });
    }
    let Some(expr) = text.strip_prefix("expect ") else {
        return Err(format!("invalid test expectation: {text}"));
    };
    Ok(TestExpectation::Expr {
        expr: parse_expr(expr)?,
    })
}

fn parse_human_block(
    lines: &[SourceLine],
    index: &mut usize,
    indent: usize,
) -> Result<Vec<Stmt>, String> {
    let mut body = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.text.is_empty() {
            *index += 1;
            continue;
        }
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(format!("unexpected indentation before '{}'", line.text));
        }
        let text = line.text.as_str();
        if let Some(cond) = text.strip_prefix("if ").and_then(|s| s.strip_suffix(':')) {
            *index += 1;
            let then_body = parse_human_block(lines, index, indent + 4)?;
            body.push(Stmt::If {
                cond: parse_expr(cond)?,
                then_body,
            });
            continue;
        }
        if text.starts_with("if ") {
            return Err(format!("if statement must end with ':': {text}"));
        }
        if let Some(loop_text) = text.strip_prefix("for ").and_then(|s| s.strip_suffix(':')) {
            let (var, range) =
                split_once(loop_text, " in ").ok_or_else(|| format!("invalid for loop: {text}"))?;
            let (start, end) =
                split_once(range, "..").ok_or_else(|| format!("invalid range: {text}"))?;
            *index += 1;
            let loop_body = parse_human_block(lines, index, indent + 4)?;
            body.push(Stmt::For {
                var: var.trim().to_string(),
                start: parse_expr(start)?,
                end: parse_expr(end)?,
                body: loop_body,
            });
            continue;
        }
        if text.starts_with("for ") {
            return Err(format!(
                "for loop must end with ':' and use 'for name in start .. end': {text}"
            ));
        }
        if let Some(target) = text.strip_prefix("asm ").and_then(|s| s.strip_suffix(':')) {
            *index += 1;
            let asm = parse_human_asm(target.trim(), lines, index, indent + 4);
            body.push(Stmt::Asm(asm));
            continue;
        }
        if text.starts_with("asm ") {
            return Err(format!("asm block must end with ':': {text}"));
        }
        body.push(parse_human_simple_stmt(text)?);
        *index += 1;
    }
    Ok(body)
}

fn parse_human_asm(
    target: &str,
    lines: &[SourceLine],
    index: &mut usize,
    indent: usize,
) -> AsmBlock {
    let mut asm = AsmBlock {
        target: target.to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        clobbers: Vec::new(),
        ops: Vec::new(),
    };
    while *index < lines.len() {
        let line = &lines[*index];
        if line.text.is_empty() {
            *index += 1;
            continue;
        }
        if line.indent < indent {
            break;
        }
        let text = line.text.as_str();
        if let Some(rest) = text.strip_prefix("in ") {
            if let Some((register, value)) = split_once(rest, ":") {
                asm.inputs.push(AsmBinding {
                    register: register.trim().to_string(),
                    value: value.trim().to_string(),
                });
            }
        } else if let Some(rest) = text.strip_prefix("out ") {
            if let Some((register, value)) = split_once(rest, ":") {
                asm.outputs.push(AsmBinding {
                    register: register.trim().to_string(),
                    value: value.trim().to_string(),
                });
            }
        } else if let Some(rest) = text.strip_prefix("clobber ") {
            asm.clobbers.push(rest.trim().to_string());
        } else if let Some(rest) = text.strip_prefix("op ") {
            asm.ops.push(
                rest.split_whitespace()
                    .map(|part| part.to_string())
                    .collect(),
            );
        }
        *index += 1;
    }
    asm
}

fn parse_human_simple_stmt(text: &str) -> Result<Stmt, String> {
    if let Some(expr) = text.strip_prefix("return ") {
        return Ok(Stmt::Return(Some(parse_expr(expr)?)));
    }
    if text == "return" {
        return Ok(Stmt::Return(None));
    }
    if let Some(expr) = text.strip_prefix("require ") {
        return Ok(Stmt::Require(parse_expr(expr)?));
    }
    if let Some(rest) = text.strip_prefix("let ") {
        let (left, expr) =
            split_assignment(rest).ok_or_else(|| format!("invalid let statement: {text}"))?;
        let (name, ty) = if let Some((name, ty)) = split_once(left, ":") {
            (name.trim().to_string(), Some(Type::parse(ty)))
        } else {
            (left.trim().to_string(), None)
        };
        return Ok(Stmt::Let {
            name,
            ty,
            expr: parse_expr(expr)?,
        });
    }
    if let Some((target, expr)) = split_assignment(text) {
        return Ok(Stmt::Assign {
            target: parse_expr(target)?,
            expr: parse_expr(expr)?,
        });
    }
    Ok(Stmt::Expr(parse_expr(text)?))
}

fn parse_agent_struct(line: &str) -> Result<StructDecl, String> {
    let rest = line
        .strip_prefix("S ")
        .ok_or_else(|| format!("invalid struct line: {line}"))?;
    let open = rest
        .find('{')
        .ok_or_else(|| format!("agent struct missing '{{': {line}"))?;
    let close = rest
        .rfind('}')
        .ok_or_else(|| format!("agent struct missing '}}': {line}"))?;
    let name = rest[..open].trim().to_string();
    let mut fields = Vec::new();
    for field in split_top_level(&rest[open + 1..close], ';') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let mut parts = field.split_whitespace();
        let name = parts
            .next()
            .ok_or_else(|| format!("invalid field in {line}"))?
            .to_string();
        let rest = parts.collect::<Vec<_>>().join(" ");
        let (ty, attrs) = split_type_and_inline_attrs(&rest)?;
        fields.push(Field {
            name,
            ty: Type::parse(ty),
            attrs,
        });
    }
    Ok(StructDecl::new(name, fields))
}

fn parse_agent_function_header(line: &str) -> Result<FunctionHeader, String> {
    let header = line.trim_end_matches('{').trim();
    let (unsafe_fn, rest) = if let Some(rest) = header.strip_prefix("UF ") {
        (true, rest)
    } else if let Some(rest) = header.strip_prefix("unsafe F ") {
        (true, rest)
    } else if let Some(rest) = header.strip_prefix("F ") {
        (false, rest)
    } else {
        return Err(format!("invalid agent function header: {line}"));
    };
    let open = rest
        .find('(')
        .ok_or_else(|| format!("agent function missing '(': {line}"))?;
    let close = rest
        .rfind(')')
        .ok_or_else(|| format!("agent function missing ')': {line}"))?;
    let name = rest[..open].trim().to_string();
    let params = parse_agent_params(&rest[open + 1..close])?;
    let after = rest[close + 1..].trim();
    let (ret, effects) = parse_return_and_effects(after)?;
    Ok((name, unsafe_fn, params, ret, effects))
}

fn parse_agent_params(raw: &str) -> Result<Vec<Param>, String> {
    let mut params = Vec::new();
    for part in split_top_level(raw, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut pieces = part.split_whitespace();
        let name = pieces
            .next()
            .ok_or_else(|| format!("invalid agent parameter: {part}"))?;
        let ty = pieces.collect::<Vec<_>>().join("");
        if ty.is_empty() {
            return Err(format!("agent parameter missing type: {part}"));
        }
        params.push(Param {
            name: name.to_string(),
            ty: Type::parse(&ty),
        });
    }
    Ok(params)
}

fn parse_agent_test_header(line: &str) -> Result<(String, bool), String> {
    let header = line.trim_end_matches('{').trim();
    let (xfail, rest) = if let Some(rest) = header.strip_prefix("XTEST ") {
        (true, rest)
    } else if let Some(rest) = header.strip_prefix("TEST ") {
        (false, rest)
    } else {
        return Err(format!("invalid agent test header: {line}"));
    };
    Ok((parse_quoted_name(rest.trim())?, xfail))
}

fn parse_agent_test_block(
    lines: &[String],
    index: &mut usize,
) -> Result<Vec<TestExpectation>, String> {
    let mut expectations = Vec::new();
    let mut closed = false;
    while *index < lines.len() {
        let line = lines[*index].trim();
        if line == "}" {
            *index += 1;
            closed = true;
            break;
        }
        expectations.push(parse_agent_test_expectation(line)?);
        *index += 1;
    }
    if !closed {
        return Err("agent test block is missing closing '}' before end of file".to_string());
    }
    if expectations.is_empty() {
        return Err("agent test block must contain at least one expectation".to_string());
    }
    Ok(expectations)
}

fn parse_agent_test_expectation(line: &str) -> Result<TestExpectation, String> {
    if let Some(code) = line.strip_prefix("EXPECT_DIAGNOSTIC ") {
        return Ok(TestExpectation::Diagnostic {
            code: code.trim().to_string(),
        });
    }
    if let Some(rest) = line.strip_prefix("EXPECT EQ ") {
        let (left, right) = split_expect_eq(rest)?;
        return Ok(TestExpectation::Expr {
            expr: Expr::Binary {
                op: "==".to_string(),
                left: Box::new(parse_expr(left)?),
                right: Box::new(parse_expr(right)?),
            },
        });
    }
    let Some(expr) = line.strip_prefix("EXPECT ") else {
        return Err(format!("invalid agent test expectation: {line}"));
    };
    Ok(TestExpectation::Expr {
        expr: parse_expr(expr)?,
    })
}

fn parse_agent_block(lines: &[String], index: &mut usize) -> Result<Vec<Stmt>, String> {
    let mut body = Vec::new();
    let mut closed = false;
    while *index < lines.len() {
        let line = lines[*index].trim();
        if line == "}" {
            *index += 1;
            closed = true;
            break;
        }
        if line.starts_with("S ")
            || line.starts_with("F ")
            || line.starts_with("UF ")
            || line.starts_with("unsafe F ")
            || line.starts_with("TEST ")
            || line.starts_with("XTEST ")
        {
            return Err(format!(
                "agent block is missing closing '}}' before: {line}"
            ));
        }
        if line.starts_with("IF ") && line.ends_with('{') {
            let cond = line
                .trim_end_matches('{')
                .trim()
                .strip_prefix("IF ")
                .unwrap()
                .trim();
            *index += 1;
            body.push(Stmt::If {
                cond: parse_expr(cond)?,
                then_body: parse_agent_block(lines, index)?,
            });
            continue;
        }
        if line.starts_with("FOR ") && line.ends_with('{') {
            let rest = line
                .trim_end_matches('{')
                .trim()
                .strip_prefix("FOR ")
                .unwrap()
                .trim();
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 3 {
                return Err(format!("invalid agent for loop: {line}"));
            }
            let var = parts[0].to_string();
            let start = parts[1];
            let end = parts[2..].join(" ");
            *index += 1;
            body.push(Stmt::For {
                var,
                start: parse_expr(start)?,
                end: parse_expr(&end)?,
                body: parse_agent_block(lines, index)?,
            });
            continue;
        }
        if line.starts_with("ASM ") && line.ends_with('{') {
            let target = line
                .trim_end_matches('{')
                .trim()
                .strip_prefix("ASM ")
                .unwrap()
                .trim()
                .to_string();
            *index += 1;
            body.push(Stmt::Asm(parse_agent_asm(target, lines, index)));
            continue;
        }
        body.push(parse_agent_simple_stmt(line)?);
        *index += 1;
    }
    if !closed {
        return Err("agent block is missing closing '}' before end of file".to_string());
    }
    Ok(body)
}

fn parse_agent_asm(target: String, lines: &[String], index: &mut usize) -> AsmBlock {
    let mut asm = AsmBlock {
        target,
        inputs: Vec::new(),
        outputs: Vec::new(),
        clobbers: Vec::new(),
        ops: Vec::new(),
    };
    while *index < lines.len() {
        let line = lines[*index].trim();
        if line == "}" {
            *index += 1;
            break;
        }
        if let Some(rest) = line.strip_prefix("IN ") {
            let mut parts = rest.split_whitespace();
            if let (Some(register), Some(value)) = (parts.next(), parts.next()) {
                asm.inputs.push(AsmBinding {
                    register: register.to_string(),
                    value: value.to_string(),
                });
            }
        } else if let Some(rest) = line.strip_prefix("OUT ") {
            let mut parts = rest.split_whitespace();
            if let (Some(register), Some(value)) = (parts.next(), parts.next()) {
                asm.outputs.push(AsmBinding {
                    register: register.to_string(),
                    value: value.to_string(),
                });
            }
        } else if let Some(rest) = line.strip_prefix("CLOBBER ") {
            asm.clobbers.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("OP ") {
            asm.ops.push(
                rest.split_whitespace()
                    .map(|part| part.to_string())
                    .collect(),
            );
        }
        *index += 1;
    }
    asm
}

fn parse_agent_simple_stmt(line: &str) -> Result<Stmt, String> {
    if let Some(expr) = line.strip_prefix("R ") {
        return Ok(Stmt::Return(Some(parse_expr(expr)?)));
    }
    if line == "R" {
        return Ok(Stmt::Return(None));
    }
    if let Some(expr) = line.strip_prefix("REQ ") {
        return Ok(Stmt::Require(parse_expr(expr)?));
    }
    if let Some(rest) = line.strip_prefix("LET ") {
        let (left, expr) =
            split_assignment(rest).ok_or_else(|| format!("invalid agent let: {line}"))?;
        let (name, ty) = if let Some((name, ty)) = split_once(left, ":") {
            (name.trim().to_string(), Some(Type::parse(ty)))
        } else {
            (left.trim().to_string(), None)
        };
        return Ok(Stmt::Let {
            name,
            ty,
            expr: parse_expr(expr)?,
        });
    }
    if let Some(rest) = line.strip_prefix("SET ") {
        let (target, expr) =
            split_assignment(rest).ok_or_else(|| format!("invalid agent set: {line}"))?;
        return Ok(Stmt::Assign {
            target: parse_expr(target)?,
            expr: parse_expr(expr)?,
        });
    }
    if let Some((target, expr)) = split_assignment(line) {
        return Ok(Stmt::Assign {
            target: parse_expr(target)?,
            expr: parse_expr(expr)?,
        });
    }
    Ok(Stmt::Expr(parse_expr(line)?))
}

fn tokenize_expr(source: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }
            tokens.push(chars[start..index].iter().collect());
            continue;
        }
        if ch.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < chars.len() {
                let next = chars[index];
                if next.is_ascii_digit()
                    || (next == '.' && index + 1 < chars.len() && chars[index + 1] != '.')
                {
                    index += 1;
                } else {
                    break;
                }
            }
            tokens.push(chars[start..index].iter().collect());
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
            if chars.get(index.saturating_sub(1)) != Some(&'"') {
                return Err(format!(
                    "unterminated string literal in expression '{source}'"
                ));
            }
            tokens.push(chars[start..index].iter().collect());
            continue;
        }
        if index + 1 < chars.len() {
            let two: String = chars[index..=index + 1].iter().collect();
            if matches!(
                two.as_str(),
                "==" | "!=" | "<=" | ">=" | "&&" | "||" | ">>" | "<<" | ".."
            ) {
                tokens.push(two);
                index += 2;
                continue;
            }
        }
        if "()[],.+-*/%<>&|!=".contains(ch) {
            tokens.push(ch.to_string());
            index += 1;
            continue;
        }
        return Err(format!(
            "unexpected character '{ch}' in expression '{source}'"
        ));
    }
    Ok(tokens)
}

struct ExprParser {
    tokens: Vec<String>,
    pos: usize,
}

impl ExprParser {
    fn parse_min_prec(&mut self, min_prec: u8) -> Result<Expr, String> {
        let mut left = self.parse_prefix()?;
        while let Some(op) = self.peek().map(|value| value.to_string()) {
            let Some(prec) = binary_precedence(&op) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.pos += 1;
            let right = self.parse_min_prec(prec + 1)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, String> {
        let token = self
            .next()
            .ok_or_else(|| "expected expression, found end of input".to_string())?
            .to_string();
        let mut expr = if token == "-" || token == "!" {
            Expr::Unary {
                op: token,
                expr: Box::new(self.parse_min_prec(10)?),
            }
        } else if token == "(" {
            let inner = self.parse_min_prec(0)?;
            self.expect(")")?;
            inner
        } else if token == "true" || token == "false" {
            Expr::Bool(token == "true")
        } else if token.starts_with('"') {
            Expr::String(parse_string_token(&token)?)
        } else if let Ok(value) = token.parse::<i64>() {
            Expr::Int(value)
        } else if token.contains('.') {
            let value = token
                .parse::<f64>()
                .map_err(|_| format!("invalid float literal: {token}"))?;
            Expr::Float(value)
        } else if is_identifier(&token) {
            if self.consume("(") {
                let args = self.parse_args()?;
                if is_primitive_name(&token) && args.len() == 1 {
                    Expr::Cast {
                        ty: Type::parse(&token),
                        expr: Box::new(args.into_iter().next().unwrap()),
                    }
                } else {
                    // Expand a two-letter prelude alias to its canonical
                    // builtin name; the AST always stores the canonical.
                    let name = crate::prelude::alias_to_canonical(&token)
                        .map(str::to_string)
                        .unwrap_or(token);
                    Expr::Call { name, args }
                }
            } else {
                Expr::Var(token)
            }
        } else {
            return Err(format!("unexpected token in expression: {token}"));
        };

        loop {
            if self.consume(".") {
                let field = self
                    .next()
                    .ok_or_else(|| "expected field after '.'".to_string())?
                    .to_string();
                expr = Expr::Field {
                    base: Box::new(expr),
                    field,
                };
                continue;
            }
            if self.consume("[") {
                let index = self.parse_min_prec(0)?;
                self.expect("]")?;
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                };
                continue;
            }
            break;
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.consume(")") {
            return Ok(args);
        }
        loop {
            args.push(self.parse_min_prec(0)?);
            if self.consume(")") {
                break;
            }
            self.expect(",")?;
        }
        Ok(args)
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|token| token.as_str())
    }

    fn next(&mut self) -> Option<String> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn consume(&mut self, expected: &str) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &str) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(format!(
                "expected token '{expected}', found {:?}",
                self.peek()
            ))
        }
    }
}

fn binary_precedence(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        "<" | ">" | "<=" | ">=" => 4,
        "|" => 5,
        "&" => 6,
        "<<" | ">>" => 7,
        "+" | "-" => 8,
        "*" | "/" | "%" => 9,
        _ => return None,
    })
}

fn is_identifier(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|ch| ch.is_ascii_alphabetic() || ch == '_')
        .unwrap_or(false)
}

fn parse_string_token(token: &str) -> Result<String, String> {
    let inner = token
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| format!("invalid string literal: {token}"))?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| format!("invalid string escape in literal: {token}"))?;
        match escaped {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => return Err(format!("unsupported string escape '\\{other}' in {token}")),
        }
    }
    Ok(out)
}

fn split_once<'a>(value: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let index = value.find(needle)?;
    Some((&value[..index], &value[index + needle.len()..]))
}

fn split_expect_eq(value: &str) -> Result<(&str, &str), String> {
    let mut bracket = 0i32;
    let mut paren = 0i32;
    let mut last_space = None;
    for (idx, ch) in value.char_indices() {
        match ch {
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '(' => paren += 1,
            ')' => paren -= 1,
            ' ' if bracket == 0 && paren == 0 => last_space = Some(idx),
            _ => {}
        }
    }
    let Some(idx) = last_space else {
        return Err(format!(
            "EXPECT EQ needs left and right expressions: {value}"
        ));
    };
    let left = value[..idx].trim();
    let right = value[idx + 1..].trim();
    if left.is_empty() || right.is_empty() {
        return Err(format!(
            "EXPECT EQ needs left and right expressions: {value}"
        ));
    }
    Ok((left, right))
}

fn split_assignment(value: &str) -> Option<(&str, &str)> {
    let chars: Vec<char> = value.chars().collect();
    let mut paren = 0i32;
    let mut bracket = 0i32;
    for (index, ch) in chars.iter().enumerate() {
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '=' if paren == 0 && bracket == 0 => {
                let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
                let next = chars.get(index + 1).copied();
                if prev == Some('=') || prev == Some('<') || prev == Some('>') || prev == Some('!')
                {
                    continue;
                }
                if next == Some('=') {
                    continue;
                }
                return Some((value[..index].trim(), value[index + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}

pub fn split_top_level(value: &str, delimiter: char) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            _ if ch == delimiter && paren == 0 && bracket == 0 => {
                result.push(&value[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    result.push(&value[start..]);
    result
}
