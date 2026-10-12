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

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
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

/// Default combined stdout/stderr budget for one contained native process.
///
/// The wire format for the largest reviewed owned output can expand binary
/// data into decimal text, so this ceiling deliberately sits above the
/// 64-MiB logical output boundary while still preventing unbounded capture.
pub(crate) const MAX_CONTAINED_OUTPUT_BYTES: usize = 384 * 1024 * 1024;
/// Mandatory active-process ceiling for one contained tree on Windows.
///
/// Unix process groups do not provide an equivalent aggregate counter, so the
/// limit remains unavailable there until cgroup/process-group accounting lands.
pub(crate) const DEFAULT_CONTAINED_PROCESS_LIMIT: u32 = 64;
/// A live native runner has no supported subprocess boundary. Keeping the
/// direct runner as the Job Object's sole process makes that policy an OS
/// invariant on Windows rather than a source-level convention.
pub(crate) const DEFAULT_NATIVE_RUNNER_PROCESS_LIMIT: u32 = 1;
/// Complete `JOBOBJECT_BASIC_UI_RESTRICTIONS` mask installed on Windows Jobs:
/// external USER handles, clipboard read/write, system/display settings,
/// global atoms, desktops, and window-station shutdown are denied.
#[cfg(windows)]
pub(crate) const WINDOWS_JOB_UI_RESTRICTIONS: u32 = 0x0000_00ff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessLimits {
    pub cpu_ms: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub active_processes: Option<u32>,
    pub io_bytes: Option<u64>,
    pub output_bytes: usize,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            cpu_ms: None,
            memory_bytes: None,
            active_processes: cfg!(windows).then_some(DEFAULT_CONTAINED_PROCESS_LIMIT),
            io_bytes: None,
            output_bytes: MAX_CONTAINED_OUTPUT_BYTES,
        }
    }
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

/// A completed contained run: the child's captured output plus, where the host
/// can account it, the aggregate peak memory used across the whole process
/// tree. On Windows the Job Object maintains this high-water mark for every
/// assigned descendant; platforms without aggregate accounting report `None`.
pub(crate) struct ContainedRun {
    pub output: std::process::Output,
    pub peak_tree_memory_bytes: Option<u64>,
    pub peak_tree_processes: Option<u32>,
    pub windows_ui_restrictions: Option<u32>,
    pub windows_restricted_token: Option<bool>,
    pub windows_restricting_sid_count: Option<u32>,
    pub windows_integrity_level: Option<&'static str>,
    pub job_io: Option<JobIo>,
}

#[cfg(not(windows))]
struct ContainedChild(std::process::Child);

#[cfg(windows)]
enum ContainedChild {
    Standard(std::process::Child),
    Restricted(RestrictedWindowsChild),
}

#[cfg(windows)]
struct RestrictedWindowsChild {
    process: std::os::windows::io::OwnedHandle,
    main_thread: std::os::windows::io::OwnedHandle,
    process_id: u32,
    stdin: Option<std::fs::File>,
    stdout: Option<std::fs::File>,
    stderr: Option<std::fs::File>,
    status: Option<ExitStatus>,
}

impl ContainedChild {
    fn id(&self) -> u32 {
        #[cfg(not(windows))]
        {
            self.0.id()
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child.id(),
                Self::Restricted(child) => child.process_id,
            }
        }
    }

    fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
        #[cfg(not(windows))]
        {
            self.0
                .stdin
                .take()
                .map(|stream| Box::new(stream) as Box<dyn Write + Send>)
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child
                    .stdin
                    .take()
                    .map(|stream| Box::new(stream) as Box<dyn Write + Send>),
                Self::Restricted(child) => child
                    .stdin
                    .take()
                    .map(|stream| Box::new(stream) as Box<dyn Write + Send>),
            }
        }
    }

    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
        #[cfg(not(windows))]
        {
            self.0
                .stdout
                .take()
                .map(|stream| Box::new(stream) as Box<dyn Read + Send>)
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child
                    .stdout
                    .take()
                    .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
                Self::Restricted(child) => child
                    .stdout
                    .take()
                    .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
            }
        }
    }

    fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>> {
        #[cfg(not(windows))]
        {
            self.0
                .stderr
                .take()
                .map(|stream| Box::new(stream) as Box<dyn Read + Send>)
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child
                    .stderr
                    .take()
                    .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
                Self::Restricted(child) => child
                    .stderr
                    .take()
                    .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
            }
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        #[cfg(not(windows))]
        {
            self.0.try_wait()
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child.try_wait(),
                Self::Restricted(child) => child.try_wait(),
            }
        }
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(not(windows))]
        {
            self.0.wait()
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child.wait(),
                Self::Restricted(child) => child.wait(),
            }
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            self.0.kill()
        }
        #[cfg(windows)]
        {
            match self {
                Self::Standard(child) => child.kill(),
                Self::Restricted(child) => child.kill(),
            }
        }
    }

    fn windows_restricted_token(&self) -> Option<bool> {
        #[cfg(not(windows))]
        {
            None
        }
        #[cfg(windows)]
        {
            Some(matches!(self, Self::Restricted(_)))
        }
    }

    fn windows_restricting_sid_count(&self) -> Option<u32> {
        #[cfg(not(windows))]
        {
            None
        }
        #[cfg(windows)]
        {
            matches!(self, Self::Restricted(_)).then_some(3)
        }
    }

    fn windows_integrity_level(&self) -> Option<&'static str> {
        #[cfg(not(windows))]
        {
            None
        }
        #[cfg(windows)]
        {
            matches!(self, Self::Restricted(_)).then_some("low")
        }
    }
}

