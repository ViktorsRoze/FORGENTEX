use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::diag::Diagnostic;
use crate::effects::{analyze as analyze_effects, FunctionEffects};
use crate::format::{format_agent, format_human};
use crate::parser::parse_file;
use crate::patch::{apply_patch_with_options, PatchOptions};
use crate::project::{load_project, ProjectModule, PublicSymbol};
use crate::targets::target_metadata_for_function;
use crate::typecheck::check;

pub const GRAPH_SCHEMA: &str = "forgentex.graph.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphArtifact {
    pub schema: String,
    pub compiler: String,
    pub source: String,
    pub project_name: String,
    pub project_version: String,
    pub modules: Vec<GraphModule>,
    pub symbols: Vec<GraphSymbol>,
    pub effects: Vec<GraphEffect>,
    pub diagnostics: Vec<GraphDiagnostic>,
    pub patch_history: Vec<String>,
    pub stored_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphModule {
    pub name: String,
    pub path: String,
    pub package: String,
    pub source_kind: String,
    pub source_hash: String,
    pub canonical_ast_hash: String,
    pub tokens: usize,
    pub imports: Vec<String>,
    pub exports: usize,
    pub human: String,
    pub agent: String,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct GraphSymbol {
    pub kind: String,
    pub name: String,
    pub id: String,
    pub module: String,
    pub public: bool,
    pub unsafe_fn: bool,
    pub asm: bool,
    pub params: usize,
    pub return_type: String,
    pub fields: usize,
    pub xfail: bool,
    pub expectations: usize,
    pub effects: Vec<String>,
    pub targets: Vec<String>,
    pub tests: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEffect {
    pub id: String,
    pub declared: Vec<String>,
    pub inferred: Vec<String>,
    pub missing: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphDiagnostic {
    pub code: String,
    pub node: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphValidationReport {
    pub schema: String,
    pub path: String,
    pub status: String,
    pub graph_schema: String,
    pub modules: usize,
    pub symbols: usize,
    pub graph_hash: String,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRoundtripReport {
    pub schema: String,
    pub input: String,
    pub status: String,
    pub modules: usize,
    pub stable_modules: usize,
    pub graph_hash: String,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphPatchReport {
    pub schema: String,
    pub graph: String,
    pub patch: String,
    pub target: String,
    pub status: String,
    pub module: String,
    pub patched_graph: Option<String>,
    pub canonical_ast_equal_source_patch: bool,
    pub diagnostics: Vec<GraphDiagnostic>,
    pub review_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphDiffReport {
    pub schema: String,
    pub before: String,
    pub after: String,
    pub status: String,
    pub before_hash: String,
    pub after_hash: String,
    pub added_symbols: Vec<String>,
    pub removed_symbols: Vec<String>,
    pub changed_symbols: Vec<String>,
    pub review_consequences: Vec<String>,
}

#[derive(Clone, Debug)]
struct ModuleSeed {
    name: String,
    path: String,
    package: String,
    source_kind: String,
    source: String,
    imports: Vec<String>,
    public_symbols: Vec<PublicSymbol>,
    program: Program,
}

pub fn dump_graph_text(input: &Path) -> Result<String, String> {
    Ok(build_graph(input)?.to_text())
}

pub fn load_graph(path: &Path) -> Result<GraphArtifact, String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read graph {}: {err}", path.display()))?;
    GraphArtifact::from_text(&source)
}

pub fn validate_graph(path: &Path) -> GraphValidationReport {
    let path_text = display_path(path);
    match fs::read_to_string(path) {
        Ok(source) => match GraphArtifact::from_text(&source) {
            Ok(graph) => {
                let mut errors = Vec::new();
                if graph.schema != GRAPH_SCHEMA {
                    errors.push(format!("unexpected schema '{}'", graph.schema));
                }
                let computed = computed_graph_hash(&source);
                let stored = graph.stored_hash.clone().unwrap_or_default();
                if computed != stored {
                    errors.push("graph hash mismatch".to_string());
                }
                if graph.modules.is_empty() {
                    errors.push("graph has no modules".to_string());
                }
                let status = if errors.is_empty() { "pass" } else { "fail" };
                GraphValidationReport {
                    schema: "forgentex.graph_validate.v1".to_string(),
                    path: path_text,
                    status: status.to_string(),
                    graph_schema: graph.schema,
                    modules: graph.modules.len(),
                    symbols: graph.symbols.len(),
                    graph_hash: stored,
                    errors,
                }
            }
            Err(err) => GraphValidationReport {
                schema: "forgentex.graph_validate.v1".to_string(),
                path: path_text,
                status: "fail".to_string(),
                graph_schema: "unknown".to_string(),
                modules: 0,
                symbols: 0,
                graph_hash: String::new(),
                errors: vec![err],
            },
        },
        Err(err) => GraphValidationReport {
            schema: "forgentex.graph_validate.v1".to_string(),
            path: path_text,
            status: "fail".to_string(),
            graph_schema: "unknown".to_string(),
            modules: 0,
            symbols: 0,
            graph_hash: String::new(),
            errors: vec![format!("failed to read graph: {err}")],
        },
    }
}

pub fn view_graph(path: &Path, to: &str) -> Result<String, String> {
    let graph = load_graph(path)?;
    let mut out = String::new();
    for (idx, module) in graph.modules.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        match to {
            "fgx" => out.push_str(&module.human),
            "fgai" => out.push_str(&module.agent),
            other => return Err(format!("unknown graph view target '{other}'")),
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    Ok(out)
}

pub fn roundtrip_graph(input: &Path) -> Result<GraphRoundtripReport, String> {
    let graph = build_graph(input)?;
    let mut failures = Vec::new();
    for module in &graph.modules {
        let human = parse_file(Path::new(&module.path), &module.human).map_err(|err| {
            format!(
                "failed to parse graph human view for {}: {err}",
                module.path
            )
        })?;
        let agent_path = PathBuf::from(&module.path).with_extension("fgai");
        let agent = parse_file(&agent_path, &module.agent).map_err(|err| {
            format!(
                "failed to parse graph agent view for {}: {err}",
                module.path
            )
        })?;
        let human_hash = stable_hash(&human.to_canonical_json());
        let agent_hash = stable_hash(&agent.to_canonical_json());
        if human_hash != module.canonical_ast_hash {
            failures.push(format!("{} human canonical hash changed", module.path));
        }
        if agent_hash != module.canonical_ast_hash {
            failures.push(format!("{} agent canonical hash changed", module.path));
        }
    }
    let stable_modules = graph.modules.len().saturating_sub(failures.len());
    let status = if failures.is_empty() { "pass" } else { "fail" };
    Ok(GraphRoundtripReport {
        schema: "forgentex.graph_roundtrip.v1".to_string(),
        input: display_path(input),
        status: status.to_string(),
        modules: graph.modules.len(),
        stable_modules,
        graph_hash: graph.graph_hash(),
        failures,
    })
}

pub fn graph_context_json(
    graph_path: &Path,
    symbol: &str,
    deps: Option<&str>,
) -> Result<String, String> {
    let graph = load_graph(graph_path)?;
    let normalized = normalize_symbol(symbol);
    let symbol = graph
        .symbols
        .iter()
        .find(|candidate| symbol_matches(&normalized, candidate))
        .ok_or_else(|| format!("symbol not found in graph context: {normalized}"))?;
    let module = graph
        .modules
        .iter()
        .find(|module| module.name == symbol.module)
        .ok_or_else(|| format!("module not found for symbol {}", symbol.id))?;
    let requested = deps
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|dep| !dep.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let wants = |name: &str| requested.iter().any(|dep| dep == name);
    let mut out = String::new();
    out.push_str("{\n");
    push_prop(
        &mut out,
        1,
        "schema",
        &quoted("forgentex.graph_context.v1"),
        true,
    );
    push_prop(
        &mut out,
        1,
        "graph",
        &quoted(&display_path(graph_path)),
        true,
    );
    push_prop(&mut out, 1, "symbol", &quoted(&symbol.id), true);
    push_string_array(&mut out, 1, "requested_deps", &requested, true);
    out.push_str("  \"module\": {\n");
    push_prop(&mut out, 2, "name", &quoted(&module.name), true);
    push_prop(&mut out, 2, "path", &quoted(&module.path), true);
    push_prop(
        &mut out,
        2,
        "canonical_ast_hash",
        &quoted(&module.canonical_ast_hash),
        false,
    );
    out.push_str("  },\n");
    push_symbol_json(&mut out, 1, "symbol_fact", symbol, true);
    if wants("effects") {
        let effects = graph
            .effects
            .iter()
            .filter(|effect| effect.id == symbol.id)
            .cloned()
            .collect::<Vec<_>>();
        push_effects_json(&mut out, 1, "effects", &effects, true);
    } else {
        push_line(&mut out, 1, "\"effects\": [],");
    }
    if wants("tests") {
        let tests = graph
            .symbols
            .iter()
            .filter(|candidate| symbol.tests.iter().any(|id| id == &candidate.id))
            .cloned()
            .collect::<Vec<_>>();
        push_symbols_array(&mut out, 1, "tests", &tests, true);
    } else {
        push_line(&mut out, 1, "\"tests\": [],");
    }
    if wants("types") {
        let types = graph
            .symbols
            .iter()
            .filter(|candidate| candidate.kind == "struct")
            .cloned()
            .collect::<Vec<_>>();
        push_symbols_array(&mut out, 1, "types", &types, true);
    } else {
        push_line(&mut out, 1, "\"types\": [],");
    }
    let diagnostics = graph
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.node == symbol.id
                || diagnostic.node.starts_with(&format!("{}.", symbol.id))
                || symbol.id.starts_with(&format!("{}.", diagnostic.node))
        })
        .cloned()
        .collect::<Vec<_>>();
    push_graph_diagnostics(&mut out, 1, "diagnostics", &diagnostics, true);
    push_prop(
        &mut out,
        1,
        "rendered_human_module",
        &quoted(&module.human),
        false,
    );
    out.push('}');
    Ok(out)
}

pub fn graph_context_text(
    graph_path: &Path,
    symbol: &str,
    deps: Option<&str>,
) -> Result<String, String> {
    Ok(format!(
        "Forgentex graph context\nschema=forgentex.graph_context.v1\n{}\n",
        graph_context_json(graph_path, symbol, deps)?
    ))
}

pub fn patch_graph(
    graph_path: &Path,
    patch_path: &Path,
    out_path: Option<&Path>,
) -> Result<GraphPatchReport, String> {
    let graph = load_graph(graph_path)?;
    let patch_source = fs::read_to_string(patch_path)
        .map_err(|err| format!("failed to read patch {}: {err}", patch_path.display()))?;
    let target = patch_target(&patch_source)
        .ok_or_else(|| "graph patch requires a TARGET or PATCH line".to_string())?;
    let module_index = graph
        .symbols
        .iter()
        .find(|symbol| target == symbol.id || target.starts_with(&format!("{}.", symbol.id)))
        .and_then(|symbol| {
            graph
                .modules
                .iter()
                .position(|module| module.name == symbol.module)
        })
        .ok_or_else(|| format!("patch target not found in graph: {target}"))?;
    let module = &graph.modules[module_index];
    let program = parse_file(Path::new(&module.path), &module.human)?;
    let result =
        apply_patch_with_options(program, &patch_source, PatchOptions { check_only: true })?;
    let diagnostics = check(&result.program);
    let review_json = result.review.render_json(&diagnostics);
    let patched_human = format_human(&result.program);
    let reparsed = parse_file(Path::new(&module.path), &patched_human)?;
    let canonical_ast_equal_source_patch = result.program.canonical_eq(&reparsed);
    let diagnostics = diagnostics
        .into_iter()
        .map(graph_diagnostic_from)
        .collect::<Vec<_>>();
    let mut patched_graph = None;
    if let Some(path) = out_path {
        let mut seeds = seeds_from_graph(&graph)?;
        seeds[module_index].source = patched_human;
        seeds[module_index].program = result.program.clone();
        let next_graph = build_graph_from_seeds(
            graph.source.clone(),
            graph.project_name.clone(),
            graph.project_version.clone(),
            seeds,
        );
        fs::write(path, next_graph.to_text())
            .map_err(|err| format!("failed to write patched graph {}: {err}", path.display()))?;
        patched_graph = Some(display_path(path));
    }
    let status = if canonical_ast_equal_source_patch && diagnostics.is_empty() {
        "pass"
    } else {
        "fail"
    };
    Ok(GraphPatchReport {
        schema: "forgentex.graph_patch.v1".to_string(),
        graph: display_path(graph_path),
        patch: display_path(patch_path),
        target,
        status: status.to_string(),
        module: module.name.clone(),
        patched_graph,
        canonical_ast_equal_source_patch,
        diagnostics,
        review_json,
    })
}

pub fn diff_graphs(before_path: &Path, after_path: &Path) -> Result<GraphDiffReport, String> {
    let before = load_graph(before_path)?;
    let after = load_graph(after_path)?;
    let before_symbols = symbol_map(&before);
    let after_symbols = symbol_map(&after);
    let before_ids = before_symbols.keys().cloned().collect::<BTreeSet<_>>();
    let after_ids = after_symbols.keys().cloned().collect::<BTreeSet<_>>();
    let added_symbols = after_ids
        .difference(&before_ids)
        .cloned()
        .collect::<Vec<_>>();
    let removed_symbols = before_ids
        .difference(&after_ids)
        .cloned()
        .collect::<Vec<_>>();
    let changed_symbols = before_ids
        .intersection(&after_ids)
        .filter(|id| before_symbols.get(*id) != after_symbols.get(*id))
        .cloned()
        .collect::<Vec<_>>();
    let mut review_consequences = BTreeSet::new();
    if public_ids(&before) != public_ids(&after) {
        review_consequences.insert("public_api_changed".to_string());
    }
    if unsafe_ids(&before) != unsafe_ids(&after) {
        review_consequences.insert("unsafe_changed".to_string());
    }
    if effect_facts(&before) != effect_facts(&after) {
        review_consequences.insert("capability_changed".to_string());
    }
    if target_facts(&before) != target_facts(&after) {
        review_consequences.insert("target_support_changed".to_string());
    }
    if test_ids(&before) != test_ids(&after) {
        review_consequences.insert("tests_changed".to_string());
    }
    let before_hash = before.graph_hash();
    let after_hash = after.graph_hash();
    let status = if before_hash == after_hash {
        "same"
    } else {
        "different"
    };
    Ok(GraphDiffReport {
        schema: "forgentex.graph_diff.v1".to_string(),
        before: display_path(before_path),
        after: display_path(after_path),
        status: status.to_string(),
        before_hash,
        after_hash,
        added_symbols,
        removed_symbols,
        changed_symbols,
        review_consequences: review_consequences.into_iter().collect(),
    })
}

impl GraphArtifact {
    pub fn to_text(&self) -> String {
        let body = self.render_body();
        let hash = stable_hash(&body);
        format!("{body}GRAPH_HASH hash={}\nEND\n", quoted(&hash))
    }

    pub fn graph_hash(&self) -> String {
        self.stored_hash
            .clone()
            .unwrap_or_else(|| stable_hash(&self.render_body()))
    }

    pub fn from_text(source: &str) -> Result<Self, String> {
        let mut graph = GraphArtifact {
            schema: String::new(),
            compiler: String::new(),
            source: String::new(),
            project_name: String::new(),
            project_version: String::new(),
            modules: Vec::new(),
            symbols: Vec::new(),
            effects: Vec::new(),
            diagnostics: Vec::new(),
            patch_history: Vec::new(),
            stored_hash: None,
        };
        let mut saw_end = false;
        for raw_line in source.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "END" {
                saw_end = true;
                continue;
            }
            let (tag, fields) = parse_record(line)?;
            match tag.as_str() {
                "FGGRAPH" => {
                    graph.schema = required_field(&fields, "schema", line)?;
                    graph.compiler = required_field(&fields, "compiler", line)?;
                    graph.source = required_field(&fields, "source", line)?;
                }
                "PROJECT" => {
                    graph.project_name = required_field(&fields, "name", line)?;
                    graph.project_version = required_field(&fields, "version", line)?;
                }
                "MODULE" => graph.modules.push(GraphModule {
                    name: required_field(&fields, "name", line)?,
                    path: required_field(&fields, "path", line)?,
                    package: required_field(&fields, "package", line)?,
                    source_kind: required_field(&fields, "source_kind", line)?,
                    source_hash: required_field(&fields, "source_hash", line)?,
                    canonical_ast_hash: required_field(&fields, "ast_hash", line)?,
                    tokens: parse_usize_field(&fields, "tokens", line)?,
                    imports: parse_list(&required_field(&fields, "imports", line)?),
                    exports: parse_usize_field(&fields, "exports", line)?,
                    human: required_field(&fields, "human", line)?,
                    agent: required_field(&fields, "agent", line)?,
                }),
                "SYMBOL" => graph.symbols.push(GraphSymbol {
                    kind: required_field(&fields, "kind", line)?,
                    name: required_field(&fields, "name", line)?,
                    id: required_field(&fields, "id", line)?,
                    module: required_field(&fields, "module", line)?,
                    public: parse_bool_field(&fields, "public", line)?,
                    unsafe_fn: parse_bool_field(&fields, "unsafe", line)?,
                    asm: parse_bool_field(&fields, "asm", line)?,
                    params: parse_usize_field(&fields, "params", line)?,
                    return_type: required_field(&fields, "return", line)?,
                    fields: parse_usize_field(&fields, "fields", line)?,
                    xfail: parse_bool_field(&fields, "xfail", line)?,
                    expectations: parse_usize_field(&fields, "expectations", line)?,
                    effects: parse_list(&required_field(&fields, "effects", line)?),
                    targets: parse_list(&required_field(&fields, "targets", line)?),
                    tests: parse_list(&required_field(&fields, "tests", line)?),
                }),
                "EFFECT" => graph.effects.push(GraphEffect {
                    id: required_field(&fields, "id", line)?,
                    declared: parse_list(&required_field(&fields, "declared", line)?),
                    inferred: parse_list(&required_field(&fields, "inferred", line)?),
                    missing: parse_list(&required_field(&fields, "missing", line)?),
                    capabilities: parse_list(&required_field(&fields, "capabilities", line)?),
                }),
                "DIAGNOSTIC" | "BORROW" => graph.diagnostics.push(GraphDiagnostic {
                    code: required_field(&fields, "code", line)?,
                    node: required_field(&fields, "node", line)?,
                    message: required_field(&fields, "message", line)?,
                }),
                "PATCH_HISTORY" => {
                    graph.patch_history = parse_list(&required_field(&fields, "entries", line)?);
                }
                "GRAPH_HASH" => {
                    graph.stored_hash = Some(required_field(&fields, "hash", line)?);
                }
                other => return Err(format!("unknown graph record '{other}'")),
            }
        }
        if !saw_end {
            return Err("graph artifact missing END marker".to_string());
        }
        if graph.schema.is_empty() {
            return Err("graph artifact missing FGGRAPH header".to_string());
        }
        Ok(graph)
    }

    fn render_body(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "FGGRAPH schema={} compiler={} source={}\n",
            quoted(&self.schema),
            quoted(&self.compiler),
            quoted(&self.source)
        ));
        out.push_str(&format!(
            "PROJECT name={} version={} modules={} symbols={}\n",
            quoted(&self.project_name),
            quoted(&self.project_version),
            self.modules.len(),
            self.symbols.len()
        ));
        for module in &self.modules {
            out.push_str(&format!(
                "MODULE name={} path={} package={} source_kind={} source_hash={} ast_hash={} tokens={} imports={} exports={} human={} agent={}\n",
                quoted(&module.name),
                quoted(&module.path),
                quoted(&module.package),
                quoted(&module.source_kind),
                quoted(&module.source_hash),
                quoted(&module.canonical_ast_hash),
                module.tokens,
                quoted(&format_list(&module.imports)),
                module.exports,
                quoted(&module.human),
                quoted(&module.agent)
            ));
        }
        for symbol in &self.symbols {
            out.push_str(&format!(
                "SYMBOL kind={} name={} id={} module={} public={} unsafe={} asm={} params={} return={} fields={} xfail={} expectations={} effects={} targets={} tests={}\n",
                quoted(&symbol.kind),
                quoted(&symbol.name),
                quoted(&symbol.id),
                quoted(&symbol.module),
                symbol.public,
                symbol.unsafe_fn,
                symbol.asm,
                symbol.params,
                quoted(&symbol.return_type),
                symbol.fields,
                symbol.xfail,
                symbol.expectations,
                quoted(&format_list(&symbol.effects)),
                quoted(&format_list(&symbol.targets)),
                quoted(&format_list(&symbol.tests))
            ));
        }
        for effect in &self.effects {
            out.push_str(&format!(
                "EFFECT id={} declared={} inferred={} missing={} capabilities={}\n",
                quoted(&effect.id),
                quoted(&format_list(&effect.declared)),
                quoted(&format_list(&effect.inferred)),
                quoted(&format_list(&effect.missing)),
                quoted(&format_list(&effect.capabilities))
            ));
        }
        for diagnostic in &self.diagnostics {
            let tag = if is_borrow_code(&diagnostic.code) {
                "BORROW"
            } else {
                "DIAGNOSTIC"
            };
            out.push_str(&format!(
                "{tag} code={} node={} message={}\n",
                quoted(&diagnostic.code),
                quoted(&diagnostic.node),
                quoted(&diagnostic.message)
            ));
        }
        out.push_str(&format!(
            "PATCH_HISTORY entries={}\n",
            quoted(&format_list(&self.patch_history))
        ));
        out
    }
}

impl GraphValidationReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex graph validation\n");
        out.push_str(&format!(
            "schema={} path={} status={} graph_schema={} modules={} symbols={} graph_hash={}\n",
            self.schema,
            self.path,
            self.status,
            self.graph_schema,
            self.modules,
            self.symbols,
            self.graph_hash
        ));
        for error in &self.errors {
            out.push_str(&format!("  - error {error}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "path", &quoted(&self.path), true);
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_prop(
            &mut out,
            1,
            "graph_schema",
            &quoted(&self.graph_schema),
            true,
        );
        push_prop(&mut out, 1, "modules", &self.modules.to_string(), true);
        push_prop(&mut out, 1, "symbols", &self.symbols.to_string(), true);
        push_prop(&mut out, 1, "graph_hash", &quoted(&self.graph_hash), true);
        push_string_array(&mut out, 1, "errors", &self.errors, false);
        out.push('}');
        out
    }
}

impl GraphRoundtripReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex graph roundtrip\nschema={} input={} status={} modules={} stable_modules={} graph_hash={} failures=[{}]\n",
            self.schema,
            self.input,
            self.status,
            self.modules,
            self.stable_modules,
            self.graph_hash,
            self.failures.join(",")
        )
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "input", &quoted(&self.input), true);
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_prop(&mut out, 1, "modules", &self.modules.to_string(), true);
        push_prop(
            &mut out,
            1,
            "stable_modules",
            &self.stable_modules.to_string(),
            true,
        );
        push_prop(&mut out, 1, "graph_hash", &quoted(&self.graph_hash), true);
        push_string_array(&mut out, 1, "failures", &self.failures, false);
        out.push('}');
        out
    }
}

