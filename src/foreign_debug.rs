//! Foreign-debugger admission (debugger phase 6).
//!
//! Attaching a language's own debugger to a capsule — `debugpy` for Python,
//! `--inspect-brk` for Node, `dlv dap` for Go — is the cheap way to step into
//! foreign code, because those tools already speak DAP. But it cannot be done
//! quietly: it needs a loopback listener, it suspends the deadlines that bound
//! every capsule call, and it makes the run non-deterministic.
//!
//! Each of those directly contradicts a property the runtime otherwise
//! guarantees and tests for. This module is the gate that decides whether a
//! capsule may be debugged and records exactly what was given up, so a debug
//! run can never be mistaken for a governed one.
//!
//! The decision is made **before** any debugger is launched. Attaching itself
//! is the next slice; admission is separable, is the part that needs care, and
//! is the part that must never be skipped.

use crate::ast::Capsule;

/// Schema for the admission record attached to a debugged capsule call.
pub const FOREIGN_DEBUG_SCHEMA: &str = "forgentex.foreign_debug.v1";

/// What attaching a foreign debugger costs, relative to a governed call.
///
/// These are not hypothetical: each names a property the runtime asserts
/// elsewhere. The loopback listener contradicts an adversarial test that a
/// capsule cannot reach loopback; the deadline suspension contradicts the
/// mandatory wall/CPU budget every call carries; non-determinism contradicts
/// the reproducibility that pipeline cache identity depends on.
pub const RELAXATIONS: &[&str] = &[
    "loopback_listener",
    "deadlines_suspended",
    "nondeterministic",
];

/// The admission decision for one capsule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignDebugAdmission {
    pub capsule: String,
    pub language: String,
    /// `allow` or `deny`.
    pub decision: String,
    /// Why, in terms a reader can act on.
    pub reason: String,
    /// The debugger that would be attached, when admitted.
    pub debugger: Option<&'static str>,
    /// The declared environment the debugger would attach inside.
    pub env: Option<String>,
    /// Guarantees given up. Empty when denied.
    pub relaxations: Vec<&'static str>,
}

impl ForeignDebugAdmission {
    pub fn is_allowed(&self) -> bool {
        self.decision == "allow"
    }

    /// A record for the call audit. A debug run must be identifiable as one
    /// after the fact, so this is emitted whether or not it was admitted.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\": \"{}\", \"capsule\": {}, \"language\": {}, \"decision\": {}, \"reason\": {}, \"debugger\": {}, \"env\": {}, \"relaxations\": {}}}",
            FOREIGN_DEBUG_SCHEMA,
            crate::json::string(&self.capsule),
            crate::json::string(&self.language),
            crate::json::string(&self.decision),
            crate::json::string(&self.reason),
            crate::json::opt_string(self.debugger),
            crate::json::opt_string(self.env.as_deref()),
            crate::json::string_array(
                &self
                    .relaxations
                    .iter()
                    .map(|item| (*item).to_string())
                    .collect::<Vec<_>>()
            )
        )
    }
}

/// The debugger that speaks DAP for a given capsule language.
///
/// Only languages whose debugger already speaks DAP are listed. That is the
/// whole reason this approach is cheap: the adapter multiplexes between two DAP
/// conversations rather than learning a second protocol. Compiled languages go
/// through `gdb`/`lldb` and are a separate slice with a much sharper
/// containment conflict, so they are not admitted here.
pub fn debugger_for(language: &str) -> Option<&'static str> {
    match language {
        "python" => Some("debugpy"),
        "node" | "typescript" => Some("node --inspect-brk"),
        "go" => Some("dlv dap"),
        _ => None,
    }
}