#[cfg(windows)]
impl RestrictedWindowsChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        self.status = restricted_process_status(&self.process)?;
        Ok(self.status)
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        wait_restricted_process(&self.process)?;
        let status = restricted_process_status(&self.process)?.ok_or_else(|| {
            io::Error::other("restricted Windows process remained active after wait")
        })?;
        self.status = Some(status);
        Ok(status)
    }

    fn kill(&mut self) -> io::Result<()> {
        terminate_restricted_process(&self.process)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct JobIo {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub other_bytes: u64,
}

impl JobIo {
    pub(crate) fn total_bytes(self) -> u64 {
        self.read_bytes
            .saturating_add(self.write_bytes)
            .saturating_add(self.other_bytes)
    }
}

/// Run a tool with a wall-clock deadline. `Ok(None)` means the child exceeded
/// the deadline and its process tree was terminated; `Ok(Some(run))` is its
/// completed output plus aggregate peak-memory evidence. Descendants are also
/// terminated when the direct child exits, so a native call cannot leave
/// background work behind.
pub(crate) fn run_with_path_timeout_limited(
    argv: &[String],
    path_dir: Option<&Path>,
    timeout: Duration,
    limits: ProcessLimits,
) -> std::io::Result<Option<ContainedRun>> {
    run_with_path_containment(
        argv,
        path_dir,
        None,
        false,
        None,
        None,
        Some(timeout),
        limits,
    )
}

/// Run a child with an **explicit** environment: it sees the listed variables
/// and nothing else. Managed capsules use this, so a capsule cannot read a
/// secret its caller happens to be holding.
pub(crate) fn run_with_environment_timeout_limited(
    argv: &[String],
    environment: &[(String, String)],
    timeout: Duration,
    limits: ProcessLimits,
) -> std::io::Result<Option<ContainedRun>> {
    run_with_path_containment(
        argv,
        None,
        None,
        false,
        Some(environment),
        None,
        Some(timeout),
        limits,
    )
}

/// Run a managed capsule's runtime with its explicit environment, writing the
/// request through an inherited stdin pipe.
///
/// Managed capsules used argv for their arguments, which caps a payload at the
/// platform's command-line length — about 32 KB on Windows, reported as an
/// opaque `The filename or extension is too long`. The request is bounded data,
/// so it belongs on a pipe, exactly as the native runners' requests do.
pub(crate) fn run_with_environment_timeout_limited_input(
    argv: &[String],
    environment: &[(String, String)],
    input: &[u8],
    timeout: Duration,
    limits: ProcessLimits,
) -> std::io::Result<Option<ContainedRun>> {
    run_with_path_containment(
        argv,
        None,
        None,
        false,
        Some(environment),
        Some(input),
        Some(timeout),
        limits,
    )
}

/// Run a native capsule runner with ambient host authority reduced before the
/// child starts. The child receives a private working directory and a cleared
/// environment; only `path_dir` is restored as PATH when a compiled executable
/// needs adjacent toolchain runtime libraries. The bounded request is written
/// through an inherited anonymous stdin pipe rather than argv or a scratch
/// file.
pub(crate) fn run_with_path_timeout_limited_isolated(
    argv: &[String],
    path_dir: Option<&Path>,
    working_directory: &Path,
    input: &[u8],
    timeout: Duration,
    limits: ProcessLimits,
) -> std::io::Result<Option<ContainedRun>> {
    run_with_path_containment(
        argv,
        path_dir,
        Some(working_directory),
        true,
        None,
        Some(input),
        Some(timeout),
        limits,
    )
}

/// Exercise process-tree cleanup without a deadline in containment tests.
/// Production native callers must use `run_with_path_timeout_limited`.
#[cfg(test)]
pub(crate) fn run_with_path_contained_limited(
    argv: &[String],
    path_dir: Option<&Path>,
    limits: ProcessLimits,
) -> std::io::Result<std::process::Output> {
    run_with_path_containment(argv, path_dir, None, false, None, None, None, limits)?
        .map(|run| run.output)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "contained process unexpectedly timed out without a deadline",
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn run_with_path_containment(
    argv: &[String],
    path_dir: Option<&Path>,
    working_directory: Option<&Path>,
    clear_environment: bool,
    environment: Option<&[(String, String)]>,
    input: Option<&[u8]>,
    timeout: Option<Duration>,
    limits: ProcessLimits,
) -> std::io::Result<Option<ContainedRun>> {
    let containment = ProcessTree::new(limits)?;
    let mut child = spawn_contained_child(
        argv,
        path_dir,
        working_directory,
        clear_environment,
        environment,
        input.is_some(),
        limits,
    )?;
    if let Err(err) = containment.attach(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    let stdout = child.take_stdout().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "contained child stdout was not piped",
        )
    })?;
    let stderr = child.take_stderr().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "contained child stderr was not piped",
        )
    })?;
    let captured = Arc::new(AtomicUsize::new(0));
    let overflow_stream = Arc::new(AtomicU8::new(0));
    let stdout_reader = capture_stream(
        stdout,
        limits.output_bytes,
        1,
        Arc::clone(&captured),
        Arc::clone(&overflow_stream),
    );
    let stderr_reader = capture_stream(
        stderr,
        limits.output_bytes,
        2,
        captured,
        Arc::clone(&overflow_stream),
    );
    let stdin_writer = if let Some(bytes) = input {
        let mut stdin = child.take_stdin().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "contained child stdin was not piped",
            )
        })?;
        let bytes = bytes.to_vec();
        Some(std::thread::spawn(move || {
            stdin.write_all(&bytes)?;
            stdin.flush()
        }))
    } else {
        None
    };
    // The child is created suspended on platforms with a post-spawn containment
    // handshake (Windows Job Objects), so its budget and aggregate accounting
    // apply before it runs any code. Resume only after it is attached and its
    // output is being drained. Unix applies limits in pre_exec and never
    // suspends, so this is a no-op there.
    if let Err(err) = resume_contained_child(&child) {
        containment.terminate();
        let _ = child.kill();
        let _ = child.wait();
        let _ = join_capture(stdout_reader);
        let _ = join_capture(stderr_reader);
        let _ = join_input(stdin_writer);
        return Err(err);
    }
    let started = Instant::now();
    // The Job Object's peak reflects only currently-associated processes, so it
    // collapses once the child exits. Sample it every tick while the tree is
    // alive and keep the running maximum as the aggregate high-water mark.
    let mut peak_tree_memory_bytes: Option<u64> = None;
    let mut peak_tree_processes = containment.active_processes();
    let windows_ui_restrictions = containment.ui_restrictions();
    let windows_restricted_token = child.windows_restricted_token();
    let windows_restricting_sid_count = child.windows_restricting_sid_count();
    let windows_integrity_level = child.windows_integrity_level();
    let mut job_io = containment.io_usage();
    loop {
        peak_tree_memory_bytes = max_peak(peak_tree_memory_bytes, containment.peak_memory());
        peak_tree_processes = max_peak(peak_tree_processes, containment.active_processes());
        job_io = containment.io_usage().or(job_io);
        if limits
            .io_bytes
            .zip(job_io)
            .is_some_and(|(limit, usage)| usage.total_bytes() > limit)
        {
            containment.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_capture(stdout_reader);
            let _ = join_capture(stderr_reader);
            let _ = join_input(stdin_writer);
            return Err(io_limit_error(limits.io_bytes.unwrap_or_default(), job_io));
        }
        if overflow_stream.load(Ordering::Acquire) != 0 {
            containment.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_capture(stdout_reader);
            let _ = join_capture(stderr_reader);
            let _ = join_input(stdin_writer);
            return Err(output_limit_error(
                overflow_stream.load(Ordering::Acquire),
                limits.output_bytes,
            ));
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(err) => {
                containment.terminate();
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_capture(stdout_reader);
                let _ = join_capture(stderr_reader);
                let _ = join_input(stdin_writer);
                return Err(err);
            }
        };
        if let Some(status) = status {
            job_io = containment.io_usage().or(job_io);
            if limits
                .io_bytes
                .zip(job_io)
                .is_some_and(|(limit, usage)| usage.total_bytes() > limit)
            {
                containment.terminate();
                let _ = join_capture(stdout_reader);
                let _ = join_capture(stderr_reader);
                let _ = join_input(stdin_writer);
                return Err(io_limit_error(limits.io_bytes.unwrap_or_default(), job_io));
            }
            containment.terminate();
            let stdout = join_capture(stdout_reader)?;
            let stderr = join_capture(stderr_reader)?;
            join_input(stdin_writer)?;
            let stream = overflow_stream.load(Ordering::Acquire);
            if stream != 0 {
                return Err(output_limit_error(stream, limits.output_bytes));
            }
            return Ok(Some(ContainedRun {
                output: Output {
                    status,
                    stdout,
                    stderr,
                },
                peak_tree_memory_bytes,
                peak_tree_processes,
                windows_ui_restrictions,
                windows_restricted_token,
                windows_restricting_sid_count,
                windows_integrity_level,
                job_io,
            }));
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            containment.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_capture(stdout_reader);
            let _ = join_capture(stderr_reader);
            let _ = join_input(stdin_writer);
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn max_peak<T: Ord + Copy>(current: Option<T>, sample: Option<T>) -> Option<T> {
    match (current, sample) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, sample) => sample,
    }
}

fn capture_stream<R: Read + Send + 'static>(
    mut stream: R,
    limit: usize,
    stream_id: u8,
    captured: Arc<AtomicUsize>,
    overflow_stream: Arc<AtomicU8>,
) -> JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                return Ok(output);
            }
            let offset = captured.fetch_add(count, Ordering::AcqRel);
            let retained = count.min(limit.saturating_sub(offset));
            output.extend_from_slice(&buffer[..retained]);
            if retained != count {
                let _ = overflow_stream.compare_exchange(
                    0,
                    stream_id,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Ok(output);
            }
        }
    })
}

fn join_capture(reader: JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| io::Error::other("contained child output reader panicked"))?
}

fn join_input(writer: Option<JoinHandle<io::Result<()>>>) -> io::Result<()> {
    match writer {
        Some(writer) => writer
            .join()
            .map_err(|_| io::Error::other("contained child input writer panicked"))?,
        None => Ok(()),
    }
}

fn output_limit_error(stream: u8, limit: usize) -> io::Error {
    let stream = if stream == 1 { "stdout" } else { "stderr" };
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!("contained child {stream} exceeded combined {limit}-byte output limit"),
    )
}

fn io_limit_error(limit: u64, usage: Option<JobIo>) -> io::Error {
    let observed = usage.map(JobIo::total_bytes).unwrap_or_default();
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!("contained child exceeded aggregate {limit}-byte Job I/O limit ({observed} bytes)"),
    )
}

#[allow(clippy::too_many_arguments)]
fn standard_contained_command(
    argv: &[String],
    path_dir: Option<&Path>,
    working_directory: Option<&Path>,
    clear_environment: bool,
    environment: Option<&[(String, String)]>,
    pipe_stdin: bool,
    limits: ProcessLimits,
) -> io::Result<Command> {
    let mut command = command_with_path_policy(argv, path_dir, clear_environment, environment);
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }
    command
        .stdin(if pipe_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_tree(&mut command, limits)?;
    Ok(command)
}

