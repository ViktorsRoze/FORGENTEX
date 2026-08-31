use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::diag::Diagnostic;
use crate::parser::parse_file;
use crate::project_map::ProjectMapOptions;
use crate::stdlib::{is_standard_module, standard_library_source_root};
use crate::targets::target_metadata_for_function;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectManifest {
    pub name: String,
    pub version: String,
    pub source_root: PathBuf,
    pub application: Option<ApplicationManifest>,
    pub dependencies: Vec<ManifestDependency>,
    pub targets: Vec<String>,
    pub profiles: Vec<BuildProfile>,
    pub security: ProjectSecurity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationManifest {
    pub forgentex: String,
    pub entry: String,
    pub runtime: String,
    pub effects: Vec<String>,
    pub max_read_bytes: Option<usize>,
    pub max_write_bytes: Option<usize>,
    pub max_directory_entries: Option<usize>,
    pub artifacts: Vec<ApplicationArtifactManifest>,
    pub pipeline: Option<ApplicationPipelineManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationPipelineManifest {
    pub schedule: String,
    pub inputs: Vec<String>,
    pub output_share: String,
    pub output_path: String,
    pub max_stages: Option<usize>,
    pub max_wall_ms: Option<u128>,
    pub max_cpu_ms: Option<u128>,
    pub max_memory_bytes: Option<u64>,
    pub max_owned_bytes: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct ApplicationPipelineBuilder {
    schedule: Option<String>,
    inputs: Option<Vec<String>>,
    output_share: Option<String>,
    output_path: Option<String>,
    max_stages: Option<usize>,
    max_wall_ms: Option<u128>,
    max_cpu_ms: Option<u128>,
    max_memory_bytes: Option<u64>,
    max_owned_bytes: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationArtifactManifest {
    pub name: String,
    pub root: String,
    pub media_type: String,
    pub max_bytes: usize,
    pub required: bool,
    pub publication: String,
}

#[derive(Clone, Debug, Default)]
struct ApplicationArtifactBuilder {
    root: Option<String>,
    media_type: Option<String>,
    max_bytes: Option<usize>,
    required: Option<bool>,
    publication: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestDependency {
    pub name: String,
    pub path: PathBuf,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildProfile {
    pub name: String,
    pub backend: String,
    pub optimize: String,
    pub debug: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSecurity {
    pub trust: String,
    pub signed_by: Option<String>,
    pub signature_algorithm: Option<String>,
    pub signature: Option<String>,
    pub reproducible: bool,
    pub permissions: Vec<ManifestPermission>,
    pub sandbox: SandboxPolicy,
    pub actions: SecurityActions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestPermission {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub build_scripts: String,
    pub codegen: String,
    pub tests: String,
    pub agent_tools: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SecurityActions {
    pub build_script: Option<String>,
    pub codegen: Option<String>,
    pub test_runner: Option<String>,
    pub agent_tool: Option<String>,
}

impl Default for ProjectSecurity {
    fn default() -> Self {
        Self {
            trust: "trusted".to_string(),
            signed_by: None,
            signature_algorithm: None,
            signature: None,
            reproducible: false,
            permissions: Vec::new(),
            sandbox: SandboxPolicy::default(),
            actions: SecurityActions::default(),
        }
    }
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            build_scripts: "deny".to_string(),
            codegen: "deny".to_string(),
            tests: "sandboxed".to_string(),
            agent_tools: "deny".to_string(),
        }
    }
}

impl SandboxPolicy {
    fn set(&mut self, key: &str, value: &str) {
        match key {
            "build_scripts" | "build.scripts" | "build_script" => {
                self.build_scripts = value.to_string()
            }
            "codegen" => self.codegen = value.to_string(),
            "tests" | "test" => self.tests = value.to_string(),
            "agent_tools" | "agent.tools" | "agent_tool" => self.agent_tools = value.to_string(),
            _ => {}
        }
    }
}

impl SecurityActions {
    fn set(&mut self, key: &str, value: &str) {
        match key {
            "build_script" | "build.scripts" | "build_scripts" => {
                self.build_script = Some(value.to_string())
            }
            "codegen" | "codegen_tool" => self.codegen = Some(value.to_string()),
            "test_runner" | "tests" | "test" => self.test_runner = Some(value.to_string()),
            "agent_tool" | "agent.tools" | "agent_tools" => {
                self.agent_tool = Some(value.to_string())
            }
            _ => {}
        }
    }

    pub fn declared(&self) -> Vec<(&'static str, &str, &str)> {
        let mut actions = Vec::new();
        if let Some(value) = self.build_script.as_deref() {
            actions.push(("build_script", value, "build.scripts"));
        }
        if let Some(value) = self.codegen.as_deref() {
            actions.push(("codegen", value, "codegen"));
        }
        if let Some(value) = self.test_runner.as_deref() {
            actions.push(("test_runner", value, "tests"));
        }
        if let Some(value) = self.agent_tool.as_deref() {
            actions.push(("agent_tool", value, "agent.tools"));
        }
        actions
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub manifest_path: PathBuf,
    pub package_root: PathBuf,
    pub manifest: ProjectManifest,
    pub modules: Vec<ProjectModule>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectModule {
    pub package: String,
    pub name: String,
    pub path: PathBuf,
    pub imports: Vec<String>,
    pub public_symbols: Vec<PublicSymbol>,
    pub program: Program,
    pub source_hash: String,
    pub token_cost: usize,
    pub dependency: bool,
    pub standard: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicSymbol {
    pub kind: String,
    pub name: String,
    pub id: String,
    pub signature: String,
}

pub fn load_project(input: &Path) -> Result<Project, String> {
    let (manifest_path, package_root, manifest) = read_project_manifest(input)?;
    let source_root = absolutize(&package_root, &manifest.source_root);
    let mut files = Vec::new();
    collect_sources(&source_root, &mut files)?;
    files = preferred_source_views(files);
    files.sort();

    let mut modules = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        modules.push(load_module_from_source(
            &source_root,
            path,
            source,
            &manifest.name,
            false,
            false,
        )?);
    }
    let mut visited_dependencies = BTreeSet::new();
    include_local_dependency_modules(
        &manifest.dependencies,
        &package_root,
        &mut visited_dependencies,
        &mut modules,
    )?;
    include_standard_library_modules(&mut modules)?;

    Ok(Project {
        manifest_path,
        package_root,
        manifest,
        modules,
    })
}

fn load_module_from_source(
    source_root: &Path,
    path: PathBuf,
    source: String,
    package: &str,
    dependency: bool,
    standard: bool,
) -> Result<ProjectModule, String> {
    let program = parse_file(&path, &source)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    let name = module_name(source_root, &path, &source);
    let imports = module_imports(&source);
    let public_symbols = public_symbols(&source, &program);
    Ok(ProjectModule {
        package: package.to_string(),
        name,
        path,
        imports,
        public_symbols,
        program,
        source_hash: source_hash(&source),
        token_cost: crate::tokens::estimate(&source),
        dependency,
        standard,
    })
}

fn include_local_dependency_modules(
    dependencies: &[ManifestDependency],
    package_root: &Path,
    visited: &mut BTreeSet<PathBuf>,
    modules: &mut Vec<ProjectModule>,
) -> Result<(), String> {
    for dependency in dependencies {
        let dependency_root = absolutize(package_root, &dependency.path);
        let (manifest_path, dependency_package_root, manifest) =
            read_project_manifest(&dependency_root)?;
        let canonical_key = manifest_path
            .canonicalize()
            .unwrap_or_else(|_| manifest_path.clone());
        if !visited.insert(canonical_key) {
            continue;
        }
        let source_root = absolutize(&dependency_package_root, &manifest.source_root);
        let mut files = Vec::new();
        collect_sources(&source_root, &mut files)?;
        files = preferred_source_views(files);
        files.sort();
        for path in files {
            let source = fs::read_to_string(&path)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
            modules.push(load_module_from_source(
                &source_root,
                path,
                source,
                &manifest.name,
                true,
                false,
            )?);
        }
        include_local_dependency_modules(
            &manifest.dependencies,
            &dependency_package_root,
            visited,
            modules,
        )?;
    }
    Ok(())
}

fn include_standard_library_modules(modules: &mut Vec<ProjectModule>) -> Result<(), String> {
    let source_root = standard_library_source_root();
    let mut present = modules
        .iter()
        .map(|module| module.name.clone())
        .collect::<BTreeSet<_>>();
    let mut queue = modules
        .iter()
        .flat_map(|module| module.imports.iter().cloned())
        .filter(|name| is_standard_module(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    while let Some(name) = queue.pop() {
        if present.contains(&name) {
            continue;
        }
        let relative = name.replace('.', "/") + ".fgx";
        let path = source_root.join(relative);
        let source = fs::read_to_string(&path).map_err(|err| {
            format!(
                "failed to read standard library module '{}': {}: {err}",
                name,
                path.display()
            )
        })?;
        let module =
            load_module_from_source(&source_root, path, source, "forgentex-stdlib", false, true)?;
        for import in &module.imports {
            if is_standard_module(import) && !present.contains(import) {
                queue.push(import.clone());
            }
        }
        present.insert(module.name.clone());
        modules.push(module);
    }
    Ok(())
}

pub fn check_project(input: &Path) -> Result<Vec<Diagnostic>, String> {
    let project = load_project(input)?;
    let mut diagnostics = validate_project_graph(&project);
    if diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "duplicate_symbol")
    {
        diagnostics.extend(check(&combined_program(&project)));
    }
    Ok(diagnostics)
}

pub fn public_api(input: &Path) -> Result<String, String> {
    let project = load_project(input)?;
    let mut out = String::new();
    out.push_str(&format!(
        "API {} {}\n",
        project.manifest.name, project.manifest.version
    ));
    let mut modules = project.modules.iter().collect::<Vec<_>>();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    for module in modules {
        if module.standard || module.dependency {
            continue;
        }
        let mut symbols = module.public_symbols.clone();
        symbols.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.name.cmp(&right.name))
        });
        if symbols.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "M {} {}\n",
            module.name,
            display_path(&project.package_root, &module.path)
        ));
        for symbol in symbols {
            out.push_str(&format!(
                "  {} {} {} {}\n",
                symbol.kind, symbol.name, symbol.id, symbol.signature
            ));
        }
    }
    Ok(out)
}

pub fn generate_project_map(input: &Path) -> Result<String, String> {
    generate_project_map_with_options(input, ProjectMapOptions::default())
}

pub fn generate_project_map_with_options(
    input: &Path,
    options: ProjectMapOptions,
) -> Result<String, String> {
    let project = load_project(input)?;
    let mut out = String::new();
    out.push_str(&format!(
        "P {} {} root={}\n",
        project.manifest.name,
        project.manifest.version,
        project
            .manifest
            .source_root
            .display()
            .to_string()
            .replace('\\', "/")
    ));
    let mut modules = project.modules.iter().collect::<Vec<_>>();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    for module in modules {
        let imports = if module.imports.is_empty() {
            "-".to_string()
        } else {
            module.imports.join(",")
        };
        let source_kind = if module.standard {
            " source=stdlib"
        } else if module.dependency {
            " source=dependency"
        } else {
            ""
        };
        out.push_str(&format!(
            "M {} {} package={} imports={} exports={} hash={} tokens~{}{}\n",
            module.name,
            display_path(&project.package_root, &module.path),
            module.package,
            imports,
            module.public_symbols.len(),
            module.source_hash,
            module.token_cost,
            source_kind
        ));
        for item in &module.program.items {
            match item {
                Item::Struct(decl) => {
                    let visibility = visibility_for(&module.public_symbols, "struct", &decl.name);
                    out.push_str(&format!(
                        "  S {} {}{} fields={}\n",
                        decl.name,
                        decl.id,
                        visibility,
                        decl.fields.len()
                    ));
                }
                Item::Enum(decl) => {
                    out.push_str(&format!(
                        "  E {} {} variants={}\n",
                        decl.name,
                        decl.id,
                        decl.variants.len()
                    ));
                }
                Item::Trait(decl) => {
                    out.push_str(&format!("  TR {} {}\n", decl.name, decl.id));
                }
                Item::TraitImpl(decl) => {
                    out.push_str(&format!(
                        "  IMPL {} for {} {}\n",
                        decl.trait_name,
                        decl.for_type.to_agent(),
                        decl.id
                    ));
                }
                Item::Function(function) => {
                    let visibility = visibility_for(&module.public_symbols, "fn", &function.name);
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
                        "  F {} {}{}{}{}{}{} params={}\n",
                        function.name,
                        function.id,
                        visibility,
                        safety,
                        asm,
                        effects,
                        targets,
                        function.params.len()
                    ));
                }
                Item::Test(test) => {
                    out.push_str(&format!(
                        "  T {} {} xfail={} expectations={}\n",
                        test.name,
                        test.id,
                        test.xfail,
                        test.expectations.len()
                    ));
                }
                Item::Capsule(capsule) => {
                    let effects = if capsule.effects.is_empty() {
                        String::new()
                    } else {
                        format!(" effects={}", capsule.effects.join(","))
                    };
                    out.push_str(&format!(
                        "  CAP {} {} lang={} version={} risk={}{}\n",
                        capsule.name,
                        capsule.id,
                        capsule.language,
                        capsule.version,
                        capsule.risk_class(),
                        effects
                    ));
                }
                Item::Share(share) => {
                    out.push_str(&format!(
                        "  SHARE {} {} type={}\n",
                        share.name,
                        share.id,
                        share.ty.to_agent()
                    ));
                }
                Item::Pipeline(pipeline) => {
                    out.push_str(&format!(
                        "  PIPELINE {} {} stages={} edges={}\n",
                        pipeline.name,
                        pipeline.id,
                        pipeline.stages.len(),
                        pipeline.edges.len()
                    ));
                }
            }
        }
    }
    Ok(out)
}

pub fn read_project_manifest(input: &Path) -> Result<(PathBuf, PathBuf, ProjectManifest), String> {
    let manifest_path = find_manifest(input)?;
    let package_root = manifest_path
        .parent()
        .ok_or_else(|| {
            format!(
                "manifest has no parent directory: {}",
                manifest_path.display()
            )
        })?
        .to_path_buf();
    let manifest_source = fs::read_to_string(&manifest_path)
        .map_err(|err| format!("failed to read {}: {err}", manifest_path.display()))?;
    let manifest = parse_manifest(&manifest_source, &package_root)?;
    Ok((manifest_path, package_root, manifest))
}

fn parse_manifest(source: &str, package_root: &Path) -> Result<ProjectManifest, String> {
    let mut values = BTreeMap::new();
    let mut dependencies = Vec::new();
    let mut targets = Vec::new();
    let mut profiles = BTreeMap::<String, BuildProfile>::new();
    let mut security = ProjectSecurity::default();
    let mut application_forgentex = None;
    let mut application_entry = None;
    let mut application_runtime = "hosted_io".to_string();
    let mut application_effects = Vec::new();
    let mut application_max_read_bytes = None;
    let mut application_max_write_bytes = None;
    let mut application_max_directory_entries = None;
    let mut application_artifacts = BTreeMap::<String, ApplicationArtifactBuilder>::new();
    let mut application_pipeline = ApplicationPipelineBuilder::default();
    let mut saw_application_pipeline = false;
    let mut saw_application = false;
    let mut section = "package".to_string();
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid manifest line: {line}"));
        };
        let key = key.trim();
        let value = value.trim();
        match section.as_str() {
            "package" => {
                values.insert(key.to_string(), unquote(value).to_string());
            }
            "dependencies" => {
                dependencies.push(parse_dependency(key, value)?);
            }
            "targets" | "target" | "target.matrix" => {
                targets.extend(parse_target_value(value));
            }
            "security" | "supply_chain" => match key {
                "trust" => security.trust = unquote(value).to_string(),
                "signed_by" | "signer" => security.signed_by = Some(unquote(value).to_string()),
                "signature_algorithm" | "algorithm" | "sig_alg" => {
                    security.signature_algorithm = Some(unquote(value).to_string())
                }
                "signature" | "package_signature" | "sig" => {
                    security.signature = Some(unquote(value).to_string())
                }
                "reproducible" => {
                    security.reproducible = parse_bool(value).unwrap_or(security.reproducible)
                }
                "build_script" | "build.scripts" | "build_scripts" | "codegen" | "codegen_tool"
                | "test_runner" | "tests" | "test" | "agent_tool" | "agent.tools"
                | "agent_tools" => {
                    security.actions.set(key, unquote(value));
                }
                _ => {}
            },
            "permissions" | "permission" => {
                if let Some(permission) = parse_permission(key, value) {
                    security.permissions.push(permission);
                }
            }
            "application" | "app" => {
                saw_application = true;
                match key {
                    "forgentex" => {
                        let requirement = unquote(value).to_string();
                        parse_compatibility_line(&requirement)?;
                        application_forgentex = Some(requirement);
                    }
                    "entry" => application_entry = Some(unquote(value).to_string()),
                    "runtime" => application_runtime = unquote(value).to_string(),
                    "effects" => application_effects.extend(parse_target_value(value)),
                    _ => return Err(format!("unknown application manifest key '{key}'")),
                }
            }
            "application.limits" | "app.limits" => {
                saw_application = true;
                match key {
                    "max_read_bytes" => {
                        application_max_read_bytes = Some(parse_application_byte_limit(key, value)?)
                    }
                    "max_write_bytes" => {
                        application_max_write_bytes =
                            Some(parse_application_byte_limit(key, value)?)
                    }
                    "max_directory_entries" => {
                        application_max_directory_entries =
                            Some(parse_application_entry_limit(key, value)?)
                    }
                    _ => return Err(format!("unknown application limit '{key}'")),
                }
            }
            "application.pipeline" | "app.pipeline" => {
                saw_application = true;
                saw_application_pipeline = true;
                match key {
                    "schedule" => application_pipeline.schedule = Some(unquote(value).to_string()),
                    "inputs" => application_pipeline.inputs = Some(parse_target_value(value)),
                    "output_share" => {
                        application_pipeline.output_share = Some(unquote(value).to_string())
                    }
                    "output_path" => {
                        application_pipeline.output_path = Some(unquote(value).to_string())
                    }
                    _ => return Err(format!("unknown application pipeline key '{key}'")),
                }
            }
            "application.pipeline.limits" | "app.pipeline.limits" => {
                saw_application = true;
                saw_application_pipeline = true;
                match key {
                    "max_stages" => {
                        application_pipeline.max_stages =
                            Some(parse_positive_limit::<usize>(key, value)?)
                    }
                    "max_wall_ms" => {
                        application_pipeline.max_wall_ms =
                            Some(parse_positive_limit::<u128>(key, value)?)
                    }
                    "max_cpu_ms" => {
                        application_pipeline.max_cpu_ms =
                            Some(parse_positive_limit::<u128>(key, value)?)
                    }
                    "max_memory_bytes" => {
                        application_pipeline.max_memory_bytes =
                            Some(parse_positive_limit::<u64>(key, value)?)
                    }
                    "max_owned_bytes" => {
                        application_pipeline.max_owned_bytes =
                            Some(parse_positive_limit::<usize>(key, value)?)
                    }
                    _ => return Err(format!("unknown application pipeline limit '{key}'")),
                }
            }
            section
                if section.starts_with("application.artifact.")
                    || section.starts_with("app.artifact.") =>
            {
                saw_application = true;
                let name = section
                    .strip_prefix("application.artifact.")
                    .or_else(|| section.strip_prefix("app.artifact."))
                    .unwrap_or_default();
                validate_artifact_name(name)?;
                let artifact = application_artifacts.entry(name.to_string()).or_default();
                match key {
                    "root" => set_artifact_field(
                        &mut artifact.root,
                        unquote(value).to_string(),
                        name,
                        key,
                    )?,
                    "media_type" => set_artifact_field(
                        &mut artifact.media_type,
                        unquote(value).to_string(),
                        name,
                        key,
                    )?,
                    "max_bytes" => set_artifact_field(
                        &mut artifact.max_bytes,
                        parse_application_byte_limit(key, value)?,
                        name,
                        key,
                    )?,
                    "required" => set_artifact_field(
                        &mut artifact.required,
                        parse_bool(value).ok_or_else(|| {
                            format!(
                                "application artifact '{name}' field required must be true or false"
                            )
                        })?,
                        name,
                        key,
                    )?,
                    "publication" => set_artifact_field(
                        &mut artifact.publication,
                        unquote(value).to_string(),
                        name,
                        key,
                    )?,
                    _ => {
                        return Err(format!(
                            "unknown application artifact field '{key}' for '{name}'"
                        ))
                    }
                }
            }
            "sandbox" | "security.sandbox" => {
                security.sandbox.set(key, unquote(value));
            }
            "actions" | "security.actions" => {
                security.actions.set(key, unquote(value));
            }
            section if section.starts_with("profile.") || section.starts_with("profiles.") => {
                let profile_name = section
                    .strip_prefix("profile.")
                    .or_else(|| section.strip_prefix("profiles."))
                    .unwrap_or("dev")
                    .to_string();
                let profile =
                    profiles
                        .entry(profile_name.clone())
                        .or_insert_with(|| BuildProfile {
                            name: profile_name,
                            backend: "c".to_string(),
                            optimize: "none".to_string(),
                            debug: true,
                        });
                match key {
                    "backend" => profile.backend = unquote(value).to_string(),
                    "optimize" | "opt" => profile.optimize = unquote(value).to_string(),
                    "debug" => profile.debug = parse_bool(value).unwrap_or(profile.debug),
                    _ => {}
                }
            }
            _ => {
                values.insert(key.to_string(), unquote(value).to_string());
            }
        }
    }
    let name = values
        .remove("name")
        .unwrap_or_else(|| "forgentex-package".to_string());
    let version = values
        .remove("version")
        .unwrap_or_else(|| "0.1.0".to_string());
    let root = values.remove("root").unwrap_or_else(|| ".".to_string());
    let source_root = PathBuf::from(root);
    let application = if saw_application {
        let forgentex = application_forgentex.ok_or_else(|| {
            "application manifest requires forgentex = \"MAJOR.MINOR\"".to_string()
        })?;
        let entry = application_entry
            .filter(|entry| !entry.trim().is_empty())
            .ok_or_else(|| "application manifest requires a non-empty entry".to_string())?;
        if application_runtime != "hosted_io" && application_runtime != "pipeline" {
            return Err(format!(
                "application runtime '{}' is unsupported; expected hosted_io or pipeline",
                application_runtime
            ));
        }
        application_effects.sort();
        application_effects.dedup();
        for effect in &application_effects {
            if application_runtime == "hosted_io" {
                // `alloc` is a pure in-process memory effect that grants no
                // ambient authority, so a hosted entry may construct an owned
                // buffer instead of only forwarding bytes it already read.
                // Native-execution effects such as `cpu` stay rejected: this
                // runtime cannot govern a native process tree.
                if effect != "fs.read" && effect != "fs.write" && effect != "alloc" {
                    return Err(format!(
                        "application effect '{effect}' is unsupported by the hosted_io runtime"
                    ));
                }
            } else if !valid_effect_name(effect) {
                return Err(format!(
                    "application pipeline effect '{effect}' must be a dotted lowercase ASCII capability name"
                ));
            }
        }
        if application_effects.iter().any(|effect| effect == "fs.read")
            && application_max_read_bytes.is_none()
        {
            return Err(
                "application effect fs.read requires application.limits.max_read_bytes".to_string(),
            );
        }
        if application_effects
            .iter()
            .any(|effect| effect == "fs.write")
            && application_max_write_bytes.is_none()
        {
            return Err(
                "application effect fs.write requires application.limits.max_write_bytes"
                    .to_string(),
            );
        }
        if application_max_directory_entries.is_some()
            && !application_effects.iter().any(|effect| effect == "fs.read")
        {
            return Err(
                "application.limits.max_directory_entries requires the fs.read effect".to_string(),
            );
        }
        if !application_artifacts.is_empty()
            && !application_effects
                .iter()
                .any(|effect| effect == "fs.write")
        {
            return Err(
                "application artifact declarations require the fs.write effect".to_string(),
            );
        }
        let max_write_bytes = application_max_write_bytes;
        let artifacts = application_artifacts
            .into_iter()
            .map(|(name, artifact)| {
                let root = artifact.root.ok_or_else(|| {
                    format!("application artifact '{name}' requires a root field")
                })?;
                let root = crate::interp::normalize_portable_path(&root).map_err(|error| {
                    format!("application artifact '{name}' has invalid root: {error}")
                })?;
                let media_type = artifact.media_type.ok_or_else(|| {
                    format!("application artifact '{name}' requires a media_type field")
                })?;
                validate_artifact_media_type(&name, &media_type)?;
                let max_bytes = artifact.max_bytes.ok_or_else(|| {
                    format!("application artifact '{name}' requires a max_bytes field")
                })?;
                if let Some(run_limit) = max_write_bytes {
                    if max_bytes > run_limit {
                        return Err(format!(
                            "application artifact '{name}' max_bytes {max_bytes} exceeds application.limits.max_write_bytes {run_limit}"
                        ));
                    }
                }
                let publication = artifact
                    .publication
                    .unwrap_or_else(|| "create_new".to_string());
                if publication != "create_new" {
                    return Err(format!(
                        "application artifact '{name}' publication '{publication}' is unsupported; expected create_new"
                    ));
                }
                Ok(ApplicationArtifactManifest {
                    name,
                    root,
                    media_type,
                    max_bytes,
                    required: artifact.required.unwrap_or(true),
                    publication,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let pipeline = if application_runtime == "pipeline" {
            if !saw_application_pipeline {
                return Err(
                    "application runtime pipeline requires [application.pipeline]".to_string(),
                );
            }
            if artifacts.len() != 1 {
                return Err(
                    "application runtime pipeline requires exactly one [application.artifact.<name>] declaration"
                        .to_string(),
                );
            }
            let schedule = application_pipeline
                .schedule
                .unwrap_or_else(|| "source".to_string());
            if schedule != "source" {
                return Err(format!(
                    "application pipeline schedule '{schedule}' is unsupported; expected source"
                ));
            }
            let inputs = application_pipeline.inputs.unwrap_or_default();
            let mut distinct_inputs = BTreeSet::new();
            for input in &inputs {
                if !valid_manifest_identifier(input) {
                    return Err(format!(
                        "application pipeline input '{input}' is not a valid shared-value name"
                    ));
                }
                if !distinct_inputs.insert(input.clone()) {
                    return Err(format!(
                        "application pipeline input '{input}' is declared more than once"
                    ));
                }
            }
            let output_share = application_pipeline
                .output_share
                .filter(|value| valid_manifest_identifier(value))
                .ok_or_else(|| "application pipeline requires a valid output_share".to_string())?;
            let output_path = application_pipeline
                .output_path
                .ok_or_else(|| "application pipeline requires an output_path".to_string())?;
            let output_path = crate::interp::normalize_portable_path(&output_path)
                .map_err(|error| format!("application pipeline output_path is invalid: {error}"))?;
            let max_stages = application_pipeline.max_stages.ok_or_else(|| {
                "application pipeline requires application.pipeline.limits.max_stages".to_string()
            })?;
            let max_wall_ms = application_pipeline.max_wall_ms.ok_or_else(|| {
                "application pipeline requires application.pipeline.limits.max_wall_ms".to_string()
            })?;
            let max_cpu_ms = application_pipeline.max_cpu_ms.ok_or_else(|| {
                "application pipeline requires application.pipeline.limits.max_cpu_ms".to_string()
            })?;
            let max_memory_bytes = application_pipeline.max_memory_bytes.ok_or_else(|| {
                "application pipeline requires application.pipeline.limits.max_memory_bytes"
                    .to_string()
            })?;
            let max_owned_bytes = application_pipeline.max_owned_bytes.ok_or_else(|| {
                "application pipeline requires application.pipeline.limits.max_owned_bytes"
                    .to_string()
            })?;
            Some(ApplicationPipelineManifest {
                schedule,
                inputs,
                output_share,
                output_path,
                max_stages: Some(max_stages),
                max_wall_ms: Some(max_wall_ms),
                max_cpu_ms: Some(max_cpu_ms),
                max_memory_bytes: Some(max_memory_bytes),
                max_owned_bytes: Some(max_owned_bytes),
            })
        } else {
            if saw_application_pipeline {
                return Err(
                    "[application.pipeline] requires application runtime = \"pipeline\""
                        .to_string(),
                );
            }
            None
        };
        Some(ApplicationManifest {
            forgentex,
            entry,
            runtime: application_runtime,
            effects: application_effects,
            max_read_bytes: application_max_read_bytes,
            max_write_bytes: application_max_write_bytes,
            max_directory_entries: application_max_directory_entries,
            artifacts,
            pipeline,
        })
    } else {
        None
    };
    if absolutize(package_root, &source_root).exists() {
        Ok(ProjectManifest {
            name,
            version,
            source_root,
            application,
            dependencies,
            targets,
            profiles: profiles.into_values().collect(),
            security,
        })
    } else {
        Err(format!(
            "manifest source root does not exist: {}",
            source_root.display()
        ))
    }
}

fn parse_positive_limit<T>(key: &str, value: &str) -> Result<T, String>
where
    T: std::str::FromStr + PartialEq + Default,
{
    let parsed = unquote(value)
        .parse::<T>()
        .map_err(|_| format!("application pipeline limit '{key}' must be a positive integer"))?;
    if parsed == T::default() {
        return Err(format!(
            "application pipeline limit '{key}' must be a positive integer"
        ));
    }
    Ok(parsed)
}

fn valid_effect_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.split('.').all(valid_manifest_identifier)
}

fn valid_manifest_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn parse_compatibility_line(value: &str) -> Result<(u64, u64), String> {
    let Some((major, minor)) = value.split_once('.') else {
        return Err(format!(
            "application Forgentex compatibility '{value}' is invalid; expected MAJOR.MINOR"
        ));
    };
    if major.is_empty()
        || minor.is_empty()
        || minor.contains('.')
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
        || (major.len() > 1 && major.starts_with('0'))
        || (minor.len() > 1 && minor.starts_with('0'))
    {
        return Err(format!(
            "application Forgentex compatibility '{value}' is invalid; expected canonical MAJOR.MINOR"
        ));
    }
    let major = major.parse::<u64>().map_err(|_| {
        format!("application Forgentex compatibility '{value}' has an out-of-range major version")
    })?;
    let minor = minor.parse::<u64>().map_err(|_| {
        format!("application Forgentex compatibility '{value}' has an out-of-range minor version")
    })?;
    Ok((major, minor))
}

fn parse_application_byte_limit(key: &str, value: &str) -> Result<usize, String> {
    const MAX_APPLICATION_IO_BYTES: usize = 1_073_741_824;
    let parsed = unquote(value)
        .parse::<usize>()
        .map_err(|_| format!("application limit '{key}' must be a positive integer"))?;
    if !(1..=MAX_APPLICATION_IO_BYTES).contains(&parsed) {
        return Err(format!(
            "application limit '{key}' must be between 1 and {MAX_APPLICATION_IO_BYTES}"
        ));
    }
    Ok(parsed)
}

fn parse_application_entry_limit(key: &str, value: &str) -> Result<usize, String> {
    const MAX_APPLICATION_DIRECTORY_ENTRIES: usize = 1_000_000;
    let parsed = unquote(value)
        .parse::<usize>()
        .map_err(|_| format!("application limit '{key}' must be a positive integer"))?;
    if !(1..=MAX_APPLICATION_DIRECTORY_ENTRIES).contains(&parsed) {
        return Err(format!(
            "application limit '{key}' must be between 1 and {MAX_APPLICATION_DIRECTORY_ENTRIES}"
        ));
    }
    Ok(parsed)
}

fn set_artifact_field<T>(
    slot: &mut Option<T>,
    value: T,
    artifact: &str,
    field: &str,
) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(format!(
            "application artifact '{artifact}' declares '{field}' more than once"
        ));
    }
    Ok(())
}

fn validate_artifact_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "application artifact name '{name}' must start with an ASCII letter and contain at most 64 ASCII letters, digits, '_' or '-'"
        ))
    }
}

