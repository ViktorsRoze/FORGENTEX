//! rt.3 — warm capsule workers.
//!
//! A warm worker is a persistent host-runtime process for one capsule:
//! spawned on first call, reused for every later call in the run, killed when
//! the live-runtime capability is dropped. This removes the per-call spawn
//! cost that makes cold capsule composition demo-speed only — and for Go it
//! also removes the per-call *compile* (`go run` builds once at spawn).
//!
//! Protocol (identical for every language; line-delimited JSON over
//! stdin/stdout):
//!   → `[arg1, arg2, …]\n`
//!   ← `<nonce> {"ok": <result>}\n` or `<nonce> {"err": "<message>"}\n`
//!
//! Exceptions/panics are caught inside the worker loop and returned as
//! `{"err": …}`, so a failing call never kills the worker — the next call
//! still runs warm. See docs/rt3_warm_workers_plan.md.
//!
//! The response line carries a per-worker nonce and the body's own printing is
//! redirected to the error stream, because a worker whose capsule prints used to
//! return that print as its result. `managed_protocol` owns both halves of that
//! separation. A Python or Node capsule's printing arrives on the result pipe as
//! tagged diagnostic lines, in order with the answer; the worker's error stream
//! is discarded, which is why a warm Go capsule's printing is not reported.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ast::Capsule;

/// A persistent host-runtime process serving one capsule's entry function.
pub struct CapsuleWorker {
    child: Child,
    stdin: std::process::ChildStdin,
    /// Response lines, read by a thread so a call can wait with a deadline.
    /// A blocking read cannot be abandoned portably, so the read lives
    /// somewhere it is allowed to block and the call waits on a channel.
    lines: Receiver<String>,
    scratch: std::path::PathBuf,
    /// This worker's result tag. Fixed at spawn: the driver closes over it.
    nonce: String,
    /// What the capsule has printed since the last call was read, collected from
    /// the tagged diagnostic lines that arrive on the result pipe.
    diagnostics: Arc<Mutex<String>>,
}

/// Back-compat alias for the original Python-only worker name.
pub use CapsuleWorker as PythonWorker;

/// How much untagged output one call may produce before it is treated as a
/// worker that will never answer. Generous enough for a chatty library, finite
/// so a run cannot wait forever.
const MAX_UNTAGGED_LINES: usize = 10_000;

/// How long one warm call may take before it is treated as unanswerable.
///
/// A warm worker applies no per-call limits — that is why a capsule declaring
/// any runs cold instead — but "no limits" must not mean "may hang the run
/// forever": a capsule that stops answering used to block its caller with no
/// deadline at all. The value matches the managed default so the two paths
/// agree on how long is too long.
const WARM_CALL_DEADLINE: Duration =
    Duration::from_millis(crate::managed_exec::DEFAULT_MANAGED_WALL_MS);

/// The languages a warm worker exists for.
pub fn supports_language(language: &str) -> bool {
    matches!(language, "python" | "node" | "typescript" | "go")
}

impl CapsuleWorker {
    /// Spawn a warm worker for a capsule, dispatching on its language. Gated
    /// by the capsule's sandbox approval, exactly like the cold path.
    pub fn spawn(capsule: &Capsule) -> Result<Self, String> {
        if !crate::sandbox::is_approved(capsule) {
            return Err("capsule effects are not covered by its sandbox".to_string());
        }
        match capsule.language.as_str() {
            "python" => Self::spawn_python(capsule),
            "node" | "typescript" => Self::spawn_node(capsule),
            "go" => Self::spawn_go(capsule),
            other => Err(format!("no warm worker for '{other}' capsules yet")),
        }
    }

    fn spawn_python(capsule: &Capsule) -> Result<Self, String> {
        let body = crate::python_run::dedent(&capsule.code);
        let entry = crate::python_run::find_entry(&body)
            .ok_or_else(|| "no top-level def found in capsule body".to_string())?;
        let (py_name, _version) = crate::python_run::detect_python_for(capsule)?;
        // The read-eval loop: one JSON request per line, one enveloped JSON
        // response per line, flushed. Exceptions become {"err": …} responses.
        let nonce = crate::managed_protocol::nonce();
        let driver = format!(
            "{prelude}{body}\nfor _line in sys.stdin:\n    _line = _line.strip()\n    if not _line:\n        continue\n    try:\n        _args = json.loads(_line)\n        _envelope = {{\"ok\": {entry}(*_args)}}\n    except Exception as _e:\n        _envelope = {{\"err\": str(_e)}}\n    {emit}\n",
            prelude = crate::managed_protocol::python_prelude(&nonce),
            emit = crate::managed_protocol::python_emit(&nonce, "_envelope"),
        );
        let scratch = worker_scratch("py")?;
        let driver_path = scratch.join("worker.py");
        std::fs::write(&driver_path, driver)
            .map_err(|err| format!("failed to write worker driver: {err}"))?;
        Self::launch(
            capsule,
            Command::new(&py_name).arg(&driver_path),
            scratch,
            "python",
            nonce,
        )
    }