#[cfg(not(windows))]
#[allow(clippy::too_many_arguments)]
fn spawn_contained_child(
    argv: &[String],
    path_dir: Option<&Path>,
    working_directory: Option<&Path>,
    clear_environment: bool,
    environment: Option<&[(String, String)]>,
    pipe_stdin: bool,
    limits: ProcessLimits,
) -> io::Result<ContainedChild> {
    standard_contained_command(
        argv,
        path_dir,
        working_directory,
        clear_environment,
        environment,
        pipe_stdin,
        limits,
    )?
    .spawn()
    .map(ContainedChild)
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn spawn_contained_child(
    argv: &[String],
    path_dir: Option<&Path>,
    working_directory: Option<&Path>,
    clear_environment: bool,
    environment: Option<&[(String, String)]>,
    pipe_stdin: bool,
    limits: ProcessLimits,
) -> io::Result<ContainedChild> {
    if clear_environment {
        let directory = working_directory.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "restricted native runner requires a private working directory",
            )
        })?;
        // The restricted path builds its own minimal environment.
        spawn_restricted_windows_child(argv, path_dir, directory, pipe_stdin)
            .map(ContainedChild::Restricted)
    } else {
        standard_contained_command(
            argv,
            path_dir,
            working_directory,
            false,
            environment,
            pipe_stdin,
            limits,
        )?
        .spawn()
        .map(ContainedChild::Standard)
    }
}

#[cfg(windows)]
fn spawn_restricted_windows_child(
    argv: &[String],
    path_dir: Option<&Path>,
    working_directory: &Path,
    pipe_stdin: bool,
) -> io::Result<RestrictedWindowsChild> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    if argv.len() != 1 || !pipe_stdin {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "restricted Windows native runner requires one executable and piped stdin",
        ));
    }
    let restricted_token = create_windows_restricted_token()?;
    let appcontainer_sid = windows_native_appcontainer_sid()?;
    let appcontainer_folder = windows_native_appcontainer_folder()?;
    let (stdin_parent, stdin_child) = windows_anonymous_pipe(false)?;
    let (stdout_parent, stdout_child) = windows_anonymous_pipe(true)?;
    let (stderr_parent, stderr_child) = windows_anonymous_pipe(true)?;

    let inherited_handles = [
        stdin_child.as_raw_handle(),
        stdout_child.as_raw_handle(),
        stderr_child.as_raw_handle(),
    ];
    let mut attribute_bytes = 0usize;
    // SAFETY: a null-list sizing call writes only the required byte count.
    unsafe {
        InitializeProcThreadAttributeList(
            std::ptr::null_mut(),
            2,
            0,
            std::ptr::addr_of_mut!(attribute_bytes),
        );
    }
    if attribute_bytes == 0 {
        return Err(io::Error::last_os_error());
    }
    let words = attribute_bytes.div_ceil(std::mem::size_of::<usize>());
    let mut attribute_storage = vec![0usize; words];
    let attribute_list = attribute_storage.as_mut_ptr().cast();
    // SAFETY: the aligned storage is at least the size returned by the sizing
    // call and remains live until process creation completes.
    let initialized = unsafe {
        InitializeProcThreadAttributeList(
            attribute_list,
            2,
            0,
            std::ptr::addr_of_mut!(attribute_bytes),
        )
    };
    if initialized == 0 {
        return Err(io::Error::last_os_error());
    }
    let attribute_guard = ProcThreadAttributeListGuard(attribute_list);
    const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
    // SAFETY: each handle is live and inheritable, and the array remains live
    // until CreateProcessAsUserW returns.
    let updated = unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            inherited_handles.as_ptr().cast_mut().cast(),
            std::mem::size_of_val(&inherited_handles),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if updated == 0 {
        return Err(io::Error::last_os_error());
    }
    let security_capabilities = SecurityCapabilities {
        appcontainer_sid: appcontainer_sid.0,
        capabilities: std::ptr::null_mut(),
        capability_count: 0,
        reserved: 0,
    };
    const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x0002_0009;
    // SAFETY: the AppContainer SID and capability structure remain live until
    // process creation completes; zero capabilities intentionally denies
    // network and other optional AppContainer authorities.
    let updated = unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            std::ptr::addr_of!(security_capabilities).cast_mut().cast(),
            std::mem::size_of::<SecurityCapabilities>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if updated == 0 {
        return Err(io::Error::last_os_error());
    }

    const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
    let mut startup = StartupInfoExW::default();
    startup.startup_info.cb = std::mem::size_of::<StartupInfoExW>() as u32;
    startup.startup_info.flags = STARTF_USESTDHANDLES;
    startup.startup_info.stdin = stdin_child.as_raw_handle().cast();
    startup.startup_info.stdout = stdout_child.as_raw_handle().cast();
    startup.startup_info.stderr = stderr_child.as_raw_handle().cast();
    startup.attribute_list = attribute_list;

    let application = std::ffi::OsStr::new(&argv[0])
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let directory = working_directory
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut environment = windows_native_environment(path_dir, &appcontainer_folder);
    let mut process_info = ProcessInformation::default();
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    // SAFETY: all strings/blocks are null-terminated and writable where
    // required; STARTUPINFOEX and its explicit handle list remain live for the
    // call, and the output structure points to writable storage.
    let created = unsafe {
        CreateProcessAsUserW(
            restricted_token.as_raw_handle().cast(),
            application.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_mut_ptr().cast(),
            directory.as_ptr(),
            std::ptr::addr_of!(startup.startup_info),
            std::ptr::addr_of_mut!(process_info),
        )
    };
    // Keep the guard alive through CreateProcessAsUserW, then release the
    // opaque attribute list before returning.
    drop(attribute_guard);
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful process creation returns four owned handles. The
    // child-side pipe handles remain owned separately and close below.
    let process = unsafe { OwnedHandle::from_raw_handle(process_info.process.cast()) };
    let main_thread = unsafe { OwnedHandle::from_raw_handle(process_info.thread.cast()) };
    if let Err(error) = verify_windows_restricted_process(&process, &appcontainer_sid) {
        unsafe {
            let _ = TerminateProcess(process.as_raw_handle().cast(), 1);
        }
        return Err(error);
    }
    drop(stdin_child);
    drop(stdout_child);
    drop(stderr_child);
    Ok(RestrictedWindowsChild {
        process,
        main_thread,
        process_id: process_info.process_id,
        stdin: Some(stdin_parent.into()),
        stdout: Some(stdout_parent.into()),
        stderr: Some(stderr_parent.into()),
        status: None,
    })
}