impl GraphPatchReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex graph patch\nschema={} graph={} patch={} target={} status={} module={} canonical_ast_equal_source_patch={} diagnostics={}\n",
            self.schema,
            self.graph,
            self.patch,
            self.target,
            self.status,
            self.module,
            self.canonical_ast_equal_source_patch,
            self.diagnostics.len()
        )
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "graph", &quoted(&self.graph), true);
        push_prop(&mut out, 1, "patch", &quoted(&self.patch), true);
        push_prop(&mut out, 1, "target", &quoted(&self.target), true);
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_prop(&mut out, 1, "module", &quoted(&self.module), true);
        push_prop(
            &mut out,
            1,
            "patched_graph",
            &self
                .patched_graph
                .as_ref()
                .map(|path| quoted(path))
                .unwrap_or_else(|| "null".to_string()),
            true,
        );
        push_prop(
            &mut out,
            1,
            "canonical_ast_equal_source_patch",
            if self.canonical_ast_equal_source_patch {
                "true"
            } else {
                "false"
            },
            true,
        );
        push_graph_diagnostics(&mut out, 1, "diagnostics", &self.diagnostics, true);
        push_line(&mut out, 1, "\"patch_review\": ");
        out.push_str(&indent_json(&self.review_json, 1));
        out.push('\n');
        out.push('}');
        out
    }
}