fn validate_artifact_media_type(name: &str, media_type: &str) -> Result<(), String> {
    let Some((type_name, subtype)) = media_type.split_once('/') else {
        return Err(format!(
            "application artifact '{name}' media_type '{media_type}' must contain type/subtype"
        ));
    };
    let token = |value: &str| {
        !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                    )
            })
    };
    if media_type.len() <= 127
        && media_type.bytes().all(|byte| !byte.is_ascii_uppercase())
        && token(type_name)
        && token(subtype)
    {
        Ok(())
    } else {
        Err(format!(
            "application artifact '{name}' media_type '{media_type}' must be a lowercase ASCII type/subtype token of at most 127 bytes"
        ))
    }
}

fn validate_project_graph(project: &Project) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut modules = BTreeSet::new();
    for module in &project.modules {
        if !modules.insert(module.name.clone()) {
            diagnostics.push(
                Diagnostic::new(
                    "duplicate_module",
                    format!("@module.{}", module.name),
                    format!("module '{}' is declared more than once", module.name),
                )
                .suggest("rename_module_or_remove_duplicate"),
            );
        }
    }

    for module in &project.modules {
        for import in &module.imports {
            if !modules.contains(import) {
                diagnostics.push(
                    Diagnostic::new(
                        "unresolved_import",
                        format!("@module.{}.import.{}", module.name, import),
                        format!(
                            "module '{}' imports missing module '{}'",
                            module.name, import
                        ),
                    )
                    .suggest("add_module_file")
                    .suggest("fix_import_name"),
                );
            }
        }
    }

    diagnostics.extend(import_cycle_diagnostics(project));
    diagnostics.extend(duplicate_symbol_diagnostics(project));
    diagnostics
}

