//! The wire protocol between Forgentex and a managed capsule.
//!
//! A capsule's body is ordinary code in its own language, so it prints. Real
//! libraries print: deprecation warnings, progress bars, banners, a stray debug
//! line left in by whoever wrote the body. The first protocol took *all* of a
//! capsule's standard output as its result, so one `print()` turned a working
//! capsule into `worker returned invalid JSON` — a failure with nothing to do
//! with what the capsule computed.
//!
//! Two channels fix that, and they are separated twice over:
//!
//! 1. **The guest's standard output is captured** before the body runs, so
//!    anything the body prints is diagnostics by construction. Where the
//!    language allows its print facility to be replaced (Python, Node), each
//!    printed line is re-emitted as a tagged *diagnostic* line on the result
//!    stream: that keeps a print ordered against the result it accompanies,
//!    which matters for a warm worker, where the answer and the printing would
//!    otherwise race down two pipes. Elsewhere (Go, the JVM, .NET) the body's
//!    output is redirected to the error stream instead.
//! 2. **The result line is tagged with a per-call nonce.** Redirection covers
//!    the language's own print facility; it cannot cover a C extension, a
//!    subprocess, or a runtime that writes to the file descriptor directly. The
//!    host reads the line carrying the nonce and treats every other line as
//!    diagnostics, so output the language never routed is still not mistaken
//!    for an answer.
//!
//! Arguments travel on stdin rather than argv. On argv a payload above roughly
//! 32 KB fails on Windows with an opaque `The filename or extension is too
//! long`, and because a capsule that declares any `limit.*` is not eligible for
//! a warm worker, declaring a limit used to silently cap a capsule's input.
//! Containment and capacity should not trade against each other.
//!
//! Diagnostics are captured and reported rather than dropped. A print that
//! vanishes is its own debugging problem.

/// How much guest diagnostic output to keep per call. Enough for a stack trace
/// or a few warnings; bounded because a runaway loop can print without end and
/// this text ends up in a receipt.
pub const MAX_DIAGNOSTIC_BYTES: usize = 4096;

/// A per-call tag for the result line.
///
/// Derived from the process, a counter, and the clock, then hashed, so it is
/// unguessable enough that guest output cannot collide with it by accident —
/// including guest output that echoes its own input.
pub fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let seed = format!("{}:{seq}:{nanos}", std::process::id());
    format!("fgx{}", &crate::crypto::sha256_hex(seed.as_bytes())[..16])
}

/// What a managed call produced: the tagged result line, and whatever else the
/// guest wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManagedOutput {
    /// The payload of the nonce-tagged line, if the guest produced one.
    pub result: Option<String>,
    /// Everything the guest wrote that was not the result, bounded.
    pub diagnostics: String,
}

impl ManagedOutput {
    /// The result, or an error that carries the guest's own diagnostics —
    /// because when a capsule fails to answer, what it printed while failing is
    /// usually the explanation.
    pub fn result_or_error(self, capsule: &str) -> Result<String, String> {
        match self.result {
            Some(result) => Ok(result),
            None if self.diagnostics.is_empty() => Err(format!(
                "managed_capsule_no_result: capsule '{capsule}' produced no result line"
            )),
            None => Err(format!(
                "managed_capsule_no_result: capsule '{capsule}' produced no result line; it wrote: {}",
                self.diagnostics
            )),
        }
    }
}

/// What a tagged line carries.
pub enum Tagged<'a> {
    /// The call's answer.
    Result(&'a str),
    /// One line the capsule printed.
    Diagnostic(&'a str),
}

/// Classify one line of a capsule's output.
///
/// A line is the protocol's only when it carries the call's nonce *and* a kind,
/// so neither guest output nor a line from another call can be mistaken for an
/// answer.
pub fn classify<'a>(line: &'a str, nonce: &str) -> Option<Tagged<'a>> {
    let rest = line
        .trim_end_matches('\r')
        .strip_prefix(nonce)?
        .strip_prefix(' ')?;
    if let Some(payload) = rest.strip_prefix("R ") {
        return Some(Tagged::Result(payload));
    }
    if let Some(payload) = rest.strip_prefix("D ") {
        return Some(Tagged::Diagnostic(payload));
    }
    // An empty diagnostic line keeps its blank shape rather than disappearing.
    if rest == "D" {
        return Some(Tagged::Diagnostic(""));
    }
    None
}