/// Decide whether a capsule may be debugged.
///
/// `requested` is the caller's explicit opt-in. Without it the answer is always
/// no: the relaxations below must never be applied because a debugger happened
/// to be available.
pub fn admit(capsule: &Capsule, requested: bool) -> ForeignDebugAdmission {
    let deny = |reason: String| ForeignDebugAdmission {
        capsule: capsule.name.clone(),
        language: capsule.language.clone(),
        decision: "deny".to_string(),
        reason,
        debugger: None,
        env: capsule.env.clone(),
        relaxations: Vec::new(),
    };

    if !requested {
        return deny(
            "foreign debugging was not requested; capsules run under their normal containment"
                .to_string(),
        );
    }

    let Some(debugger) = debugger_for(&capsule.language) else {
        return deny(format!(
            "no DAP-speaking debugger for '{}' capsules; compiled languages need a gdb/lldb bridge",
            capsule.language
        ));
    };

    // The debugger must be launched inside a known interpreter. Without a
    // resolvable environment there is nothing definite to attach to, and
    // guessing would mean debugging a different runtime than the one that runs.
    let resolved = match crate::capsule_env::resolve(capsule.env.as_deref(), &capsule.language) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => {
            return deny(
                "capsule declares no resolvable environment; add env \"conda:<name>\", \"venv:<path>\", or \"path:<file>\" so the debugger attaches to a known interpreter"
                    .to_string(),
            )
        }
        Err(err) => return deny(err),
    };

    ForeignDebugAdmission {
        capsule: capsule.name.clone(),
        language: capsule.language.clone(),
        decision: "allow".to_string(),
        reason: format!("attaching {debugger} inside {}", resolved.interpreter),
        debugger: Some(debugger),
        env: capsule.env.clone(),
        relaxations: RELAXATIONS.to_vec(),
    }
}

// --- Attaching the foreign debugger ----------------------------------------

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A capsule running under its own language's debugger, with a DAP connection
/// to it.
pub struct AttachedDebugger {
    child: Child,
    stream: BufReader<TcpStream>,
    writer: TcpStream,
    seq: i64,
    /// The scratch directory holding the generated script, removed on drop.
    scratch: PathBuf,
    /// The script the capsule body was written to, which is what breakpoints
    /// and stack frames refer to.
    pub script: PathBuf,
    /// The loopback port the debugger adapter is listening on.
    pub port: u16,
}

impl Drop for AttachedDebugger {
    fn drop(&mut self) {
        // A debugged capsule is paused by definition, so it will not exit on
        // its own. Killing it is the only way to avoid leaking the process.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

/// Strip insignificant whitespace so a match does not depend on how the child
/// adapter spaces its JSON.
fn compact(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in message.chars() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

/// Reserve a loopback port by binding and immediately releasing it.
///
/// Racy in principle, but the alternative — a fixed port — collides with any
/// other debug session, and debugpy needs the port named up front.
fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|err| format!("could not reserve a debug port: {err}"))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|err| err.to_string())
}

