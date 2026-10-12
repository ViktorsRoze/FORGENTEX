//! Deterministic project-scoped token codebooks.
//!
//! The semantic tier learns recurring identifiers, AST-derived units, and
//! delimiter-bounded naming morphology from canonical agent renderings. Every
//! handle is reversible, content-addressed, and admitted to exchange only when
//! the complete payload is smaller.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::format::format_agent;
use crate::interp::{parse_json_value, JsonValue};

pub const CODEBOOK_SCHEMA: &str = "forgentex.codebook.v1";
pub const DEFAULT_CODEBOOK_BUDGET: usize = 1024;
pub const MIN_CODEBOOK_BUDGET: usize = 512;
pub const MAX_CODEBOOK_BUDGET: usize = 65_536;

const SIGILS: &[char] = &['§', '¶', '‡', '¤', '❖'];
const MIN_IDENTIFIER_CHARS: usize = 6;
const MIN_FREQUENCY: usize = 2;
const MAX_MORPHOLOGY_COMPONENTS: usize = 6;
const MAX_MORPHOLOGY_WORD_CHARS: usize = 128;
const PARAMETER_FRAME_TOKENS: usize = 4;
const MAX_PARAMETER_SLOTS: usize = 2;
const MAX_PARAMETER_LEAVES: usize = 16;
const MAX_MULTI_PARAMETER_LEAVES: usize = 8;
const MAX_PARAMETER_TEMPLATE_CHARS: usize = 256;
const MAX_PARAMETER_ARGUMENT_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodebookEntry {
    pub kind: String,
    pub code: String,
    pub expansion: String,
    pub frequency: usize,
    pub expansion_tokens: usize,
    pub code_tokens: usize,
    pub corpus_saving: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectCodebook {
    pub schema: String,
    pub version: u32,
    pub tokenizer: String,
    pub tier: String,
    pub mode: String,
    pub sigil: String,
    pub generation: u32,
    pub parent_hash: Option<String>,
    pub retired_codes: Vec<String>,
    pub project: String,
    pub corpus_hash: String,
    pub corpus_files: usize,
    pub budget_tokens: usize,
    pub codebook_tokens: usize,
    pub corpus_tokens: usize,
    pub encoded_tokens: usize,
    pub gross_savings: usize,
    pub net_first_interaction: i64,
    pub break_even_interactions: Option<usize>,
    pub status: String,
    pub entries: Vec<CodebookEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodebookCheck {
    pub schema: String,
    pub status: String,
    pub project: String,
    pub codebook: String,
    pub expected_corpus_hash: String,
    pub actual_corpus_hash: String,
    pub entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodebookMeasure {
    pub schema: String,
    pub status: String,
    pub project: String,
    pub corpus_hash: String,
    pub codebook_corpus_hash: String,
    pub corpus_tokens: usize,
    pub encoded_tokens: usize,
    pub gross_savings: usize,
    pub codebook_tokens: usize,
    pub break_even_interactions: Option<usize>,
    pub entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodebookPayload {
    pub text: String,
    pub decision: String,
    pub plain_tokens: usize,
    pub encoded_tokens: usize,
}

#[derive(Clone, Debug)]
struct ProjectCorpus {
    project: String,
    files: usize,
    text: String,
    hash: String,
    semantic_units: Vec<(String, String)>,
    parameter_units: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
struct Candidate {
    kind: String,
    expansion: String,
    frequency: usize,
    expansion_tokens: usize,
    code: String,
    code_tokens: usize,
    corpus_saving: usize,
}

#[derive(Clone, Debug)]
struct CodebookEvolution {
    generation: u32,
    parent_hash: Option<String>,
    retired_codes: Vec<String>,
}

pub fn build(input: &Path, budget_tokens: usize, cryptic: bool) -> Result<ProjectCodebook, String> {
    build_with_previous(input, budget_tokens, cryptic, None)
}

pub fn build_with_previous(
    input: &Path,
    budget_tokens: usize,
    cryptic: bool,
    previous: Option<&ProjectCodebook>,
) -> Result<ProjectCodebook, String> {
    validate_budget(budget_tokens)?;
    let corpus = project_corpus(input)?;
    let mode = if cryptic { "cryptic" } else { "mnemonic" };
    let sigil = match previous {
        Some(codebook) => {
            let sigil = codebook
                .sigil
                .chars()
                .next()
                .ok_or_else(|| "previous codebook has an empty sigil".to_string())?;
            if corpus.text.contains(sigil) {
                return Err(format!(
                    "project corpus now contains reserved codebook sigil '{sigil}'"
                ));
            }
            sigil
        }
        None => choose_sigil(&corpus.text)
            .ok_or_else(|| "codebook corpus contains every reserved handle sigil".to_string())?,
    };
    validate_previous(previous, &corpus, mode, sigil)?;
    if let Some(previous) = previous {
        if previous.corpus_hash == corpus.hash && previous.budget_tokens == budget_tokens {
            return Ok(previous.clone());
        }
    }
    let generation = previous
        .map(|codebook| codebook.generation.saturating_add(1))
        .unwrap_or(1);
    let parent_hash = previous.map(ProjectCodebook::artifact_hash);
    let inherited_retired = previous
        .map(|codebook| codebook.retired_codes.clone())
        .unwrap_or_default();
    let candidates = candidates(
        &corpus.text,
        &corpus.semantic_units,
        &corpus.parameter_units,
        sigil,
        cryptic,
        previous,
    );
    let mut entries = Vec::new();
    let mut selected_savings = 0usize;
    for mut candidate in candidates {
        let inherited = previous.is_some_and(|codebook| {
            codebook
                .entries
                .iter()
                .any(|entry| entry.expansion == candidate.expansion)
        });
        if !inherited {
            let mut used_codes = previous
                .into_iter()
                .flat_map(|codebook| {
                    codebook
                        .entries
                        .iter()
                        .map(|entry| entry.code.clone())
                        .chain(codebook.retired_codes.iter().cloned())
                })
                .chain(
                    entries
                        .iter()
                        .map(|entry: &CodebookEntry| entry.code.clone()),
                )
                .collect::<BTreeSet<_>>();
            candidate.code = code_for(
                &candidate.kind,
                &candidate.expansion,
                sigil,
                cryptic,
                used_codes.len(),
                &mut used_codes,
            );
            candidate.code_tokens = crate::tokens::estimate(&candidate.code);
            candidate.corpus_saving = candidate
                .expansion_tokens
                .saturating_sub(
                    candidate.code_tokens
                        + parameter_framing_tokens(&candidate.kind, &candidate.expansion),
                )
                .saturating_mul(candidate.frequency);
            if candidate.corpus_saving == 0 {
                continue;
            }
        }
        let mut proposed = entries.clone();
        proposed.push(CodebookEntry {
            kind: candidate.kind,
            code: candidate.code,
            expansion: candidate.expansion,
            frequency: candidate.frequency,
            expansion_tokens: candidate.expansion_tokens,
            code_tokens: candidate.code_tokens,
            corpus_saving: candidate.corpus_saving,
        });
        let retired_codes = evolved_retired_codes(previous, &proposed, &inherited_retired);
        let report = finalize_report(
            &corpus,
            budget_tokens,
            mode,
            sigil,
            CodebookEvolution {
                generation,
                parent_hash: parent_hash.clone(),
                retired_codes,
            },
            proposed,
        );
        if report.codebook_tokens <= budget_tokens && report.gross_savings > selected_savings {
            selected_savings = report.gross_savings;
            entries = report.entries;
        }
    }
    let retired_codes = evolved_retired_codes(previous, &entries, &inherited_retired);
    let report = finalize_report(
        &corpus,
        budget_tokens,
        mode,
        sigil,
        CodebookEvolution {
            generation,
            parent_hash,
            retired_codes,
        },
        entries,
    );
    if report.codebook_tokens > budget_tokens {
        return Err(format!(
            "codebook budget {budget_tokens} is smaller than the {}-token artifact envelope",
            report.codebook_tokens
        ));
    }
    Ok(report)
}

pub fn parse(source: &str) -> Result<ProjectCodebook, String> {
    let value = parse_json_value(source).map_err(|error| format!("codebook invalid: {error}"))?;
    let JsonValue::Object(root) = value else {
        return Err("codebook invalid: expected JSON object".to_string());
    };
    let schema = string_field(&root, "schema")?;
    if schema != CODEBOOK_SCHEMA {
        return Err(format!("codebook invalid: unsupported schema '{schema}'"));
    }
    let version = usize_field(&root, "version")?;
    if version != 1 {
        return Err(format!("codebook invalid: unsupported version {version}"));
    }
    let tokenizer = string_field(&root, "tokenizer")?;
    if tokenizer != "heuristic-v1" {
        return Err(format!(
            "codebook invalid: unsupported tokenizer '{tokenizer}'"
        ));
    }
    let tier = string_field(&root, "tier")?;
    if !matches!(tier.as_str(), "identifiers" | "semantic") {
        return Err(format!("codebook invalid: unsupported tier '{tier}'"));
    }
    let mode = string_field(&root, "mode")?;
    if !matches!(mode.as_str(), "mnemonic" | "cryptic") {
        return Err(format!("codebook invalid: unsupported mode '{mode}'"));
    }
    let sigil = string_field(&root, "sigil")?;
    if sigil.chars().count() != 1 {
        return Err("codebook invalid: sigil must be one character".to_string());
    }
    let entries = match root.get("entries") {
        Some(JsonValue::Array(values)) => values
            .iter()
            .map(parse_entry)
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("codebook invalid: missing entries array".to_string()),
    };
    validate_entries(&entries, &sigil)?;
    let generation = usize_field(&root, "generation")?;
    if generation == 0 || generation > u32::MAX as usize {
        return Err("codebook invalid: generation must be a positive u32".to_string());
    }
    let parent_hash = optional_string_field(&root, "parent_hash")?;
    if let Some(hash) = &parent_hash {
        validate_sha256("parent_hash", hash)?;
    }
    if (generation == 1) != parent_hash.is_none() {
        return Err(
            "codebook invalid: generation 1 must have no parent; later generations require one"
                .to_string(),
        );
    }
    let retired_codes = string_array_field(&root, "retired_codes")?;
    validate_retired_codes(&entries, &retired_codes, &sigil)?;
    let corpus_hash = string_field(&root, "corpus_hash")?;
    validate_sha256("corpus_hash", &corpus_hash)?;
    let break_even_interactions = optional_usize_field(&root, "break_even_interactions")?;
    let budget_tokens = usize_field(&root, "budget_tokens")?;
    validate_budget(budget_tokens).map_err(|error| format!("codebook invalid: {error}"))?;
    let codebook_tokens = usize_field(&root, "codebook_tokens")?;
    let corpus_tokens = usize_field(&root, "corpus_tokens")?;
    let encoded_tokens = usize_field(&root, "encoded_tokens")?;
    let gross_savings = usize_field(&root, "gross_savings")?;
    let net_first_interaction = int_field(&root, "net_first_interaction")?;
    if codebook_tokens > budget_tokens
        || encoded_tokens > corpus_tokens
        || gross_savings != corpus_tokens - encoded_tokens
        || net_first_interaction != gross_savings as i64 - codebook_tokens as i64
        || break_even_interactions != break_even(codebook_tokens, gross_savings)
    {
        return Err("codebook invalid: inconsistent report metrics".to_string());
    }
    let report = ProjectCodebook {
        schema,
        version: version as u32,
        tokenizer,
        tier,
        mode,
        sigil,
        generation: generation as u32,
        parent_hash,
        retired_codes,
        project: string_field(&root, "project")?,
        corpus_hash,
        corpus_files: usize_field(&root, "corpus_files")?,
        budget_tokens,
        codebook_tokens,
        corpus_tokens,
        encoded_tokens,
        gross_savings,
        net_first_interaction,
        break_even_interactions,
        status: string_field(&root, "status")?,
        entries,
    };
    if crate::tokens::estimate(&report.to_json()) != report.codebook_tokens {
        return Err(
            "codebook invalid: codebook_tokens does not match serialized artifact".to_string(),
        );
    }
    Ok(report)
}

pub fn read(path: &Path) -> Result<ProjectCodebook, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read codebook {}: {error}", path.display()))?;
    parse(&source)
}

pub fn check(input: &Path, path: &Path) -> Result<CodebookCheck, String> {
    let actual = read(path)?;
    let measured = measure(input, &actual)?;
    let pass = measured.status == "current"
        && measured.project == actual.project
        && measured.corpus_tokens == actual.corpus_tokens
        && measured.encoded_tokens == actual.encoded_tokens
        && measured.gross_savings == actual.gross_savings
        && measured.break_even_interactions == actual.break_even_interactions;
    Ok(CodebookCheck {
        schema: "forgentex.codebook_check.v1".to_string(),
        status: if pass { "pass" } else { "stale" }.to_string(),
        project: measured.project,
        codebook: path.display().to_string().replace('\\', "/"),
        expected_corpus_hash: measured.corpus_hash,
        actual_corpus_hash: actual.corpus_hash,
        entries: actual.entries.len(),
    })
}

pub fn measure(input: &Path, codebook: &ProjectCodebook) -> Result<CodebookMeasure, String> {
    let corpus = project_corpus(input)?;
    let encoded = apply_entries(&corpus.text, &codebook.entries);
    let corpus_tokens = crate::tokens::estimate(&corpus.text);
    let encoded_tokens = crate::tokens::estimate(&encoded);
    let gross_savings = corpus_tokens.saturating_sub(encoded_tokens);
    Ok(CodebookMeasure {
        schema: "forgentex.codebook_measure.v1".to_string(),
        status: if corpus.hash == codebook.corpus_hash {
            "current"
        } else {
            "stale"
        }
        .to_string(),
        project: corpus.project,
        corpus_hash: corpus.hash,
        codebook_corpus_hash: codebook.corpus_hash.clone(),
        corpus_tokens,
        encoded_tokens,
        gross_savings,
        codebook_tokens: codebook.codebook_tokens,
        break_even_interactions: break_even(codebook.codebook_tokens, gross_savings),
        entries: codebook.entries.len(),
    })
}

fn validate_budget(value: usize) -> Result<(), String> {
    if !(MIN_CODEBOOK_BUDGET..=MAX_CODEBOOK_BUDGET).contains(&value) {
        return Err(format!(
            "codebook budget must be between {MIN_CODEBOOK_BUDGET} and {MAX_CODEBOOK_BUDGET} tokens"
        ));
    }
    Ok(())
}

fn project_corpus(input: &Path) -> Result<ProjectCorpus, String> {
    let project = crate::project::load_project(input)?;
    let mut semantic_units = Vec::new();
    for module in project
        .modules
        .iter()
        .filter(|module| !module.standard && !module.dependency)
    {
        collect_semantic_units(&module.program, &mut semantic_units);
    }
    let (mut parameter_units, mut semantic_units): (Vec<_>, Vec<_>) = semantic_units
        .into_iter()
        .partition(|(kind, _)| is_parameter_kind(kind));
    semantic_units.sort();
    semantic_units.dedup();
    parameter_units.sort();
    let mut modules = project
        .modules
        .iter()
        .filter(|module| !module.standard && !module.dependency)
        .map(|module| {
            (
                module
                    .path
                    .strip_prefix(&project.package_root)
                    .unwrap_or(&module.path)
                    .display()
                    .to_string()
                    .replace('\\', "/"),
                format_agent(&module.program),
            )
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.0.cmp(&right.0));
    let mut text = String::new();
    for (path, source) in &modules {
        text.push_str("#FILE ");
        text.push_str(path);
        text.push('\n');
        text.push_str(source);
        if !source.ends_with('\n') {
            text.push('\n');
        }
    }
    let hash = crate::crypto::sha256_hex(text.as_bytes());
    Ok(ProjectCorpus {
        project: project.manifest.name,
        files: modules.len(),
        text,
        hash,
        semantic_units,
        parameter_units,
    })
}

fn collect_semantic_units(program: &crate::ast::Program, out: &mut Vec<(String, String)>) {
    use crate::ast::Item;
    for item in &program.items {
        match item {
            // Carried sections hold content, not code to learn from.
            Item::Data(_) | Item::Blob(_) | Item::Note(_) => {}
            Item::Struct(decl) => {
                for field in &decl.fields {
                    collect_type_unit(&field.ty, out);
                }
            }
            Item::Enum(decl) => {
                for variant in &decl.variants {
                    if let Some(ty) = &variant.payload {
                        collect_type_unit(ty, out);
                    }
                }
            }
            Item::Trait(decl) => {
                for constant in &decl.constants {
                    collect_type_unit(&constant.ty, out);
                }
                for method in &decl.methods {
                    for param in &method.params {
                        collect_type_unit(&param.ty, out);
                    }
                    collect_type_unit(&method.ret, out);
                }
            }
            Item::TraitImpl(decl) => {
                collect_type_unit(&decl.for_type, out);
                for binding in &decl.associated_types {
                    collect_type_unit(&binding.ty, out);
                }
            }
            Item::Function(function) => {
                for param in &function.params {
                    collect_type_unit(&param.ty, out);
                }
                collect_type_unit(&function.ret, out);
                collect_effect_unit(&function.effects, false, out);
                collect_stmt_units(&function.body, out);
            }
            Item::Capsule(capsule) => {
                for binding in capsule.inputs.iter().chain(&capsule.outputs) {
                    collect_type_unit(&binding.ty, out);
                }
                collect_effect_unit(&capsule.effects, true, out);
            }
            Item::Share(share) => collect_type_unit(&share.ty, out),
            Item::Pipeline(_) | Item::Test(_) => {}
        }
    }
}

fn collect_type_unit(ty: &crate::ast::Type, out: &mut Vec<(String, String)>) {
    let expansion = ty.to_agent();
    if crate::tokens::estimate(&expansion) > 2 {
        out.push(("type".to_string(), expansion));
    }
}

fn collect_effect_unit(effects: &[String], capsule: bool, out: &mut Vec<(String, String)>) {
    if effects.is_empty() {
        return;
    }
    let expansion = if capsule {
        format!("effects [{}]", effects.join(", "))
    } else {
        format!("effects[{}]", effects.join(","))
    };
    if crate::tokens::estimate(&expansion) > 2 {
        out.push(("effect_set".to_string(), expansion));
    }
}

fn collect_stmt_units(statements: &[crate::ast::Stmt], out: &mut Vec<(String, String)>) {
    use crate::ast::Stmt;
    for statement in statements {
        if let Some(pattern) = statement_pattern(statement) {
            if crate::tokens::estimate(&pattern) > 2 {
                out.push(("statement".to_string(), pattern));
            }
        }
        for pattern in parameterized_statement_patterns(statement) {
            out.push(("statement_template".to_string(), pattern));
        }
        match statement {
            Stmt::Let { ty, expr, .. } => {
                if let Some(ty) = ty {
                    collect_type_unit(ty, out);
                }
                collect_expr_units(expr, out);
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    collect_expr_units(expr, out);
                }
            }
            Stmt::Require(expr) | Stmt::Expr(expr) => collect_expr_units(expr, out),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                collect_expr_units(cond, out);
                collect_stmt_units(then_body, out);
                collect_stmt_units(else_body, out);
            }
            Stmt::For {
                start, end, body, ..
            } => {
                collect_expr_units(start, out);
                collect_expr_units(end, out);
                collect_stmt_units(body, out);
            }
            Stmt::While { cond, body } => {
                collect_expr_units(cond, out);
                collect_stmt_units(body, out);
            }
            Stmt::Match { scrutinee, arms } => {
                collect_expr_units(scrutinee, out);
                for arm in arms {
                    collect_stmt_units(&arm.body, out);
                }
            }
            Stmt::Assign { target, expr } => {
                collect_expr_units(target, out);
                collect_expr_units(expr, out);
            }
            Stmt::Break | Stmt::Continue | Stmt::Asm(_) => {}
        }
    }
}

fn collect_expr_units(expr: &crate::ast::Expr, out: &mut Vec<(String, String)>) {
    use crate::ast::Expr;
    let expansion = crate::format::format_expr(expr);
    if crate::tokens::estimate(&expansion) > 2 {
        out.push(("expression".to_string(), expansion));
    }
    for pattern in parameterized_expression_patterns(expr) {
        out.push(("expression_template".to_string(), pattern));
    }
    match expr {
        Expr::TraitConst { for_type, .. } => collect_type_unit(for_type, out),
        Expr::Unary { expr, .. } => collect_expr_units(expr, out),
        Expr::Binary { left, right, .. } => {
            collect_expr_units(left, out);
            collect_expr_units(right, out);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_units(arg, out);
            }
        }
        Expr::Cast { ty, expr } => {
            collect_type_unit(ty, out);
            collect_expr_units(expr, out);
        }
        Expr::Field { base, .. } => collect_expr_units(base, out),
        Expr::Index { base, index } => {
            collect_expr_units(base, out);
            collect_expr_units(index, out);
        }
        Expr::SliceRange { base, start, end } => {
            collect_expr_units(base, out);
            collect_expr_units(start, out);
            collect_expr_units(end, out);
        }
        Expr::Lambda { params, body } => {
            for param in params {
                collect_type_unit(&param.ty, out);
            }
            collect_expr_units(body, out);
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => {}
    }
}

fn statement_pattern(statement: &crate::ast::Stmt) -> Option<String> {
    use crate::ast::Stmt;
    match statement {
        Stmt::Let { name, ty, expr } => Some(match ty {
            Some(ty) => format!(
                "LET {name}: {} = {}",
                ty.to_agent(),
                crate::format::format_expr(expr)
            ),
            None => format!("LET {name} = {}", crate::format::format_expr(expr)),
        }),
        Stmt::Return(Some(expr)) => Some(format!("R {}", crate::format::format_expr(expr))),
        Stmt::Return(None) => Some("R".to_string()),
        Stmt::Require(expr) => Some(format!("REQ {}", crate::format::format_expr(expr))),
        Stmt::Assign { target, expr } => Some(format!(
            "SET {} = {}",
            crate::format::format_expr(target),
            crate::format::format_expr(expr)
        )),
        Stmt::Expr(expr) => Some(crate::format::format_expr(expr)),
        Stmt::Break => Some("BR".to_string()),
        Stmt::Continue => Some("CN".to_string()),
        Stmt::If { .. }
        | Stmt::For { .. }
        | Stmt::While { .. }
        | Stmt::Match { .. }
        | Stmt::Asm(_) => None,
    }
}

fn parameterized_expression_patterns(expr: &crate::ast::Expr) -> Vec<String> {
    let original = crate::format::format_expr(expr);
    let mut patterns = BTreeSet::new();
    for target in 0..MAX_PARAMETER_LEAVES {
        let mut candidate = expr.clone();
        let mut remaining = target;
        let Some(argument) = replace_parameter_leaf(&mut candidate, &mut remaining, "$0") else {
            break;
        };
        let template = crate::format::format_expr(&candidate);
        admit_parameter_template(&mut patterns, template, vec![argument], &original);
    }
    for first in 0..MAX_MULTI_PARAMETER_LEAVES {
        for second in first + 1..MAX_MULTI_PARAMETER_LEAVES {
            let mut candidate = expr.clone();
            let mut remaining = second;
            let Some(second_argument) =
                replace_parameter_leaf(&mut candidate, &mut remaining, "$1")
            else {
                break;
            };
            let mut remaining = first;
            let Some(first_argument) = replace_parameter_leaf(&mut candidate, &mut remaining, "$0")
            else {
                break;
            };
            let template = crate::format::format_expr(&candidate);
            admit_parameter_template(
                &mut patterns,
                template,
                vec![first_argument, second_argument],
                &original,
            );
        }
    }
    patterns.into_iter().collect()
}

fn parameterized_statement_patterns(statement: &crate::ast::Stmt) -> Vec<String> {
    let Some(original) = statement_pattern(statement) else {
        return Vec::new();
    };
    let mut patterns = BTreeSet::new();
    for target in 0..MAX_PARAMETER_LEAVES {
        let mut candidate = statement.clone();
        let mut remaining = target;
        let Some(argument) = replace_statement_parameter_leaf(&mut candidate, &mut remaining, "$0")
        else {
            break;
        };
        let Some(template) = statement_pattern(&candidate) else {
            break;
        };
        admit_parameter_template(&mut patterns, template, vec![argument], &original);
    }
    for first in 0..MAX_MULTI_PARAMETER_LEAVES {
        for second in first + 1..MAX_MULTI_PARAMETER_LEAVES {
            let mut candidate = statement.clone();
            let mut remaining = second;
            let Some(second_argument) =
                replace_statement_parameter_leaf(&mut candidate, &mut remaining, "$1")
            else {
                break;
            };
            let mut remaining = first;
            let Some(first_argument) =
                replace_statement_parameter_leaf(&mut candidate, &mut remaining, "$0")
            else {
                break;
            };
            let Some(template) = statement_pattern(&candidate) else {
                break;
            };
            admit_parameter_template(
                &mut patterns,
                template,
                vec![first_argument, second_argument],
                &original,
            );
        }
    }
    patterns.into_iter().collect()
}

fn admit_parameter_template(
    patterns: &mut BTreeSet<String>,
    template: String,
    arguments: Vec<String>,
    original: &str,
) {
    if eligible_parameter_template(&template)
        && arguments
            .iter()
            .all(|argument| eligible_parameter_argument(argument))
        && expand_parameter_template(&template, &arguments).as_deref() == Some(original)
    {
        patterns.insert(template);
    }
}

fn replace_statement_parameter_leaf(
    statement: &mut crate::ast::Stmt,
    remaining: &mut usize,
    slot: &str,
) -> Option<String> {
    use crate::ast::Stmt;
    match statement {
        Stmt::Let { expr, .. }
        | Stmt::Return(Some(expr))
        | Stmt::Require(expr)
        | Stmt::Expr(expr) => replace_parameter_leaf(expr, remaining, slot),
        Stmt::Assign { target, expr } => replace_parameter_leaf(target, remaining, slot)
            .or_else(|| replace_parameter_leaf(expr, remaining, slot)),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::If { .. }
        | Stmt::For { .. }
        | Stmt::While { .. }
        | Stmt::Match { .. }
        | Stmt::Asm(_) => None,
    }
}

fn replace_parameter_leaf(
    expr: &mut crate::ast::Expr,
    remaining: &mut usize,
    slot: &str,
) -> Option<String> {
    use crate::ast::Expr;
    if matches!(
        expr,
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_)
    ) {
        if *remaining == 0 {
            let argument = crate::format::format_expr(expr);
            *expr = Expr::Var(slot.to_string());
            return Some(argument);
        }
        *remaining -= 1;
        return None;
    }
    match expr {
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            replace_parameter_leaf(expr, remaining, slot)
        }
        Expr::Binary { left, right, .. } => replace_parameter_leaf(left, remaining, slot)
            .or_else(|| replace_parameter_leaf(right, remaining, slot)),
        Expr::Call { args, .. } => args
            .iter_mut()
            .find_map(|argument| replace_parameter_leaf(argument, remaining, slot)),
        Expr::Index { base, index } => replace_parameter_leaf(base, remaining, slot)
            .or_else(|| replace_parameter_leaf(index, remaining, slot)),
        Expr::SliceRange {
            base, start, end, ..
        } => replace_parameter_leaf(base, remaining, slot)
            .or_else(|| replace_parameter_leaf(start, remaining, slot))
            .or_else(|| replace_parameter_leaf(end, remaining, slot)),
        Expr::Lambda { body, .. } => replace_parameter_leaf(body, remaining, slot),
        Expr::TraitConst { .. }
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_) => None,
    }
}

