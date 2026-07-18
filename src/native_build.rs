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
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProcessLimits {
    pub cpu_ms: Option<u64>,
    pub memory_bytes: Option<u64>,
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

/// Detect a usable host C++ compiler: `$CXX` first, then the conventional
/// `c++`, `clang++`, and `g++` driver names. Native C++ capsules use the C++
/// driver for compilation and linking so their runtime dependencies resolve
/// without teaching the bridge vendor-specific linker flags.
pub fn detect_cpp_toolchain() -> Option<Toolchain> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(cxx) = std::env::var("CXX") {
        if !cxx.trim().is_empty() {
            candidates.push(cxx.trim().to_string());
        }
    }
    candidates.extend(
        ["c++", "clang++", "g++"]
            .iter()
            .map(|name| name.to_string()),
    );
    for name in candidates {
        if let Some(version) = probe_version(&name) {
            return Some(Toolchain { name, version });
        }
    }
    None
}

/// Detect a usable Rust compiler for live Rust capsules. `$RUSTC` takes
/// precedence so pinned/custom toolchains can be selected without changing
/// the Forgentex process. The bridge invokes rustc directly and adds no Rust
/// crate dependency to Forgentex itself.
pub fn detect_rust_toolchain() -> Option<Toolchain> {
    let mut candidates = Vec::new();
    if let Ok(rustc) = std::env::var("RUSTC") {
        if !rustc.trim().is_empty() {
            candidates.push(rustc.trim().to_string());
        }
    }
    candidates.push("rustc".to_string());
    for name in candidates {
        if let Some(version) = probe_version(&name) {
            return Some(Toolchain { name, version });
        }
    }
    None
}

/// Detect a usable Zig compiler for live Zig capsules. `$ZIG` takes
/// precedence so a pinned distribution can be selected explicitly. Zig is a
/// host tool, not a Rust dependency of the trusted core.
pub fn detect_zig_toolchain() -> Option<Toolchain> {
    let mut candidates = Vec::new();
    if let Ok(zig) = std::env::var("ZIG") {
        if !zig.trim().is_empty() {
            candidates.push(zig.trim().to_string());
        }
    }
    candidates.push("zig".to_string());
    for name in candidates {
        if let Some(version) = probe_zig_version(&name) {
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
    command_with_path(argv, path_dir).output()
}

/// Run a tool with a wall-clock deadline. `Ok(None)` means the child exceeded
/// the deadline and its process tree was terminated; `Ok(Some(output))` is its
/// completed output. Descendants are also terminated when the direct child
/// exits, so a native call cannot leave background work behind.
pub(crate) fn run_with_path_timeout_limited(
    argv: &[String],
    path_dir: Option<&Path>,
    timeout: Duration,
    limits: ProcessLimits,
) -> std::io::Result<Option<std::process::Output>> {
    run_with_path_containment(argv, path_dir, Some(timeout), limits)
}

/// Run an untrusted native child without a deadline while still ensuring its
/// descendants cannot outlive the direct runner.
pub(crate) fn run_with_path_contained_limited(
    argv: &[String],
    path_dir: Option<&Path>,
    limits: ProcessLimits,
) -> std::io::Result<std::process::Output> {
    run_with_path_containment(argv, path_dir, None, limits)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "contained process unexpectedly timed out without a deadline",
        )
    })
}

