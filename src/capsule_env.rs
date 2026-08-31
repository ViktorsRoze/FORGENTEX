//! Declared capsule environments (debugger phase 6 prerequisite).
//!
//! A capsule may declare `env "..."` to name the environment its runtime comes
//! from. The field has been parsed into the AST and emitted in capsule JSON
//! since v2.0, but nothing consulted it: managed capsules resolved their
//! interpreter from `$PYTHON`/`$NODE`/`$GO` alone, so which Python actually ran
//! a capsule depended on the host's ambient environment rather than the
//! capsule's own contract.
//!
//! Resolution happens once, before execution, and yields an **absolute
//! interpreter path** that is then used directly. That keeps the per-call cost
//! identical to the ambient path, makes the choice recordable, and fails closed
//! when a declared environment is absent — a capsule that names an environment
//! it cannot have must not quietly fall back to a different one.

use std::path::{Path, PathBuf};

/// A resolved runtime for one capsule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEnv {
    /// Absolute path to the interpreter that will execute the capsule.
    pub interpreter: String,
    /// How it was chosen: `declared`, `ambient`, or `probe`.
    pub source: &'static str,
    /// The declared spec, when there was one.
    pub declared: Option<String>,
}

/// Resolve a declared `env` spec to an interpreter.
///
/// Supported schemes:
/// - `conda:<name>` — a named conda environment, located by layout under a
///   discovered conda root.
/// - `venv:<path>` — a virtualenv or venv directory.
/// - `path:<absolute>` — an interpreter named outright.
///
/// `Ok(None)` means the caller keeps its ambient behaviour: either no `env` was
/// declared, or the declaration is one of the descriptive runtime labels the
/// field has always carried. A spec that *is* scheme-shaped but names an
/// unknown scheme, or an environment that cannot be found, is an error rather
/// than a silent fallback — a capsule that asks for an environment must not
/// quietly run in a different one.
pub fn resolve(declared: Option<&str>, language: &str) -> Result<Option<ResolvedEnv>, String> {
    let Some(spec) = declared.map(str::trim).filter(|spec| !spec.is_empty()) else {
        return Ok(None);
    };
    // `env` has carried descriptive runtime labels since v2.0 —
    // `cpython-3.13-linux-x86_64`, `temurin-21`, `cuda-12.4-sm_80`. Those stay
    // metadata: only a scheme-prefixed spec selects a runtime, so adding
    // selection cannot change what an existing capsule does.
    let Some((scheme, value)) = spec.split_once(':') else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("capsule env '{spec}' names no environment"));
    }

    let interpreter = match scheme.trim() {
        "conda" => conda_interpreter(value, language)?,
        "venv" => venv_interpreter(Path::new(value), language)?,
        "path" => {
            let path = PathBuf::from(value);
            if !path.is_file() {
                return Err(format!(
                    "capsule env 'path:{value}' does not name an existing file"
                ));
            }
            path
        }
        _ => return Err(unsupported_spec(spec)),
    };

    Ok(Some(ResolvedEnv {
        interpreter: interpreter.to_string_lossy().to_string(),
        source: "declared",
        declared: Some(spec.to_string()),
    }))
}

fn unsupported_spec(spec: &str) -> String {
    format!(
        "capsule env '{spec}' is not supported: expected conda:<name>, venv:<path>, or path:<file>"
    )
}