fn eligible_parameter_template(template: &str) -> bool {
    if template.chars().count() > MAX_PARAMETER_TEMPLATE_CHARS {
        return false;
    }
    parameter_template_segments(template).is_some()
}

fn parameter_template_segments(template: &str) -> Option<Vec<&str>> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for slot in 0..MAX_PARAMETER_SLOTS {
        let marker = format!("${slot}");
        let Some(relative) = template[cursor..].find(&marker) else {
            break;
        };
        let marker_start = cursor + relative;
        let segment = &template[cursor..marker_start];
        if segment.is_empty() || contains_parameter_marker(segment) {
            return None;
        }
        segments.push(segment);
        cursor = marker_start + marker.len();
    }
    if segments.is_empty() {
        return None;
    }
    let tail = &template[cursor..];
    if tail.is_empty() || contains_parameter_marker(tail) {
        return None;
    }
    segments.push(tail);
    Some(segments)
}

fn contains_parameter_marker(value: &str) -> bool {
    value
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'$' && pair[1].is_ascii_digit())
}

fn expand_parameter_template(template: &str, arguments: &[String]) -> Option<String> {
    let segments = parameter_template_segments(template)?;
    if segments.len() != arguments.len() + 1 {
        return None;
    }
    let mut expanded = String::new();
    for (index, segment) in segments.iter().enumerate() {
        expanded.push_str(segment);
        if let Some(argument) = arguments.get(index) {
            expanded.push_str(argument);
        }
    }
    Some(expanded)
}

