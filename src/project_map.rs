use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::format::format_human;
use crate::parser::parse_file;
use crate::targets::target_metadata_for_function;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProjectMapOptions {
    pub include_targets: bool,
}

pub fn generate_map(root: &Path) -> Result<String, String> {
    generate_map_with_options(root, ProjectMapOptions::default())
}

pub fn generate_map_with_options(
    root: &Path,
    options: ProjectMapOptions,
) -> Result<String, String> {
    let mut files = Vec::new();
    collect_sources(root, &mut files)?;
    files = preferred_source_views(files);
    files.sort();
    let mut out = String::new();
    for file in files {
        let source = fs::read_to_string(&file)
            .map_err(|err| format!("failed to read {}: {err}", file.display()))?;
        let program = parse_file(&file, &source)
            .map_err(|err| format!("failed to parse {}: {err}", file.display()))?;
        out.push_str(&format!("M {}\n", display_path(root, &file)));
        for item in &program.items {
            let token_cost = crate::tokens::estimate(&format_human(&Program {
                items: vec![item.clone()],
            }));
            match item {
                Item::Data(section) => out.push_str(&format!(
                    "  DATA {} {} {} lines={}
",
                    section.name,
                    section.id,
                    section.kind,
                    section.content.len()
                )),
                Item::Blob(section) => out.push_str(&format!(
                    "  BLOB {} {} {} bytes={}
",
                    section.name, section.id, section.media_type, section.bytes
                )),
                Item::Note(note) => out.push_str(&format!(
                    "  NOTE {} {} on {} by {}
",
                    note.name, note.id, note.target, note.author
                )),
                Item::Struct(decl) => {
                    out.push_str(&format!(
                        "  S {} {} fields={} tokens~{}\n",
                        decl.name,
                        decl.id,
                        decl.fields.len(),
                        token_cost
                    ));
                }
                Item::Enum(decl) => {
                    out.push_str(&format!(
                        "  E {} {} variants={} tokens~{}\n",
                        decl.name,
                        decl.id,
                        decl.variants.len(),
                        token_cost
                    ));
                }
                Item::Trait(decl) => {
                    out.push_str(&format!(
                        "  TR {} {} tokens~{}\n",
                        decl.name, decl.id, token_cost
                    ));
                }
                Item::TraitImpl(decl) => {
                    out.push_str(&format!(
                        "  IM {} {} {} tokens~{}\n",
                        decl.trait_name,
                        decl.for_type.to_agent(),
                        decl.id,
                        token_cost
                    ));
                }
                Item::Function(function) => {
                    let safety = if function.unsafe_fn { " unsafe" } else { "" };
                    let asm = if function.has_asm() { " asm" } else { "" };
                    let effects = if function.effects.is_empty() {
                        String::new()
                    } else {
                        format!(" effects={}", function.effects.join(","))
                    };
                    let targets = if options.include_targets {
                        let metadata = target_metadata_for_function(function);
                        if metadata.labels.is_empty() {
                            String::new()
                        } else {
                            format!(" targets={}", metadata.labels.join(","))
                        }
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        "  F {} {}{}{}{}{} params={} tokens~{}\n",
                        function.name,
                        function.id,
                        safety,
                        asm,
                        effects,
                        targets,
                        function.params.len(),
                        token_cost
                    ));
                }
                Item::Test(test) => {
                    out.push_str(&format!(
                        "  T {} {} xfail={} expectations={} tokens~{}\n",
                        test.name,
                        test.id,
                        test.xfail,
                        test.expectations.len(),
                        token_cost
                    ));
                }
                Item::Capsule(capsule) => {
                    out.push_str(&format!(
                        "  CAP {} {} lang={} version={} risk={} tokens~{}\n",
                        capsule.name,
                        capsule.id,
                        capsule.language,
                        capsule.version,
                        capsule.risk_class(),
                        token_cost
                    ));
                }
                Item::Share(share) => {
                    out.push_str(&format!(
                        "  SHARE {} {} type={} tokens~{}\n",
                        share.name,
                        share.id,
                        share.ty.to_agent(),
                        token_cost
                    ));
                }
                Item::Pipeline(pipeline) => {
                    out.push_str(&format!(
                        "  PIPELINE {} {} stages={} edges={} tokens~{}\n",
                        pipeline.name,
                        pipeline.id,
                        pipeline.stages.len(),
                        pipeline.edges.len(),
                        token_cost
                    ));
                }
            }
        }
    }
    Ok(out)
}

pub fn get_symbol(root: &Path, target: &str, deps: Option<&str>) -> Result<String, String> {
    let mut files = Vec::new();
    collect_sources(root, &mut files)?;
    files.sort();
    let mut structs = Vec::new();
    let mut seen_structs = HashSet::new();
    let mut found = None;
    for file in files {
        let source = fs::read_to_string(&file)
            .map_err(|err| format!("failed to read {}: {err}", file.display()))?;
        let program = parse_file(&file, &source)
            .map_err(|err| format!("failed to parse {}: {err}", file.display()))?;
        for item in program.items {
            match &item {
                Item::Data(section) => {
                    if target == section.id {
                        found = Some(item.clone());
                    }
                }
                Item::Blob(section) => {
                    if target == section.id {
                        found = Some(item.clone());
                    }
                }
                Item::Note(note) => {
                    if target == note.id {
                        found = Some(item.clone());
                    }
                }
                Item::Struct(decl) => {
                    if target == decl.id {
                        found = Some(item.clone());
                    }
                    if seen_structs.insert(decl.id.clone()) {
                        structs.push(item);
                    }
                }
                Item::Enum(decl) => {
                    if target == decl.id {
                        found = Some(item);
                    }
                }
                Item::Trait(decl) => {
                    if target == decl.id {
                        found = Some(item);
                    }
                }
                Item::TraitImpl(decl) => {
                    if target == decl.id {
                        found = Some(item);
                    }
                }
                Item::Function(function) => {
                    if target == function.id {
                        found = Some(item);
                    }
                }
                Item::Test(test) => {
                    if target == test.id {
                        found = Some(item);
                    }
                }
                Item::Capsule(capsule) => {
                    if target == capsule.id {
                        found = Some(item);
                    }
                }
                Item::Share(share) => {
                    if target == share.id {
                        found = Some(item);
                    }
                }
                Item::Pipeline(pipeline) => {
                    if target == pipeline.id {
                        found = Some(item);
                    }
                }
            }
        }
    }
    let Some(item) = found else {
        return Err(format!("symbol not found: {target}"));
    };
    let mut items = Vec::new();
    if deps == Some("types") {
        items.extend(structs);
    }
    items.push(item);
    Ok(format_human(&Program { items }))
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
    let mut grouped: HashMap<PathBuf, PathBuf> = HashMap::new();
    for file in files {
        let key = file.with_extension("");
        match grouped.get(&key) {
            Some(existing) if preferred_over(existing, &file) => {}
            _ => {
                grouped.insert(key, file);
            }
        }
    }
    grouped.into_values().collect()
}

fn preferred_over(existing: &Path, candidate: &Path) -> bool {
    source_rank(existing) <= source_rank(candidate)
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

fn display_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
        .replace('\\', "/")
}