/// Resolve a named conda environment.
///
/// `conda run` would be the obvious route but depends on a working conda
/// launcher, and this host has a `conda.exe` that prints "failed to create
/// process." for every invocation *and exits 0* — which would resolve to an
/// empty path. So the environment is located directly.
///
/// The primary source is `~/.conda/environments.txt`, the registry conda
/// maintains of every environment it knows about, listed by absolute path.
/// That matters because a machine can have several conda roots: guessing at
/// directory names finds only the ones you thought of, and silently misses an
/// environment that plainly exists.
fn conda_interpreter(name: &str, language: &str) -> Result<PathBuf, String> {
    let binary = interpreter_binary(language)?;
    let mut searched = Vec::new();

    for env in registered_conda_envs() {
        // A registry entry is the environment prefix itself, so its final
        // component is the environment name. Roots appear too, which is how
        // `base` resolves.
        let matches = env
            .file_name()
            .and_then(|component| component.to_str())
            .is_some_and(|component| component == name);
        if matches {
            if let Ok(interpreter) = venv_interpreter(&env, language) {
                return Ok(interpreter);
            }
        }
        searched.push(env);
    }

    // Fall back to probing standard roots, for a host whose registry is absent
    // or stale.
    for root in conda_roots() {
        let candidate = root.join("envs").join(name);
        if let Ok(interpreter) = venv_interpreter(&candidate, language) {
            return Ok(interpreter);
        }
        searched.push(candidate);
        if name == "base" {
            if let Ok(interpreter) = venv_interpreter(&root, language) {
                return Ok(interpreter);
            }
        }
    }

    if searched.is_empty() {
        return Err(format!(
            "capsule env 'conda:{name}' found no conda installation (looked at ~/.conda/environments.txt, $CONDA_PREFIX, $CONDA_EXE, and the standard install roots)"
        ));
    }
    Err(format!(
        "capsule env 'conda:{name}' has no {binary} interpreter; searched {}",
        searched
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Environments conda has registered, by absolute prefix.
///
/// Conda appends every environment it creates to this file, across every root
/// on the machine, which makes it the one place that knows about all of them.
fn registered_conda_envs() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let registry = home.join(".conda").join("environments.txt");
    let Ok(text) = std::fs::read_to_string(registry) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .collect()
}

/// Conda installation roots, for hosts with no registry. An activated shell
/// exports `CONDA_PREFIX`, so its root is tried before the standard locations.
fn conda_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(prefix) = std::env::var("CONDA_PREFIX") {
        let prefix = PathBuf::from(prefix);
        // Inside an env this is `<root>/envs/<name>`; at base it is the root.
        if prefix
            .parent()
            .is_some_and(|parent| parent.ends_with("envs"))
        {
            if let Some(root) = prefix.parent().and_then(Path::parent) {
                roots.push(root.to_path_buf());
            }
        }
        roots.push(prefix);
    }
    if let Ok(exe) = std::env::var("CONDA_EXE") {
        // `<root>/Scripts/conda.exe` or `<root>/bin/conda`.
        if let Some(root) = PathBuf::from(exe).parent().and_then(Path::parent) {
            roots.push(root.to_path_buf());
        }
    }
    if let Some(home) = home_dir() {
        for name in ["anaconda3", "miniconda3", "mambaforge", "miniforge3"] {
            roots.push(home.join(name));
        }
    }
    roots.push(PathBuf::from("/opt/conda"));
    roots.retain(|root| root.is_dir());
    roots.dedup();
    roots
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

fn venv_interpreter(root: &Path, language: &str) -> Result<PathBuf, String> {
    let binary = interpreter_binary(language)?;
    // Layouts differ: a Windows venv uses `Scripts`, a Windows conda env puts
    // the interpreter at the root, and Unix uses `bin` for both. All are
    // checked so one spec works across hosts and both tools.
    let candidates = if cfg!(windows) {
        vec![
            root.join("Scripts").join(format!("{binary}.exe")),
            root.join(format!("{binary}.exe")),
            root.join("bin").join(binary),
        ]
    } else {
        vec![root.join("bin").join(binary), root.join(binary)]
    };
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            format!(
                "capsule env 'venv:{}' has no {binary} interpreter",
                root.display()
            )
        })
}

/// The interpreter a declared environment is expected to contain.
fn interpreter_binary(language: &str) -> Result<&'static str, String> {
    match language {
        "python" => Ok("python"),
        "node" | "typescript" => Ok("node"),
        "go" => Ok("go"),
        other => Err(format!(
            "capsule env is not supported for '{other}' capsules yet"
        )),
    }
}