fn import_cycle_diagnostics(project: &Project) -> Vec<Diagnostic> {
    let graph = project
        .modules
        .iter()
        .map(|module| (module.name.as_str(), module.imports.as_slice()))
        .collect::<HashMap<_, _>>();
    let mut diagnostics = Vec::new();
    let mut reported = BTreeSet::new();
    for module in &project.modules {
        let mut path = Vec::new();
        let mut visiting = HashSet::new();
        find_cycles(
            module.name.as_str(),
            &graph,
            &mut visiting,
            &mut path,
            &mut reported,
            &mut diagnostics,
        );
    }
    diagnostics
}

fn find_cycles<'a>(
    module: &'a str,
    graph: &HashMap<&'a str, &'a [String]>,
    visiting: &mut HashSet<&'a str>,
    path: &mut Vec<&'a str>,
    reported: &mut BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !visiting.insert(module) {
        if let Some(start) = path.iter().position(|entry| *entry == module) {
            let mut cycle = path[start..].to_vec();
            cycle.push(module);
            let key = cycle.join(" -> ");
            if reported.insert(key.clone()) {
                diagnostics.push(
                    Diagnostic::new(
                        "import_cycle",
                        format!("@module.{module}"),
                        format!("import cycle detected: {key}"),
                    )
                    .suggest("remove_or_invert_import"),
                );
            }
        }
        return;
    }
    path.push(module);
    if let Some(imports) = graph.get(module) {
        for import in *imports {
            if graph.contains_key(import.as_str()) {
                find_cycles(import, graph, visiting, path, reported, diagnostics);
            }
        }
    }
    path.pop();
    visiting.remove(module);
}