#[cfg(windows)]
fn windows_native_environment(path_dir: Option<&Path>, appcontainer_folder: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    let mut block = Vec::new();
    let temp = appcontainer_folder.join("Temp");
    let mut variables = vec![
        (
            std::ffi::OsString::from("LOCALAPPDATA"),
            appcontainer_folder.as_os_str().to_os_string(),
        ),
        (
            std::ffi::OsString::from("TEMP"),
            temp.as_os_str().to_os_string(),
        ),
        (
            std::ffi::OsString::from("TMP"),
            temp.as_os_str().to_os_string(),
        ),
    ];
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        variables.push((std::ffi::OsString::from("SystemRoot"), system_root.clone()));
        variables.push((std::ffi::OsString::from("WINDIR"), system_root));
    }
    if let Some(path) = path_dir {
        variables.push((
            std::ffi::OsString::from("PATH"),
            path.as_os_str().to_os_string(),
        ));
    }
    variables.sort_by(|left, right| {
        left.0
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.0.to_string_lossy().to_ascii_lowercase())
    });
    for (name, value) in variables {
        block.extend(name.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(windows)]
fn windows_anonymous_pipe(
    parent_reads: bool,
) -> io::Result<(
    std::os::windows::io::OwnedHandle,
    std::os::windows::io::OwnedHandle,
)> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    let mut attributes = WinSecurityAttributes {
        length: std::mem::size_of::<WinSecurityAttributes>() as u32,
        security_descriptor: std::ptr::null_mut(),
        inherit_handle: 1,
    };
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    // SAFETY: all pointers refer to writable handle storage and the security
    // attributes remain live for the call.
    let created = unsafe {
        CreatePipe(
            std::ptr::addr_of_mut!(read),
            std::ptr::addr_of_mut!(write),
            std::ptr::addr_of_mut!(attributes),
            0,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe returned two newly owned, non-null handles.
    let read = unsafe { OwnedHandle::from_raw_handle(read.cast()) };
    let write = unsafe { OwnedHandle::from_raw_handle(write.cast()) };
    let parent = if parent_reads { &read } else { &write };
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    // SAFETY: the selected parent handle is live; clearing its inherit flag
    // ensures only the opposite child end appears in the explicit handle list.
    let protected =
        unsafe { SetHandleInformation(parent.as_raw_handle().cast(), HANDLE_FLAG_INHERIT, 0) };
    if protected == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(if parent_reads {
        (read, write)
    } else {
        (write, read)
    })
}

#[cfg(windows)]
pub(crate) const NATIVE_APPCONTAINER_NAME: &str = "Forgentex.NativeRunner.0_1";

#[cfg(windows)]
pub(crate) const NATIVE_APPCONTAINER_PROFILE_POLICY: &str = "versioned_immutable_no_runtime_delete";

#[cfg(windows)]
struct WindowsMutexGuard(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for WindowsMutexGuard {
    fn drop(&mut self) {
        // SAFETY: the handle is a mutex successfully acquired by this thread.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn lock_windows_native_appcontainer_profile() -> io::Result<WindowsMutexGuard> {
    let name = format!(
        r"Local\{NATIVE_APPCONTAINER_NAME}.{NATIVE_APPCONTAINER_PROFILE_POLICY}.ProfileLock"
    )
    .encode_utf16()
    .chain(std::iter::once(0))
    .collect::<Vec<_>>();
    // SAFETY: the name is null-terminated; the returned handle is checked and
    // owned by the guard. A Local named mutex coordinates every fgx process in
    // this Windows session without requiring a global-object privilege.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_ABANDONED: u32 = 0x80;
    const WAIT_TIMEOUT: u32 = 0x102;
    const WAIT_FAILED: u32 = u32::MAX;
    // Profile creation should take milliseconds. A finite ceiling prevents a
    // wedged external process from hanging every future native invocation.
    let waited = unsafe { WaitForSingleObject(handle, 30_000) };
    match waited {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(WindowsMutexGuard(handle)),
        WAIT_TIMEOUT => {
            unsafe { CloseHandle(handle) };
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for the native AppContainer profile lock",
            ))
        }
        WAIT_FAILED => {
            let error = io::Error::last_os_error();
            unsafe { CloseHandle(handle) };
            Err(error)
        }
        other => {
            unsafe { CloseHandle(handle) };
            Err(io::Error::other(format!(
                "unexpected AppContainer profile lock result 0x{other:08x}"
            )))
        }
    }
}

#[cfg(windows)]
struct OwnedWindowsSid(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for OwnedWindowsSid {
    fn drop(&mut self) {
        // SAFETY: AppContainer profile APIs return SID storage that is
        // released exactly once with FreeSid.
        unsafe {
            let _ = FreeSid(self.0);
        }
    }
}

#[cfg(windows)]
fn windows_native_appcontainer_sid() -> io::Result<OwnedWindowsSid> {
    use std::os::windows::ffi::OsStrExt;

    // CreateAppContainerProfile is not concurrency-safe even when the stable
    // profile already exists: parallel callers can receive sharing violations,
    // ERROR_BAD_ENVIRONMENT, or an unmapped HRESULT. Serialize lookup/creation
    // across threads and fgx processes, then derive the existing SID normally.
    let _profile_lock = lock_windows_native_appcontainer_profile()?;
    let name = std::ffi::OsStr::new(NATIVE_APPCONTAINER_NAME)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let display_name = "Forgentex Native Runner"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let description = "Zero-capability sandbox for Forgentex native capsules"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut sid = std::ptr::null_mut();
    // SAFETY: all strings are null-terminated and `sid` receives one
    // FreeSid-owned AppContainer SID on success.
    let created = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            display_name.as_ptr(),
            description.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::addr_of_mut!(sid),
        )
    };
    const HRESULT_ALREADY_EXISTS: i32 = 0x8007_00b7u32 as i32;
    if created == HRESULT_ALREADY_EXISTS {
        // SAFETY: `name` remains live and `sid` receives one FreeSid-owned SID.
        let derived = unsafe {
            DeriveAppContainerSidFromAppContainerName(name.as_ptr(), std::ptr::addr_of_mut!(sid))
        };
        if derived < 0 {
            return Err(windows_hresult_error(
                derived,
                "failed to derive existing native AppContainer SID",
            ));
        }
    } else if created < 0 {
        return Err(windows_hresult_error(
            created,
            "failed to create native AppContainer profile",
        ));
    }
    if sid.is_null() {
        return Err(io::Error::other(
            "native AppContainer profile returned a null SID",
        ));
    }
    Ok(OwnedWindowsSid(sid))
}

#[cfg(windows)]
fn windows_hresult_error(hresult: i32, context: &str) -> io::Error {
    let code = (hresult as u32 & 0xffff) as i32;
    if code == 0 {
        io::Error::other(format!("{context}: HRESULT 0x{:08x}", hresult as u32))
    } else {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{context}: {}", io::Error::from_raw_os_error(code)),
        )
    }
}

#[cfg(windows)]
pub(crate) fn windows_native_appcontainer_sid_string() -> io::Result<String> {
    let sid = windows_native_appcontainer_sid()?;
    let mut sid_string = std::ptr::null_mut();
    // SAFETY: the SID remains live and `sid_string` receives one
    // LocalFree-owned null-terminated string.
    let converted = unsafe { ConvertSidToStringSidW(sid.0, std::ptr::addr_of_mut!(sid_string)) };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut length = 0usize;
    // SAFETY: ConvertSidToStringSidW returned a valid null-terminated string.
    unsafe {
        while *sid_string.add(length) != 0 {
            length += 1;
        }
    }
    // SAFETY: `length` was obtained by scanning the live returned string.
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(sid_string, length) })
        .map_err(|_| io::Error::other("AppContainer SID string was invalid UTF-16"));
    // SAFETY: the string was allocated by LocalAlloc and is released once.
    unsafe {
        let _ = LocalFree(sid_string.cast());
    }
    text
}

#[cfg(windows)]
fn windows_native_appcontainer_folder() -> io::Result<std::path::PathBuf> {
    let sid = windows_native_appcontainer_sid_string()?;
    let sid = sid
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut folder = std::ptr::null_mut();
    // SAFETY: the SID string is null-terminated and `folder` receives one
    // CoTaskMemFree-owned null-terminated path on success.
    let result = unsafe { GetAppContainerFolderPath(sid.as_ptr(), std::ptr::addr_of_mut!(folder)) };
    if result < 0 {
        return Err(windows_hresult_error(
            result,
            "failed to resolve native AppContainer profile folder",
        ));
    }
    if folder.is_null() {
        return Err(io::Error::other(
            "native AppContainer profile returned a null folder path",
        ));
    }
    let mut length = 0usize;
    // SAFETY: GetAppContainerFolderPath returned a null-terminated string.
    unsafe {
        while *folder.add(length) != 0 {
            length += 1;
        }
    }
    // SAFETY: `length` was obtained by scanning the live returned path.
    let path = String::from_utf16(unsafe { std::slice::from_raw_parts(folder, length) })
        .map(std::path::PathBuf::from)
        .map_err(|_| io::Error::other("AppContainer profile path was invalid UTF-16"));
    // SAFETY: the path was allocated with COM task memory and is released once.
    unsafe {
        CoTaskMemFree(folder.cast());
    }
    path
}