/// Split a capsule's output into its result and its diagnostics.
///
/// `stdout` and `stderr` are both scanned for the tagged line: a redirected
/// runtime writes the result to the descriptor the host reads as stdout, but a
/// language that buffers differently, or a wrapper that merges the two, must
/// not lose the answer.
pub fn split(stdout: &str, stderr: &str, nonce: &str) -> ManagedOutput {
    let mut result = None;
    let mut diagnostics = String::new();
    for line in stdout.lines().chain(stderr.lines()) {
        match classify(line, nonce) {
            // The last result wins: a warm worker's stream holds one per call,
            // and a cold call that somehow wrote twice should report what it
            // finished with.
            Some(Tagged::Result(payload)) => {
                result = Some(payload.to_string());
                continue;
            }
            Some(Tagged::Diagnostic(text)) => {
                push_diagnostic(&mut diagnostics, text);
                continue;
            }
            None => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        // Output the language never routed — a C extension, a subprocess — is
        // still not an answer.
        push_diagnostic(&mut diagnostics, line);
    }
    ManagedOutput {
        result,
        diagnostics,
    }
}

/// Append one diagnostic line, bounded, cutting only on a character boundary:
/// this text ends up in a receipt, and a half-written character would make it
/// unprintable.
pub fn push_diagnostic(diagnostics: &mut String, line: &str) {
    if diagnostics.len() >= MAX_DIAGNOSTIC_BYTES {
        return;
    }
    if !diagnostics.is_empty() {
        diagnostics.push('\n');
    }
    let room = MAX_DIAGNOSTIC_BYTES.saturating_sub(diagnostics.len());
    if line.len() <= room {
        diagnostics.push_str(line);
        return;
    }
    let mut cut = room;
    while cut > 0 && !line.is_char_boundary(cut) {
        cut -= 1;
    }
    diagnostics.push_str(&line[..cut]);
    diagnostics.push('…');
}

/// Run a prepared command with its request on stdin, for the fixture runners
/// (`fgx capsule-run`), which execute outside the contained live path.
pub fn run_with_stdin(
    command: &mut std::process::Command,
    input: &str,
) -> std::io::Result<std::process::Output> {
    use std::io::Write;
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())?;
    }
    // Closing stdin is what tells the guest its request is complete; without it
    // a runtime reading to end-of-input waits forever.
    drop(child.stdin.take());
    child.wait_with_output()
}

/// The Python prelude: keep the real result channel, and turn everything the
/// body prints into tagged diagnostic lines on it.
///
/// `sys.stdout` is rebound before the body is defined, so even a body that
/// prints at import time is diagnostics rather than a corrupt result. Partial
/// writes are buffered until their newline, so `print(x, end="")` cannot leave
/// half a line in front of a result.
pub fn python_prelude(nonce: &str) -> String {
    // Built a line at a time, because this is Python and its indentation is
    // load-bearing. The guest's print facility becomes a writer that emits one
    // tagged diagnostic line per newline and buffers a partial write, so
    // `print(x, end="")` cannot leave half a line in front of a result.
    [
        "import json, sys".to_string(),
        "_fgx_result = sys.stdout".to_string(),
        "class _FgxDiagnostics:".to_string(),
        "    def __init__(self, sink):".to_string(),
        "        self._sink = sink".to_string(),
        "        self._held = ''".to_string(),
        "    def write(self, text):".to_string(),
        "        self._held += str(text)".to_string(),
        "        while '\\n' in self._held:".to_string(),
        "            line, self._held = self._held.split('\\n', 1)".to_string(),
        format!("            self._sink.write('{nonce} D ' + line + '\\n')"),
        "            self._sink.flush()".to_string(),
        "        return len(text)".to_string(),
        "    def flush(self):".to_string(),
        "        if self._held:".to_string(),
        format!("            self._sink.write('{nonce} D ' + self._held + '\\n')"),
        "            self._sink.flush()".to_string(),
        "            self._held = ''".to_string(),
        "    def isatty(self):".to_string(),
        "        return False".to_string(),
        "sys.stdout = _FgxDiagnostics(_fgx_result)".to_string(),
        String::new(),
    ]
    .join("\n")
}