fn duplicate_symbol_diagnostics(project: &Project) -> Vec<Diagnostic> {
    let mut owners: HashMap<String, String> = HashMap::new();
    let mut diagnostics = Vec::new();
    for module in &project.modules {
        for item in &module.program.items {
            let (kind, name, node) = match item {
                Item::Struct(decl) => ("struct", decl.name.as_str(), decl.id.as_str()),
                Item::Enum(decl) => ("enum", decl.name.as_str(), decl.id.as_str()),
                Item::Trait(decl) => ("trait", decl.name.as_str(), decl.id.as_str()),
                Item::TraitImpl(_) => continue,
                Item::Function(function) => {
                    ("function", function.name.as_str(), function.id.as_str())
                }
                Item::Capsule(capsule) => ("capsule", capsule.name.as_str(), capsule.id.as_str()),
                Item::Share(share) => ("share", share.name.as_str(), share.id.as_str()),
                Item::Pipeline(pipeline) => {
                    ("pipeline", pipeline.name.as_str(), pipeline.id.as_str())
                }
                Item::Test(_) => continue,
            };
            let key = format!("{kind}:{name}");
            if let Some(previous) = owners.insert(key, module.name.clone()) {
                diagnostics.push(
                    Diagnostic::new(
                        "duplicate_symbol",
                        node,
                        format!(
                            "{kind} '{name}' is declared in both '{previous}' and '{}'",
                            module.name
                        ),
                    )
                    .suggest("rename_symbol_or_split_namespace"),
                );
            }
        }
    }
    diagnostics
}

