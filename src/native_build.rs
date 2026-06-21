//! Native build core (v2.12).
//!
//! The first real backend: Forgentex's own pure/numeric core becomes a real
//! native artifact. We do not write a code generator — we reuse the existing
//! C transpiler and drive the **host C compiler** (`cc`/`clang`/`gcc`,
//! overridable with `$CC`). The compiler is detected at use time and the
//! build degrades gracefully when none is present (status
//! `compiler_unavailable`), so the Rust toolchain stays dependency-free and
//! CI never requires a C compiler.
//!
//! The deterministic, testable surface is the generated C source hash plus
//! the program facts (function count, compiled-in effects, `@exact`
//! functions honored). The compiled object's bytes depend on the host
//! toolchain, so the report records the toolchain identity rather than
//! pretending the artifact is reproducible across machines.

use std::path::Path;
use std::process::Command;

use crate::c_backend::build_c_project;
use crate::json;
use crate::project::load_project;

/// What the host compiler should emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// A relocatable object file (`cc -c`). Always valid for a library of
    /// functions and exactly the linkable unit the v2.13 FFI bridge needs.
    Object,
    /// A shared library (`cc -shared -fPIC`).
    Shared,
}

impl ArtifactKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "obj" | "object" => Some(ArtifactKind::Object),
            "shared" | "dylib" => Some(ArtifactKind::Shared),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Object => "object",
            ArtifactKind::Shared => "shared",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            ArtifactKind::Object => "o",
            ArtifactKind::Shared => {
                if cfg!(windows) {
                    "dll"
                } else {
                    "so"
                }
            }
        }
    }
}

/// A detected host C toolchain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBuild {
    pub schema: String,
    pub project: String,
    pub status: String,
    pub kind: String,
    pub toolchain: Option<Toolchain>,
    pub command: Option<String>,
    pub c_header_hash: String,
    pub c_source_hash: String,
    pub artifact_path: Option<String>,
    pub artifact_bytes: Option<u64>,
    pub artifact_hash: Option<String>,
    pub functions: usize,
    pub effects: Vec<String>,
    pub exact_functions: Vec<String>,
    pub message: Option<String>,
}

/// FNV-1a, matching the rest of the toolchain's stable hashing.
fn fnv_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Detect a usable host C compiler: `$CC` first, then `cc`, `clang`, `gcc`.
pub fn detect_toolchain() -> Option<Toolchain> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(cc) = std::env::var("CC") {
        if !cc.trim().is_empty() {
            candidates.push(cc.trim().to_string());
        }
    }
    candidates.extend(["cc", "clang", "gcc"].iter().map(|s| s.to_string()));
    for name in candidates {
        if let Some(version) = probe_version(&name) {
            return Some(Toolchain { name, version });
        }
    }
    None
}

/// Run a tool, optionally prepending `path_dir` to the child's PATH. This is
/// how a path-form compiler (for example `$CC=C:\w64devkit\bin\gcc.exe`) and
/// the executables it produces find sibling tools and runtime libraries.
pub(crate) fn run_with_path(
    argv: &[String],
    path_dir: Option<&Path>,
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    if let Some(dir) = path_dir {
        if !dir.as_os_str().is_empty() {
            let existing = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = vec![dir.to_path_buf()];
            paths.extend(std::env::split_paths(&existing));
            if let Ok(joined) = std::env::join_paths(paths) {
                command.env("PATH", joined);
            }
        }
    }
    command.output()
}

/// Run the compiler, putting its own directory on PATH so it can find its
/// sibling tools (the assembler and linker).
fn run_compiler(argv: &[String]) -> std::io::Result<std::process::Output> {
    run_with_path(argv, Path::new(&argv[0]).parent())
}

fn probe_version(name: &str) -> Option<String> {
    let output = Command::new(name).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().next().unwrap_or_default().trim().to_string())
}

/// Per-project facts that are independent of the compiler: function count,
/// the union of declared effects, and the `@exact` functions honored.
fn collect_facts(input: &Path) -> (usize, Vec<String>, Vec<String>) {
    let project = match load_project(input) {
        Ok(project) => project,
        Err(_) => return (0, Vec::new(), Vec::new()),
    };
    let mut functions = 0usize;
    let mut effects = std::collections::BTreeSet::new();
    let mut exact = Vec::new();
    for module in &project.modules {
        if module.standard || module.dependency {
            continue;
        }
        for function in module.program.functions() {
            functions += 1;
            for effect in &function.effects {
                effects.insert(effect.clone());
            }
            if function.attrs.iter().any(|attr| attr.name == "exact") {
                exact.push(function.name.clone());
            }
        }
    }
    (functions, effects.into_iter().collect(), exact)
}