fn parameter_template_arity(template: &str) -> Option<usize> {
    parameter_template_segments(template).map(|segments| segments.len() - 1)
}

fn eligible_parameter_argument(argument: &str) -> bool {
    !argument.is_empty()
        && argument.len() <= MAX_PARAMETER_ARGUMENT_BYTES
        && parameter_atom_end(argument, 0) == Some(argument.len())
}

fn is_parameter_kind(kind: &str) -> bool {
    matches!(kind, "expression_template" | "statement_template")
}

fn parameter_framing_tokens(kind: &str, expansion: &str) -> usize {
    if is_parameter_kind(kind) {
        parameter_template_arity(expansion)
            .unwrap_or(0)
            .saturating_mul(PARAMETER_FRAME_TOKENS)
    } else {
        0
    }
}

fn validate_previous(
    previous: Option<&ProjectCodebook>,
    corpus: &ProjectCorpus,
    mode: &str,
    sigil: char,
) -> Result<(), String> {
    let Some(previous) = previous else {
        return Ok(());
    };
    if previous.project != corpus.project {
        return Err(format!(
            "previous codebook project '{}' does not match '{}'",
            previous.project, corpus.project
        ));
    }
    if previous.tokenizer != "heuristic-v1"
        || !matches!(previous.tier.as_str(), "identifiers" | "semantic")
        || previous.mode != mode
        || previous.sigil != sigil.to_string()
    {
        return Err(
            "previous codebook tokenizer, tier, mode, and sigil must match the new build"
                .to_string(),
        );
    }
    Ok(())
}