impl GraphDiffReport {
    pub fn to_text(&self) -> String {
        format!(
            "Forgentex graph diff\nschema={} before={} after={} status={} added={} removed={} changed={} consequences=[{}]\n",
            self.schema,
            self.before,
            self.after,
            self.status,
            self.added_symbols.len(),
            self.removed_symbols.len(),
            self.changed_symbols.len(),
            self.review_consequences.join(",")
        )
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "before", &quoted(&self.before), true);
        push_prop(&mut out, 1, "after", &quoted(&self.after), true);
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_prop(&mut out, 1, "before_hash", &quoted(&self.before_hash), true);
        push_prop(&mut out, 1, "after_hash", &quoted(&self.after_hash), true);
        push_string_array(&mut out, 1, "added_symbols", &self.added_symbols, true);
        push_string_array(&mut out, 1, "removed_symbols", &self.removed_symbols, true);
        push_string_array(&mut out, 1, "changed_symbols", &self.changed_symbols, true);
        push_string_array(
            &mut out,
            1,
            "review_consequences",
            &self.review_consequences,
            false,
        );
        out.push('}');
        out
    }
}

fn build_graph(input: &Path) -> Result<GraphArtifact, String> {
    if input.is_file() && is_source(input) {
        return build_file_graph(input);
    }
    match load_project(input) {
        Ok(project) => {
            let mut seeds = Vec::new();
            for module in &project.modules {
                let source = fs::read_to_string(&module.path)
                    .map_err(|err| format!("failed to read {}: {err}", module.path.display()))?;
                seeds.push(seed_from_project_module(
                    &project.package_root,
                    module,
                    source,
                ));
            }
            Ok(build_graph_from_seeds(
                display_path(input),
                project.manifest.name,
                project.manifest.version,
                seeds,
            ))
        }
        Err(project_err) => {
            if input.is_dir() {
                build_directory_graph(input)
            } else {
                Err(project_err)
            }
        }
    }
}