/// Write one tagged result line to the saved Python channel.
pub fn python_emit(nonce: &str, expression: &str) -> String {
    // One line, semicolon-joined: this is spliced into an indented loop body in
    // the warm worker, and a continuation line would land at the wrong
    // indentation — which stopped the worker answering at all, silently.
    format!(
        "sys.stdout.flush(); _fgx_result.write('{nonce} R ' + json.dumps({expression}) + '\\n'); _fgx_result.flush()"
    )
}

/// The Node prelude. `console.log` and friends are the language's print
/// facility, so they become tagged diagnostic lines on the result stream.
pub fn node_prelude(nonce: &str) -> String {
    [
        "const _fgxResult = (s) => process.stdout.write(s);".to_string(),
        // One tagged line per line of console output, so a multi-line message
        // stays legible in a receipt.
        format!(
            "const _fgxDiag = (...args) => {{ const text = args.map((a) => typeof a === 'string' ? a : JSON.stringify(a)).join(' '); for (const line of text.split('\\n')) _fgxResult('{nonce} D ' + line + '\\n'); }};"
        ),
        "console.log = _fgxDiag;".to_string(),
        "console.info = _fgxDiag;".to_string(),
        "console.debug = _fgxDiag;".to_string(),
        "console.warn = _fgxDiag;".to_string(),
        String::new(),
    ]
    .join("\n")
}

pub fn node_emit(nonce: &str, expression: &str) -> String {
    format!("_fgxResult('{nonce} R ' + JSON.stringify({expression}) + '\\n')")
}

/// The Go prelude, as statements inside `main`. `fmt.Println` resolves
/// `os.Stdout` at call time, so rebinding it redirects the body's printing.
pub fn go_prelude() -> &'static str {
    "\t_fgxResult := os.Stdout\n\tos.Stdout = os.Stderr\n"
}

pub fn go_emit(nonce: &str, expression: &str) -> String {
    format!("fmt.Fprintf(_fgxResult, \"{nonce} R %s\\n\", {expression})")
}

/// The JVM prelude, as statements inside `main`.
pub fn jvm_prelude() -> &'static str {
    "        java.io.PrintStream _fgxResult = System.out;\n        System.setOut(System.err);\n"
}

pub fn jvm_emit(nonce: &str, expression: &str) -> String {
    format!("_fgxResult.println(\"{nonce} R \" + {expression}); _fgxResult.flush();")
}

/// The .NET prelude, as statements inside `Main`.
pub fn dotnet_prelude() -> &'static str {
    "        var _fgxResult = System.Console.Out;\n        System.Console.SetOut(System.Console.Error);\n"
}

pub fn dotnet_emit(nonce: &str, expression: &str) -> String {
    format!("_fgxResult.WriteLine(\"{nonce} R \" + {expression}); _fgxResult.Flush();")
}

/// The Julia prelude, at top level before the body. `redirect_stdout` rebinds
/// what `print` and `println` write to, so the body's printing — including at
/// definition time — becomes stderr diagnostics.
pub fn julia_prelude() -> &'static str {
    "const _fgx_result = stdout\nredirect_stdout(stderr)\n"
}

pub fn julia_emit(nonce: &str, expression: &str) -> String {
    format!(
        "print(_fgx_result, \"{nonce} R \", FgxWire.json({expression}), \"\\n\"); flush(_fgx_result)"
    )
}