fn evolved_retired_codes(
    previous: Option<&ProjectCodebook>,
    selected: &[CodebookEntry],
    inherited: &[String],
) -> Vec<String> {
    let selected_expansions = selected
        .iter()
        .map(|entry| entry.expansion.as_str())
        .collect::<BTreeSet<_>>();
    let mut retired = inherited.iter().cloned().collect::<BTreeSet<_>>();
    if let Some(previous) = previous {
        for entry in &previous.entries {
            if !selected_expansions.contains(entry.expansion.as_str()) {
                retired.insert(entry.code.clone());
            }
        }
    }
    retired.into_iter().collect()
}

fn candidates(
    corpus: &str,
    semantic_units: &[(String, String)],
    parameter_units: &[(String, String)],
    sigil: char,
    cryptic: bool,
    previous: Option<&ProjectCodebook>,
) -> Vec<Candidate> {
    let mut units = BTreeMap::<String, (String, usize)>::new();
    let words = word_tokens(corpus);
    for token in &words {
        if eligible_identifier(token) {
            let entry = units
                .entry(token.clone())
                .or_insert_with(|| ("identifier".to_string(), 0));
            entry.1 += 1;
        }
    }
    for (expansion, frequency) in morphology_units(&words) {
        units
            .entry(expansion)
            .or_insert_with(|| ("morphology".to_string(), frequency));
    }
    for (kind, expansion) in semantic_units {
        if units.contains_key(expansion) {
            continue;
        }
        let frequency = corpus.match_indices(expansion).count();
        if frequency >= MIN_FREQUENCY {
            units.insert(expansion.clone(), (kind.clone(), frequency));
        }
    }
    for (kind, template) in parameter_units {
        let entry = units
            .entry(template.clone())
            .or_insert_with(|| (kind.clone(), 0));
        entry.1 += 1;
    }
    let previous_by_expansion = previous
        .map(|codebook| {
            codebook
                .entries
                .iter()
                .map(|entry| (entry.expansion.as_str(), entry.code.as_str()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let reserved_codes = previous
        .map(|codebook| {
            codebook
                .entries
                .iter()
                .map(|entry| entry.code.clone())
                .chain(codebook.retired_codes.iter().cloned())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut values = units
        .into_iter()
        .filter(|(_, (_, count))| *count >= MIN_FREQUENCY)
        .filter_map(|(expansion, (kind, frequency))| {
            let expansion_tokens = crate::tokens::estimate(&expansion);
            let code = previous_by_expansion
                .get(expansion.as_str())
                .map(|code| (*code).to_string())
                .unwrap_or_else(|| {
                    let mut used_codes = reserved_codes.clone();
                    code_for(
                        &kind,
                        &expansion,
                        sigil,
                        cryptic,
                        used_codes.len(),
                        &mut used_codes,
                    )
                });
            let code_tokens = crate::tokens::estimate(&code);
            let framing = parameter_framing_tokens(&kind, &expansion);
            let per_use = expansion_tokens.saturating_sub(code_tokens.saturating_add(framing));
            let corpus_saving = per_use.saturating_mul(frequency);
            (corpus_saving > 0).then_some(Candidate {
                kind,
                expansion,
                frequency,
                expansion_tokens,
                code,
                code_tokens,
                corpus_saving,
            })
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .corpus_saving
            .cmp(&left.corpus_saving)
            .then_with(|| left.expansion.cmp(&right.expansion))
    });
    values
}

fn finalize_report(
    corpus: &ProjectCorpus,
    budget_tokens: usize,
    mode: &str,
    sigil: char,
    evolution: CodebookEvolution,
    entries: Vec<CodebookEntry>,
) -> ProjectCodebook {
    let corpus_tokens = crate::tokens::estimate(&corpus.text);
    let encoded_tokens = crate::tokens::estimate(&apply_entries(&corpus.text, &entries));
    let gross_savings = corpus_tokens.saturating_sub(encoded_tokens);
    let mut report = ProjectCodebook {
        schema: CODEBOOK_SCHEMA.to_string(),
        version: 1,
        tokenizer: "heuristic-v1".to_string(),
        tier: "semantic".to_string(),
        mode: mode.to_string(),
        sigil: sigil.to_string(),
        generation: evolution.generation,
        parent_hash: evolution.parent_hash,
        retired_codes: evolution.retired_codes,
        project: corpus.project.clone(),
        corpus_hash: corpus.hash.clone(),
        corpus_files: corpus.files,
        budget_tokens,
        codebook_tokens: 0,
        corpus_tokens,
        encoded_tokens,
        gross_savings,
        net_first_interaction: 0,
        break_even_interactions: None,
        status: if entries.is_empty() {
            "no_positive_entries"
        } else {
            "ready"
        }
        .to_string(),
        entries,
    };
    for _ in 0..4 {
        report.codebook_tokens = crate::tokens::estimate(&report.to_json());
        report.net_first_interaction = report.gross_savings as i64 - report.codebook_tokens as i64;
        report.break_even_interactions = break_even(report.codebook_tokens, report.gross_savings);
    }
    report
}

fn break_even(codebook_tokens: usize, saving_per_interaction: usize) -> Option<usize> {
    (saving_per_interaction > 0).then(|| {
        codebook_tokens.saturating_add(saving_per_interaction - 1) / saving_per_interaction
    })
}

fn choose_sigil(corpus: &str) -> Option<char> {
    SIGILS
        .iter()
        .copied()
        .find(|sigil| !corpus.contains(*sigil))
}

fn eligible_identifier(value: &str) -> bool {
    value.chars().count() >= MIN_IDENTIFIER_CHARS
        && !matches!(
            value,
            "module"
                | "import"
                | "effects"
                | "return"
                | "foreign"
                | "sandbox"
                | "version"
                | "output"
                | "input"
                | "struct"
                | "pipeline"
        )
}

fn morphology_units(words: &[String]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for word in words {
        let fragments = morphology_fragments(word);
        for fragment in fragments {
            *counts.entry(fragment).or_insert(0) += 1;
        }
    }
    counts.retain(|fragment, frequency| {
        *frequency >= MIN_FREQUENCY && eligible_morphology_fragment(fragment)
    });
    counts
}

fn morphology_fragments(word: &str) -> BTreeSet<String> {
    if word.len() > MAX_MORPHOLOGY_WORD_CHARS {
        return BTreeSet::new();
    }
    let chars = word.char_indices().collect::<Vec<_>>();
    let separators = chars
        .iter()
        .filter_map(|(index, value)| matches!(value, '_' | '.').then_some(*index))
        .collect::<Vec<_>>();
    let mut fragments = BTreeSet::new();
    for start_boundary in 0..=separators.len() {
        let max_end = start_boundary
            .saturating_add(MAX_MORPHOLOGY_COMPONENTS - 1)
            .min(separators.len());
        for end_boundary in start_boundary..=max_end {
            if start_boundary == 0 && end_boundary == separators.len() {
                continue;
            }
            let start = if start_boundary == 0 {
                0
            } else {
                separators[start_boundary - 1]
            };
            let end = if end_boundary == separators.len() {
                word.len()
            } else {
                separators[end_boundary] + 1
            };
            if start < end {
                fragments.insert(word[start..end].to_string());
            }
        }
    }
    fragments
}

fn eligible_morphology_fragment(value: &str) -> bool {
    let has_separator = value.contains(['_', '.']);
    let bounded = value.starts_with(['_', '.']) || value.ends_with(['_', '.']);
    has_separator
        && bounded
        && value
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '_' | '.'))
}

fn code_for(
    kind: &str,
    expansion: &str,
    sigil: char,
    cryptic: bool,
    ordinal: usize,
    used: &mut BTreeSet<String>,
) -> String {
    if kind == "morphology" {
        return morphology_code_for(expansion, sigil, cryptic, ordinal, used);
    }
    if is_parameter_kind(kind) {
        return parameter_code_for(expansion, sigil, cryptic, ordinal, used);
    }
    let stem = if cryptic {
        base36(ordinal)
    } else {
        let parts = expansion
            .split(['_', '.'])
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.len() > 1 {
            parts
                .iter()
                .filter_map(|part| part.chars().next())
                .take(5)
                .collect::<String>()
        } else {
            expansion.chars().take(3).collect::<String>()
        }
    };
    let mut suffix = stem;
    let mut collision = 0usize;
    loop {
        let code = if collision == 0 {
            format!("{sigil}{suffix}")
        } else {
            format!("{sigil}{suffix}{}", base36(collision))
        };
        if used.insert(code.clone()) {
            return code;
        }
        collision += 1;
        if suffix.is_empty() {
            suffix = base36(ordinal);
        }
    }
}

fn morphology_code_for(
    expansion: &str,
    sigil: char,
    cryptic: bool,
    ordinal: usize,
    used: &mut BTreeSet<String>,
) -> String {
    let stem = if cryptic {
        base36(ordinal)
    } else {
        expansion
            .split(['_', '.'])
            .filter_map(|part| part.chars().next())
            .take(5)
            .collect::<String>()
    };
    let stem = if stem.is_empty() {
        base36(ordinal)
    } else {
        stem
    };
    let mut collision = 0usize;
    loop {
        let suffix = if collision == 0 {
            String::new()
        } else {
            base36(collision)
        };
        let code = format!("{sigil}~{stem}{suffix}~");
        if used.insert(code.clone()) {
            return code;
        }
        collision += 1;
    }
}

fn parameter_code_for(
    expansion: &str,
    sigil: char,
    cryptic: bool,
    ordinal: usize,
    used: &mut BTreeSet<String>,
) -> String {
    let stem = if cryptic {
        base36(ordinal)
    } else {
        expansion
            .split(|value: char| !value.is_ascii_alphanumeric())
            .filter_map(|part| part.chars().find(char::is_ascii_alphabetic))
            .take(5)
            .collect::<String>()
    };
    let stem = if stem.is_empty() {
        base36(ordinal)
    } else {
        stem
    };
    let mut collision = 0usize;
    loop {
        let suffix = if collision == 0 {
            String::new()
        } else {
            base36(collision)
        };
        let code = format!("{sigil}!{stem}{suffix}!");
        if used.insert(code.clone()) {
            return code;
        }
        collision += 1;
    }
}

fn base36(mut value: usize) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut result = Vec::new();
    while value > 0 {
        result.push(DIGITS[value % 36] as char);
        value /= 36;
    }
    result.into_iter().rev().collect()
}

fn word_tokens(source: &str) -> Vec<String> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index].is_ascii_alphabetic() || chars[index] == '_' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '_'
                    || chars[index] == '.')
            {
                index += 1;
            }
            tokens.push(chars[start..index].iter().collect());
        } else {
            index += 1;
        }
    }
    tokens
}