    fn spawn_node(capsule: &Capsule) -> Result<Self, String> {
        let body = crate::node_run::dedent(&capsule.code);
        let entry = crate::node_run::find_entry(&body)
            .ok_or_else(|| "no top-level function found in capsule body".to_string())?;
        let (node_name, _version) = crate::node_run::detect_node()
            .ok_or_else(|| "no host node found (looked for $NODE, node)".to_string())?;
        let is_ts = capsule.language == "typescript";
        // ESM driver (matching the cold .mjs/.ts path); errors are caught per
        // line so the worker survives a raising call.
        let nonce = crate::managed_protocol::nonce();
        let driver = format!(
            "{prelude}{body}\nimport readline from 'node:readline';\nconst _rl = readline.createInterface({{ input: process.stdin }});\n_rl.on('line', (_line) => {{\n  _line = _line.trim();\n  if (!_line) return;\n  let _envelope;\n  try {{\n    const _args = JSON.parse(_line);\n    _envelope = {{ ok: {entry}(..._args) }};\n  }} catch (_e) {{\n    _envelope = {{ err: String(_e) }};\n  }}\n  {emit};\n}});\n",
            prelude = crate::managed_protocol::node_prelude(&nonce),
            emit = crate::managed_protocol::node_emit(&nonce, "_envelope"),
        );
        let scratch = worker_scratch("node")?;
        let driver_path = scratch.join(if is_ts { "worker.ts" } else { "worker.mjs" });
        std::fs::write(&driver_path, driver)
            .map_err(|err| format!("failed to write worker driver: {err}"))?;
        let mut command = Command::new(&node_name);
        if is_ts {
            command.arg("--experimental-strip-types");
        }
        command.arg(&driver_path);
        Self::launch(capsule, &mut command, scratch, "node", nonce)
    }

    fn spawn_go(capsule: &Capsule) -> Result<Self, String> {
        let body = crate::go_run::dedent(&capsule.code);
        let entry = crate::go_run::find_entry(&body)
            .ok_or_else(|| "no func found in capsule body".to_string())?;
        let (go, _version) = crate::go_run::detect_go()
            .ok_or_else(|| "no host Go found (looked for $GO, go)".to_string())?;
        // Typed binds per declared input; a recover() turns panics into
        // {"err": …} responses so the worker loop survives them. `go run`
        // compiles ONCE at spawn — the whole per-call compile cost disappears.
        let nonce = crate::managed_protocol::nonce();
        let mut binds = Vec::new();
        let mut call_args = Vec::new();
        for (i, input) in capsule.inputs.iter().enumerate() {
            binds.push(format!(
                "\tvar a{i} {}\n\tjson.Unmarshal(_args[{i}], &a{i})",
                crate::go_run::go_json_type(&input.ty.to_agent())
            ));
            call_args.push(format!("a{i}"));
        }
        let driver = format!(
            "package main\n\nimport (\n\t\"bufio\"\n\t\"encoding/json\"\n\t\"fmt\"\n\t\"os\"\n)\n\n{body}\n\nfunc _call(_args []json.RawMessage) (_out string) {{\n\tdefer func() {{\n\t\tif r := recover(); r != nil {{\n\t\t\t_b, _ := json.Marshal(map[string]any{{\"err\": fmt.Sprint(r)}})\n\t\t\t_out = string(_b)\n\t\t}}\n\t}}()\n{binds}\n\t_b, _ := json.Marshal(map[string]any{{\"ok\": {entry}({args})}})\n\treturn string(_b)\n}}\n\nfunc main() {{\n{prelude}\t_scanner := bufio.NewScanner(os.Stdin)\n\t_scanner.Buffer(make([]byte, 0, 64*1024), 64*1024*1024)\n\tfor _scanner.Scan() {{\n\t\t_line := _scanner.Text()\n\t\tif _line == \"\" {{\n\t\t\tcontinue\n\t\t}}\n\t\tvar _args []json.RawMessage\n\t\tif err := json.Unmarshal([]byte(_line), &_args); err != nil {{\n\t\t\t{bad}\n\t\t\tcontinue\n\t\t}}\n\t\t{emit}\n\t}}\n}}\n",
            prelude = crate::managed_protocol::go_prelude(),
            bad = crate::managed_protocol::go_emit(&nonce, "`{\"err\":\"bad request\"}`"),
            emit = crate::managed_protocol::go_emit(&nonce, "_call(_args)"),
            binds = binds.join("\n"),
            args = call_args.join(", ")
        );
        let scratch = worker_scratch("go")?;
        let driver_path = scratch.join("worker.go");
        std::fs::write(&driver_path, driver)
            .map_err(|err| format!("failed to write worker driver: {err}"))?;
        Self::launch(
            capsule,
            Command::new(&go).arg("run").arg(&driver_path),
            scratch,
            "go",
            nonce,
        )
    }