fn combined_program(project: &Project) -> Program {
    let mut items = Vec::new();
    for module in &project.modules {
        items.extend(module.program.items.clone());
    }
    Program { items }
}

fn module_name(root: &Path, path: &Path, source: &str) -> String {
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

fn public_symbols(source: &str, program: &Program) -> Vec<PublicSymbol> {
    let exported = exported_names(source);
    let mut symbols = Vec::new();
    for item in &program.items {
        match item {
            Item::Enum(decl) if exported.contains(&format!("enum:{}", decl.name)) => {
                let variants = decl
                    .variants
                    .iter()
                    .map(crate::ast::variant_repr)
                    .collect::<Vec<_>>()
                    .join(",");
                symbols.push(PublicSymbol {
                    kind: "enum".to_string(),
                    name: decl.name.clone(),
                    id: decl.id.clone(),
                    signature: format!("variants=[{variants}]"),
                });
            }
            Item::Struct(decl) if exported.contains(&format!("struct:{}", decl.name)) => {
                let fields = decl
                    .fields
                    .iter()
                    .map(|field| format!("{}:{}", field.name, field.ty.to_agent()))
                    .collect::<Vec<_>>()
                    .join(",");
                symbols.push(PublicSymbol {
                    kind: "struct".to_string(),
                    name: decl.name.clone(),
                    id: decl.id.clone(),
                    signature: format!("fields=[{fields}]"),
                });
            }
            Item::Function(function) if exported.contains(&format!("fn:{}", function.name)) => {
                let params = function
                    .params
                    .iter()
                    .map(|param| format!("{}:{}", param.name, param.ty.to_agent()))
                    .collect::<Vec<_>>()
                    .join(",");
                let effects = if function.effects.is_empty() {
                    String::new()
                } else {
                    format!(" effects=[{}]", function.effects.join(","))
                };
                let safety = if function.unsafe_fn { "unsafe " } else { "" };
                symbols.push(PublicSymbol {
                    kind: "fn".to_string(),
                    name: function.name.clone(),
                    id: function.id.clone(),
                    signature: format!(
                        "{}({}) -> {}{}",
                        safety,
                        params,
                        function.ret.to_agent(),
                        effects
                    ),
                });
            }
            Item::Test(_) => {}
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
        } else if let Some(rest) = public.strip_prefix("enum ") {
            if let Some(name) = rest.split([':', ' ']).find(|part| !part.is_empty()) {
                exported.insert(format!("enum:{name}"));
            }
        } else if let Some(rest) = public.strip_prefix("fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                exported.insert(format!("fn:{}", name.trim()));
            }
        } else if let Some(rest) = public.strip_prefix("unsafe fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                exported.insert(format!("fn:{}", name.trim()));
            }
        } else if let Some(rest) = public.strip_prefix("S ") {
            if let Some((name, _)) = rest.split_once('{') {
                exported.insert(format!("struct:{}", name.trim()));
            }
        } else if let Some(rest) = public.strip_prefix("EN ") {
            if let Some((name, _)) = rest.split_once('{') {
                exported.insert(format!("enum:{}", name.trim()));
            }
        } else if let Some(rest) = public
            .strip_prefix("F ")
            .or_else(|| public.strip_prefix("UF "))
            .or_else(|| public.strip_prefix("unsafe F "))
        {
            if let Some((name, _)) = rest.split_once('(') {
                exported.insert(format!("fn:{}", name.trim()));
            }
        }
    }
    exported
}

