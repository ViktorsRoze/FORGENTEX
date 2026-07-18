use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::diag::Diagnostic;
use crate::effects::{analyze as analyze_effects, FunctionEffects};
use crate::format::format_human;
use crate::parser::parse_file;
use crate::patch::{apply_patch_with_options, PatchOptions};
use crate::project::{check_project, load_project, Project, ProjectModule};
use crate::targets::target_metadata_for_function;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextOptions {
    pub deps: Vec<String>,
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
        Self { deps }
    }

    fn wants(&self, dep: &str) -> bool {
        self.deps.iter().any(|candidate| candidate == dep)
    }
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
    let symbol = normalize_symbol(symbol);
    let located = locate_symbol(&project, &symbol)?;
    let module = &project.modules[located.module_index];
    let item_program = Program {
        items: vec![located.item.clone()],
    };
    let symbol_source = format_human(&item_program);
    let symbol_tokens = token_count(&symbol_source);
    let requested_deps = if options.deps.is_empty() {
        Vec::new()
    } else {
        options.deps.clone()
    };

    let types = if options.wants("types") {
        type_slice(&project, &located.item)
    } else {
        Vec::new()
    };
    let type_tokens = types
        .iter()
        .map(|item| {
            token_count(&format_human(&Program {
                items: vec![item.clone()],
            }))
        })
        .sum::<usize>();

    let effects = if options.wants("effects") {
        symbol_effects(&project, &symbol)
    } else {
        None
    };

    let tests = if options.wants("tests") {
        test_slice(&project, module, &located.item)?
    } else {
        Vec::new()
    };
    let test_tokens = tests.iter().map(|test| test.tokens).sum::<usize>();

    let diagnostics = diagnostics_for_symbol(&project, &symbol);
    let unsafe_slices = unsafe_slices(&project);
    let symbol_map = symbol_map_lines(&project, module, &located.item);
    let project_tokens = project
        .modules
        .iter()
        .map(|module| module.token_cost)
        .sum::<usize>();
    let total_context_tokens = symbol_tokens + type_tokens + test_tokens + symbol_map.len() * 6;

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
    push_string_array(&mut out, 1, "requested_deps", &requested_deps, true);
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
    push_string_array(&mut out, 1, "symbol_map", &symbol_map, true);
    push_prop(&mut out, 1, "symbol_source", &quoted(&symbol_source), true);
    push_type_slice(&mut out, 1, &types, true);
    push_effects(&mut out, 1, effects.as_ref(), true);
    push_tests(&mut out, 1, &tests, true);
    push_diagnostics(&mut out, 1, &diagnostics, true);
    push_unsafe_slices(&mut out, 1, &project, &unsafe_slices, true);
    out.push_str("  \"token_estimate\": {\n");
    push_prop(&mut out, 2, "symbol", &symbol_tokens.to_string(), true);
    push_prop(&mut out, 2, "types", &type_tokens.to_string(), true);
    push_prop(&mut out, 2, "tests", &test_tokens.to_string(), true);
    push_prop(
        &mut out,
        2,
        "total_context",
        &total_context_tokens.to_string(),
        false,
    );
    out.push_str("  }\n");
    out.push('}');
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
    let patch_source = fs::read_to_string(patch_path)
        .map_err(|err| format!("failed to read patch {}: {err}", patch_path.display()))?;
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
    source.split_whitespace().count()
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