    fn launch(
        capsule: &Capsule,
        command: &mut Command,
        scratch: std::path::PathBuf,
        label: &str,
        nonce: String,
    ) -> Result<Self, String> {
        // A warm worker outlives a single call, but its environment is fixed at
        // spawn, so it gets the same explicit one a cold call would: what the
        // runtime needs, plus the caller variables the capsule declared. Without
        // this a long-lived worker would hold the caller's secrets for the whole
        // run — and warm workers are the default for live runs.
        let (environment, _granted) = crate::managed_exec::environment(capsule, &capsule.language);
        command.env_clear();
        for (name, value) in &environment {
            command.env(name, value);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Discarded deliberately. Python and Node capsules send their own
            // printing down the result pipe as tagged diagnostic lines, so a
            // second pipe earns nothing here — and measurably costs: piping it
            // added a pipe pair and a drain thread per worker, which took the
            // native runners' output-capture flake from 0 failures in 3 serial
            // suite runs to 2 in 3. Go's worker redirects its body's output
            // here, so warm Go diagnostics are dropped; that is recorded in
            // docs/enforcement_matrix.md rather than paid for in stability.
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("failed to spawn {label} worker: {err}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "worker stdin unavailable".to_string())?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| "worker stdout unavailable".to_string())?,
        );
        let (send_lines, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines() {
                let Ok(line) = line else { break };
                if send_lines.send(line).is_err() {
                    break;
                }
            }
        });
        let diagnostics = Arc::new(Mutex::new(String::new()));
        Ok(Self {
            child,
            stdin,
            lines,
            scratch,
            nonce,
            diagnostics,
        })
    }

    /// One warm call: write the JSON argument array, then read until the
    /// tagged response line (`<nonce> {"ok": …}` / `<nonce> {"err": …}`).
    ///
    /// Untagged lines are the capsule's own output and are kept as diagnostics
    /// instead of being mistaken for the answer.
    pub fn call(&mut self, json_args: &str) -> Result<String, String> {
        self.stdin
            .write_all(json_args.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|err| format!("warm worker write failed (worker died?): {err}"))?;
        // Bounded: a worker that keeps its pipe open but never emits a tagged
        // line must fail the call rather than hang the run. The old protocol
        // read exactly one line, so it could return nonsense but could not
        // block; skipping untagged output must not reintroduce a wait with no
        // end. (A per-call deadline for warm workers remains open; this bounds
        // the lines, not the time.)
        let mut skipped = 0usize;
        let deadline = Instant::now() + WARM_CALL_DEADLINE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    let diagnostics = self.take_diagnostics();
                    return Err(format!(
                        "warm worker did not answer within {} ms{}",
                        WARM_CALL_DEADLINE.as_millis(),
                        if diagnostics.is_empty() {
                            String::new()
                        } else {
                            format!("; it wrote: {diagnostics}")
                        }
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let diagnostics = self.take_diagnostics();
                    return Err(if diagnostics.is_empty() {
                        "warm worker exited unexpectedly".to_string()
                    } else {
                        format!("warm worker exited unexpectedly; it wrote: {diagnostics}")
                    });
                }
            };
            let line = line.trim_end_matches(['\n', '\r']);
            match crate::managed_protocol::classify(line, &self.nonce) {
                Some(crate::managed_protocol::Tagged::Result(payload)) => {
                    return Ok(payload.trim().to_string())
                }
                // A print the capsule made during this call, ordered with the
                // answer because it came down the same pipe.
                Some(crate::managed_protocol::Tagged::Diagnostic(text)) => {
                    self.record_diagnostic(text);
                    continue;
                }
                None => {}
            }
            if line.trim().is_empty() {
                continue;
            }
            // A runtime that writes to the descriptor directly, past its own
            // print facility, still must not corrupt the result.
            self.record_diagnostic(line);
            skipped += 1;
            if skipped > MAX_UNTAGGED_LINES {
                let diagnostics = self.take_diagnostics();
                return Err(format!(
                    "managed_capsule_no_result: warm worker wrote {skipped} lines without a result line; it wrote: {diagnostics}"
                ));
            }
        }
    }

    fn record_diagnostic(&self, line: &str) {
        let Ok(mut held) = self.diagnostics.lock() else {
            return;
        };
        if held.len() >= crate::managed_protocol::MAX_DIAGNOSTIC_BYTES {
            return;
        }
        if !held.is_empty() {
            held.push('\n');
        }
        held.push_str(line);
    }

    /// Whatever the capsule has printed since this was last called.
    pub fn take_diagnostics(&self) -> String {
        match self.diagnostics.lock() {
            Ok(mut held) => std::mem::take(&mut *held),
            Err(_) => String::new(),
        }
    }
}

fn worker_scratch(label: &str) -> Result<std::path::PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WORKER_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = WORKER_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!(
        "forgentex_{}_{}_{label}worker",
        std::process::id(),
        seq
    ));
    std::fs::create_dir_all(&scratch)
        .map_err(|err| format!("failed to create scratch dir: {err}"))?;
    Ok(scratch)
}

impl Drop for CapsuleWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}