/// Launch a Python capsule under `debugpy` and connect to it.
///
/// The admission decision must already have been made; this does not re-check
/// it, so that the relaxations are never applied by a path that skipped the
/// gate.
pub fn attach_python(
    admission: &ForeignDebugAdmission,
    interpreter: &Path,
    body: &str,
    entry: &str,
    json_args: &str,
) -> Result<AttachedDebugger, String> {
    if !admission.is_allowed() {
        return Err(format!(
            "capsule '{}' was not admitted for debugging: {}",
            admission.capsule, admission.reason
        ));
    }

    let scratch = std::env::temp_dir().join(format!(
        "forgentex_fdbg_{}_{}",
        std::process::id(),
        admission.capsule
    ));
    std::fs::create_dir_all(&scratch)
        .map_err(|err| format!("failed to create debug scratch: {err}"))?;
    let script = scratch.join("capsule.py");
    // The body is written verbatim first so its line numbers match the capsule
    // source a breakpoint refers to; the driver is appended after it.
    let driver = format!(
        "{body}\nimport json, sys\n_args = json.loads({args})\nprint(json.dumps({{\"ok\": {entry}(*_args)}}), flush=True)\n",
        args = crate::json::string(json_args)
    );
    std::fs::write(&script, driver)
        .map_err(|err| format!("failed to write debug script: {err}"))?;

    let port = free_port()?;
    let child = Command::new(interpreter)
        // Frozen modules make debugpy miss breakpoints, and the validation
        // warning is noise on stderr; both are disabled explicitly.
        .arg("-Xfrozen_modules=off")
        .args(["-m", "debugpy", "--listen"])
        .arg(format!("127.0.0.1:{port}"))
        .arg("--wait-for-client")
        .arg(&script)
        .env("PYDEVD_DISABLE_FILE_VALIDATION", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to launch debugpy: {err}"))?;

    // debugpy takes a moment to bind. Poll rather than sleep a fixed amount so
    // a fast host is not penalised and a slow one still connects.
    let deadline = Instant::now() + Duration::from_secs(30);
    let stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(err) if Instant::now() >= deadline => {
                return Err(format!("debugpy did not accept a connection: {err}"))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|err| format!("could not bound debugpy reads: {err}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|err| format!("could not bound debugpy writes: {err}"))?;
    let writer = stream
        .try_clone()
        .map_err(|err| format!("could not split the debug connection: {err}"))?;

    let mut attached = AttachedDebugger {
        child,
        stream: BufReader::new(stream),
        writer,
        seq: 0,
        scratch,
        script,
        port,
    };
    attached.handshake()?;
    Ok(attached)
}

impl AttachedDebugger {
    /// Bring the child adapter to the point where it accepts configuration.
    ///
    /// debugpy's ordering is particular and not what the DAP specification's
    /// happy path suggests:
    ///
    /// * the `initialized` **event** is withheld until the session is attached,
    ///   so waiting for it before sending `attach` deadlocks;
    /// * `attach` must carry `connect` naming the adapter's own endpoint.
    ///   Without it the adapter answers the request but never registers the
    ///   attach, and later reports "Missing launch request or attach request
    ///   before configuration done request" while the client waits.
    ///
    /// Both were found by driving a real debugpy 1.8 session; encoding them
    /// here keeps every caller from rediscovering them.
    fn handshake(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            "{\"adapterID\":\"forgentex\",\"pathFormat\":\"path\",\"linesStartAt1\":true,\"columnsStartAt1\":true}",
        )?;
        self.read_until(|message| {
            compact(message).contains("\"type\":\"response\"")
                && compact(message).contains("\"command\":\"initialize\"")
        })?
        .ok_or_else(|| "debugpy closed before answering initialize".to_string())?;

        let port = self.port;
        self.request(
            "attach",
            &format!("{{\"connect\":{{\"host\":\"127.0.0.1\",\"port\":{port}}}}}"),
        )?;
        self.read_until(|message| compact(message).contains("\"event\":\"initialized\""))?
            .ok_or_else(|| {
                "debugpy never reported `initialized`; the attach was not registered".to_string()
            })?;
        Ok(())
    }

    /// Send a DAP request and return its sequence number.
    pub fn request(&mut self, command: &str, arguments: &str) -> Result<i64, String> {
        self.seq += 1;
        let message = format!(
            "{{\"seq\":{},\"type\":\"request\",\"command\":{},\"arguments\":{arguments}}}",
            self.seq,
            crate::json::string(command)
        );
        crate::dap::write_message(&mut self.writer, &message)?;
        Ok(self.seq)
    }

    /// Read one message from the child adapter.
    pub fn read(&mut self) -> Result<Option<String>, String> {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let read = self
                .stream
                .read_line(&mut line)
                .map_err(|err| format!("failed to read from debugpy: {err}"))?;
            if read == 0 {
                return Ok(None);
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
        let Some(length) = content_length else {
            return Ok(None);
        };
        let mut body = vec![0_u8; length];
        std::io::Read::read_exact(&mut self.stream, &mut body)
            .map_err(|err| format!("failed to read debugpy body: {err}"))?;
        String::from_utf8(body).map(Some).map_err(|e| e.to_string())
    }

    /// Read until a message satisfies `wanted`, or the connection closes.
    pub fn read_until<F: Fn(&str) -> bool>(&mut self, wanted: F) -> Result<Option<String>, String> {
        // Bounded so a protocol mismatch surfaces as an error rather than a
        // hang: a debugged capsule is already paused and would wait forever.
        let mut recent = std::collections::VecDeque::with_capacity(4);
        for _ in 0..200 {
            match self.read() {
                Err(err) => {
                    let context = recent.into_iter().collect::<Vec<_>>().join(" | ");
                    return Err(if context.is_empty() {
                        err
                    } else {
                        format!("{err}; unmatched debugpy messages: {context}")
                    });
                }
                Ok(Some(message)) if wanted(&message) => return Ok(Some(message)),
                Ok(Some(message)) => {
                    if recent.len() == 4 {
                        recent.pop_front();
                    }
                    recent.push_back(message);
                }
                Ok(None) => return Ok(None),
            }
        }
        Err(format!(
            "debugpy produced no matching message within the message budget; unmatched messages: {}",
            recent.into_iter().collect::<Vec<_>>().join(" | ")
        ))
    }
}