fn visibility_for(symbols: &[PublicSymbol], kind: &str, name: &str) -> &'static str {
    if symbols
        .iter()
        .any(|symbol| symbol.kind == kind && symbol.name == name)
    {
        " public"
    } else {
        ""
    }
}

pub fn find_manifest(input: &Path) -> Result<PathBuf, String> {
    if input.is_file() {
        if input.file_name().and_then(|name| name.to_str()) == Some("Forgentex.toml") {
            return Ok(input.to_path_buf());
        }
        return Err(format!(
            "project path is not a manifest: {}",
            input.display()
        ));
    }
    let start = if input.exists() {
        input.to_path_buf()
    } else {
        return Err(format!("project path does not exist: {}", input.display()));
    };
    for dir in start.ancestors() {
        let candidate = dir.join("Forgentex.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Forgentex.toml not found from project path {}",
        input.display()
    ))
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

fn display_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn absolutize(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn parse_dependency(name: &str, value: &str) -> Result<ManifestDependency, String> {
    if value.starts_with('{') {
        let body = value.trim_matches(['{', '}']);
        let mut path = None;
        let mut version = None;
        for part in body.split(',') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key.trim() {
                "path" => path = Some(PathBuf::from(unquote(value.trim()))),
                "version" => version = Some(unquote(value.trim()).to_string()),
                _ => {}
            }
        }
        let Some(path) = path else {
            return Err(format!("dependency '{name}' is missing path"));
        };
        Ok(ManifestDependency {
            name: name.to_string(),
            path,
            version,
        })
    } else {
        Ok(ManifestDependency {
            name: name.to_string(),
            path: PathBuf::from(unquote(value)),
            version: None,
        })
    }
}

fn parse_target_value(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed
            .trim_matches(['[', ']'])
            .split(',')
            .map(|part| unquote(part.trim()).to_string())
            .filter(|part| !part.is_empty())
            .collect()
    } else {
        vec![unquote(trimmed).to_string()]
    }
}

fn parse_permission(name: &str, value: &str) -> Option<ManifestPermission> {
    if matches!(parse_bool(value), Some(false)) {
        return None;
    }
    Some(ManifestPermission {
        name: name.to_string(),
        values: parse_manifest_values(value),
    })
}

fn parse_manifest_values(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if matches!(parse_bool(trimmed), Some(true)) {
        return vec!["allowed".to_string()];
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return trimmed
            .trim_matches(['[', ']'])
            .split(',')
            .map(|part| unquote(part.trim()).to_string())
            .filter(|part| !part.is_empty())
            .collect();
    }
    let value = unquote(trimmed).trim();
    if value.is_empty() {
        Vec::new()
    } else {
        vec![value.to_string()]
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match unquote(value).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn source_hash(source: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