#[cfg(windows)]
fn create_windows_restricted_token() -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    const TOKEN_ASSIGN_PRIMARY: u32 = 0x0001;
    const TOKEN_DUPLICATE: u32 = 0x0002;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ADJUST_DEFAULT: u32 = 0x0080;
    let mut current = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns the caller pseudo-handle and `current`
    // receives a newly owned token handle.
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
            std::ptr::addr_of_mut!(current),
        )
    };
    if opened == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken returned a newly owned handle.
    let current = unsafe { OwnedHandle::from_raw_handle(current.cast()) };
    const TOKEN_USER_CLASS: u32 = 1;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    let mut user_bytes = 0u32;
    // SAFETY: the null-buffer sizing call writes only `user_bytes`.
    unsafe {
        GetTokenInformation(
            current.as_raw_handle().cast(),
            TOKEN_USER_CLASS,
            std::ptr::null_mut(),
            0,
            std::ptr::addr_of_mut!(user_bytes),
        );
    }
    let user_error = io::Error::last_os_error();
    if user_bytes == 0 || user_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
        return Err(user_error);
    }
    let user_words = (user_bytes as usize).div_ceil(std::mem::size_of::<usize>());
    let mut user_storage = vec![0usize; user_words];
    // SAFETY: aligned storage has the exact byte capacity requested by the
    // sizing call and remains live while its SID pointer is used.
    let queried = unsafe {
        GetTokenInformation(
            current.as_raw_handle().cast(),
            TOKEN_USER_CLASS,
            user_storage.as_mut_ptr().cast(),
            user_bytes,
            std::ptr::addr_of_mut!(user_bytes),
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: TOKEN_USER begins with one SID_AND_ATTRIBUTES structure.
    let token_user = unsafe { &*user_storage.as_ptr().cast::<SidAndAttributes>() };
    const SECURITY_MAX_SID_SIZE: usize = 68;
    const WIN_BUILTIN_USERS_SID: u32 = 27;
    let mut users_sid = [0u8; SECURITY_MAX_SID_SIZE];
    let mut users_sid_bytes = users_sid.len() as u32;
    // SAFETY: the fixed buffer is large enough for any well-known SID and its
    // length remains writable for the call.
    let sid_created = unsafe {
        CreateWellKnownSid(
            WIN_BUILTIN_USERS_SID,
            std::ptr::null(),
            users_sid.as_mut_ptr().cast(),
            std::ptr::addr_of_mut!(users_sid_bytes),
        )
    };
    if sid_created == 0 {
        return Err(io::Error::last_os_error());
    }
    const WIN_RESTRICTED_CODE_SID: u32 = 18;
    let mut restricted_code_sid = [0u8; SECURITY_MAX_SID_SIZE];
    let mut restricted_code_sid_bytes = restricted_code_sid.len() as u32;
    // SAFETY: the fixed buffer is large enough for any well-known SID and its
    // length remains writable for the call.
    let sid_created = unsafe {
        CreateWellKnownSid(
            WIN_RESTRICTED_CODE_SID,
            std::ptr::null(),
            restricted_code_sid.as_mut_ptr().cast(),
            std::ptr::addr_of_mut!(restricted_code_sid_bytes),
        )
    };
    if sid_created == 0 {
        return Err(io::Error::last_os_error());
    }
    let restricting_sids = [
        SidAndAttributes {
            sid: token_user.sid,
            attributes: 0,
        },
        SidAndAttributes {
            sid: users_sid.as_mut_ptr().cast(),
            attributes: 0,
        },
        SidAndAttributes {
            sid: restricted_code_sid.as_mut_ptr().cast(),
            attributes: 0,
        },
    ];
    let mut restricted = std::ptr::null_mut();
    const DISABLE_MAX_PRIVILEGE: u32 = 0x0000_0001;
    const LUA_TOKEN: u32 = 0x0000_0004;
    // SAFETY: the source token is live and has TOKEN_DUPLICATE; both
    // restricting SID buffers remain live through the call.
    let created = unsafe {
        CreateRestrictedToken(
            current.as_raw_handle().cast(),
            DISABLE_MAX_PRIVILEGE | LUA_TOKEN,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            restricting_sids.len() as u32,
            restricting_sids.as_ptr().cast(),
            std::ptr::addr_of_mut!(restricted),
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateRestrictedToken returned a newly owned handle.
    let restricted = unsafe { OwnedHandle::from_raw_handle(restricted.cast()) };
    if unsafe { IsTokenRestricted(restricted.as_raw_handle().cast()) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "CreateRestrictedToken returned an unrestricted token",
        ));
    }
    const WIN_LOW_LABEL_SID: u32 = 66;
    let mut low_sid = [0u8; SECURITY_MAX_SID_SIZE];
    let mut low_sid_bytes = low_sid.len() as u32;
    // SAFETY: the fixed buffer is large enough for any well-known SID and its
    // length remains writable for the call.
    let sid_created = unsafe {
        CreateWellKnownSid(
            WIN_LOW_LABEL_SID,
            std::ptr::null(),
            low_sid.as_mut_ptr().cast(),
            std::ptr::addr_of_mut!(low_sid_bytes),
        )
    };
    if sid_created == 0 {
        return Err(io::Error::last_os_error());
    }
    const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;
    let low_label = SidAndAttributes {
        sid: low_sid.as_mut_ptr().cast(),
        attributes: SE_GROUP_INTEGRITY,
    };
    const TOKEN_INTEGRITY_LEVEL_CLASS: u32 = 25;
    // SAFETY: the returned token inherits TOKEN_ADJUST_DEFAULT from the source
    // handle; the label and its SID remain live for the complete call.
    let labeled = unsafe {
        SetTokenInformation(
            restricted.as_raw_handle().cast(),
            TOKEN_INTEGRITY_LEVEL_CLASS,
            std::ptr::addr_of!(low_label).cast_mut().cast(),
            (std::mem::size_of::<SidAndAttributes>() + low_sid_bytes as usize) as u32,
        )
    };
    if labeled == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(restricted)
}

#[cfg(windows)]
fn query_windows_token_information(
    token: &std::os::windows::io::OwnedHandle,
    information_class: u32,
) -> io::Result<(Vec<usize>, usize)> {
    use std::os::windows::io::AsRawHandle;

    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    let mut information_bytes = 0u32;
    // SAFETY: the null-buffer sizing call writes only `information_bytes`.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            information_class,
            std::ptr::null_mut(),
            0,
            std::ptr::addr_of_mut!(information_bytes),
        );
    }
    let sizing_error = io::Error::last_os_error();
    if information_bytes == 0 || sizing_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
        return Err(sizing_error);
    }
    let words = (information_bytes as usize).div_ceil(std::mem::size_of::<usize>());
    let mut storage = vec![0usize; words];
    let capacity_bytes = storage.len() * std::mem::size_of::<usize>();
    // SAFETY: aligned storage has at least the byte capacity requested by the
    // sizing call and remains writable throughout the query.
    let queried = unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            information_class,
            storage.as_mut_ptr().cast(),
            information_bytes,
            std::ptr::addr_of_mut!(information_bytes),
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    let information_bytes = information_bytes as usize;
    if information_bytes > capacity_bytes {
        return Err(io::Error::other(
            "Windows token information exceeded its sized buffer",
        ));
    }
    Ok((storage, information_bytes))
}