fn apply_entries(source: &str, entries: &[CodebookEntry]) -> String {
    let mut semantic = entries
        .iter()
        .filter(|entry| {
            !matches!(entry.kind.as_str(), "identifier" | "morphology")
                && !is_parameter_kind(&entry.kind)
        })
        .collect::<Vec<_>>();
    semantic.sort_by(|left, right| {
        right
            .expansion
            .len()
            .cmp(&left.expansion.len())
            .then_with(|| left.expansion.cmp(&right.expansion))
    });
    let mut source = source.to_string();
    for entry in semantic {
        source = source.replace(&entry.expansion, &entry.code);
    }
    let mut parameter_entries = entries
        .iter()
        .filter(|entry| is_parameter_kind(&entry.kind))
        .collect::<Vec<_>>();
    parameter_entries.sort_by(|left, right| {
        right
            .expansion
            .len()
            .cmp(&left.expansion.len())
            .then_with(|| left.expansion.cmp(&right.expansion))
    });
    let mut protected = entries.iter().collect::<Vec<_>>();
    protected.sort_by_key(|entry| std::cmp::Reverse(entry.code.len()));
    for entry in parameter_entries {
        source = apply_parameter_entry(&source, entry, &protected);
    }
    let lookup = entries
        .iter()
        .filter(|entry| entry.kind == "identifier")
        .map(|entry| (entry.expansion.as_str(), entry.code.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut morphology = entries
        .iter()
        .filter(|entry| entry.kind == "morphology")
        .collect::<Vec<_>>();
    morphology.sort_by(|left, right| {
        right
            .expansion
            .len()
            .cmp(&left.expansion.len())
            .then_with(|| left.expansion.cmp(&right.expansion))
    });
    let mut out = String::new();
    let mut index = 0usize;
    while index < source.len() {
        if let Some(end) = protected_code_end(&source, index, &protected) {
            out.push_str(&source[index..end]);
            index = end;
            continue;
        }
        let current = source[index..].chars().next().unwrap_or_default();
        if current.is_ascii_alphabetic() || current == '_' {
            let start = index;
            index += current.len_utf8();
            while index < source.len() {
                let next = source[index..].chars().next().unwrap_or_default();
                if !(next.is_ascii_alphanumeric() || matches!(next, '_' | '.')) {
                    break;
                }
                index += next.len_utf8();
            }
            let word = &source[start..index];
            if let Some(code) = lookup.get(word) {
                out.push_str(code);
            } else {
                let mut encoded = word.to_string();
                for entry in &morphology {
                    encoded = encoded.replace(&entry.expansion, &entry.code);
                }
                out.push_str(&encoded);
            }
        } else {
            out.push(current);
            index += current.len_utf8();
        }
    }
    out
}

fn apply_parameter_entry(
    source: &str,
    entry: &CodebookEntry,
    protected: &[&CodebookEntry],
) -> String {
    let Some(segments) = parameter_template_segments(&entry.expansion) else {
        return source.to_string();
    };
    let prefix = segments[0];
    let mut out = String::new();
    let mut index = 0usize;
    while index < source.len() {
        if let Some(end) = protected_code_end(source, index, protected) {
            out.push_str(&source[index..end]);
            index = end;
            continue;
        }
        if source[index..].starts_with(prefix) {
            let mut cursor = index + prefix.len();
            let mut arguments = Vec::new();
            let mut matched = true;
            for suffix in segments.iter().skip(1) {
                let Some(argument_end) = parameter_atom_end(source, cursor) else {
                    matched = false;
                    break;
                };
                let argument = &source[cursor..argument_end];
                if !eligible_parameter_argument(argument)
                    || !source[argument_end..].starts_with(suffix)
                {
                    matched = false;
                    break;
                }
                arguments.push(argument);
                cursor = argument_end + suffix.len();
            }
            if matched {
                let mut invocation = entry.code.clone();
                for argument in arguments {
                    invocation.push_str(&format!("{}:{argument}", argument.len()));
                }
                if crate::tokens::estimate(&invocation)
                    < crate::tokens::estimate(&source[index..cursor])
                {
                    out.push_str(&invocation);
                    index = cursor;
                    continue;
                }
            }
        }
        let current = source[index..].chars().next().unwrap_or_default();
        out.push(current);
        index += current.len_utf8();
    }
    out
}

fn protected_code_end(source: &str, index: usize, entries: &[&CodebookEntry]) -> Option<usize> {
    let entry = entries
        .iter()
        .find(|entry| source[index..].starts_with(&entry.code))?;
    let code_end = index + entry.code.len();
    if is_parameter_kind(&entry.kind) {
        parameter_invocation_end(
            source,
            code_end,
            parameter_template_arity(&entry.expansion)?,
        )
    } else {
        Some(code_end)
    }
}

fn parameter_invocation_end(source: &str, frame_start: usize, arity: usize) -> Option<usize> {
    let mut cursor = frame_start;
    for _ in 0..arity {
        let (_, end) = parameter_argument_bounds(source, cursor)?;
        cursor = end;
    }
    Some(cursor)
}

fn parameter_argument_bounds(source: &str, frame_start: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut cursor = frame_start;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == frame_start || cursor >= bytes.len() || bytes[cursor] != b':' {
        return None;
    }
    let length_text = &source[frame_start..cursor];
    if length_text.len() > 1 && length_text.starts_with('0') {
        return None;
    }
    let length = length_text.parse::<usize>().ok()?;
    if length == 0 || length > MAX_PARAMETER_ARGUMENT_BYTES {
        return None;
    }
    let start = cursor + 1;
    let end = start.checked_add(length)?;
    (end <= source.len() && source.is_char_boundary(end)).then_some((start, end))
}

fn parameter_atom_end(source: &str, start: usize) -> Option<usize> {
    let first = source.get(start..)?.chars().next()?;
    if first == '"' {
        let mut escaped = false;
        for (offset, value) in source[start + 1..].char_indices() {
            if escaped {
                escaped = false;
            } else if value == '\\' {
                escaped = true;
            } else if value == '"' {
                return Some(start + 1 + offset + value.len_utf8());
            }
        }
        return None;
    }
    if first.is_ascii_alphabetic() || first == '_' {
        let mut end = start + first.len_utf8();
        while end < source.len() {
            let value = source[end..].chars().next()?;
            if !(value.is_ascii_alphanumeric() || value == '_') {
                break;
            }
            end += value.len_utf8();
        }
        return Some(end);
    }
    let bytes = source.as_bytes();
    let mut cursor = start;
    if bytes.get(cursor) == Some(&b'-') {
        cursor += 1;
    }
    let integer_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == integer_start {
        return None;
    }
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
    }
    if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let exponent_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == exponent_start {
            return None;
        }
    }
    Some(cursor)
}

