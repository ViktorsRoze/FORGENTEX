use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::diag::Diagnostic;
use crate::effects::{analyze as analyze_effects, FunctionEffects};
use crate::format::format_human;
use crate::interp::{parse_json_value, JsonValue};
use crate::parser::parse_file;
use crate::patch::{apply_patch_with_options, PatchOptions};
use crate::project::{check_project, load_project, Project, ProjectModule};
use crate::targets::target_metadata_for_function;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextOptions {
    pub deps: Vec<String>,
    pub max_tokens: Option<usize>,
    baseline: Option<ContextBaseline>,
    codebook: Option<crate::codebook::ProjectCodebook>,
    force_codebook: bool,
}

impl ContextOptions {
    pub fn from_csv(raw: Option<&str>) -> Self {
        let deps = raw
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|dep| !dep.is_empty())
            .map(str::to_string)
            .collect();
        Self {
            deps,
            max_tokens: None,
            baseline: None,
            codebook: None,
            force_codebook: false,
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<usize>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_baseline_json(mut self, source: &str) -> Result<Self, String> {
        self.baseline = Some(parse_context_baseline(source)?);
        Ok(self)
    }

    pub fn with_codebook(mut self, codebook: crate::codebook::ProjectCodebook) -> Self {
        self.codebook = Some(codebook);
        self
    }

    pub fn with_force_codebook(mut self, force: bool) -> Self {
        self.force_codebook = force;
        self
    }

    fn wants(&self, dep: &str) -> bool {
        self.deps.iter().any(|candidate| candidate == dep)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContextBaseline {
    symbol: String,
    packet_hash: String,
    section_hashes: BTreeMap<String, String>,
    state_sections: BTreeSet<String>,
    codebook_ref: Option<String>,
}

impl ContextBaseline {
    fn matches(&self, section: &str, hash: &str) -> bool {
        self.state_sections.contains(section)
            && self
                .section_hashes
                .get(section)
                .is_some_and(|known| known == hash)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContextExclusion {
    category: String,
    reason: String,
    count: usize,
    tokens: usize,
}

#[derive(Clone, Debug)]
struct LocatedSymbol {
    module_index: usize,
    item: Item,
}

pub fn context_json(
    project_input: &Path,
    symbol: &str,
    options: ContextOptions,
) -> Result<String, String> {
    let project = load_project(project_input)?;
    if let Some(codebook) = &options.codebook {
        codebook.ensure_current(project_input)?;
    }
    let codebook_ref = options
        .codebook
        .as_ref()
        .map(|codebook| codebook.artifact_hash());
    let symbol = normalize_symbol(symbol);
    let located = locate_symbol(&project, &symbol)?;
    let module = &project.modules[located.module_index];
    let item_program = Program {
        items: vec![located.item.clone()],
    };
    let canonical_symbol_source = format_human(&item_program);
    let unencoded_symbol_tokens = token_count(&canonical_symbol_source);
    let symbol_source = encode_context_text(&options, &canonical_symbol_source)?;
    let symbol_tokens = token_count(&symbol_source);
    validate_context_options(&options)?;
    let requested_deps = options.deps.clone();

    let available_types = type_slice(&project, &located.item);
    let available_unencoded_type_tokens = available_types
        .iter()
        .map(|item| {
            token_count(&format_human(&Program {
                items: vec![item.clone()],
            }))
        })
        .sum::<usize>();
    let available_type_tokens = available_types
        .iter()
        .map(|item| {
            encode_context_text(
                &options,
                &format_human(&Program {
                    items: vec![item.clone()],
                }),
            )
            .map(|source| token_count(&source))
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .sum::<usize>();
    let available_effects = symbol_effects(&project, &symbol);
    let available_effect_tokens = available_effects.as_ref().map(effect_tokens).unwrap_or(0);
    let available_tests = test_slice(&project, module, &located.item)?;
    let available_test_tokens = available_tests
        .iter()
        .map(|test| test.tokens)
        .sum::<usize>();

    let diagnostics = diagnostics_for_symbol(&project, &symbol);
    let unsafe_slices = unsafe_slices(&project);
    let canonical_symbol_map = symbol_map_lines(&project, module, &located.item);
    let unencoded_symbol_map_tokens = token_count(&canonical_symbol_map.join("\n"));
    let symbol_map = canonical_symbol_map
        .iter()
        .map(|line| encode_context_text(&options, line))
        .collect::<Result<Vec<_>, String>>()?;
    let project_tokens = project
        .modules
        .iter()
        .map(|module| module.token_cost)
        .sum::<usize>();
    let symbol_map_tokens = token_count(&symbol_map.join("\n"));
    if options
        .baseline
        .as_ref()
        .is_some_and(|baseline| baseline.symbol != symbol)
    {
        return Err(format!(
            "context_baseline_symbol_mismatch: baseline is for '{}', requested '{symbol}'",
            options
                .baseline
                .as_ref()
                .map(|baseline| baseline.symbol.as_str())
                .unwrap_or_default()
        ));
    }
    let mut section_hashes = BTreeMap::new();
    section_hashes.insert(
        "symbol_map".to_string(),
        section_hash(&canonical_symbol_map.join("\n")),
    );
    section_hashes.insert(
        "symbol_source".to_string(),
        section_hash(&canonical_symbol_source),
    );
    section_hashes.insert(
        "types".to_string(),
        section_hash(&type_section(&available_types)),
    );
    section_hashes.insert(
        "effects".to_string(),
        section_hash(
            &available_effects
                .as_ref()
                .map(effect_section)
                .unwrap_or_default(),
        ),
    );
    section_hashes.insert(
        "tests".to_string(),
        section_hash(&test_section(&available_tests)),
    );

    let mut elided_sections = Vec::new();
    let symbol_map_elided = baseline_matches(&options, "symbol_map", &section_hashes);
    let symbol_source_elided = baseline_matches(&options, "symbol_source", &section_hashes);
    if symbol_map_elided {
        elided_sections.push("symbol_map".to_string());
    }
    if symbol_source_elided {
        elided_sections.push("symbol_source".to_string());
    }
    let logical_mandatory_tokens = symbol_tokens
        .saturating_add(symbol_map_tokens)
        .saturating_add(if options.wants("effects") {
            available_effect_tokens
        } else {
            0
        });
    let unencoded_logical_mandatory_tokens = unencoded_symbol_tokens
        .saturating_add(unencoded_symbol_map_tokens)
        .saturating_add(if options.wants("effects") {
            available_effect_tokens
        } else {
            0
        });
    let mut mandatory_tokens = if symbol_source_elided {
        0
    } else {
        symbol_tokens
    }
    .saturating_add(if symbol_map_elided {
        0
    } else {
        symbol_map_tokens
    });
    if options.wants("effects") {
        // Effects are review-critical context. A caller may omit the category
        // explicitly, but a requested effect surface is never silently
        // sacrificed to fit a token budget.
        mandatory_tokens = mandatory_tokens.saturating_add(available_effect_tokens);
    }
    if options
        .max_tokens
        .is_some_and(|max_tokens| mandatory_tokens > max_tokens)
    {
        return Err(format!(
            "context_token_budget_too_small: max_tokens {} is smaller than mandatory focused context {mandatory_tokens}",
            options.max_tokens.unwrap_or_default()
        ));
    }

    let mut included_deps = Vec::new();
    let mut exclusions = Vec::new();
    let mut total_context_tokens = mandatory_tokens;
    let mut logical_context_tokens = logical_mandatory_tokens;
    let mut unencoded_logical_context_tokens = unencoded_logical_mandatory_tokens;
    let mut types = Vec::new();
    let mut effects = None;
    let mut tests = Vec::new();
    if options.wants("effects") {
        included_deps.push("effects".to_string());
        effects = available_effects.clone();
    }
    for dep in &requested_deps {
        if dep == "effects" {
            continue;
        }
        let (tokens, count, section) = match dep.as_str() {
            "types" => (available_type_tokens, available_types.len(), "types"),
            "tests" => (available_test_tokens, available_tests.len(), "tests"),
            _ => unreachable!("context dependency names validated above"),
        };
        let hash_elided = baseline_matches(&options, section, &section_hashes);
        let transmitted_tokens = if hash_elided { 0 } else { tokens };
        let admitted = options.max_tokens.is_none_or(|max_tokens| {
            total_context_tokens.saturating_add(transmitted_tokens) <= max_tokens
        });
        if admitted {
            total_context_tokens = total_context_tokens.saturating_add(transmitted_tokens);
            logical_context_tokens = logical_context_tokens.saturating_add(tokens);
            unencoded_logical_context_tokens =
                unencoded_logical_context_tokens.saturating_add(if dep == "types" {
                    available_unencoded_type_tokens
                } else {
                    tokens
                });
            included_deps.push(dep.clone());
            if hash_elided {
                elided_sections.push(section.to_string());
            }
            match dep.as_str() {
                "types" => types = available_types.clone(),
                "tests" => tests = available_tests.clone(),
                _ => unreachable!("context dependency names validated above"),
            }
        } else {
            exclusions.push(ContextExclusion {
                category: dep.clone(),
                reason: "token_budget".to_string(),
                count,
                tokens,
            });
        }
    }
    for dep in ["types", "effects", "tests"] {
        if !options.wants(dep) {
            let (tokens, count) = match dep {
                "types" => (available_type_tokens, available_types.len()),
                "effects" => (
                    available_effect_tokens,
                    usize::from(available_effects.is_some()),
                ),
                "tests" => (available_test_tokens, available_tests.len()),
                _ => unreachable!(),
            };
            exclusions.push(ContextExclusion {
                category: dep.to_string(),
                reason: "not_requested".to_string(),
                count,
                tokens,
            });
        }
    }
    let project_symbol_count = project
        .modules
        .iter()
        .filter(|module| !module.standard)
        .map(|module| module.program.items.len())
        .sum::<usize>();
    exclusions.push(ContextExclusion {
        category: "project_symbols".to_string(),
        reason: "focused_symbol_only".to_string(),
        count: project_symbol_count.saturating_sub(1),
        tokens: 0,
    });
    let type_tokens = if included_deps.iter().any(|dep| dep == "types") {
        available_type_tokens
    } else {
        0
    };
    let effect_tokens = if included_deps.iter().any(|dep| dep == "effects") {
        available_effect_tokens
    } else {
        0
    };
    let test_tokens = if included_deps.iter().any(|dep| dep == "tests") {
        available_test_tokens
    } else {
        0
    };
    let mut state_sections = vec!["symbol_map".to_string(), "symbol_source".to_string()];
    state_sections.extend(included_deps.iter().cloned());
    let packet_hash = context_packet_hash(
        &symbol,
        &requested_deps,
        &included_deps,
        &section_hashes,
        &exclusions,
        options.max_tokens,
        codebook_ref.as_deref(),
    );

    let mut out = String::new();
    out.push_str("{\n");
    push_prop(
        &mut out,
        1,
        "schema",
        &quoted("forgentex.agent_context.v1"),
        true,
    );
    push_prop(&mut out, 1, "symbol", &quoted(&symbol), true);
    push_prop(
        &mut out,
        1,
        "mode",
        &quoted(if elided_sections.is_empty() {
            "full"
        } else {
            "delta"
        }),
        true,
    );
    push_prop(&mut out, 1, "packet_hash", &quoted(&packet_hash), true);
    push_prop(
        &mut out,
        1,
        "baseline_packet_hash",
        &options
            .baseline
            .as_ref()
            .map(|baseline| quoted(&baseline.packet_hash))
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    push_prop(
        &mut out,
        1,
        "codebook_ref",
        &codebook_ref
            .as_deref()
            .map(quoted)
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    push_prop(
        &mut out,
        1,
        "codebook_tier",
        &options
            .codebook
            .as_ref()
            .map(|codebook| quoted(&codebook.tier))
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    push_prop(
        &mut out,
        1,
        "codebook_mode",
        &options
            .codebook
            .as_ref()
            .map(|codebook| quoted(&codebook.mode))
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    if options.force_codebook {
        push_prop(&mut out, 1, "codebook_decision", &quoted("forced"), true);
    }
    push_string_array(&mut out, 1, "requested_deps", &requested_deps, true);
    push_string_array(&mut out, 1, "included_deps", &included_deps, true);
    push_string_array(&mut out, 1, "state_sections", &state_sections, true);
    push_string_array(&mut out, 1, "elided_sections", &elided_sections, true);
    push_section_hashes(&mut out, 1, &section_hashes, true);
    push_exclusions(&mut out, 1, &exclusions, true);
    out.push_str("  \"project\": {\n");
    push_prop(&mut out, 2, "name", &quoted(&project.manifest.name), true);
    push_prop(
        &mut out,
        2,
        "version",
        &quoted(&project.manifest.version),
        true,
    );
    push_prop(
        &mut out,
        2,
        "root",
        &quoted(&display_path(
            &project.package_root,
            &project.manifest.source_root,
        )),
        true,
    );
    push_prop(
        &mut out,
        2,
        "modules",
        &project.modules.len().to_string(),
        true,
    );
    push_prop(&mut out, 2, "tokens", &project_tokens.to_string(), false);
    out.push_str("  },\n");
    out.push_str("  \"source\": {\n");
    push_prop(&mut out, 2, "module", &quoted(&module.name), true);
    push_prop(
        &mut out,
        2,
        "path",
        &quoted(&display_path(&project.package_root, &module.path)),
        true,
    );
    push_prop(&mut out, 2, "hash", &quoted(&module.source_hash), true);
    push_prop(&mut out, 2, "tokens", &module.token_cost.to_string(), false);
    out.push_str("  },\n");
    if !symbol_map_elided {
        push_string_array(&mut out, 1, "symbol_map", &symbol_map, true);
    }
    if !symbol_source_elided {
        push_prop(&mut out, 1, "symbol_source", &quoted(&symbol_source), true);
    }
    if !elided_sections.iter().any(|section| section == "types") {
        push_type_slice_encoded(&mut out, 1, &types, options.codebook.as_ref(), true)?;
    }
    push_effects(&mut out, 1, effects.as_ref(), true);
    if !elided_sections.iter().any(|section| section == "tests") {
        push_tests(&mut out, 1, &tests, true);
    }
    push_diagnostics(&mut out, 1, &diagnostics, true);
    push_unsafe_slices(&mut out, 1, &project, &unsafe_slices, true);
    out.push_str("  \"token_estimate\": {\n");
    push_prop(&mut out, 2, "symbol", &symbol_tokens.to_string(), true);
    push_prop(
        &mut out,
        2,
        "unencoded_symbol",
        &unencoded_symbol_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "symbol_map",
        &symbol_map_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "unencoded_symbol_map",
        &unencoded_symbol_map_tokens.to_string(),
        true,
    );
    push_prop(&mut out, 2, "types", &type_tokens.to_string(), true);
    push_prop(
        &mut out,
        2,
        "unencoded_types",
        &if included_deps.iter().any(|dep| dep == "types") {
            available_unencoded_type_tokens
        } else {
            0
        }
        .to_string(),
        true,
    );
    push_prop(&mut out, 2, "effects", &effect_tokens.to_string(), true);
    push_prop(&mut out, 2, "tests", &test_tokens.to_string(), true);
    push_prop(
        &mut out,
        2,
        "mandatory",
        &mandatory_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "logical_mandatory",
        &logical_mandatory_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "logical_context",
        &logical_context_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "unencoded_logical_context",
        &unencoded_logical_context_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "saved_by_codebook",
        &unencoded_logical_context_tokens
            .saturating_sub(logical_context_tokens)
            .to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "saved_by_hash_elision",
        &logical_context_tokens
            .saturating_sub(total_context_tokens)
            .to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "total_context",
        &total_context_tokens.to_string(),
        false,
    );
    out.push_str("  },\n");
    out.push_str("  \"budget\": {\n");
    push_prop(&mut out, 2, "tokenizer", &quoted("heuristic-v1"), true);
    push_prop(
        &mut out,
        2,
        "max_tokens",
        &options
            .max_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    push_prop(
        &mut out,
        2,
        "used_tokens",
        &total_context_tokens.to_string(),
        true,
    );
    push_prop(
        &mut out,
        2,
        "decision",
        &quoted(if options.max_tokens.is_some() {
            "within_budget"
        } else {
            "unrestricted"
        }),
        false,
    );
    out.push_str("  }\n");
    out.push('}');
    if options.codebook.is_some() && !options.force_codebook {
        let encoded_tokens = crate::tokens::estimate(&out);
        let mut plain_options = options.clone();
        plain_options.codebook = None;
        let plain = context_json(project_input, &symbol, plain_options)?;
        if encoded_tokens >= crate::tokens::estimate(&plain) {
            return Ok(plain);
        }
    }
    Ok(out)
}

pub fn context_text(
    project_input: &Path,
    symbol: &str,
    options: ContextOptions,
) -> Result<String, String> {
    let json = context_json(project_input, symbol, options)?;
    Ok(format!(
        "Agent context packet\nschema=forgentex.agent_context.v1\n{json}\n"
    ))
}

pub fn review_json(
    patch_path: &Path,
    source_path: Option<&Path>,
    project_input: &Path,
) -> Result<String, String> {
    review_json_with_codebook(patch_path, source_path, project_input, None)
}

pub fn review_json_with_codebook(
    patch_path: &Path,
    source_path: Option<&Path>,
    project_input: &Path,
    codebook: Option<&crate::codebook::ProjectCodebook>,
) -> Result<String, String> {
    let transport_patch = fs::read_to_string(patch_path)
        .map_err(|err| format!("failed to read patch {}: {err}", patch_path.display()))?;
    let patch_source = match codebook {
        Some(codebook) => {
            codebook.ensure_current(project_input)?;
            codebook.decode_payload(&transport_patch)?
        }
        None => transport_patch.clone(),
    };
    let source_path = match source_path {
        Some(path) => path.to_path_buf(),
        None => locate_patch_source(project_input, &patch_source)?,
    };
    let source = fs::read_to_string(&source_path)
        .map_err(|err| format!("failed to read source {}: {err}", source_path.display()))?;
    let program = parse_file(&source_path, &source)
        .map_err(|err| format!("failed to parse source {}: {err}", source_path.display()))?;
    let result =
        apply_patch_with_options(program, &patch_source, PatchOptions { check_only: true })?;
    let diagnostics = check(&result.program);
    let patch_review = result.review.render_json(&diagnostics);
    let patch_tokens = token_count(&patch_source);
    let transport_patch_tokens = token_count(&transport_patch);
    let source_tokens = token_count(&source);

    let mut out = String::new();
    out.push_str("{\n");
    push_prop(
        &mut out,
        1,
        "schema",
        &quoted("forgentex.agent_review.v1"),
        true,
    );
    push_prop(
        &mut out,
        1,
        "codebook_ref",
        &codebook
            .map(|codebook| quoted(&codebook.artifact_hash()))
            .unwrap_or_else(|| "null".to_string()),
        true,
    );
    push_prop(
        &mut out,
        1,
        "patch",
        &quoted(&patch_path.display().to_string().replace('\\', "/")),
        true,
    );
    push_prop(
        &mut out,
        1,
        "source",
        &quoted(&source_path.display().to_string().replace('\\', "/")),
        true,
    );
    push_string_array(
        &mut out,
        1,
        "negotiation",
        &[
            "propose".to_string(),
            "check".to_string(),
            "review".to_string(),
            "apply".to_string(),
            "sign".to_string(),
        ],
        true,
    );
    push_string_array(
        &mut out,
        1,
        "explanation",
        &review_explanation(&patch_review, diagnostics.is_empty()),
        true,
    );
    out.push_str("  \"token_estimate\": {\n");
    push_prop(
        &mut out,
        2,
        "transport_patch",
        &transport_patch_tokens.to_string(),
        true,
    );
    push_prop(&mut out, 2, "patch", &patch_tokens.to_string(), true);
    push_prop(&mut out, 2, "source", &source_tokens.to_string(), false);
    out.push_str("  },\n");
    out.push_str("  \"patch_review\": ");
    out.push_str(&patch_review);
    out.push('\n');
    out.push('}');
    Ok(out)
}

pub fn review_text(
    patch_path: &Path,
    source_path: Option<&Path>,
    project_input: &Path,
) -> Result<String, String> {
    let json = review_json(patch_path, source_path, project_input)?;
    Ok(format!(
        "Agent review packet\nschema=forgentex.agent_review.v1\n{json}\n"
    ))
}

pub fn review_text_with_codebook(
    patch_path: &Path,
    source_path: Option<&Path>,
    project_input: &Path,
    codebook: Option<&crate::codebook::ProjectCodebook>,
) -> Result<String, String> {
    let json = review_json_with_codebook(patch_path, source_path, project_input, codebook)?;
    Ok(format!(
        "Agent review packet\nschema=forgentex.agent_review.v1\n{json}\n"
    ))
}

fn normalize_symbol(symbol: &str) -> String {
    if symbol.starts_with('@') {
        symbol.to_string()
    } else if let Some(rest) = symbol.strip_prefix("function.") {
        format!("@function.{rest}")
    } else if let Some(rest) = symbol.strip_prefix("struct.") {
        format!("@struct.{rest}")
    } else {
        format!("@function.{symbol}")
    }
}

fn locate_symbol(project: &Project, symbol: &str) -> Result<LocatedSymbol, String> {
    for (module_index, module) in project.modules.iter().enumerate() {
        for item in &module.program.items {
            if node_matches(symbol, item_id(item)) {
                return Ok(LocatedSymbol {
                    module_index,
                    item: item.clone(),
                });
            }
        }
    }
    Err(format!("symbol not found in project context: {symbol}"))
}

fn locate_patch_source(project_input: &Path, patch_source: &str) -> Result<PathBuf, String> {
    let target = patch_target(patch_source).ok_or_else(|| {
        "review requires --source when patch has no TARGET or PATCH line".to_string()
    })?;
    let project = load_project(project_input)?;
    for module in &project.modules {
        for item in &module.program.items {
            if node_matches(&target, item_id(item)) {
                return Ok(module.path.clone());
            }
        }
    }
    Err(format!(
        "could not locate source for patch target {target}; pass --source <file>"
    ))
}

fn patch_target(source: &str) -> Option<String> {
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("TARGET ") {
            return rest.split_whitespace().next().map(str::to_string);
        }
        if let Some(rest) = line.strip_prefix("PATCH ") {
            return rest.split_whitespace().next().map(str::to_string);
        }
    }
    None
}

fn node_matches(target: &str, id: &str) -> bool {
    target == id || target.starts_with(&format!("{id}."))
}

fn item_id(item: &Item) -> &str {
    match item {
        Item::Struct(decl) => &decl.id,
        Item::Enum(decl) => &decl.id,
        Item::Trait(decl) => &decl.id,
        Item::TraitImpl(decl) => &decl.id,
        Item::Function(function) => &function.id,
        Item::Test(test) => &test.id,
        Item::Capsule(capsule) => &capsule.id,
        Item::Share(share) => &share.id,
        Item::Pipeline(pipeline) => &pipeline.id,
        Item::Data(section) => &section.id,
        Item::Blob(section) => &section.id,
        Item::Note(note) => &note.id,
    }
}

fn item_name(item: &Item) -> &str {
    match item {
        Item::Struct(decl) => &decl.name,
        Item::Enum(decl) => &decl.name,
        Item::Trait(decl) => &decl.name,
        Item::TraitImpl(decl) => &decl.trait_name,
        Item::Function(function) => &function.name,
        Item::Test(test) => &test.name,
        Item::Capsule(capsule) => &capsule.name,
        Item::Share(share) => &share.name,
        Item::Pipeline(pipeline) => &pipeline.name,
        Item::Data(section) => &section.name,
        Item::Blob(section) => &section.name,
        Item::Note(note) => &note.name,
    }
}

fn type_slice(project: &Project, item: &Item) -> Vec<Item> {
    let mut wanted = BTreeSet::new();
    collect_item_type_names(item, &mut wanted);
    let mut structs = Vec::new();
    for module in &project.modules {
        if module.standard {
            continue;
        }
        for candidate in &module.program.items {
            let Item::Struct(decl) = candidate else {
                continue;
            };
            if wanted.is_empty() || wanted.contains(&decl.name) {
                structs.push(candidate.clone());
            }
        }
    }
    structs
}

fn collect_item_type_names(item: &Item, out: &mut BTreeSet<String>) {
    match item {
        // Carried sections reference no declared types.
        Item::Data(_) | Item::Blob(_) | Item::Note(_) => {}
        Item::Struct(decl) => {
            for field in &decl.fields {
                collect_type_name(&field.ty, out);
            }
        }
        Item::Enum(_) => {}
        Item::Trait(_) => {}
        Item::TraitImpl(decl) => collect_type_name(&decl.for_type, out),
        Item::Function(function) => {
            for param in &function.params {
                collect_type_name(&param.ty, out);
            }
            collect_type_name(&function.ret, out);
        }
        Item::Capsule(capsule) => {
            for binding in capsule.inputs.iter().chain(&capsule.outputs) {
                collect_type_name(&binding.ty, out);
            }
        }
        Item::Share(share) => collect_type_name(&share.ty, out),
        Item::Pipeline(_) => {}
        Item::Test(_) => {}
    }
}

fn collect_type_name(ty: &crate::ast::Type, out: &mut BTreeSet<String>) {
    match ty {
        crate::ast::Type::Named(name) => {
            out.insert(name.clone());
        }
        crate::ast::Type::Generic { name, args } => {
            out.insert(name.clone());
            for arg in args {
                collect_type_name(arg, out);
            }
        }
        crate::ast::Type::Array { elem, .. }
        | crate::ast::Type::Slice { elem, .. }
        | crate::ast::Type::Pointer { elem, .. } => collect_type_name(elem, out),
        crate::ast::Type::Bool
        | crate::ast::Type::Primitive(_)
        | crate::ast::Type::Void
        | crate::ast::Type::Unknown => {}
    }
}

fn symbol_effects(project: &Project, symbol: &str) -> Option<FunctionEffects> {
    let program = combined_program(project);
    analyze_effects(&program)
        .into_iter()
        .find(|effects| effects.id == symbol)
}

fn diagnostics_for_symbol(project: &Project, symbol: &str) -> Vec<Diagnostic> {
    check_project(&project.manifest_path)
        .unwrap_or_default()
        .into_iter()
        .filter(|diagnostic| {
            node_matches(&diagnostic.node, symbol) || node_matches(symbol, &diagnostic.node)
        })
        .collect()
}

fn unsafe_slices(project: &Project) -> Vec<(usize, Item)> {
    let mut slices = Vec::new();
    for (module_index, module) in project.modules.iter().enumerate() {
        if module.standard {
            continue;
        }
        for item in &module.program.items {
            if let Item::Function(function) = item {
                if function.unsafe_fn || function.has_asm() {
                    slices.push((module_index, item.clone()));
                }
            }
        }
    }
    slices
}

fn symbol_map_lines(project: &Project, module: &ProjectModule, item: &Item) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "M {} {} hash={} tokens~{}",
        module.name,
        display_path(&project.package_root, &module.path),
        module.source_hash,
        module.token_cost
    ));
    match item {
        Item::Data(section) => lines.push(format!(
            "  DATA {} {} {} lines={}",
            section.name,
            section.id,
            section.kind,
            section.content.len()
        )),
        Item::Blob(section) => lines.push(format!(
            "  BLOB {} {} {} bytes={}",
            section.name, section.id, section.media_type, section.bytes
        )),
        Item::Note(note) => lines.push(format!(
            "  NOTE {} {} on {} by {} {}",
            note.name, note.id, note.target, note.author, note.status
        )),
        Item::Struct(decl) => lines.push(format!(
            "  S {} {} fields={}",
            decl.name,
            decl.id,
            decl.fields.len()
        )),
        Item::Enum(decl) => lines.push(format!(
            "  E {} {} variants={}",
            decl.name,
            decl.id,
            decl.variants.len()
        )),
        Item::Trait(decl) => lines.push(format!("  TR {} {}", decl.name, decl.id)),
        Item::TraitImpl(decl) => lines.push(format!(
            "  IMPL {} for {} {}",
            decl.trait_name,
            decl.for_type.to_agent(),
            decl.id
        )),
        Item::Function(function) => {
            let safety = if function.unsafe_fn { " unsafe" } else { "" };
            let asm = if function.has_asm() { " asm" } else { "" };
            let effects = if function.effects.is_empty() {
                String::new()
            } else {
                format!(" effects={}", function.effects.join(","))
            };
            let metadata = target_metadata_for_function(function);
            let targets = if metadata.labels.is_empty() {
                String::new()
            } else {
                format!(" targets={}", metadata.labels.join(","))
            };
            lines.push(format!(
                "  F {} {}{}{}{}{} params={}",
                function.name,
                function.id,
                safety,
                asm,
                effects,
                targets,
                function.params.len()
            ));
        }
        Item::Test(test) => lines.push(format!(
            "  T {} {} xfail={} expectations={}",
            test.name,
            test.id,
            test.xfail,
            test.expectations.len()
        )),
        Item::Capsule(capsule) => lines.push(format!(
            "  CAP {} {} lang={} version={} risk={}",
            capsule.name,
            capsule.id,
            capsule.language,
            capsule.version,
            capsule.risk_class()
        )),
        Item::Share(share) => lines.push(format!(
            "  SHARE {} {} type={}",
            share.name,
            share.id,
            share.ty.to_agent()
        )),
        Item::Pipeline(pipeline) => lines.push(format!(
            "  PIPELINE {} {} stages={} edges={}",
            pipeline.name,
            pipeline.id,
            pipeline.stages.len(),
            pipeline.edges.len()
        )),
    }
    lines
}

#[derive(Clone, Debug)]
struct TestInput {
    path: String,
    tokens: usize,
}

fn test_slice(
    project: &Project,
    module: &ProjectModule,
    item: &Item,
) -> Result<Vec<TestInput>, String> {
    let mut files = Vec::<PathBuf>::new();
    let run_root = project.package_root.join("examples").join("run");
    if !run_root.is_dir() {
        return Ok(Vec::new());
    }
    collect_json_files(&run_root, &mut files)?;
    files.sort();
    let mut terms = vec![item_name(item).to_ascii_lowercase()];
    if let Some(last) = module.name.rsplit('.').next() {
        terms.push(last.to_ascii_lowercase());
    }
    if let Some(stem) = module.path.file_stem().and_then(|stem| stem.to_str()) {
        terms.push(stem.to_ascii_lowercase());
    }
    let mut tests = Vec::new();
    for file in files {
        let stem = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !terms
            .iter()
            .any(|term| !term.is_empty() && stem.contains(term))
        {
            continue;
        }
        let source = fs::read_to_string(&file)
            .map_err(|err| format!("failed to read test input {}: {err}", file.display()))?;
        tests.push(TestInput {
            path: display_path(&project.package_root, &file),
            tokens: token_count(&source),
        });
    }
    Ok(tests)
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root)
        .map_err(|err| format!("failed to read directory {}: {err}", root.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(())
}

fn combined_program(project: &Project) -> Program {
    let mut items = Vec::new();
    for module in &project.modules {
        items.extend(module.program.items.clone());
    }
    Program { items }
}

fn review_explanation(patch_review: &str, diagnostics_ok: bool) -> Vec<String> {
    let mut explanation = vec![
        "Patch was parsed as a semantic patch envelope.".to_string(),
        "Patch was applied to an in-memory preview.".to_string(),
    ];
    if patch_review.contains("\"signature_status\": \"signed-mock-accepted\"") {
        explanation.push("Signed mock envelope was verified before operations ran.".to_string());
    } else if patch_review.contains("\"signature_status\": \"mock-accepted\"") {
        explanation.push("Mock signature placeholder was accepted.".to_string());
    }
    if diagnostics_ok {
        explanation.push("Preview type-check completed without diagnostics.".to_string());
    } else {
        explanation.push("Preview produced diagnostics for human review.".to_string());
    }
    explanation
}

fn token_count(source: &str) -> usize {
    crate::tokens::estimate(source)
}

fn section_hash(source: &str) -> String {
    crate::crypto::sha256_hex(source.as_bytes())
}

fn baseline_matches(
    options: &ContextOptions,
    section: &str,
    hashes: &BTreeMap<String, String>,
) -> bool {
    options.baseline.as_ref().is_some_and(|baseline| {
        baseline.codebook_ref
            == options
                .codebook
                .as_ref()
                .map(|codebook| codebook.artifact_hash())
            && hashes
                .get(section)
                .is_some_and(|hash| baseline.matches(section, hash))
    })
}

fn encode_context_text(options: &ContextOptions, source: &str) -> Result<String, String> {
    match &options.codebook {
        Some(codebook) => codebook.encode_text(source),
        None => Ok(source.to_string()),
    }
}

fn type_section(types: &[Item]) -> String {
    let mut out = String::new();
    push_type_slice(&mut out, 0, types, false);
    out
}

fn effect_section(effects: &FunctionEffects) -> String {
    let mut out = String::new();
    push_effects(&mut out, 0, Some(effects), false);
    out
}

fn test_section(tests: &[TestInput]) -> String {
    let mut out = String::new();
    push_tests(&mut out, 0, tests, false);
    out
}

fn context_packet_hash(
    symbol: &str,
    requested_deps: &[String],
    included_deps: &[String],
    section_hashes: &BTreeMap<String, String>,
    exclusions: &[ContextExclusion],
    max_tokens: Option<usize>,
    codebook_ref: Option<&str>,
) -> String {
    let mut material = format!(
        "schema=forgentex.agent_context.v1\nsymbol={symbol}\nrequested={}\nincluded={}\nmax_tokens={}\ncodebook_ref={}\n",
        requested_deps.join(","),
        included_deps.join(","),
        max_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        codebook_ref.unwrap_or("none")
    );
    for (section, hash) in section_hashes {
        material.push_str(&format!("section={section}:{hash}\n"));
    }
    for exclusion in exclusions {
        material.push_str(&format!(
            "excluded={}:{}:{}:{}\n",
            exclusion.category, exclusion.reason, exclusion.count, exclusion.tokens
        ));
    }
    section_hash(&material)
}

fn parse_context_baseline(source: &str) -> Result<ContextBaseline, String> {
    let value =
        parse_json_value(source).map_err(|error| format!("context_baseline_invalid: {error}"))?;
    let JsonValue::Object(root) = value else {
        return Err("context_baseline_invalid: expected a JSON object".to_string());
    };
    let schema = json_string_field(&root, "schema")?;
    if schema != "forgentex.agent_context.v1" {
        return Err(format!(
            "context_baseline_invalid: schema '{schema}' is not forgentex.agent_context.v1"
        ));
    }
    let symbol = json_string_field(&root, "symbol")?;
    let packet_hash = json_string_field(&root, "packet_hash")?;
    validate_sha256("packet_hash", &packet_hash)?;
    let section_hashes = match root.get("section_hashes") {
        Some(JsonValue::Object(values)) => {
            let mut hashes = BTreeMap::new();
            for (section, value) in values {
                let JsonValue::String(hash) = value else {
                    return Err(format!(
                        "context_baseline_invalid: section hash '{section}' must be a string"
                    ));
                };
                validate_sha256(section, hash)?;
                hashes.insert(section.clone(), hash.clone());
            }
            hashes
        }
        _ => return Err("context_baseline_invalid: missing section_hashes object".to_string()),
    };
    let state_sections = match root.get("state_sections") {
        Some(JsonValue::Array(values)) => values
            .iter()
            .map(|value| match value {
                JsonValue::String(section) => Ok(section.clone()),
                _ => {
                    Err("context_baseline_invalid: state_sections must contain strings".to_string())
                }
            })
            .collect::<Result<BTreeSet<_>, String>>()?,
        _ => return Err("context_baseline_invalid: missing state_sections array".to_string()),
    };
    let codebook_ref = match root.get("codebook_ref") {
        Some(JsonValue::String(value)) => {
            validate_sha256("codebook_ref", value)?;
            Some(value.clone())
        }
        Some(JsonValue::Null) | None => None,
        _ => {
            return Err(
                "context_baseline_invalid: codebook_ref must be a SHA-256 string or null"
                    .to_string(),
            )
        }
    };
    Ok(ContextBaseline {
        symbol,
        packet_hash,
        section_hashes,
        state_sections,
        codebook_ref,
    })
}

fn json_string_field(object: &BTreeMap<String, JsonValue>, key: &str) -> Result<String, String> {
    match object.get(key) {
        Some(JsonValue::String(value)) => Ok(value.clone()),
        _ => Err(format!(
            "context_baseline_invalid: missing string field '{key}'"
        )),
    }
}

fn validate_sha256(field: &str, value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!(
            "context_baseline_invalid: '{field}' must be a 64-character SHA-256"
        ))
    }
}

fn validate_context_options(options: &ContextOptions) -> Result<(), String> {
    if options.max_tokens == Some(0) {
        return Err("context max_tokens must be a positive integer".to_string());
    }
    let mut seen = BTreeSet::new();
    for dep in &options.deps {
        if !matches!(dep.as_str(), "types" | "effects" | "tests") {
            return Err(format!(
                "unknown context dependency '{dep}': expected types, effects, or tests"
            ));
        }
        if !seen.insert(dep) {
            return Err(format!(
                "context dependency '{dep}' was requested more than once"
            ));
        }
    }
    Ok(())
}

fn effect_tokens(effects: &FunctionEffects) -> usize {
    token_count(&format!(
        "declared={}\ninferred={}\nmissing={}\ninvalid={}\ncapabilities={}\n",
        effects.declared.join(","),
        effects.inferred.join(","),
        effects.missing.join(","),
        effects.invalid.join(","),
        effects.capabilities.join(",")
    ))
}

fn push_exclusions(
    out: &mut String,
    indent: usize,
    exclusions: &[ContextExclusion],
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str("\"excluded\": [");
    for (index, exclusion) in exclusions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        push_prop(
            out,
            indent + 2,
            "category",
            &quoted(&exclusion.category),
            true,
        );
        push_prop(out, indent + 2, "reason", &quoted(&exclusion.reason), true);
        push_prop(out, indent + 2, "count", &exclusion.count.to_string(), true);
        push_prop(
            out,
            indent + 2,
            "tokens",
            &exclusion.tokens.to_string(),
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !exclusions.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_section_hashes(
    out: &mut String,
    indent: usize,
    hashes: &BTreeMap<String, String>,
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str("\"section_hashes\": {");
    for (index, (section, hash)) in hashes.iter().enumerate() {
        if index == 0 {
            out.push('\n');
        }
        push_prop(
            out,
            indent + 1,
            section,
            &quoted(hash),
            index + 1 < hashes.len(),
        );
    }
    if !hashes.is_empty() {
        push_indent(out, indent);
    }
    out.push('}');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_type_slice(out: &mut String, indent: usize, types: &[Item], trailing: bool) {
    push_indent(out, indent);
    out.push_str("\"types\": [");
    for (idx, item) in types.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        let source = format_human(&Program {
            items: vec![item.clone()],
        });
        push_prop(out, indent + 2, "id", &quoted(item_id(item)), true);
        push_prop(out, indent + 2, "source", &quoted(&source), true);
        push_prop(
            out,
            indent + 2,
            "tokens",
            &token_count(&source).to_string(),
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !types.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_type_slice_encoded(
    out: &mut String,
    indent: usize,
    types: &[Item],
    codebook: Option<&crate::codebook::ProjectCodebook>,
    trailing: bool,
) -> Result<(), String> {
    push_indent(out, indent);
    out.push_str("\"types\": [");
    for (idx, item) in types.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        let canonical = format_human(&Program {
            items: vec![item.clone()],
        });
        let source = match codebook {
            Some(codebook) => codebook.encode_text(&canonical)?,
            None => canonical,
        };
        push_prop(out, indent + 2, "id", &quoted(item_id(item)), true);
        push_prop(out, indent + 2, "source", &quoted(&source), true);
        push_prop(
            out,
            indent + 2,
            "tokens",
            &token_count(&source).to_string(),
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !types.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
    Ok(())
}

fn push_effects(
    out: &mut String,
    indent: usize,
    effects: Option<&FunctionEffects>,
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str("\"effects\": ");
    if let Some(effects) = effects {
        out.push_str("{\n");
        push_string_array(out, indent + 1, "declared", &effects.declared, true);
        push_string_array(out, indent + 1, "inferred", &effects.inferred, true);
        push_string_array(out, indent + 1, "missing", &effects.missing, true);
        push_string_array(out, indent + 1, "invalid", &effects.invalid, true);
        push_string_array(
            out,
            indent + 1,
            "capabilities",
            &effects.capabilities,
            false,
        );
        push_indent(out, indent);
        out.push('}');
    } else {
        out.push_str("null");
    }
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_tests(out: &mut String, indent: usize, tests: &[TestInput], trailing: bool) {
    push_indent(out, indent);
    out.push_str("\"tests\": [");
    for (idx, test) in tests.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        push_prop(out, indent + 2, "path", &quoted(&test.path), true);
        push_prop(out, indent + 2, "tokens", &test.tokens.to_string(), false);
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !tests.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_diagnostics(out: &mut String, indent: usize, diagnostics: &[Diagnostic], trailing: bool) {
    push_indent(out, indent);
    out.push_str("\"diagnostics\": [");
    for (idx, diagnostic) in diagnostics.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        push_prop(out, indent + 2, "error", &quoted(&diagnostic.code), true);
        push_prop(out, indent + 2, "node", &quoted(&diagnostic.node), true);
        push_prop(
            out,
            indent + 2,
            "message",
            &quoted(&diagnostic.message),
            true,
        );
        push_string_array(
            out,
            indent + 2,
            "suggested_patches",
            &diagnostic.suggested_patches,
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !diagnostics.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_unsafe_slices(
    out: &mut String,
    indent: usize,
    project: &Project,
    slices: &[(usize, Item)],
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str("\"unsafe_slices\": [");
    for (idx, (module_index, item)) in slices.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let module = &project.modules[*module_index];
        let Item::Function(function) = item else {
            continue;
        };
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        push_prop(out, indent + 2, "id", &quoted(&function.id), true);
        push_prop(out, indent + 2, "module", &quoted(&module.name), true);
        push_prop(
            out,
            indent + 2,
            "path",
            &quoted(&display_path(&project.package_root, &module.path)),
            true,
        );
        push_prop(
            out,
            indent + 2,
            "unsafe",
            if function.unsafe_fn { "true" } else { "false" },
            true,
        );
        push_prop(
            out,
            indent + 2,
            "assembly",
            if function.has_asm() { "true" } else { "false" },
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !slices.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_string_array(
    out: &mut String,
    indent: usize,
    key: &str,
    values: &[String],
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_prop(out: &mut String, indent: usize, key: &str, value: &str, trailing: bool) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": {value}"));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape_json(value))
}

use crate::json::escape as escape_json;

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}