/// MoonBit has one output channel inside its Wasm sandbox, so the body's
/// `println` lines stay untagged and `split` files them as diagnostics. The
/// nonce reaches the program as the first request line, which keeps the
/// compiled module identical across calls (and cacheable).
pub fn moonbit_emit(nonce_variable: &str, expression: &str) -> String {
    format!("println({nonce_variable} + \" R \" + {expression})")
}

/// One fixture case of a managed capsule run.
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedCase {
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

/// The fixture-run report the newer managed bridges share (`fgx capsule-run
/// --julia` / `--moonbit`): the same fields and JSON shape as the Python
/// bridge's `forgentex.python_run.v1`, under each language's own schema.
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedCapsuleRun {
    pub schema: String,
    /// Display name for the text report, e.g. `julia`.
    pub language: String,
    pub file: String,
    pub capsule: String,
    pub runtime: Option<String>,
    pub entry: Option<String>,
    pub inputs: Vec<(String, String)>,
    pub outputs: Vec<(String, String)>,
    pub deps: Vec<String>,
    pub effects: Vec<String>,
    pub sandbox_decision: String,
    pub body_tokens: usize,
    pub boundary_tokens: usize,
    pub status: String,
    pub cases: Vec<ManagedCase>,
    pub passed: usize,
    pub total: usize,
    pub message: Option<String>,
}

impl ManagedCapsuleRun {
    /// A report for `capsule` before anything has run.
    pub fn new(schema: &str, language: &str, file: &str, capsule: &crate::ast::Capsule) -> Self {
        let bindings = |list: &[crate::ast::CapsuleBinding]| {
            list.iter()
                .map(|binding| (binding.name.clone(), binding.ty.to_agent()))
                .collect()
        };
        ManagedCapsuleRun {
            schema: schema.to_string(),
            language: language.to_string(),
            file: file.to_string(),
            capsule: capsule.name.clone(),
            runtime: None,
            entry: None,
            inputs: bindings(&capsule.inputs),
            outputs: bindings(&capsule.outputs),
            deps: capsule.deps.iter().map(|dep| dep.source.clone()).collect(),
            effects: capsule.effects.clone(),
            sandbox_decision: if crate::sandbox::is_approved(capsule) {
                "allow"
            } else {
                "deny"
            }
            .to_string(),
            body_tokens: crate::tokens::estimate(&capsule.code.join("\n")),
            boundary_tokens: crate::tokens::estimate(&crate::capsule_tokens::boundary_card(
                capsule,
            )),
            status: "ok".to_string(),
            cases: Vec::new(),
            passed: 0,
            total: capsule.fixtures.len(),
            message: None,
        }
    }

    /// Stop with `status` and an explanatory message.
    pub fn stop(mut self, status: &str, message: impl Into<String>) -> Self {
        self.status = status.to_string();
        self.message = Some(message.into());
        self
    }

    /// Record one fixture's tagged result against its expectation.
    pub fn record(&mut self, fixture: &crate::ast::CapsuleFixture, got: String) {
        let expected = fixture.outputs.first();
        let pass = expected.is_some_and(|value| fixture_matches(value, &got));
        if pass {
            self.passed += 1;
        }
        self.cases.push(ManagedCase {
            inputs: fixture
                .inputs
                .iter()
                .map(|value| value.to_literal())
                .collect(),
            output: got,
            expected: expected.map(|value| value.to_literal()).unwrap_or_default(),
            pass,
        });
    }

    /// `mismatch` when every fixture ran but not all matched.
    pub fn finish(mut self) -> Self {
        if self.status == "ok" && self.passed != self.total {
            self.status = "mismatch".to_string();
        }
        self
    }

    pub fn is_success(&self) -> bool {
        self.status == "ok" || self.status == "runtime_unavailable"
    }

    pub fn to_text(&self) -> String {
        let line = |label: &str, bindings: &[(String, String)]| {
            let rendered = bindings
                .iter()
                .map(|(name, ty)| format!("{name}:{ty}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("  {label}: [{rendered}]\n")
        };
        let mut out = format!(
            "Forgentex {} capsule run\nschema={} file={} capsule={} status={}\n",
            self.language, self.schema, self.file, self.capsule, self.status
        );
        if let Some(runtime) = &self.runtime {
            out.push_str(&format!("  runtime: {runtime}\n"));
        }
        if let Some(entry) = &self.entry {
            out.push_str(&format!("  entry: {entry}()\n"));
        }
        out.push_str(&line("inputs", &self.inputs));
        out.push_str(&line("outputs", &self.outputs));
        out.push_str(&format!(
            "  effects=[{}] sandbox={} deps={}\n",
            self.effects.join(", "),
            self.sandbox_decision,
            self.deps.len()
        ));
        out.push_str(&format!(
            "  tokens: body={} boundary={} (agent pays the boundary, not the body)\n",
            self.body_tokens, self.boundary_tokens
        ));
        for case in &self.cases {
            out.push_str(&format!(
                "  {} {}({}) -> {} (expected {})\n",
                if case.pass { "pass" } else { "FAIL" },
                self.entry.as_deref().unwrap_or("f"),
                case.inputs.join(", "),
                case.output,
                case.expected
            ));
        }
        out.push_str(&format!(
            "  {}/{} fixtures passed\n",
            self.passed, self.total
        ));
        if let Some(message) = &self.message {
            out.push_str(&format!("  note: {message}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        use crate::json;
        let cases = self
            .cases
            .iter()
            .map(|case| {
                format!(
                    "{{ \"inputs\": [{}], \"output\": {}, \"expected\": {}, \"pass\": {} }}",
                    case.inputs
                        .iter()
                        .map(|value| json::string(value))
                        .collect::<Vec<_>>()
                        .join(", "),
                    json::string(&case.output),
                    json::string(&case.expected),
                    case.pass
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsule\": {},\n  \"runtime\": {},\n  \"entry\": {},\n  \"effects\": {},\n  \"sandbox_decision\": {},\n  \"deps\": {},\n  \"body_tokens\": {},\n  \"boundary_tokens\": {},\n  \"status\": {},\n  \"cases\": [\n    {}\n  ],\n  \"passed\": {},\n  \"total\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.capsule),
            json::opt_string(self.runtime.as_deref()),
            json::opt_string(self.entry.as_deref()),
            json::string_array(&self.effects),
            json::string(&self.sandbox_decision),
            self.deps.len(),
            self.body_tokens,
            self.boundary_tokens,
            json::string(&self.status),
            cases,
            self.passed,
            self.total,
            json::opt_string(self.message.as_deref()),
        )
    }
}

/// Whether a capsule's JSON result matches a fixture's expected value, with
/// the same tolerances as the Python bridge: integers within 0.5, floats
/// within 1e-9, booleans exactly, strings with their JSON quotes dropped.
pub fn fixture_matches(expected: &crate::ast::CapsuleValue, got: &str) -> bool {
    use crate::ast::CapsuleValue;
    let got = got.trim();
    match expected {
        CapsuleValue::Int(value) => got
            .parse::<f64>()
            .is_ok_and(|parsed| (parsed - *value as f64).abs() < 0.5),
        CapsuleValue::UInt(value) => got.parse::<u64>().is_ok_and(|parsed| parsed == *value),
        CapsuleValue::Float(value) => got
            .parse::<f64>()
            .is_ok_and(|parsed| (parsed - *value).abs() < 1e-9),
        CapsuleValue::Bool(value) => got == if *value { "true" } else { "false" },
        CapsuleValue::Str(value) => got.trim_matches('"') == value,
    }
}

/// A fixture's inputs as the JSON argument array the live path also sends.
pub fn fixture_request(values: &[crate::ast::CapsuleValue]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| value.to_literal())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The entry function a body defines: the one named like the capsule when the
/// body defines it, otherwise the first one `find` reports. Helper functions
/// may then come before the boundary function.
pub fn choose_entry(capsule: &str, defined: &[String]) -> Option<String> {
    defined
        .iter()
        .find(|name| name.as_str() == capsule)
        .or_else(|| defined.first())
        .cloned()
}