fn parse_entry(value: &JsonValue) -> Result<CodebookEntry, String> {
    let JsonValue::Object(object) = value else {
        return Err("codebook invalid: entry must be an object".to_string());
    };
    Ok(CodebookEntry {
        kind: match object.get("kind") {
            Some(JsonValue::String(kind)) => kind.clone(),
            None => "identifier".to_string(),
            _ => return Err("codebook invalid: entry kind must be a string".to_string()),
        },
        code: string_field(object, "code")?,
        expansion: string_field(object, "expansion")?,
        frequency: usize_field(object, "frequency")?,
        expansion_tokens: usize_field(object, "expansion_tokens")?,
        code_tokens: usize_field(object, "code_tokens")?,
        corpus_saving: usize_field(object, "corpus_saving")?,
    })
}

fn validate_entries(entries: &[CodebookEntry], sigil: &str) -> Result<(), String> {
    let mut codes = BTreeSet::new();
    let mut expansions = BTreeSet::new();
    for entry in entries {
        if !matches!(
            entry.kind.as_str(),
            "identifier"
                | "type"
                | "effect_set"
                | "expression"
                | "statement"
                | "morphology"
                | "expression_template"
                | "statement_template"
        ) {
            return Err(format!(
                "codebook invalid: unsupported entry kind '{}'",
                entry.kind
            ));
        }
        if !codes.insert(&entry.code) {
            return Err(format!("codebook invalid: duplicate code '{}'", entry.code));
        }
        if !expansions.insert(&entry.expansion) {
            return Err(format!(
                "codebook invalid: duplicate expansion '{}'",
                entry.expansion
            ));
        }
        if !entry.code.starts_with(sigil)
            || (entry.kind == "morphology"
                && (!entry.code.starts_with(&format!("{sigil}~"))
                    || !entry.code.ends_with('~')
                    || !eligible_morphology_fragment(&entry.expansion)))
            || (is_parameter_kind(&entry.kind)
                && (!entry.code.starts_with(&format!("{sigil}!"))
                    || !entry.code.ends_with('!')
                    || !eligible_parameter_template(&entry.expansion)))
            || entry.expansion_tokens != crate::tokens::estimate(&entry.expansion)
            || entry.code_tokens != crate::tokens::estimate(&entry.code)
            || entry.frequency < MIN_FREQUENCY
            || entry.expansion_tokens
                <= entry.code_tokens + parameter_framing_tokens(&entry.kind, &entry.expansion)
            || entry.corpus_saving
                != entry
                    .expansion_tokens
                    .saturating_sub(
                        entry.code_tokens + parameter_framing_tokens(&entry.kind, &entry.expansion),
                    )
                    .saturating_mul(entry.frequency)
        {
            return Err(format!(
                "codebook invalid: inconsistent entry '{}'",
                entry.expansion
            ));
        }
    }
    Ok(())
}