fn build_file_graph(input: &Path) -> Result<GraphArtifact, String> {
    let source = fs::read_to_string(input)
        .map_err(|err| format!("failed to read {}: {err}", input.display()))?;
    let program = parse_file(input, &source)?;
    let seed = seed_from_source(
        module_name_from_source(
            input.parent().unwrap_or_else(|| Path::new(".")),
            input,
            &source,
        ),
        display_path(input),
        "single-file".to_string(),
        "file".to_string(),
        source,
        program,
        Vec::new(),
    );
    Ok(build_graph_from_seeds(
        display_path(input),
        "single-file".to_string(),
        "0.0.0".to_string(),
        vec![seed],
    ))
}

fn build_directory_graph(input: &Path) -> Result<GraphArtifact, String> {
    let mut files = Vec::new();
    collect_sources(input, &mut files)?;
    files = preferred_source_views(files);
    files.sort();
    let mut seeds = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file)
            .map_err(|err| format!("failed to read {}: {err}", file.display()))?;
        let program = parse_file(&file, &source)?;
        seeds.push(seed_from_source(
            module_name_from_source(input, &file, &source),
            display_path(&file),
            "directory".to_string(),
            "local".to_string(),
            source,
            program,
            Vec::new(),
        ));
    }
    Ok(build_graph_from_seeds(
        display_path(input),
        "directory".to_string(),
        "0.0.0".to_string(),
        seeds,
    ))
}