pub fn build(input: &Path, out_dir: &Path, kind: ArtifactKind) -> NativeBuild {
    let project = input.display().to_string();
    let (functions, effects, exact_functions) = collect_facts(input);

    // 1. Generate and write the C header/source (reuses the C transpiler).
    let artifacts = match build_c_project(input, out_dir) {
        Ok(artifacts) => artifacts,
        Err(message) => {
            return NativeBuild {
                schema: schema(),
                project,
                status: "failed".to_string(),
                kind: kind.as_str().to_string(),
                toolchain: None,
                command: None,
                c_header_hash: String::new(),
                c_source_hash: String::new(),
                artifact_path: None,
                artifact_bytes: None,
                artifact_hash: None,
                functions,
                effects,
                exact_functions,
                message: Some(message),
            };
        }
    };
    let header_src = std::fs::read(&artifacts.header).unwrap_or_default();
    let source_src = std::fs::read(&artifacts.source).unwrap_or_default();
    let c_header_hash = fnv_hash(&header_src);
    let c_source_hash = fnv_hash(&source_src);

    // 2. Detect the host compiler; degrade gracefully if absent.
    let toolchain = match detect_toolchain() {
        Some(toolchain) => toolchain,
        None => {
            return NativeBuild {
                schema: schema(),
                project,
                status: "compiler_unavailable".to_string(),
                kind: kind.as_str().to_string(),
                toolchain: None,
                command: None,
                c_header_hash,
                c_source_hash,
                artifact_path: None,
                artifact_bytes: None,
                artifact_hash: None,
                functions,
                effects,
                exact_functions,
                message: Some(
                    "no host C compiler found (looked for $CC, cc, clang, gcc)".to_string(),
                ),
            };
        }
    };

    // 3. Compile the generated C to the requested native artifact.
    let stem = artifacts
        .source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();
    let artifact = out_dir.join(format!("{stem}.{}", kind.extension()));
    let source = artifacts.source.display().to_string();
    let artifact_str = artifact.display().to_string();
    let include = out_dir.display().to_string();

    let mut argv: Vec<String> = vec![
        toolchain.name.clone(),
        "-std=c11".to_string(),
        "-O2".to_string(),
    ];
    match kind {
        ArtifactKind::Object => argv.push("-c".to_string()),
        ArtifactKind::Shared => {
            argv.push("-shared".to_string());
            argv.push("-fPIC".to_string());
        }
    }
    argv.extend([
        "-I".to_string(),
        include,
        source,
        "-o".to_string(),
        artifact_str.clone(),
    ]);
    let command = argv.join(" ");

    let output = run_compiler(&argv);
    match output {
        Ok(output) if output.status.success() => {
            let bytes = std::fs::metadata(&artifact).map(|m| m.len()).ok();
            let artifact_hash = std::fs::read(&artifact).ok().map(|b| fnv_hash(&b));
            NativeBuild {
                schema: schema(),
                project,
                status: "built".to_string(),
                kind: kind.as_str().to_string(),
                toolchain: Some(toolchain),
                command: Some(command),
                c_header_hash,
                c_source_hash,
                artifact_path: Some(artifact_str),
                artifact_bytes: bytes,
                artifact_hash,
                functions,
                effects,
                exact_functions,
                message: None,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            NativeBuild {
                schema: schema(),
                project,
                status: "failed".to_string(),
                kind: kind.as_str().to_string(),
                toolchain: Some(toolchain),
                command: Some(command),
                c_header_hash,
                c_source_hash,
                artifact_path: None,
                artifact_bytes: None,
                artifact_hash: None,
                functions,
                effects,
                exact_functions,
                message: Some(stderr.trim().to_string()),
            }
        }
        Err(err) => NativeBuild {
            schema: schema(),
            project,
            status: "failed".to_string(),
            kind: kind.as_str().to_string(),
            toolchain: Some(toolchain),
            command: Some(command),
            c_header_hash,
            c_source_hash,
            artifact_path: None,
            artifact_bytes: None,
            artifact_hash: None,
            functions,
            effects,
            exact_functions,
            message: Some(format!("failed to launch compiler: {err}")),
        },
    }
}

fn schema() -> String {
    "forgentex.native_build.v1".to_string()
}

impl NativeBuild {
    pub fn is_success(&self) -> bool {
        // A missing host compiler is not a failure of Forgentex itself.
        self.status == "built" || self.status == "compiler_unavailable"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex native build\n");
        out.push_str(&format!(
            "schema={} project={} status={} kind={}\n",
            self.schema, self.project, self.status, self.kind
        ));
        if let Some(toolchain) = &self.toolchain {
            out.push_str(&format!(
                "  toolchain: {} ({})\n",
                toolchain.name, toolchain.version
            ));
        }
        out.push_str(&format!(
            "  functions={} effects=[{}] exact=[{}]\n",
            self.functions,
            self.effects.join(", "),
            self.exact_functions.join(", ")
        ));
        out.push_str(&format!(
            "  c_source_hash={} c_header_hash={}\n",
            self.c_source_hash, self.c_header_hash
        ));
        if let Some(command) = &self.command {
            out.push_str(&format!("  command: {command}\n"));
        }
        if let Some(path) = &self.artifact_path {
            out.push_str(&format!(
                "  artifact: {} ({} bytes, hash {})\n",
                path,
                self.artifact_bytes.unwrap_or(0),
                self.artifact_hash.as_deref().unwrap_or("-")
            ));
        }
        if let Some(message) = &self.message {
            out.push_str(&format!("  note: {message}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let toolchain = match &self.toolchain {
            Some(toolchain) => format!(
                "{{ \"name\": {}, \"version\": {} }}",
                json::string(&toolchain.name),
                json::string(&toolchain.version)
            ),
            None => "null".to_string(),
        };
        format!(
            "{{\n  \"schema\": {},\n  \"project\": {},\n  \"status\": {},\n  \"kind\": {},\n  \"toolchain\": {},\n  \"command\": {},\n  \"c_source_hash\": {},\n  \"c_header_hash\": {},\n  \"artifact_path\": {},\n  \"artifact_bytes\": {},\n  \"artifact_hash\": {},\n  \"functions\": {},\n  \"effects\": {},\n  \"exact_functions\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.project),
            json::string(&self.status),
            json::string(&self.kind),
            toolchain,
            json::opt_string(self.command.as_deref()),
            json::string(&self.c_source_hash),
            json::string(&self.c_header_hash),
            json::opt_string(self.artifact_path.as_deref()),
            self.artifact_bytes
                .map(|b| b.to_string())
                .unwrap_or_else(|| "null".to_string()),
            json::opt_string(self.artifact_hash.as_deref()),
            self.functions,
            json::string_array(&self.effects),
            json::string_array(&self.exact_functions),
            json::opt_string(self.message.as_deref()),
        )
    }
}