#[cfg(windows)]
fn verify_windows_restricted_process(
    process: &std::os::windows::io::OwnedHandle,
    expected_appcontainer_sid: &OwnedWindowsSid,
) -> io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    const TOKEN_QUERY: u32 = 0x0008;
    let mut token = std::ptr::null_mut();
    // SAFETY: the suspended process handle is live and `token` receives one
    // newly owned query handle.
    let opened = unsafe {
        OpenProcessToken(
            process.as_raw_handle().cast(),
            TOKEN_QUERY,
            std::ptr::addr_of_mut!(token),
        )
    };
    if opened == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken returned a newly owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token.cast()) };
    const TOKEN_IS_APPCONTAINER_CLASS: u32 = 29;
    let (appcontainer_storage, appcontainer_bytes) =
        query_windows_token_information(&token, TOKEN_IS_APPCONTAINER_CLASS)?;
    if appcontainer_bytes < std::mem::size_of::<u32>() || appcontainer_storage[0] as u32 == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native runner process did not enter its AppContainer",
        ));
    }
    const TOKEN_CAPABILITIES_CLASS: u32 = 30;
    let (capability_storage, capability_bytes) =
        query_windows_token_information(&token, TOKEN_CAPABILITIES_CLASS)?;
    // SAFETY: only the initialized byte prefix returned by the kernel is read.
    let capability_data = unsafe {
        std::slice::from_raw_parts(capability_storage.as_ptr().cast::<u8>(), capability_bytes)
    };
    let Some(capability_count_bytes) = capability_data.get(0..4) else {
        return Err(io::Error::other(
            "runner AppContainer capability information omitted its count",
        ));
    };
    let capability_count = u32::from_ne_bytes(
        capability_count_bytes
            .try_into()
            .expect("four AppContainer capability count bytes"),
    );
    if capability_count != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "native runner unexpectedly received {capability_count} AppContainer capabilities"
            ),
        ));
    }
    const TOKEN_APPCONTAINER_SID_CLASS: u32 = 31;
    let (sid_storage, sid_bytes) =
        query_windows_token_information(&token, TOKEN_APPCONTAINER_SID_CLASS)?;
    if sid_bytes < std::mem::size_of::<TokenAppContainerInformation>() {
        return Err(io::Error::other(
            "runner AppContainer information omitted its package SID",
        ));
    }
    // SAFETY: TOKEN_APPCONTAINER_INFORMATION contains one SID pointer and the
    // aligned query buffer is at least that structure's size.
    let process_appcontainer =
        unsafe { &*sid_storage.as_ptr().cast::<TokenAppContainerInformation>() };
    if process_appcontainer.sid.is_null()
        || unsafe { EqualSid(process_appcontainer.sid, expected_appcontainer_sid.0) } == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native runner process received an unexpected AppContainer SID",
        ));
    }
    if unsafe { IsTokenRestricted(token.as_raw_handle().cast()) } == 0 {
        terminate_restricted_process(process)?;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native runner process did not receive its restricted token",
        ));
    }
    const TOKEN_USER_CLASS: u32 = 1;
    let (user_storage, user_bytes) = query_windows_token_information(&token, TOKEN_USER_CLASS)?;
    if user_bytes < std::mem::size_of::<SidAndAttributes>() {
        return Err(io::Error::other(
            "runner token user information omitted its SID",
        ));
    }
    // SAFETY: TOKEN_USER begins with SID_AND_ATTRIBUTES and the aligned buffer
    // is at least that structure's size.
    let token_user = unsafe { &*user_storage.as_ptr().cast::<SidAndAttributes>() };

    const TOKEN_RESTRICTED_SIDS_CLASS: u32 = 11;
    let (restricted_storage, restricted_bytes) =
        query_windows_token_information(&token, TOKEN_RESTRICTED_SIDS_CLASS)?;
    // SAFETY: only the initialized byte prefix returned by the kernel is read.
    let restricted_data = unsafe {
        std::slice::from_raw_parts(restricted_storage.as_ptr().cast::<u8>(), restricted_bytes)
    };
    let Some(count_bytes) = restricted_data.get(0..4) else {
        return Err(io::Error::other(
            "runner token restricting-SID information omitted its count",
        ));
    };
    let restricting_sid_count = u32::from_ne_bytes(
        count_bytes
            .try_into()
            .expect("four restricting-SID count bytes"),
    );
    let entries_offset = 4usize.next_multiple_of(std::mem::align_of::<SidAndAttributes>());
    let required_bytes = (restricting_sid_count as usize)
        .checked_mul(std::mem::size_of::<SidAndAttributes>())
        .and_then(|entries| entries.checked_add(entries_offset))
        .ok_or_else(|| io::Error::other("runner restricting-SID count overflow"))?;
    if restricting_sid_count != 3 || required_bytes > restricted_data.len() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "runner token carried {restricting_sid_count} restricting SIDs instead of the required 3"
            ),
        ));
    }
    // SAFETY: the bounds and alignment of the TOKEN_GROUPS trailing array were
    // checked above and every entry was initialized by GetTokenInformation.
    let restricting_sids = unsafe {
        std::slice::from_raw_parts(
            restricted_data
                .as_ptr()
                .add(entries_offset)
                .cast::<SidAndAttributes>(),
            restricting_sid_count as usize,
        )
    };
    const WIN_RESTRICTED_CODE_SID: u32 = 18;
    const WIN_BUILTIN_USERS_SID: u32 = 27;
    let has_user = restricting_sids
        .iter()
        .any(|entry| unsafe { EqualSid(entry.sid, token_user.sid) } != 0);
    let has_users = restricting_sids
        .iter()
        .any(|entry| unsafe { IsWellKnownSid(entry.sid, WIN_BUILTIN_USERS_SID) } != 0);
    let has_restricted_code = restricting_sids
        .iter()
        .any(|entry| unsafe { IsWellKnownSid(entry.sid, WIN_RESTRICTED_CODE_SID) } != 0);
    if !has_user || !has_users || !has_restricted_code {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runner token did not preserve the required user/Users/Restricted-Code restricting SID profile",
        ));
    }

    const TOKEN_INTEGRITY_LEVEL_CLASS: u32 = 25;
    let (integrity_storage, integrity_bytes) =
        query_windows_token_information(&token, TOKEN_INTEGRITY_LEVEL_CLASS)?;
    if integrity_bytes < std::mem::size_of::<SidAndAttributes>() {
        return Err(io::Error::other(
            "runner token integrity information omitted its label",
        ));
    }
    // SAFETY: TOKEN_MANDATORY_LABEL begins with SID_AND_ATTRIBUTES and the
    // aligned buffer is at least that structure's size.
    let integrity_label = unsafe { &*integrity_storage.as_ptr().cast::<SidAndAttributes>() };
    const WIN_LOW_LABEL_SID: u32 = 66;
    const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;
    if unsafe { IsWellKnownSid(integrity_label.sid, WIN_LOW_LABEL_SID) } == 0
        || integrity_label.attributes & SE_GROUP_INTEGRITY == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runner process token did not receive the required low integrity label",
        ));
    }

    const TOKEN_PRIVILEGES_CLASS: u32 = 3;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    let mut privilege_bytes = 0u32;
    // SAFETY: the null-buffer sizing call writes only `privilege_bytes`.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TOKEN_PRIVILEGES_CLASS,
            std::ptr::null_mut(),
            0,
            std::ptr::addr_of_mut!(privilege_bytes),
        );
    }
    let privilege_error = io::Error::last_os_error();
    if privilege_bytes == 0 || privilege_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
        terminate_restricted_process(process)?;
        return Err(privilege_error);
    }
    let privilege_words = (privilege_bytes as usize).div_ceil(std::mem::size_of::<usize>());
    let mut privilege_storage = vec![0usize; privilege_words];
    // SAFETY: aligned storage has the byte capacity returned by the sizing
    // query and remains writable through the call.
    let queried = unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TOKEN_PRIVILEGES_CLASS,
            privilege_storage.as_mut_ptr().cast(),
            privilege_bytes,
            std::ptr::addr_of_mut!(privilege_bytes),
        )
    };
    if queried == 0 {
        terminate_restricted_process(process)?;
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the initialized prefix length is exactly the byte count returned
    // by GetTokenInformation and cannot exceed the allocated storage.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            privilege_storage.as_ptr().cast::<u8>(),
            privilege_bytes as usize,
        )
    };
    let Some(count_bytes) = bytes.get(0..4) else {
        terminate_restricted_process(process)?;
        return Err(io::Error::other(
            "restricted-token privilege data omitted its entry count",
        ));
    };
    let count = u32::from_ne_bytes(count_bytes.try_into().expect("four privilege count bytes"));
    const LUID_AND_ATTRIBUTES_BYTES: usize = 12;
    let required_bytes = (count as usize)
        .checked_mul(LUID_AND_ATTRIBUTES_BYTES)
        .and_then(|entries| entries.checked_add(4))
        .ok_or_else(|| io::Error::other("restricted-token privilege count overflow"))?;
    if required_bytes > bytes.len() {
        terminate_restricted_process(process)?;
        return Err(io::Error::other(
            "restricted-token privilege data was shorter than its entry count",
        ));
    }
    const SE_PRIVILEGE_ENABLED: u32 = 0x0000_0002;
    let enabled = (0..count as usize)
        .filter(|index| {
            let offset = 4 + index * LUID_AND_ATTRIBUTES_BYTES + 8;
            let attributes = u32::from_ne_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("privilege bytes"),
            );
            attributes & SE_PRIVILEGE_ENABLED != 0
        })
        .count();
    if enabled > 1 {
        terminate_restricted_process(process)?;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("restricted native runner retained {enabled} enabled privileges"),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn resume_restricted_windows_child(child: &RestrictedWindowsChild) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    // SAFETY: the main thread handle was returned suspended by
    // CreateProcessAsUserW and remains owned by the child wrapper.
    let previous = unsafe { ResumeThread(child.main_thread.as_raw_handle().cast()) };
    if previous == u32::MAX {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn restricted_process_status(
    process: &std::os::windows::io::OwnedHandle,
) -> io::Result<Option<ExitStatus>> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::ExitStatusExt;

    let mut code = 0u32;
    // SAFETY: the process handle is live and `code` is writable.
    let queried = unsafe { GetExitCodeProcess(process.as_raw_handle().cast(), &mut code) };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    const STILL_ACTIVE: u32 = 259;
    Ok((code != STILL_ACTIVE).then(|| ExitStatus::from_raw(code)))
}

#[cfg(windows)]
fn wait_restricted_process(process: &std::os::windows::io::OwnedHandle) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    const INFINITE: u32 = u32::MAX;
    const WAIT_FAILED: u32 = u32::MAX;
    // SAFETY: the process handle is live and wait does not mutate Rust memory.
    let result = unsafe { WaitForSingleObject(process.as_raw_handle().cast(), INFINITE) };
    if result == WAIT_FAILED {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn terminate_restricted_process(process: &std::os::windows::io::OwnedHandle) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    // SAFETY: the process handle is live and owned by the child wrapper.
    let terminated = unsafe { TerminateProcess(process.as_raw_handle().cast(), 1) };
    if terminated == 0 {
        let error = io::Error::last_os_error();
        if restricted_process_status(process)?.is_some() {
            Ok(())
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

#[cfg(windows)]
struct ProcThreadAttributeListGuard(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for ProcThreadAttributeListGuard {
    fn drop(&mut self) {
        // SAFETY: the pointer was initialized successfully and is deleted once.
        unsafe { DeleteProcThreadAttributeList(self.0) };
    }
}

#[cfg(unix)]
fn configure_process_tree(command: &mut Command, limits: ProcessLimits) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    if limits.cpu_ms.is_some() || limits.memory_bytes.is_some() {
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
fn configure_process_tree(command: &mut Command, _limits: ProcessLimits) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    // Start suspended so the child is assigned to its Job Object (and thus
    // budgeted and accounted) before executing a single instruction. std adds
    // CREATE_UNICODE_ENVIRONMENT on top of these flags when an environment is
    // present, so PATH handling is preserved.
    command.creation_flags(CREATE_SUSPENDED);
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
fn resume_contained_child(_child: &ContainedChild) -> std::io::Result<()> {
    // Unix children are never suspended; limits are installed in pre_exec.
    Ok(())
}

#[cfg(windows)]
fn resume_contained_child(child: &ContainedChild) -> std::io::Result<()> {
    if let ContainedChild::Restricted(child) = child {
        return resume_restricted_windows_child(child);
    }
    const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
    const THREAD_SUSPEND_RESUME: u32 = 0x0002;
    const INVALID_HANDLE_VALUE: isize = -1;
    let pid = child.id();
    // SAFETY: a thread snapshot handle is created and closed here; the returned
    // handle is validated before use.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot as isize == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let mut entry = ThreadEntry32 {
        size: std::mem::size_of::<ThreadEntry32>() as u32,
        ..ThreadEntry32::default()
    };
    let mut resumed = false;
    let mut result = Ok(());
    // SAFETY: `entry.size` is set to the documented size and the snapshot is live.
    let mut more = unsafe { Thread32First(snapshot, std::ptr::addr_of_mut!(entry)) } != 0;
    while more {
        if entry.owner_process_id == pid {
            // SAFETY: opening a thread by id with resume rights; handle checked.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.thread_id) };
            if thread.is_null() {
                result = Err(std::io::Error::last_os_error());
            } else {
                // SAFETY: `thread` was returned by OpenThread and is owned here.
                let previous = unsafe { ResumeThread(thread) };
                unsafe { CloseHandle(thread) };
                if previous == u32::MAX {
                    result = Err(std::io::Error::last_os_error());
                } else {
                    resumed = true;
                }
            }
        }
        // SAFETY: snapshot and entry remain valid for iteration.
        more = unsafe { Thread32Next(snapshot, std::ptr::addr_of_mut!(entry)) } != 0;
    }
    // SAFETY: snapshot was returned by CreateToolhelp32Snapshot and is owned here.
    unsafe { CloseHandle(snapshot) };
    result?;
    if resumed {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no suspended thread was found for the contained child",
        ))
    }
}

#[cfg(not(any(unix, windows)))]
fn resume_contained_child(_child: &ContainedChild) -> std::io::Result<()> {
    Ok(())
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

    fn attach(&self, child: &ContainedChild) -> std::io::Result<()> {
        let group = i32::try_from(child.id()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "child process id does not fit a POSIX process-group id",
            )
        })?;
        self.process_group.set(group);
        Ok(())
    }

    fn peak_memory(&self) -> Option<u64> {
        // Unix rlimits are inherited per-process, so there is no aggregate
        // whole-tree high-water mark to read back here. Aggregate accounting
        // (cgroup v2 memory.peak or /proc process-group polling) is a
        // documented follow-up; reporting None keeps the receipt truthful.
        None
    }

    fn active_processes(&self) -> Option<u32> {
        // A POSIX process group identifies the tree for termination but does
        // not expose an aggregate active-process counter.
        None
    }

    fn ui_restrictions(&self) -> Option<u32> {
        None
    }

    fn io_usage(&self) -> Option<JobIo> {
        None
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
        const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
        const JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION: u32 = 0x0000_0400;
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
        const JOB_OBJECT_BASIC_UI_RESTRICTIONS_CLASS: i32 = 4;
        let mut job_limits = JobObjectExtendedLimitInformation::default();
        job_limits.basic_limit_information.limit_flags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        if let Some(cpu_ms) = process_limits.cpu_ms {
            job_limits.basic_limit_information.limit_flags |= JOB_OBJECT_LIMIT_JOB_TIME;
            job_limits.basic_limit_information.per_job_user_time_limit =
                i64::try_from(cpu_ms.saturating_mul(10_000)).unwrap_or(i64::MAX);
        }
        if let Some(memory_bytes) = process_limits.memory_bytes {
            job_limits.basic_limit_information.limit_flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            job_limits.job_memory_limit = usize::try_from(memory_bytes).unwrap_or(usize::MAX);
        }
        if let Some(active_processes) = process_limits.active_processes {
            job_limits.basic_limit_information.limit_flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            job_limits.basic_limit_information.active_process_limit = active_processes;
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
        let ui_restrictions = WINDOWS_JOB_UI_RESTRICTIONS;
        // SAFETY: class 4 consumes the documented single-u32
        // JOBOBJECT_BASIC_UI_RESTRICTIONS layout.
        let ui_configured = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_BASIC_UI_RESTRICTIONS_CLASS,
                std::ptr::addr_of!(ui_restrictions).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if ui_configured == 0 {
            let error = std::io::Error::last_os_error();
            // SAFETY: handle was returned by CreateJobObjectW and is owned here.
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn attach(&self, child: &ContainedChild) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        let process = match child {
            ContainedChild::Standard(child) => child.as_raw_handle(),
            ContainedChild::Restricted(child) => child.process.as_raw_handle(),
        };
        // SAFETY: both handles are live for the duration of this call.
        let assigned = unsafe { AssignProcessToJobObject(self.handle, process.cast()) };
        if assigned == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn peak_memory(&self) -> Option<u64> {
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
        let mut info = JobObjectExtendedLimitInformation::default();
        let mut returned: u32 = 0;
        // SAFETY: the job handle is live and owned by self; `info` has the
        // documented class-9 layout and outlives the call, and `returned`
        // receives the written length.
        let queried = unsafe {
            QueryInformationJobObject(
                self.handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                std::ptr::addr_of_mut!(info).cast(),
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
                std::ptr::addr_of_mut!(returned),
            )
        };
        if queried == 0 {
            return None;
        }
        // peak_job_memory_used is the aggregate memory charged to every process
        // currently associated with the job. It collapses once the tree drains,
        // so callers sample it live and keep the maximum. Zero means the kernel
        // recorded nothing, which we report as absent rather than a bogus zero.
        u64::try_from(info.peak_job_memory_used)
            .ok()
            .filter(|bytes| *bytes > 0)
    }

    fn active_processes(&self) -> Option<u32> {
        const JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION_CLASS: i32 = 1;
        let mut info = JobObjectBasicAccountingInformation::default();
        let mut returned: u32 = 0;
        // SAFETY: the job handle is live and owned by self; `info` has the
        // documented class-1 layout and remains writable for the call.
        let queried = unsafe {
            QueryInformationJobObject(
                self.handle,
                JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION_CLASS,
                std::ptr::addr_of_mut!(info).cast(),
                std::mem::size_of::<JobObjectBasicAccountingInformation>() as u32,
                std::ptr::addr_of_mut!(returned),
            )
        };
        (queried != 0).then_some(info.active_processes)
    }

    fn ui_restrictions(&self) -> Option<u32> {
        const JOB_OBJECT_BASIC_UI_RESTRICTIONS_CLASS: i32 = 4;
        let mut restrictions = 0u32;
        let mut returned = 0u32;
        // SAFETY: class 4 writes the documented single-u32
        // JOBOBJECT_BASIC_UI_RESTRICTIONS layout.
        let queried = unsafe {
            QueryInformationJobObject(
                self.handle,
                JOB_OBJECT_BASIC_UI_RESTRICTIONS_CLASS,
                std::ptr::addr_of_mut!(restrictions).cast(),
                std::mem::size_of::<u32>() as u32,
                std::ptr::addr_of_mut!(returned),
            )
        };
        (queried != 0).then_some(restrictions)
    }

    fn io_usage(&self) -> Option<JobIo> {
        const JOB_OBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION_CLASS: i32 = 8;
        let mut info = JobObjectBasicAndIoAccountingInformation::default();
        let mut returned = 0u32;
        // SAFETY: class 8 writes the documented basic-accounting plus
        // IO_COUNTERS layout for every process ever associated with the Job.
        let queried = unsafe {
            QueryInformationJobObject(
                self.handle,
                JOB_OBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION_CLASS,
                std::ptr::addr_of_mut!(info).cast(),
                std::mem::size_of::<JobObjectBasicAndIoAccountingInformation>() as u32,
                std::ptr::addr_of_mut!(returned),
            )
        };
        (queried != 0).then_some(JobIo {
            read_bytes: info.io_info.read_transfer_count,
            write_bytes: info.io_info.write_transfer_count,
            other_bytes: info.io_info.other_transfer_count,
        })
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
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicAccountingInformation {
    total_user_time: i64,
    total_kernel_time: i64,
    this_period_total_user_time: i64,
    this_period_total_kernel_time: i64,
    total_page_fault_count: u32,
    total_processes: u32,
    active_processes: u32,
    total_terminated_processes: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicAndIoAccountingInformation {
    basic_info: JobObjectBasicAccountingInformation,
    io_info: IoCounters,
}

#[cfg(windows)]
#[repr(C)]
struct WinSecurityAttributes {
    length: u32,
    security_descriptor: *mut std::ffi::c_void,
    inherit_handle: i32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct StartupInfoW {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2_bytes: u16,
    reserved2: *mut u8,
    stdin: *mut std::ffi::c_void,
    stdout: *mut std::ffi::c_void,
    stderr: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct StartupInfoExW {
    startup_info: StartupInfoW,
    attribute_list: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct ProcessInformation {
    process: *mut std::ffi::c_void,
    thread: *mut std::ffi::c_void,
    process_id: u32,
    thread_id: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct SidAndAttributes {
    sid: *mut std::ffi::c_void,
    attributes: u32,
}

#[cfg(windows)]
#[repr(C)]
struct SecurityCapabilities {
    appcontainer_sid: *mut std::ffi::c_void,
    capabilities: *mut SidAndAttributes,
    capability_count: u32,
    reserved: u32,
}

#[cfg(windows)]
#[repr(C)]
struct TokenAppContainerInformation {
    sid: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(
        mutex_attributes: *const std::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn ReleaseMutex(mutex: *mut std::ffi::c_void) -> i32;
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
    fn QueryInformationJobObject(
        job: *mut std::ffi::c_void,
        information_class: i32,
        information: *mut std::ffi::c_void,
        information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(object: *mut std::ffi::c_void) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut std::ffi::c_void;
    fn Thread32First(snapshot: *mut std::ffi::c_void, entry: *mut ThreadEntry32) -> i32;
    fn Thread32Next(snapshot: *mut std::ffi::c_void, entry: *mut ThreadEntry32) -> i32;
    fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> *mut std::ffi::c_void;
    fn ResumeThread(thread: *mut std::ffi::c_void) -> u32;
    fn CreatePipe(
        read_pipe: *mut *mut std::ffi::c_void,
        write_pipe: *mut *mut std::ffi::c_void,
        pipe_attributes: *mut WinSecurityAttributes,
        size: u32,
    ) -> i32;
    fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    fn InitializeProcThreadAttributeList(
        attribute_list: *mut std::ffi::c_void,
        attribute_count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        attribute_list: *mut std::ffi::c_void,
        flags: u32,
        attribute: usize,
        value: *mut std::ffi::c_void,
        size: usize,
        previous_value: *mut std::ffi::c_void,
        return_size: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(attribute_list: *mut std::ffi::c_void);
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn GetExitCodeProcess(process: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn TerminateProcess(process: *mut std::ffi::c_void, exit_code: u32) -> i32;
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(
        process: *mut std::ffi::c_void,
        desired_access: u32,
        token: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn CreateRestrictedToken(
        existing_token: *mut std::ffi::c_void,
        flags: u32,
        disable_sid_count: u32,
        sids_to_disable: *const std::ffi::c_void,
        delete_privilege_count: u32,
        privileges_to_delete: *const std::ffi::c_void,
        restricted_sid_count: u32,
        sids_to_restrict: *const std::ffi::c_void,
        new_token: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn IsTokenRestricted(token: *mut std::ffi::c_void) -> i32;
    fn SetTokenInformation(
        token: *mut std::ffi::c_void,
        information_class: u32,
        information: *mut std::ffi::c_void,
        information_length: u32,
    ) -> i32;
    fn GetTokenInformation(
        token: *mut std::ffi::c_void,
        information_class: u32,
        information: *mut std::ffi::c_void,
        information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn CreateWellKnownSid(
        sid_type: u32,
        domain_sid: *const std::ffi::c_void,
        sid: *mut std::ffi::c_void,
        sid_size: *mut u32,
    ) -> i32;
    fn EqualSid(first_sid: *mut std::ffi::c_void, second_sid: *mut std::ffi::c_void) -> i32;
    fn IsWellKnownSid(sid: *mut std::ffi::c_void, sid_type: u32) -> i32;
    fn ConvertSidToStringSidW(sid: *mut std::ffi::c_void, string_sid: *mut *mut u16) -> i32;
    fn FreeSid(sid: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CreateProcessAsUserW(
        token: *mut std::ffi::c_void,
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const WinSecurityAttributes,
        thread_attributes: *const WinSecurityAttributes,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *mut std::ffi::c_void,
        current_directory: *const u16,
        startup_info: *const StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "userenv")]
unsafe extern "system" {
    fn CreateAppContainerProfile(
        appcontainer_name: *const u16,
        display_name: *const u16,
        description: *const u16,
        capabilities: *const SidAndAttributes,
        capability_count: u32,
        appcontainer_sid: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn DeriveAppContainerSidFromAppContainerName(
        appcontainer_name: *const u16,
        appcontainer_sid: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn GetAppContainerFolderPath(
        appcontainer_sid: *const u16,
        appcontainer_folder: *mut *mut u16,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(memory: *mut std::ffi::c_void);
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct ThreadEntry32 {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_process_id: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
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
    fn peak_memory(&self) -> Option<u64> {
        None
    }
    fn active_processes(&self) -> Option<u32> {
        None
    }
    fn ui_restrictions(&self) -> Option<u32> {
        None
    }
    fn io_usage(&self) -> Option<JobIo> {
        None
    }
    fn terminate(&self) {}
}

fn command_with_path(argv: &[String], path_dir: Option<&Path>) -> Command {
    command_with_path_policy(argv, path_dir, false, None)
}

fn command_with_path_policy(
    argv: &[String],
    path_dir: Option<&Path>,
    clear_environment: bool,
    environment: Option<&[(String, String)]>,
) -> Command {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    if clear_environment {
        command.env_clear();
    }
    // An explicit environment replaces the inherited one outright: the child
    // sees these variables and nothing else. Managed capsules use this so a
    // capsule cannot read a secret the parent happens to be holding.
    if let Some(environment) = environment {
        command.env_clear();
        for (name, value) in environment {
            command.env(name, value);
        }
    }
    if let Some(dir) = path_dir {
        if !dir.as_os_str().is_empty() {
            let mut paths = vec![dir.to_path_buf()];
            if !clear_environment && environment.is_none() {
                let existing = std::env::var_os("PATH").unwrap_or_default();
                paths.extend(std::env::split_paths(&existing));
            }
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
