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
//!   ← `{"ok": <result>}\n` or `{"err": "<message>"}\n`
//!
//! Exceptions/panics are caught inside the worker loop and returned as
//! `{"err": …}`, so a failing call never kills the worker — the next call
//! still runs warm. See docs/rt3_warm_workers_plan.md.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::ast::Capsule;

/// A persistent host-runtime process serving one capsule's entry function.
pub struct CapsuleWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    scratch: std::path::PathBuf,
}

/// Back-compat alias for the original Python-only worker name.
pub use CapsuleWorker as PythonWorker;

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
        let (py_name, _version) = crate::python_run::detect_python().ok_or_else(|| {
            "no host python found (looked for $PYTHON, python3, python)".to_string()
        })?;
        // The read-eval loop: one JSON request per line, one enveloped JSON
        // response per line, flushed. Exceptions become {"err": …} responses.
        let driver = format!(
            "import json, sys\n{body}\nfor _line in sys.stdin:\n    _line = _line.strip()\n    if not _line:\n        continue\n    try:\n        _args = json.loads(_line)\n        print(json.dumps({{\"ok\": {entry}(*_args)}}), flush=True)\n    except Exception as _e:\n        print(json.dumps({{\"err\": str(_e)}}), flush=True)\n"
        );
        let scratch = worker_scratch("py")?;
        let driver_path = scratch.join("worker.py");
        std::fs::write(&driver_path, driver)
            .map_err(|err| format!("failed to write worker driver: {err}"))?;
        Self::launch(Command::new(&py_name).arg(&driver_path), scratch, "python")
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
        let driver = format!(
            "{body}\nimport readline from 'node:readline';\nconst _rl = readline.createInterface({{ input: process.stdin }});\n_rl.on('line', (_line) => {{\n  _line = _line.trim();\n  if (!_line) return;\n  try {{\n    const _args = JSON.parse(_line);\n    process.stdout.write(JSON.stringify({{ ok: {entry}(..._args) }}) + '\\n');\n  }} catch (_e) {{\n    process.stdout.write(JSON.stringify({{ err: String(_e) }}) + '\\n');\n  }}\n}});\n"
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
        Self::launch(&mut command, scratch, "node")
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
            "package main\n\nimport (\n\t\"bufio\"\n\t\"encoding/json\"\n\t\"fmt\"\n\t\"os\"\n)\n\n{body}\n\nfunc _call(_args []json.RawMessage) (_out string) {{\n\tdefer func() {{\n\t\tif r := recover(); r != nil {{\n\t\t\t_b, _ := json.Marshal(map[string]any{{\"err\": fmt.Sprint(r)}})\n\t\t\t_out = string(_b)\n\t\t}}\n\t}}()\n{binds}\n\t_b, _ := json.Marshal(map[string]any{{\"ok\": {entry}({args})}})\n\treturn string(_b)\n}}\n\nfunc main() {{\n\t_scanner := bufio.NewScanner(os.Stdin)\n\tfor _scanner.Scan() {{\n\t\t_line := _scanner.Text()\n\t\tif _line == \"\" {{\n\t\t\tcontinue\n\t\t}}\n\t\tvar _args []json.RawMessage\n\t\tif err := json.Unmarshal([]byte(_line), &_args); err != nil {{\n\t\t\tfmt.Println(`{{\"err\":\"bad request\"}}`)\n\t\t\tcontinue\n\t\t}}\n\t\tfmt.Println(_call(_args))\n\t}}\n}}\n",
            binds = binds.join("\n"),
            args = call_args.join(", ")
        );
        let scratch = worker_scratch("go")?;
        let driver_path = scratch.join("worker.go");
        std::fs::write(&driver_path, driver)
            .map_err(|err| format!("failed to write worker driver: {err}"))?;
        Self::launch(
            Command::new(&go).arg("run").arg(&driver_path),
            scratch,
            "go",
        )
    }

    fn launch(
        command: &mut Command,
        scratch: std::path::PathBuf,
        label: &str,
    ) -> Result<Self, String> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
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
        Ok(Self {
            child,
            stdin,
            stdout,
            scratch,
        })
    }

    /// One warm call: write the JSON argument array, read the enveloped
    /// response line (`{"ok": …}` / `{"err": …}`).
    pub fn call(&mut self, json_args: &str) -> Result<String, String> {
        self.stdin
            .write_all(json_args.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|err| format!("warm worker write failed (worker died?): {err}"))?;
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .map_err(|err| format!("warm worker read failed: {err}"))?;
        if read == 0 {
            return Err("warm worker exited unexpectedly".to_string());
        }
        Ok(line.trim().to_string())
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