fn build_graph_from_seeds(
    source: String,
    project_name: String,
    project_version: String,
    mut seeds: Vec<ModuleSeed>,
) -> GraphArtifact {
    seeds.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut modules = Vec::new();
    let mut symbols = Vec::new();
    let mut items = Vec::new();
    for seed in &seeds {
        let human = format_human(&seed.program);
        let agent = format_agent(&seed.program);
        let canonical = seed.program.to_canonical_json();
        let test_ids = seed
            .program
            .tests()
            .map(|test| test.id.clone())
            .collect::<Vec<_>>();
        let public_ids = seed
            .public_symbols
            .iter()
            .map(|symbol| symbol.id.clone())
            .collect::<BTreeSet<_>>();
        modules.push(GraphModule {
            name: seed.name.clone(),
            path: seed.path.clone(),
            package: seed.package.clone(),
            source_kind: seed.source_kind.clone(),
            source_hash: stable_hash(&seed.source),
            canonical_ast_hash: stable_hash(&canonical),
            tokens: seed.source.split_whitespace().count(),
            imports: seed.imports.clone(),
            exports: seed.public_symbols.len(),
            human,
            agent,
        });
        for item in &seed.program.items {
            symbols.push(symbol_from_item(item, &seed.name, &public_ids, &test_ids));
        }
        items.extend(seed.program.items.clone());
    }
    symbols.sort_by(|left, right| left.id.cmp(&right.id));
    let program = Program { items };
    let effects = analyze_effects(&program)
        .into_iter()
        .map(graph_effect_from)
        .collect::<Vec<_>>();
    let diagnostics = check(&program)
        .into_iter()
        .map(graph_diagnostic_from)
        .collect::<Vec<_>>();
    GraphArtifact {
        schema: GRAPH_SCHEMA.to_string(),
        compiler: "forgentex-stage31".to_string(),
        source,
        project_name,
        project_version,
        modules,
        symbols,
        effects,
        diagnostics,
        patch_history: Vec::new(),
        stored_hash: None,
    }
}

fn seed_from_project_module(
    package_root: &Path,
    module: &ProjectModule,
    source: String,
) -> ModuleSeed {
    let source_kind = if module.standard {
        "stdlib"
    } else if module.dependency {
        "dependency"
    } else {
        "local"
    };
    ModuleSeed {
        name: module.name.clone(),
        path: module
            .path
            .strip_prefix(package_root)
            .unwrap_or(&module.path)
            .display()
            .to_string()
            .replace('\\', "/"),
        package: module.package.clone(),
        source_kind: source_kind.to_string(),
        source,
        imports: module.imports.clone(),
        public_symbols: module.public_symbols.clone(),
        program: module.program.clone(),
    }
}

