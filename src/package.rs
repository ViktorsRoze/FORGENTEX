use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::build_project_with_backend;
use crate::diag::Diagnostic;
use crate::project::{
    check_project, load_project, read_project_manifest, BuildProfile, Project, ProjectManifest,
};
use crate::security::{enforce_package_security, package_signature_status, verify_project};
use crate::simd::check_simd_fallbacks;
use crate::targets::check_program_target;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedPackage {
    pub manifest: PathBuf,
    pub source: PathBuf,
    /// Additional scaffolded paths, in creation order. Empty for the library
    /// template; the application template adds its declared artifact root.
    pub extra: Vec<PathBuf>,
}

/// Which starting package `fgx new` writes. The library template stays the
/// default so existing scaffolds keep their exact shape; the application
/// template adds the `[application]` contract that `fgx app-run` requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PackageTemplate {
    #[default]
    Lib,
    App,
}

impl PackageTemplate {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "lib" => Ok(PackageTemplate::Lib),
            "app" => Ok(PackageTemplate::App),
            other => Err(format!(
                "unknown template '{other}': expected 'lib' or 'app'"
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageNode {
    pub name: String,
    pub version: String,
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub dependencies: Vec<String>,
    pub targets: Vec<String>,
    pub profiles: Vec<BuildProfile>,
    pub permissions: Vec<String>,
    pub signature_status: String,
    pub checksum: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageLock {
    pub schema: String,
    pub packages: Vec<PackageNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageTestReport {
    pub schema: String,
    pub packages: Vec<PackageTestResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageTestResult {
    pub name: String,
    pub version: String,
    pub targets: Vec<TargetTestResult>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetTestResult {
    pub triple: String,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageBuildReport {
    pub schema: String,
    pub package: String,
    pub backend: String,
    pub profile: String,
    pub targets: Vec<PackageBuildTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageBuildTarget {
    pub triple: String,
    pub status: String,
    pub files: Vec<PathBuf>,
    pub metadata_files: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishDryRunReport {
    pub schema: String,
    pub package: String,
    pub version: String,
    pub ready: bool,
    pub files: Vec<String>,
    pub checks: Vec<String>,
}

const DEFAULT_TARGET_MATRIX: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "riscv64gc-unknown-linux-gnu",
];

pub fn create_package(path: &Path, name: Option<&str>) -> Result<CreatedPackage, String> {
    create_package_with_template(path, name, PackageTemplate::Lib)
}

pub fn create_package_with_template(
    path: &Path,
    name: Option<&str>,
    template: PackageTemplate,
) -> Result<CreatedPackage, String> {
    let package_name = name
        .map(str::to_string)
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "forgentex-package".to_string());
    if path.exists()
        && path
            .read_dir()
            .map_err(|err| err.to_string())?
            .next()
            .is_some()
    {
        return Err(format!(
            "package directory already exists and is not empty: {}",
            path.display()
        ));
    }
    let src_dir = path.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|err| format!("failed to create {}: {err}", src_dir.display()))?;

    let manifest = path.join("Forgentex.toml");
    let source = src_dir.join("main.fgx");
    let module_name = package_module_name(&package_name);
    let (manifest_body, source_body) = match template {
        PackageTemplate::Lib => (
            library_manifest(&package_name),
            format!("module {module_name}\n\nexport fn hello() -> i32:\n    return 42\n"),
        ),
        PackageTemplate::App => (
            application_manifest(&package_name),
            APPLICATION_SOURCE.to_string(),
        ),
    };
    fs::write(&manifest, manifest_body)
        .map_err(|err| format!("failed to write {}: {err}", manifest.display()))?;
    fs::write(&source, source_body)
        .map_err(|err| format!("failed to write {}: {err}", source.display()))?;

    let mut extra = Vec::new();
    if template == PackageTemplate::App {
        // The artifact root must exist before the first run: publication is
        // host-controlled and create-new, so it never creates the directory.
        let output_dir = path.join(APPLICATION_ARTIFACT_ROOT);
        fs::create_dir_all(&output_dir)
            .map_err(|err| format!("failed to create {}: {err}", output_dir.display()))?;
        let keep = output_dir.join(".gitkeep");
        fs::write(&keep, "").map_err(|err| format!("failed to write {}: {err}", keep.display()))?;
        extra.push(output_dir);
        extra.push(keep);
    }

    Ok(CreatedPackage {
        manifest,
        source,
        extra,
    })
}

const APPLICATION_ARTIFACT_ROOT: &str = "output";

/// Entry, artifact name, and artifact path are bound together across the
/// manifest and the source; changing one requires changing the others.
const APPLICATION_SOURCE: &str = "import core.io\n\nfn greet(message: str) -> usize effects [fs.write]:\n    let destination: Path = core_path_parse(\"output/greeting.txt\")\n    return core_io_publish_text_artifact(\"greeting\", destination, message)\n";

fn library_manifest(package_name: &str) -> String {
    format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[targets]\nmatrix = [\"x86_64-unknown-linux-gnu\", \"aarch64-unknown-linux-gnu\", \"riscv64gc-unknown-linux-gnu\"]\n\n[profile.dev]\nbackend = \"c\"\noptimize = \"none\"\ndebug = true\n\n[security]\ntrust = \"trusted\"\nreproducible = true\n\n[permissions]\nfs.read = [\"src\"]\nfs.write = [\"build\"]\nio = [\"stdout\"]\n\n[sandbox]\nbuild_scripts = \"deny\"\ncodegen = \"deny\"\ntests = \"sandboxed\"\nagent_tools = \"deny\"\n"
    )
}

fn application_manifest(package_name: &str) -> String {
    format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"greet\"\nruntime = \"hosted_io\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 65536\n\n[application.artifact.greeting]\nroot = \"{APPLICATION_ARTIFACT_ROOT}\"\nmedia_type = \"text/plain\"\nmax_bytes = 65536\nrequired = true\npublication = \"create_new\"\n\n[security]\ntrust = \"untrusted\"\nsigned_by = \"agent-public-demo\"\nsignature_algorithm = \"Ed25519\"\nsignature = \"mock-sig-{package_name}\"\nreproducible = true\n\n[permissions]\nfs.write = [\"{APPLICATION_ARTIFACT_ROOT}\"]\n\n[sandbox]\nbuild_scripts = \"deny\"\ncodegen = \"deny\"\ntests = \"sandboxed\"\nagent_tools = \"deny\"\n"
    )
}

pub fn generate_lock(input: &Path) -> Result<PackageLock, String> {
    let mut visited = BTreeSet::new();
    let mut packages = Vec::new();
    collect_package_node(input, &mut visited, &mut packages)?;
    packages.sort_by(|left, right| left.name.cmp(&right.name).then(left.root.cmp(&right.root)));
    Ok(PackageLock {
        schema: "forgentex.lock.v1".to_string(),
        packages,
    })
}

pub fn run_package_tests(
    input: &Path,
    target: Option<&str>,
    all_targets: bool,
) -> Result<PackageTestReport, String> {
    enforce_package_security(input)?;
    let lock = generate_lock(input)?;
    let mut packages = Vec::new();
    for package in &lock.packages {
        let graph_diagnostics = check_project(&package.root)?;
        let project = load_project(&package.root)?;
        let program = combined_program(&project);
        let targets = selected_targets(&package.targets, target, all_targets);
        let mut target_results = Vec::new();
        for triple in targets {
            let mut diagnostics = graph_diagnostics.clone();
            diagnostics.extend(check_program_target(&program, &triple));
            diagnostics.extend(check_simd_fallbacks(&program, &triple));
            let diagnostic_codes = diagnostics.iter().map(diagnostic_code).collect::<Vec<_>>();
            target_results.push(TargetTestResult {
                triple,
                status: if diagnostic_codes.is_empty() {
                    "pass".to_string()
                } else {
                    "fail".to_string()
                },
                diagnostics: diagnostic_codes,
            });
        }
        packages.push(PackageTestResult {
            name: package.name.clone(),
            version: package.version.clone(),
            targets: target_results,
            diagnostics: graph_diagnostics.iter().map(diagnostic_code).collect(),
        });
    }
    Ok(PackageTestReport {
        schema: "forgentex.package_test.v1".to_string(),
        packages,
    })
}

pub fn build_package_matrix(
    input: &Path,
    backend: &str,
    out_dir: &Path,
    profile: &str,
    target: Option<&str>,
    all_targets: bool,
) -> Result<PackageBuildReport, String> {
    enforce_package_security(input)?;
    let (_, _, manifest) = read_project_manifest(input)?;
    let test_report = run_package_tests(input, target, all_targets)?;
    if !test_report.is_success() {
        return Err(format!(
            "package tests failed before build:\n{}",
            test_report.to_text()
        ));
    }
    let targets = selected_targets(&manifest.targets, target, all_targets);
    let mut built_targets = Vec::new();
    for triple in targets {
        let target_out = out_dir.join(profile).join(sanitize_path_segment(&triple));
        let artifacts = build_project_with_backend(input, backend, &target_out)?;
        built_targets.push(PackageBuildTarget {
            triple,
            status: "built".to_string(),
            files: artifacts.files,
            metadata_files: artifacts.metadata_files,
        });
    }
    Ok(PackageBuildReport {
        schema: "forgentex.package_build.v1".to_string(),
        package: manifest.name,
        backend: backend.to_string(),
        profile: profile.to_string(),
        targets: built_targets,
    })
}

pub fn publish_dry_run(input: &Path) -> Result<PublishDryRunReport, String> {
    let (_, root, manifest) = read_project_manifest(input)?;
    let test_report = run_package_tests(input, None, true)?;
    let lock = generate_lock(input)?;
    let verify = verify_project(input)?;
    let ready = test_report.is_success() && verify.is_success();
    let mut files = vec!["Forgentex.toml".to_string()];
    let source_root = root.join(&manifest.source_root);
    collect_publish_files(&root, &source_root, &mut files)?;
    files.sort();
    Ok(PublishDryRunReport {
        schema: "forgentex.publish_dry_run.v1".to_string(),
        package: manifest.name,
        version: manifest.version,
        ready,
        files,
        checks: vec![
            format!("package_tests={}", if ready { "pass" } else { "fail" }),
            format!("lock_packages={}", lock.packages.len()),
            format!("package_verify={}", verify.status),
            "registry_upload=deferred".to_string(),
        ],
    })
}

impl PackageLock {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("schema = \"forgentex.lock.v1\"\n\n");
        for package in &self.packages {
            out.push_str("[[package]]\n");
            out.push_str(&format!("name = \"{}\"\n", package.name));
            out.push_str(&format!("version = \"{}\"\n", package.version));
            out.push_str(&format!("path = \"{}\"\n", display_path(&package.root)));
            out.push_str(&format!("checksum = \"{}\"\n", package.checksum));
            out.push_str(&format!(
                "signature_status = \"{}\"\n",
                package.signature_status
            ));
            out.push_str(&format!(
                "dependencies = [{}]\n",
                package
                    .dependencies
                    .iter()
                    .map(|dependency| format!("\"{dependency}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            out.push_str(&format!(
                "targets = [{}]\n\n",
                package
                    .targets
                    .iter()
                    .map(|target| format!("\"{target}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str("  \"packages\": [");
        for (idx, package) in self.packages.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"name\": {}, \"version\": {}, \"path\": {}, \"checksum\": {}, ",
                quoted(&package.name),
                quoted(&package.version),
                quoted(&display_path(&package.root)),
                quoted(&package.checksum)
            ));
            push_string_array_inline(&mut out, "dependencies", &package.dependencies);
            out.push_str(", ");
            push_string_array_inline(&mut out, "targets", &package.targets);
            out.push_str(", ");
            push_string_array_inline(&mut out, "permissions", &package.permissions);
            out.push_str(&format!(
                ", \"signature_status\": {}",
                quoted(&package.signature_status)
            ));
            out.push('}');
        }
        if !self.packages.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

impl PackageTestReport {
    pub fn is_success(&self) -> bool {
        self.packages.iter().all(|package| {
            package.diagnostics.is_empty()
                && package.targets.iter().all(|target| target.status == "pass")
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex package test report\n");
        out.push_str(&format!(
            "schema={} status={}\n",
            self.schema,
            self.status()
        ));
        for package in &self.packages {
            out.push_str(&format!("  - {} {}\n", package.name, package.version));
            for target in &package.targets {
                out.push_str(&format!(
                    "      {} status={} diagnostics=[{}]\n",
                    target.triple,
                    target.status,
                    target.diagnostics.join(",")
                ));
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"status\": {},\n", quoted(&self.status())));
        out.push_str("  \"packages\": [");
        for (idx, package) in self.packages.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"name\": {}, \"version\": {}, ",
                quoted(&package.name),
                quoted(&package.version)
            ));
            push_string_array_inline(&mut out, "diagnostics", &package.diagnostics);
            out.push_str(", \"targets\": [");
            for (target_idx, target) in package.targets.iter().enumerate() {
                if target_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"triple\": {}, \"status\": {}, ",
                    quoted(&target.triple),
                    quoted(&target.status)
                ));
                push_string_array_inline(&mut out, "diagnostics", &target.diagnostics);
                out.push('}');
            }
            out.push_str("]}");
        }
        if !self.packages.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }

    fn status(&self) -> String {
        if self.is_success() { "pass" } else { "fail" }.to_string()
    }
}

impl PackageBuildReport {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex package build report\n");
        out.push_str(&format!(
            "schema={} package={} backend={} profile={}\n",
            self.schema, self.package, self.backend, self.profile
        ));
        for target in &self.targets {
            out.push_str(&format!(
                "  - {} status={} files=[{}] metadata=[{}]\n",
                target.triple,
                target.status,
                target
                    .files
                    .iter()
                    .map(|path| display_path(path))
                    .collect::<Vec<_>>()
                    .join(","),
                target
                    .metadata_files
                    .iter()
                    .map(|path| display_path(path))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"package\": {},\n", quoted(&self.package)));
        out.push_str(&format!("  \"backend\": {},\n", quoted(&self.backend)));
        out.push_str(&format!("  \"profile\": {},\n", quoted(&self.profile)));
        out.push_str("  \"targets\": [");
        for (idx, target) in self.targets.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"triple\": {}, \"status\": {}, ",
                quoted(&target.triple),
                quoted(&target.status)
            ));
            let files = target
                .files
                .iter()
                .map(|path| display_path(path))
                .collect::<Vec<_>>();
            push_string_array_inline(&mut out, "files", &files);
            out.push_str(", ");
            let metadata_files = target
                .metadata_files
                .iter()
                .map(|path| display_path(path))
                .collect::<Vec<_>>();
            push_string_array_inline(&mut out, "metadata_files", &metadata_files);
            out.push('}');
        }
        if !self.targets.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

impl PublishDryRunReport {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex publish dry-run\n");
        out.push_str(&format!(
            "schema={} package={} version={} ready={}\n",
            self.schema, self.package, self.version, self.ready
        ));
        out.push_str(&format!("files=[{}]\n", self.files.join(",")));
        out.push_str(&format!("checks=[{}]\n", self.checks.join(",")));
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"package\": {},\n", quoted(&self.package)));
        out.push_str(&format!("  \"version\": {},\n", quoted(&self.version)));
        out.push_str(&format!("  \"ready\": {},\n", bool_json(self.ready)));
        out.push_str("  ");
        push_string_array_inline(&mut out, "files", &self.files);
        out.push_str(",\n  ");
        push_string_array_inline(&mut out, "checks", &self.checks);
        out.push_str("\n}");
        out
    }
}

fn collect_package_node(
    input: &Path,
    visited: &mut BTreeSet<PathBuf>,
    packages: &mut Vec<PackageNode>,
) -> Result<(), String> {
    let (manifest_path, root, manifest) = read_project_manifest(input)?;
    let canonical_key = manifest_path
        .canonicalize()
        .unwrap_or_else(|_| manifest_path.clone());
    if !visited.insert(canonical_key) {
        return Ok(());
    }
    for dependency in &manifest.dependencies {
        let dependency_path = root.join(&dependency.path);
        collect_package_node(&dependency_path, visited, packages)?;
    }
    packages.push(package_node(root, manifest_path, manifest)?);
    Ok(())
}

fn package_node(
    root: PathBuf,
    manifest_path: PathBuf,
    manifest: ProjectManifest,
) -> Result<PackageNode, String> {
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|err| format!("failed to read {}: {err}", manifest_path.display()))?;
    let signature_status = package_signature_status(&manifest);
    Ok(PackageNode {
        name: manifest.name,
        version: manifest.version,
        root,
        manifest: manifest_path,
        dependencies: manifest
            .dependencies
            .iter()
            .map(|dependency| dependency.name.clone())
            .collect(),
        targets: target_matrix(&manifest.targets),
        profiles: profiles_or_default(&manifest.profiles),
        permissions: manifest
            .security
            .permissions
            .iter()
            .map(|permission| permission.name.clone())
            .collect(),
        signature_status,
        checksum: stable_hash(&manifest_text),
    })
}

fn selected_targets(
    manifest_targets: &[String],
    target: Option<&str>,
    all_targets: bool,
) -> Vec<String> {
    if let Some(target) = target {
        return vec![target.to_string()];
    }
    if all_targets {
        return target_matrix(manifest_targets);
    }
    manifest_targets
        .first()
        .cloned()
        .map(|target| vec![target])
        .unwrap_or_else(|| vec![DEFAULT_TARGET_MATRIX[0].to_string()])
}

fn target_matrix(targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        DEFAULT_TARGET_MATRIX
            .iter()
            .map(|target| (*target).to_string())
            .collect()
    } else {
        targets.to_vec()
    }
}

fn profiles_or_default(profiles: &[BuildProfile]) -> Vec<BuildProfile> {
    if profiles.is_empty() {
        vec![BuildProfile {
            name: "dev".to_string(),
            backend: "c".to_string(),
            optimize: "none".to_string(),
            debug: true,
        }]
    } else {
        profiles.to_vec()
    }
}

fn combined_program(project: &Project) -> crate::ast::Program {
    let mut items = Vec::new();
    for module in &project.modules {
        items.extend(module.program.items.clone());
    }
    crate::ast::Program { items }
}

fn diagnostic_code(diagnostic: &Diagnostic) -> String {
    format!("{}:{}", diagnostic.code, diagnostic.node)
}

fn collect_publish_files(root: &Path, path: &Path, files: &mut Vec<String>) -> Result<(), String> {
    if path.is_file() {
        files.push(
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string()
                .replace('\\', "/"),
        );
        return Ok(());
    }
    for entry in
        fs::read_dir(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() || is_publish_source(&path) {
            collect_publish_files(root, &path, files)?;
        }
    }
    Ok(())
}

fn is_publish_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("fgx") | Some("fgai") | Some("fgp") | Some("fgmap")
    )
}

fn package_module_name(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn display_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn stable_hash(source: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn push_string_array_inline(out: &mut String, key: &str, values: &[String]) {
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}
