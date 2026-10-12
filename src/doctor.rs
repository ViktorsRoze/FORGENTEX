//! Local toolchain and installation diagnostics.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::distribution::DIST_HOSTS;
use crate::json::{opt_string, string, string_array};

pub const DOCTOR_SCHEMA: &str = "forgentex.doctor.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolProbe {
    pub name: String,
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorReport {
    pub schema: String,
    pub version: String,
    pub status: String,
    pub executable: String,
    pub host: String,
    pub os_version: String,
    pub supported_host: bool,
    pub installed_on_path: bool,
    pub interpreter_ready: bool,
    pub source_build_ready: bool,
    pub native_c_ready: bool,
    pub zig_ready: bool,
    pub tools: Vec<ToolProbe>,
    pub issues: Vec<String>,
    pub recommendations: Vec<String>,
}

pub fn doctor_status(supported_host: bool, installed_on_path: bool) -> &'static str {
    if !supported_host {
        "unsupported_host"
    } else if !installed_on_path {
        "action_required"
    } else {
        "pass"
    }
}

pub fn doctor_report() -> DoctorReport {
    let executable_path = env::current_exe().unwrap_or_else(|_| PathBuf::from("fgx"));
    let executable = executable_path.display().to_string();
    let installed_on_path = executable_on_path(&executable_path, "fgx");

    let rustc = probe_tool("rustc", &["--version"]);
    let cargo = probe_tool("cargo", &["--version"]);
    let git = probe_tool("git", &["--version"]);
    let native_c = probe_first_tool(&["cc", "clang", "gcc", "cl"]);
    let zig = probe_tool("zig", &["version"]);
    let host = rust_host(&rustc).unwrap_or_else(compile_time_host);
    let supported_host = DIST_HOSTS.iter().any(|candidate| candidate.triple == host);
    let source_build_ready = rustc.found && cargo.found;
    let native_c_ready = native_c.found;
    let zig_ready = zig.found;

    let mut issues = Vec::new();
    let mut recommendations = Vec::new();
    if !supported_host {
        issues.push(format!(
            "host target {host} is not in the supported distribution matrix"
        ));
        recommendations.push(
            "use a supported host target or treat this checkout as an experimental port"
                .to_string(),
        );
    }
    if !installed_on_path {
        issues.push("this fgx executable is not the fgx resolved from PATH".to_string());
        recommendations.push(
            "run `cargo install --path . --bins --force`, then open a new terminal".to_string(),
        );
    }
    if !source_build_ready {
        recommendations.push(
            "install the stable Rust toolchain when rebuilding Forgentex from source".to_string(),
        );
    }
    if !native_c_ready {
        recommendations.push(
            "install a C11 compiler to enable C-backend and native capsule demos".to_string(),
        );
    }
    if !zig_ready {
        recommendations.push("install Zig 0.14 only when using the Zig backend".to_string());
    }

    DoctorReport {
        schema: DOCTOR_SCHEMA.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: doctor_status(supported_host, installed_on_path).to_string(),
        executable,
        host,
        os_version: os_version(),
        supported_host,
        installed_on_path,
        interpreter_ready: true,
        source_build_ready,
        native_c_ready,
        zig_ready,
        tools: vec![rustc, cargo, git, native_c, zig],
        issues,
        recommendations,
    }
}