fn seed_from_source(
    name: String,
    path: String,
    package: String,
    source_kind: String,
    source: String,
    program: Program,
    public_symbols: Vec<PublicSymbol>,
) -> ModuleSeed {
    let imports = module_imports(&source);
    let public_symbols = if public_symbols.is_empty() {
        public_symbols_from_source(&source, &program)
    } else {
        public_symbols
    };
    ModuleSeed {
        name,
        path,
        package,
        source_kind,
        source,
        imports,
        public_symbols,
        program,
    }
}

fn seeds_from_graph(graph: &GraphArtifact) -> Result<Vec<ModuleSeed>, String> {
    graph
        .modules
        .iter()
        .map(|module| {
            let program = parse_file(Path::new(&module.path), &module.human)?;
            Ok(seed_from_source(
                module.name.clone(),
                module.path.clone(),
                module.package.clone(),
                module.source_kind.clone(),
                module.human.clone(),
                program,
                Vec::new(),
            ))
        })
        .collect()
}

fn symbol_from_item(
    item: &Item,
    module: &str,
    public_ids: &BTreeSet<String>,
    module_tests: &[String],
) -> GraphSymbol {
    match item {
        // Carried sections appear in the project map, not the symbol graph:
        // they have no callers, callees, or types to relate.
        Item::Data(section) => GraphSymbol {
            kind: "data".to_string(),
            name: section.name.clone(),
            id: section.id.clone(),
            ..GraphSymbol::default()
        },
        Item::Blob(section) => GraphSymbol {
            kind: "blob".to_string(),
            name: section.name.clone(),
            id: section.id.clone(),
            ..GraphSymbol::default()
        },
        Item::Note(note) => GraphSymbol {
            kind: "note".to_string(),
            name: note.name.clone(),
            id: note.id.clone(),
            ..GraphSymbol::default()
        },
        Item::Struct(decl) => GraphSymbol {
            kind: "struct".to_string(),
            name: decl.name.clone(),
            id: decl.id.clone(),
            module: module.to_string(),
            public: public_ids.contains(&decl.id),
            unsafe_fn: false,
            asm: false,
            params: 0,
            return_type: "type".to_string(),
            fields: decl.fields.len(),
            xfail: false,
            expectations: 0,
            effects: Vec::new(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::Enum(decl) => GraphSymbol {
            kind: "enum".to_string(),
            name: decl.name.clone(),
            id: decl.id.clone(),
            module: module.to_string(),
            public: public_ids.contains(&decl.id),
            unsafe_fn: false,
            asm: false,
            params: 0,
            return_type: "type".to_string(),
            fields: decl.variants.len(),
            xfail: false,
            expectations: 0,
            effects: Vec::new(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::Trait(decl) => GraphSymbol {
            kind: "trait".to_string(),
            name: decl.name.clone(),
            id: decl.id.clone(),
            module: module.to_string(),
            public: public_ids.contains(&decl.id),
            unsafe_fn: false,
            asm: false,
            params: 0,
            return_type: "marker_trait".to_string(),
            fields: 0,
            xfail: false,
            expectations: 0,
            effects: Vec::new(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::TraitImpl(decl) => GraphSymbol {
            kind: "trait_impl".to_string(),
            name: format!("{} for {}", decl.trait_name, decl.for_type.to_agent()),
            id: decl.id.clone(),
            module: module.to_string(),
            public: false,
            unsafe_fn: false,
            asm: false,
            params: 0,
            return_type: decl.for_type.to_agent(),
            fields: 0,
            xfail: false,
            expectations: 0,
            effects: Vec::new(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::Function(function) => {
            let metadata = target_metadata_for_function(function);
            GraphSymbol {
                kind: "function".to_string(),
                name: function.name.clone(),
                id: function.id.clone(),
                module: module.to_string(),
                public: public_ids.contains(&function.id),
                unsafe_fn: function.unsafe_fn,
                asm: function.has_asm(),
                params: function.params.len(),
                return_type: function.ret.to_agent(),
                fields: 0,
                xfail: false,
                expectations: 0,
                effects: function.effects.clone(),
                targets: metadata.labels,
                tests: module_tests.to_vec(),
            }
        }
        Item::Test(test) => GraphSymbol {
            kind: "test".to_string(),
            name: test.name.clone(),
            id: test.id.clone(),
            module: module.to_string(),
            public: false,
            unsafe_fn: false,
            asm: false,
            params: 0,
            return_type: "test".to_string(),
            fields: 0,
            xfail: test.xfail,
            expectations: test.expectations.len(),
            effects: Vec::new(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::Capsule(capsule) => GraphSymbol {
            kind: "capsule".to_string(),
            name: capsule.name.clone(),
            id: capsule.id.clone(),
            module: module.to_string(),
            public: public_ids.contains(&capsule.id),
            unsafe_fn: false,
            asm: false,
            params: capsule.inputs.len(),
            return_type: capsule.language.clone(),
            fields: capsule.outputs.len(),
            xfail: false,
            expectations: 0,
            effects: capsule.effects.clone(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::Share(share) => GraphSymbol {
            kind: "share".to_string(),
            name: share.name.clone(),
            id: share.id.clone(),
            module: module.to_string(),
            public: public_ids.contains(&share.id),
            unsafe_fn: false,
            asm: false,
            params: 0,
            return_type: share.ty.to_agent(),
            fields: 0,
            xfail: false,
            expectations: 0,
            effects: Vec::new(),
            targets: Vec::new(),
            tests: Vec::new(),
        },
        Item::Pipeline(pipeline) => GraphSymbol {
            kind: "pipeline".to_string(),
            name: pipeline.name.clone(),
            id: pipeline.id.clone(),
            module: module.to_string(),
            public: public_ids.contains(&pipeline.id),
            unsafe_fn: false,
            asm: false,
            params: pipeline.stages.len(),
            return_type: "pipeline".to_string(),
            fields: pipeline.edges.len(),
            xfail: false,
            expectations: 0,
            effects: Vec::new(),
            targets: pipeline.stages.clone(),
            tests: Vec::new(),
        },
    }
}

fn graph_effect_from(effect: FunctionEffects) -> GraphEffect {
    GraphEffect {
        id: effect.id,
        declared: effect.declared,
        inferred: effect.inferred,
        missing: effect.missing,
        capabilities: effect.capabilities,
    }
}

fn graph_diagnostic_from(diagnostic: Diagnostic) -> GraphDiagnostic {
    GraphDiagnostic {
        code: diagnostic.code,
        node: diagnostic.node,
        message: diagnostic.message,
    }
}

fn symbol_map(graph: &GraphArtifact) -> BTreeMap<String, GraphSymbol> {
    graph
        .symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol.clone()))
        .collect()
}

fn public_ids(graph: &GraphArtifact) -> BTreeSet<String> {
    graph
        .symbols
        .iter()
        .filter(|symbol| symbol.public)
        .map(|symbol| symbol.id.clone())
        .collect()
}

fn unsafe_ids(graph: &GraphArtifact) -> BTreeSet<String> {
    graph
        .symbols
        .iter()
        .filter(|symbol| symbol.unsafe_fn || symbol.asm)
        .map(|symbol| symbol.id.clone())
        .collect()
}

fn effect_facts(graph: &GraphArtifact) -> BTreeSet<String> {
    graph
        .effects
        .iter()
        .map(|effect| {
            format!(
                "{}:{}:{}:{}",
                effect.id,
                format_list(&effect.declared),
                format_list(&effect.inferred),
                format_list(&effect.capabilities)
            )
        })
        .collect()
}

fn target_facts(graph: &GraphArtifact) -> BTreeSet<String> {
    graph
        .symbols
        .iter()
        .filter(|symbol| !symbol.targets.is_empty())
        .map(|symbol| format!("{}:{}", symbol.id, format_list(&symbol.targets)))
        .collect()
}

fn test_ids(graph: &GraphArtifact) -> BTreeSet<String> {
    graph
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == "test")
        .map(|symbol| symbol.id.clone())
        .collect()
}

fn public_symbols_from_source(source: &str, program: &Program) -> Vec<PublicSymbol> {
    let exported = exported_names(source);
    let mut symbols = Vec::new();
    for item in &program.items {
        match item {
            Item::Struct(decl) if exported.contains(&format!("struct:{}", decl.name)) => {
                symbols.push(PublicSymbol {
                    kind: "struct".to_string(),
                    name: decl.name.clone(),
                    id: decl.id.clone(),
                    signature: format!("fields={}", decl.fields.len()),
                });
            }
            Item::Function(function) if exported.contains(&format!("fn:{}", function.name)) => {
                symbols.push(PublicSymbol {
                    kind: "fn".to_string(),
                    name: function.name.clone(),
                    id: function.id.clone(),
                    signature: format!(
                        "params={} return={}",
                        function.params.len(),
                        function.ret.to_agent()
                    ),
                });
            }
            _ => {}
        }
    }
    symbols
}

fn exported_names(source: &str) -> BTreeSet<String> {
    let mut exported = BTreeSet::new();
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        let public = line
            .strip_prefix("export ")
            .or_else(|| line.strip_prefix("pub "))
            .or_else(|| line.strip_prefix("EXPORT "))
            .or_else(|| line.strip_prefix("PUB "));
        let Some(public) = public else {
            continue;
        };
        if let Some(rest) = public.strip_prefix("struct ") {
            if let Some(name) = rest.split([':', ' ']).find(|part| !part.is_empty()) {
                exported.insert(format!("struct:{name}"));
            }
        } else if let Some(rest) = public.strip_prefix("fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                exported.insert(format!("fn:{}", name.trim()));
            }
        } else if let Some(rest) = public.strip_prefix("unsafe fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                exported.insert(format!("fn:{}", name.trim()));
            }
        }
    }
    exported
}

fn module_imports(source: &str) -> Vec<String> {
    let mut imports = BTreeSet::new();
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        let import = line
            .strip_prefix("import ")
            .or_else(|| line.strip_prefix("I "))
            .or_else(|| line.strip_prefix("IMPORT "));
        if let Some(import) = import {
            if let Some(name) = import.split_whitespace().next() {
                imports.insert(name.to_string());
            }
        }
    }
    imports.into_iter().collect()
}

fn module_name_from_source(root: &Path, path: &Path, source: &str) -> String {
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if let Some(name) = line.strip_prefix("module ") {
            return name.trim().to_string();
        }
        if let Some(name) = line.strip_prefix("M ") {
            return name.trim().to_string();
        }
    }
    path.strip_prefix(root)
        .unwrap_or(path)
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join(".")
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

fn normalize_symbol(symbol: &str) -> String {
    if symbol.starts_with('@') {
        symbol.to_string()
    } else if let Some(rest) = symbol.strip_prefix("function.") {
        format!("@function.{rest}")
    } else if let Some(rest) = symbol.strip_prefix("struct.") {
        format!("@struct.{rest}")
    } else if let Some(rest) = symbol.strip_prefix("test.") {
        format!("@test.{rest}")
    } else {
        format!("@function.{symbol}")
    }
}

fn symbol_matches(target: &str, symbol: &GraphSymbol) -> bool {
    target == symbol.id || target == symbol.name || target.starts_with(&format!("{}.", symbol.id))
}

fn collect_sources(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if root.is_file() {
        if is_source(root) {
            files.push(root.to_path_buf());
        }
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|err| format!("failed to read directory {}: {err}", root.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, files)?;
        } else if is_source(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn preferred_source_views(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut grouped: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for file in files {
        let key = file.with_extension("");
        match grouped.get(&key) {
            Some(existing) if source_rank(existing) <= source_rank(&file) => {}
            _ => {
                grouped.insert(key, file);
            }
        }
    }
    grouped.into_values().collect()
}

fn source_rank(path: &Path) -> u8 {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("fgx") => 0,
        Some("fgai") => 1,
        _ => 2,
    }
}

fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("fgx") | Some("fgai")
    )
}

fn parse_record(line: &str) -> Result<(String, BTreeMap<String, String>), String> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let tag = parts.next().unwrap_or("").to_string();
    let rest = parts.next().unwrap_or("").trim();
    let mut fields = BTreeMap::new();
    let mut index = 0usize;
    while index < rest.len() {
        while index < rest.len() && rest.as_bytes()[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= rest.len() {
            break;
        }
        let key_start = index;
        while index < rest.len() && rest.as_bytes()[index] != b'=' {
            index += 1;
        }
        if index >= rest.len() {
            return Err(format!("invalid graph field in line: {line}"));
        }
        let key = rest[key_start..index].trim().to_string();
        index += 1;
        let value = if index < rest.len() && rest.as_bytes()[index] == b'"' {
            index += 1;
            let mut raw = String::new();
            let mut escaped = false;
            while index < rest.len() {
                let ch = rest[index..].chars().next().unwrap();
                index += ch.len_utf8();
                if escaped {
                    raw.push(match ch {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '"' => '"',
                        '\\' => '\\',
                        other => other,
                    });
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    break;
                } else {
                    raw.push(ch);
                }
            }
            raw
        } else {
            let value_start = index;
            while index < rest.len() && !rest.as_bytes()[index].is_ascii_whitespace() {
                index += 1;
            }
            rest[value_start..index].to_string()
        };
        fields.insert(key, value);
    }
    Ok((tag, fields))
}

fn required_field(
    fields: &BTreeMap<String, String>,
    name: &str,
    line: &str,
) -> Result<String, String> {
    fields
        .get(name)
        .cloned()
        .ok_or_else(|| format!("missing graph field '{name}' in line: {line}"))
}

fn parse_usize_field(
    fields: &BTreeMap<String, String>,
    name: &str,
    line: &str,
) -> Result<usize, String> {
    required_field(fields, name, line)?
        .parse::<usize>()
        .map_err(|err| format!("invalid graph field '{name}' in line '{line}': {err}"))
}

fn parse_bool_field(
    fields: &BTreeMap<String, String>,
    name: &str,
    line: &str,
) -> Result<bool, String> {
    required_field(fields, name, line)?
        .parse::<bool>()
        .map_err(|err| format!("invalid graph field '{name}' in line '{line}': {err}"))
}

fn parse_list(value: &str) -> Vec<String> {
    if value == "-" || value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(str::to_string).collect()
    }
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

fn computed_graph_hash(source: &str) -> String {
    let mut body = String::new();
    for line in source.lines() {
        if line.starts_with("GRAPH_HASH ") || line == "END" {
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    stable_hash(&body)
}

fn stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn is_borrow_code(code: &str) -> bool {
    matches!(
        code,
        "use_after_move"
            | "mutable_borrow_alias"
            | "escaping_reference"
            | "raw_pointer_requires_unsafe"
            | "raw_pointer_dereference_requires_unsafe"
            | "immutable_write"
            | "BOR001"
            | "BOR002"
            | "BOR003"
            | "BOR004"
            | "BOR005"
    )
}

fn display_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape(value))
}

use crate::json::escape;

fn push_symbol_json(
    out: &mut String,
    indent: usize,
    key: &str,
    symbol: &GraphSymbol,
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": "));
    push_symbol_body(out, indent, symbol);
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_symbols_array(
    out: &mut String,
    indent: usize,
    key: &str,
    symbols: &[GraphSymbol],
    trailing: bool,
) {
    push_line(out, indent, &format!("\"{key}\": ["));
    for (idx, symbol) in symbols.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_indent(out, indent + 1);
        push_symbol_body(out, indent + 1, symbol);
    }
    if !symbols.is_empty() {
        out.push('\n');
    }
    push_indent(out, indent);
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_symbol_body(out: &mut String, indent: usize, symbol: &GraphSymbol) {
    out.push_str("{\n");
    push_prop(out, indent + 1, "kind", &quoted(&symbol.kind), true);
    push_prop(out, indent + 1, "name", &quoted(&symbol.name), true);
    push_prop(out, indent + 1, "id", &quoted(&symbol.id), true);
    push_prop(out, indent + 1, "module", &quoted(&symbol.module), true);
    push_prop(
        out,
        indent + 1,
        "public",
        if symbol.public { "true" } else { "false" },
        true,
    );
    push_prop(
        out,
        indent + 1,
        "unsafe",
        if symbol.unsafe_fn { "true" } else { "false" },
        true,
    );
    push_prop(
        out,
        indent + 1,
        "asm",
        if symbol.asm { "true" } else { "false" },
        true,
    );
    push_prop(
        out,
        indent + 1,
        "return",
        &quoted(&symbol.return_type),
        true,
    );
    push_string_array(out, indent + 1, "effects", &symbol.effects, true);
    push_string_array(out, indent + 1, "targets", &symbol.targets, true);
    push_string_array(out, indent + 1, "tests", &symbol.tests, false);
    push_indent(out, indent);
    out.push('}');
}

fn push_effects_json(
    out: &mut String,
    indent: usize,
    key: &str,
    effects: &[GraphEffect],
    trailing: bool,
) {
    push_line(out, indent, &format!("\"{key}\": ["));
    for (idx, effect) in effects.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_line(out, indent + 1, "{");
        push_prop(out, indent + 2, "id", &quoted(&effect.id), true);
        push_string_array(out, indent + 2, "declared", &effect.declared, true);
        push_string_array(out, indent + 2, "inferred", &effect.inferred, true);
        push_string_array(out, indent + 2, "missing", &effect.missing, true);
        push_string_array(out, indent + 2, "capabilities", &effect.capabilities, false);
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !effects.is_empty() {
        out.push('\n');
    }
    push_indent(out, indent);
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_graph_diagnostics(
    out: &mut String,
    indent: usize,
    key: &str,
    diagnostics: &[GraphDiagnostic],
    trailing: bool,
) {
    push_line(out, indent, &format!("\"{key}\": ["));
    for (idx, diagnostic) in diagnostics.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_line(out, indent + 1, "{");
        push_prop(out, indent + 2, "code", &quoted(&diagnostic.code), true);
        push_prop(out, indent + 2, "node", &quoted(&diagnostic.node), true);
        push_prop(
            out,
            indent + 2,
            "message",
            &quoted(&diagnostic.message),
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !diagnostics.is_empty() {
        out.push('\n');
    }
    push_indent(out, indent);
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

fn push_line(out: &mut String, indent: usize, line: &str) {
    push_indent(out, indent);
    out.push_str(line);
    out.push('\n');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn indent_json(value: &str, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    value
        .lines()
        .enumerate()
        .map(|(idx, line)| {
            if idx == 0 {
                line.to_string()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