fn validate_retired_codes(
    entries: &[CodebookEntry],
    retired_codes: &[String],
    sigil: &str,
) -> Result<(), String> {
    let active = entries
        .iter()
        .map(|entry| entry.code.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for code in retired_codes {
        if !code.starts_with(sigil) || !seen.insert(code) || active.contains(code.as_str()) {
            return Err(format!(
                "codebook invalid: retired code '{code}' is malformed, duplicated, or active"
            ));
        }
    }
    Ok(())
}

fn string_field(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<String, String> {
    match object.get(key) {
        Some(JsonValue::String(value)) => Ok(value.clone()),
        _ => Err(format!("codebook invalid: missing string field '{key}'")),
    }
}

fn usize_field(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<usize, String> {
    match object.get(key) {
        Some(JsonValue::UInt(value)) => usize::try_from(*value)
            .map_err(|_| format!("codebook invalid: '{key}' is out of range")),
        Some(JsonValue::Int(value)) if *value >= 0 => usize::try_from(*value as u64)
            .map_err(|_| format!("codebook invalid: '{key}' is out of range")),
        _ => Err(format!("codebook invalid: missing integer field '{key}'")),
    }
}

fn int_field(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<i64, String> {
    match object.get(key) {
        Some(JsonValue::Int(value)) => Ok(*value),
        Some(JsonValue::UInt(value)) => {
            i64::try_from(*value).map_err(|_| format!("codebook invalid: '{key}' is out of range"))
        }
        _ => Err(format!("codebook invalid: missing integer field '{key}'")),
    }
}

fn optional_usize_field(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<Option<usize>, String> {
    match object.get(key) {
        Some(JsonValue::Null) => Ok(None),
        Some(_) => usize_field(object, key).map(Some),
        None => Err(format!("codebook invalid: missing field '{key}'")),
    }
}

fn optional_string_field(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<Option<String>, String> {
    match object.get(key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) => Ok(Some(value.clone())),
        _ => Err(format!(
            "codebook invalid: '{key}' must be a string or null"
        )),
    }
}

fn string_array_field(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<Vec<String>, String> {
    match object.get(key) {
        Some(JsonValue::Array(values)) => values
            .iter()
            .map(|value| match value {
                JsonValue::String(value) => Ok(value.clone()),
                _ => Err(format!(
                    "codebook invalid: '{key}' must contain only strings"
                )),
            })
            .collect(),
        _ => Err(format!("codebook invalid: missing array field '{key}'")),
    }
}

fn validate_sha256(field: &str, value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!(
            "codebook invalid: '{field}' must be a 64-character SHA-256"
        ))
    }
}

fn decode_parameter_invocations(
    source: &str,
    entries: &[&CodebookEntry],
) -> Result<String, String> {
    let mut entries = entries.to_vec();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.code.len()));
    let mut out = String::new();
    let mut index = 0usize;
    while index < source.len() {
        if let Some(entry) = entries
            .iter()
            .find(|entry| source[index..].starts_with(&entry.code))
        {
            let frame_start = index + entry.code.len();
            let arity = parameter_template_arity(&entry.expansion)
                .ok_or_else(|| "codebook artifact has an invalid template".to_string())?;
            let mut cursor = frame_start;
            let mut arguments = Vec::with_capacity(arity);
            for _ in 0..arity {
                let (argument_start, end) =
                    parameter_argument_bounds(source, cursor).ok_or_else(|| {
                        "codebook payload has a malformed template invocation".to_string()
                    })?;
                let argument = &source[argument_start..end];
                if !eligible_parameter_argument(argument) {
                    return Err("codebook payload has an invalid template argument".to_string());
                }
                arguments.push(argument.to_string());
                cursor = end;
            }
            out.push_str(
                &expand_parameter_template(&entry.expansion, &arguments)
                    .ok_or_else(|| "codebook artifact has an invalid template".to_string())?,
            );
            index = cursor;
            continue;
        }
        let current = source[index..].chars().next().unwrap_or_default();
        out.push(current);
        index += current.len_utf8();
    }
    Ok(out)
}

impl ProjectCodebook {
    pub fn artifact_hash(&self) -> String {
        crate::crypto::sha256_hex(self.to_json().as_bytes())
    }

    pub fn encode_text(&self, source: &str) -> Result<String, String> {
        if source.contains(&self.sigil) {
            return Err(format!(
                "codebook input contains reserved sigil '{}'",
                self.sigil
            ));
        }
        Ok(apply_entries(source, &self.entries))
    }

    pub fn decode_text(&self, source: &str) -> String {
        self.decode_text_checked(source)
            .unwrap_or_else(|_| source.to_string())
    }

    fn decode_text_checked(&self, source: &str) -> Result<String, String> {
        let parameter_entries = self
            .entries
            .iter()
            .filter(|entry| is_parameter_kind(&entry.kind))
            .collect::<Vec<_>>();
        let mut decoded = decode_parameter_invocations(source, &parameter_entries)?;
        let mut entries = self
            .entries
            .iter()
            .filter(|entry| !is_parameter_kind(&entry.kind))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .code
                .len()
                .cmp(&left.code.len())
                .then_with(|| left.code.cmp(&right.code))
        });
        for entry in entries {
            decoded = decoded.replace(&entry.code, &entry.expansion);
        }
        Ok(decoded)
    }

    pub fn encode_payload(&self, source: &str) -> Result<String, String> {
        let encoded = self.encode_text(source)?;
        Ok(format!(
            "#FGCB v1 hash={} corpus={} tier={}\n{}",
            self.artifact_hash(),
            self.corpus_hash,
            self.tier,
            encoded
        ))
    }

    pub fn encode_payload_if_smaller(&self, source: &str) -> Result<CodebookPayload, String> {
        let encoded = self.encode_payload(source)?;
        let plain_tokens = crate::tokens::estimate(source);
        let encoded_tokens = crate::tokens::estimate(&encoded);
        if encoded_tokens < plain_tokens {
            Ok(CodebookPayload {
                text: encoded,
                decision: "applied".to_string(),
                plain_tokens,
                encoded_tokens,
            })
        } else {
            Ok(CodebookPayload {
                text: source.to_string(),
                decision: "skipped_not_smaller".to_string(),
                plain_tokens,
                encoded_tokens,
            })
        }
    }

    pub fn decode_payload(&self, source: &str) -> Result<String, String> {
        let Some((header, body)) = source.split_once('\n') else {
            return Ok(source.to_string());
        };
        if !header.starts_with("#FGCB ") {
            let context_reference = format!("\"codebook_ref\": \"{}\"", self.artifact_hash());
            if source.contains(&context_reference) {
                return self.decode_text_checked(source);
            }
            return Ok(source.to_string());
        }
        let fields = header
            .split_whitespace()
            .skip(2)
            .filter_map(|field| field.split_once('='))
            .collect::<BTreeMap<_, _>>();
        let hash = fields
            .get("hash")
            .ok_or_else(|| "codebook payload header is missing hash".to_string())?;
        if *hash != self.artifact_hash() {
            return Err("codebook payload references a different artifact hash".to_string());
        }
        let corpus = fields
            .get("corpus")
            .ok_or_else(|| "codebook payload header is missing corpus".to_string())?;
        if *corpus != self.corpus_hash {
            return Err("codebook payload references a different corpus hash".to_string());
        }
        let tier = fields
            .get("tier")
            .ok_or_else(|| "codebook payload header is missing tier".to_string())?;
        if *tier != self.tier {
            return Err("codebook payload references a different tier".to_string());
        }
        self.decode_text_checked(body)
    }

    pub fn ensure_current(&self, input: &Path) -> Result<CodebookMeasure, String> {
        let report = measure(input, self)?;
        if report.status == "current" {
            Ok(report)
        } else {
            Err(format!(
                "codebook stale: artifact corpus {} does not match project corpus {}",
                self.corpus_hash, report.corpus_hash
            ))
        }
    }

    pub fn to_json(&self) -> String {
        let entries = self
            .entries
            .iter()
            .map(|entry| entry.to_json(self.tier == "semantic"))
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"version\": {},\n  \"tokenizer\": {},\n  \"tier\": {},\n  \"mode\": {},\n  \"sigil\": {},\n  \"generation\": {},\n  \"parent_hash\": {},\n  \"retired_codes\": {},\n  \"project\": {},\n  \"corpus_hash\": {},\n  \"corpus_files\": {},\n  \"budget_tokens\": {},\n  \"codebook_tokens\": {},\n  \"corpus_tokens\": {},\n  \"encoded_tokens\": {},\n  \"gross_savings\": {},\n  \"net_first_interaction\": {},\n  \"break_even_interactions\": {},\n  \"status\": {},\n  \"entries\": [\n    {}\n  ]\n}}",
            crate::json::string(&self.schema),
            self.version,
            crate::json::string(&self.tokenizer),
            crate::json::string(&self.tier),
            crate::json::string(&self.mode),
            crate::json::string(&self.sigil),
            self.generation,
            self.parent_hash
                .as_deref()
                .map(crate::json::string)
                .unwrap_or_else(|| "null".to_string()),
            crate::json::string_array(&self.retired_codes),
            crate::json::string(&self.project),
            crate::json::string(&self.corpus_hash),
            self.corpus_files,
            self.budget_tokens,
            self.codebook_tokens,
            self.corpus_tokens,
            self.encoded_tokens,
            self.gross_savings,
            self.net_first_interaction,
            self.break_even_interactions
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            crate::json::string(&self.status),
            entries
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex project codebook\nschema={} project={} status={} mode={} tier={} sigil={} tokenizer={} generation={} parent_hash={} retired_codes={}\ncorpus_hash={} files={} entries={} budget_tokens={} codebook_tokens={} corpus_tokens={} encoded_tokens={} gross_savings={} net_first_interaction={} break_even_interactions={}\n",
            self.schema,
            self.project,
            self.status,
            self.mode,
            self.tier,
            self.sigil,
            self.tokenizer,
            self.generation,
            self.parent_hash.as_deref().unwrap_or("none"),
            self.retired_codes.len(),
            self.corpus_hash,
            self.corpus_files,
            self.entries.len(),
            self.budget_tokens,
            self.codebook_tokens,
            self.corpus_tokens,
            self.encoded_tokens,
            self.gross_savings,
            self.net_first_interaction,
            self.break_even_interactions
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }

    pub fn stats_text(&self) -> String {
        let mut out = self.to_text();
        for entry in &self.entries {
            out.push_str(&format!(
                "{} {} -> {} frequency={} expansion_tokens={} code_tokens={} corpus_saving={}\n",
                entry.kind,
                entry.code,
                entry.expansion,
                entry.frequency,
                entry.expansion_tokens,
                entry.code_tokens,
                entry.corpus_saving
            ));
        }
        out
    }
}