fn run_with_path_containment(
    argv: &[String],
    path_dir: Option<&Path>,
    timeout: Option<Duration>,
    limits: ProcessLimits,
) -> std::io::Result<Option<std::process::Output>> {
    let mut command = command_with_path(argv, path_dir);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_tree(&mut command, limits)?;
    let containment = ProcessTree::new(limits)?;
    let mut child = command.spawn()?;
    if let Err(err) = containment.attach(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            containment.terminate();
            return child.wait_with_output().map(Some);
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            containment.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
fn configure_process_tree(command: &mut Command, limits: ProcessLimits) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    if limits != ProcessLimits::default() {
        // SAFETY: the closure calls only async-signal-safe setrlimit between
        // fork and exec and captures plain integer values.
        unsafe {
            command.pre_exec(move || apply_unix_limits(limits));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn apply_unix_limits(limits: ProcessLimits) -> std::io::Result<()> {
    #[repr(C)]
    struct RLimit {
        current: usize,
        maximum: usize,
    }
    unsafe extern "C" {
        fn setrlimit(resource: i32, limit: *const RLimit) -> i32;
    }
    const RLIMIT_CPU: i32 = 0;
    #[cfg(target_os = "macos")]
    const RLIMIT_AS: i32 = 5;
    #[cfg(not(target_os = "macos"))]
    const RLIMIT_AS: i32 = 9;
    let set = |resource, value| {
        let limit = RLimit {
            current: value,
            maximum: value,
        };
        // SAFETY: setrlimit reads the valid local structure during the call.
        if unsafe { setrlimit(resource, &limit) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    };
    if let Some(cpu_ms) = limits.cpu_ms {
        let seconds = cpu_ms.saturating_add(999) / 1000;
        set(
            RLIMIT_CPU,
            usize::try_from(seconds.max(1)).unwrap_or(usize::MAX),
        )?;
    }
    if let Some(memory_bytes) = limits.memory_bytes {
        set(
            RLIMIT_AS,
            usize::try_from(memory_bytes).unwrap_or(usize::MAX),
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn configure_process_tree(_command: &mut Command, _limits: ProcessLimits) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn configure_process_tree(_command: &mut Command, _limits: ProcessLimits) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process-tree containment is unavailable on this platform",
    ))
}

#[cfg(unix)]
struct ProcessTree {
    process_group: std::cell::Cell<i32>,
}

#[cfg(unix)]
impl ProcessTree {
    fn new(_limits: ProcessLimits) -> std::io::Result<Self> {
        Ok(Self {
            process_group: std::cell::Cell::new(0),
        })
    }

    fn attach(&self, child: &std::process::Child) -> std::io::Result<()> {
        let group = i32::try_from(child.id()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "child process id does not fit a POSIX process-group id",
            )
        })?;
        self.process_group.set(group);
        Ok(())
    }

    fn terminate(&self) {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        const SIGKILL: i32 = 9;
        // The spawned child's pid is also its process-group id because the
        // command was configured with process_group(0).
        // SAFETY: POSIX kill accepts a negative process-group id and does not
        // dereference memory in this process.
        let group = self.process_group.get();
        if group > 0 {
            let _ = unsafe { kill(-group, SIGKILL) };
        }
    }
}

#[cfg(windows)]
struct ProcessTree {
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl ProcessTree {
    fn new(process_limits: ProcessLimits) -> std::io::Result<Self> {
        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
        const JOB_OBJECT_LIMIT_JOB_TIME: u32 = 0x0000_0004;
        const JOB_OBJECT_LIMIT_JOB_MEMORY: u32 = 0x0000_0200;
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
        let mut job_limits = JobObjectExtendedLimitInformation::default();
        job_limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(cpu_ms) = process_limits.cpu_ms {
            job_limits.basic_limit_information.limit_flags |= JOB_OBJECT_LIMIT_JOB_TIME;
            job_limits.basic_limit_information.per_job_user_time_limit =
                i64::try_from(cpu_ms.saturating_mul(10_000)).unwrap_or(i64::MAX);
        }
        if let Some(memory_bytes) = process_limits.memory_bytes {
            job_limits.basic_limit_information.limit_flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            job_limits.job_memory_limit = usize::try_from(memory_bytes).unwrap_or(usize::MAX);
        }
        // SAFETY: null name/security creates a private job object owned by this
        // process. The returned handle is checked before use.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `limits` has the documented class-9 layout and remains valid
        // for the duration of the call.
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                std::ptr::addr_of!(job_limits).cast(),
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        if configured == 0 {
            let error = std::io::Error::last_os_error();
            // SAFETY: handle was returned by CreateJobObjectW and is owned here.
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn attach(&self, child: &std::process::Child) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: both handles are live for the duration of this call.
        let assigned =
            unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle().cast()) };
        if assigned == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn terminate(&self) {
        // SAFETY: the job handle is live and owned by self.
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE is a final fail-safe for every assigned descendant.
        // SAFETY: the handle is non-null, live, and closed exactly once here.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(
        job_attributes: *const std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        information_class: i32,
        information: *const std::ffi::c_void,
        information_length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(object: *mut std::ffi::c_void) -> i32;
}

#[cfg(not(any(unix, windows)))]
struct ProcessTree;

#[cfg(not(any(unix, windows)))]
impl ProcessTree {
    fn new(_limits: ProcessLimits) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "process-tree containment is unavailable on this platform",
        ))
    }
    fn attach(&self, _child: &std::process::Child) -> std::io::Result<()> {
        unreachable!()
    }
    fn terminate(&self) {}
}

fn command_with_path(argv: &[String], path_dir: Option<&Path>) -> Command {
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
    command
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

fn probe_zig_version(name: &str) -> Option<String> {
    let output = Command::new(name).arg("version").output().ok()?;
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