impl DoctorReport {
    pub fn is_ready(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"schema\": {},\n", string(&self.schema)));
        out.push_str(&format!("  \"version\": {},\n", string(&self.version)));
        out.push_str(&format!("  \"status\": {},\n", string(&self.status)));
        out.push_str(&format!(
            "  \"executable\": {},\n",
            string(&self.executable)
        ));
        out.push_str(&format!("  \"host\": {},\n", string(&self.host)));
        out.push_str(&format!(
            "  \"os_version\": {},\n",
            string(&self.os_version)
        ));
        out.push_str(&format!("  \"supported_host\": {},\n", self.supported_host));
        out.push_str(&format!(
            "  \"installed_on_path\": {},\n",
            self.installed_on_path
        ));
        out.push_str(&format!(
            "  \"interpreter_ready\": {},\n",
            self.interpreter_ready
        ));
        out.push_str(&format!(
            "  \"source_build_ready\": {},\n",
            self.source_build_ready
        ));
        out.push_str(&format!("  \"native_c_ready\": {},\n", self.native_c_ready));
        out.push_str(&format!("  \"zig_ready\": {},\n", self.zig_ready));
        out.push_str("  \"tools\": [\n");
        for (index, tool) in self.tools.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"name\": {}, \"found\": {}, \"path\": {}, \"version\": {}}}",
                string(&tool.name),
                tool.found,
                opt_string(tool.path.as_deref()),
                opt_string(tool.version.as_deref())
            ));
            if index + 1 < self.tools.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str(&format!("  \"issues\": {},\n", string_array(&self.issues)));
        out.push_str(&format!(
            "  \"recommendations\": {}\n",
            string_array(&self.recommendations)
        ));
        out.push_str("}\n");
        out
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "Forgentex doctor\nschema={} version={} status={}\nexecutable={}\nhost={} supported={}\nos={}\ninterpreter={} installed_on_path={} source_build={} native_c={} zig={}\n",
            self.schema,
            self.version,
            self.status,
            self.executable,
            self.host,
            self.supported_host,
            self.os_version,
            readiness(self.interpreter_ready),
            readiness(self.installed_on_path),
            readiness(self.source_build_ready),
            readiness(self.native_c_ready),
            readiness(self.zig_ready)
        );
        for issue in &self.issues {
            out.push_str(&format!("issue: {issue}\n"));
        }
        for recommendation in &self.recommendations {
            out.push_str(&format!("recommendation: {recommendation}\n"));
        }
        out
    }
}

fn readiness(ready: bool) -> &'static str {
    if ready {
        "ready"
    } else {
        "missing"
    }
}

fn probe_first_tool(names: &[&str]) -> ToolProbe {
    for name in names {
        let arguments = if *name == "cl" {
            Vec::new()
        } else {
            vec!["--version"]
        };
        let probe = probe_tool(name, &arguments);
        if probe.found {
            return probe;
        }
    }
    ToolProbe {
        name: names.join("/"),
        found: false,
        path: None,
        version: None,
    }
}

fn probe_tool(name: &str, arguments: &[&str]) -> ToolProbe {
    let path = find_executable(name);
    let version = path
        .as_deref()
        .and_then(|executable| command_version(executable, arguments));
    ToolProbe {
        name: name.to_string(),
        found: path.is_some(),
        path: path.map(|value| value.display().to_string()),
        version,
    }
}

fn command_version(executable: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new(executable).args(arguments).output().ok()?;
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn rust_host(rustc: &ToolProbe) -> Option<String> {
    let executable = rustc.path.as_deref()?;
    let output = Command::new(executable).arg("-vV").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
}

fn compile_time_host() -> String {
    option_env!("TARGET")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", env::consts::ARCH, env::consts::OS))
}

fn os_version() -> String {
    #[cfg(windows)]
    {
        command_version(Path::new("cmd.exe"), &["/C", "ver"])
            .map(|version| classify_windows_version(&version))
            .unwrap_or_else(|| "windows (version unavailable)".to_string())
    }
    #[cfg(not(windows))]
    {
        command_version(Path::new("uname"), &["-sr"]).unwrap_or_else(|| env::consts::OS.to_string())
    }
}

/// Turn Windows' NT compatibility version into the product generation users
/// expect. Windows 11 deliberately still reports major/minor `10.0`; build
/// 22000 is the stable boundary between Windows 10 and Windows 11.
pub(crate) fn classify_windows_version(raw: &str) -> String {
    let version = raw
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .find_map(|candidate| {
            let parts = candidate
                .split('.')
                .filter_map(|part| part.parse::<u32>().ok())
                .collect::<Vec<_>>();
            (parts.len() >= 3).then_some(parts)
        });
    let Some(parts) = version else {
        return raw.trim().to_string();
    };
    let (major, minor, build) = (parts[0], parts[1], parts[2]);
    let revision = parts
        .get(3)
        .map(|revision| format!(".{revision}"))
        .unwrap_or_default();
    let release = if major == 10 && build >= 22_000 {
        "Windows 11"
    } else if major == 10 && build >= 10_240 {
        "Windows 10"
    } else {
        "Windows"
    };
    format!("{release} build {build}{revision} (NT {major}.{minor})")
}

fn executable_on_path(current: &Path, name: &str) -> bool {
    let Some(resolved) = find_executable(name) else {
        return false;
    };
    same_path(current, &resolved)
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 && candidate.is_file() {
        return Some(candidate.to_path_buf());
    }
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        for executable_name in executable_names(name) {
            let candidate = directory.join(executable_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_names(name: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let path = Path::new(name);
        if path.extension().is_some() {
            return vec![name.to_string()];
        }
        let extensions = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| format!("{name}{extension}"))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![name.to_string()]
    }
}