impl CodebookEntry {
    fn to_json(&self, include_kind: bool) -> String {
        let kind = if include_kind {
            format!("\"kind\":{},", crate::json::string(&self.kind))
        } else {
            String::new()
        };
        format!(
            "{{{kind}\"code\":{},\"expansion\":{},\"frequency\":{},\"expansion_tokens\":{},\"code_tokens\":{},\"corpus_saving\":{}}}",
            crate::json::string(&self.code),
            crate::json::string(&self.expansion),
            self.frequency,
            self.expansion_tokens,
            self.code_tokens,
            self.corpus_saving
        )
    }
}

impl CodebookCheck {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"status\":{},\"project\":{},\"codebook\":{},\"expected_corpus_hash\":{},\"actual_corpus_hash\":{},\"entries\":{}}}",
            crate::json::string(&self.schema),
            crate::json::string(&self.status),
            crate::json::string(&self.project),
            crate::json::string(&self.codebook),
            crate::json::string(&self.expected_corpus_hash),
            crate::json::string(&self.actual_corpus_hash),
            self.entries
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex codebook check\nschema={} status={} project={} codebook={} entries={} expected_corpus_hash={} actual_corpus_hash={}\n",
            self.schema,
            self.status,
            self.project,
            self.codebook,
            self.entries,
            self.expected_corpus_hash,
            self.actual_corpus_hash
        )
    }
}

impl CodebookMeasure {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"status\":{},\"project\":{},\"corpus_hash\":{},\"codebook_corpus_hash\":{},\"corpus_tokens\":{},\"encoded_tokens\":{},\"gross_savings\":{},\"codebook_tokens\":{},\"break_even_interactions\":{},\"entries\":{}}}",
            crate::json::string(&self.schema),
            crate::json::string(&self.status),
            crate::json::string(&self.project),
            crate::json::string(&self.corpus_hash),
            crate::json::string(&self.codebook_corpus_hash),
            self.corpus_tokens,
            self.encoded_tokens,
            self.gross_savings,
            self.codebook_tokens,
            self.break_even_interactions
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.entries
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex codebook measure\nschema={} status={} project={} entries={} corpus_tokens={} encoded_tokens={} gross_savings={} codebook_tokens={} break_even_interactions={}\n",
            self.schema,
            self.status,
            self.project,
            self.entries,
            self.corpus_tokens,
            self.encoded_tokens,
            self.gross_savings,
            self.codebook_tokens,
            self.break_even_interactions
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }
}
