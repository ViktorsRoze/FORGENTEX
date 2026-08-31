//! Debug Adapter Protocol bridge (debugger phase 5).
//!
//! This is deliberately a translation layer, not a second debugger. Every
//! capability it exposes already exists: stepping and evaluation come from the
//! phase-4 session (`interp::debug_session`), and the line numbers DAP speaks
//! in come from the phase-3 source map. The adapter's only real work is
//! converting between a line and the canonical node id the tracer uses.
//!
//! Transport is the same `Content-Length` framing the LSP uses, over stdio.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::interp::{DebugCommand, DebugEvent, DebugOptions, DebugStop};
use crate::json;
use crate::span::SourceMap;

/// Node ids the session should stop at, shared with the running interpreter so
/// a client can add or remove breakpoints while it is paused.
pub type Breakpoints = Arc<Mutex<BTreeSet<String>>>;

/// Serve one DAP session over the given streams. Returns when the client
/// disconnects or the debuggee terminates.
pub fn serve<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> Result<(), String> {
    let mut adapter = Adapter {
        seq: 0,
        breakpoints: Arc::new(Mutex::new(BTreeSet::new())),
        source_map: None,
        pending_lines: Vec::new(),
    };
    adapter.run(reader, writer)
}

struct Adapter {
    seq: i64,
    breakpoints: Breakpoints,
    source_map: Option<SourceMap>,
    /// Lines requested before the program was known, replayed once it is.
    pending_lines: Vec<usize>,
}

/// A parsed inbound DAP message.
struct Request {
    seq: i64,
    command: String,
    body: String,
}

impl Adapter {
    fn run<R: BufRead, W: Write>(&mut self, reader: &mut R, writer: &mut W) -> Result<(), String> {
        // Configuration phase: everything up to the point where the client has
        // both described the program and said it is done configuring.
        let mut launch: Option<LaunchArgs> = None;
        let mut configured = false;

        while !(launch.is_some() && configured) {
            let Some(request) = read_message(reader)? else {
                return Ok(());
            };
            match request.command.as_str() {
                "initialize" => {
                    self.respond(writer, &request, CAPABILITIES)?;
                    // The client may only configure after this event.
                    self.event(writer, "initialized", "{}")?;
                }
                "launch" => {
                    let args = LaunchArgs::parse(&request.body)?;
                    self.load_source_map(&args.program);
                    self.replay_pending_breakpoints();
                    launch = Some(args);
                    self.respond(writer, &request, "{}")?;
                }
                "setBreakpoints" => {
                    let verified = self.set_breakpoints(&request.body);
                    self.respond(writer, &request, &verified)?;
                }
                "configurationDone" => {
                    configured = true;
                    self.respond(writer, &request, "{}")?;
                }
                "disconnect" | "terminate" => {
                    self.respond(writer, &request, "{}")?;
                    return Ok(());
                }
                _ => self.respond(writer, &request, "{}")?,
            }
        }

        let launch = launch.expect("launch present");
        self.debug(reader, writer, &launch)
    }

    /// Run the debuggee, translating DAP requests into session commands while
    /// it is paused.
    fn debug<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
        launch: &LaunchArgs,
    ) -> Result<(), String> {
        let source = std::fs::read_to_string(&launch.program)
            .map_err(|err| format!("failed to read {}: {err}", launch.program))?;
        let program = crate::parser::parse_file(Path::new(&launch.program), &source)?;

        let options = DebugOptions {
            break_nodes: Some(Arc::clone(&self.breakpoints)),
            live: launch.live,
            allow_effects: launch.allow_effects.clone(),
            hosted: !launch.fs_grants.is_empty(),
            fs_grants: launch.fs_grants.clone(),
            ..Default::default()
        };

        // `debug_session` owns the interpreter thread and calls this back on
        // every stop; the borrow checker keeps the two from overlapping, so
        // fields used here are copied out first.
        let breakpoints = Arc::clone(&self.breakpoints);
        let source_map = self.source_map.clone();
        let program_path = launch.program.clone();
        let mut seq = self.seq;
        let mut failed: Option<String> = None;

        let result = crate::interp::debug_session(
            &program,
            &launch.entry,
            &launch.input,
            options,
            |event| {
                let outcome = handle_event(
                    event,
                    reader,
                    writer,
                    &mut seq,
                    &breakpoints,
                    source_map.as_ref(),
                    &program_path,
                );
                match outcome {
                    Ok(command) => command,
                    Err(err) => {
                        // A transport failure must not wedge the interpreter
                        // thread; let the run finish and report afterwards.
                        failed.get_or_insert(err);
                        DebugCommand::Continue
                    }
                }
            },
        );
        self.seq = seq;

        if let Some(err) = failed {
            return Err(err);
        }
        let report = result?;
        self.event(
            writer,
            "output",
            &format!(
                "{{\"category\":\"console\",\"output\":{}}}",
                json::string(&format!("session {}\n", report.status))
            ),
        )?;
        self.event(writer, "terminated", "{}")?;
        self.event(writer, "exited", "{\"exitCode\":0}")?;
        Ok(())
    }

    fn load_source_map(&mut self, program: &str) {
        let path = Path::new(program);
        if let Ok(source) = std::fs::read_to_string(path) {
            self.source_map = Some(crate::parser::parse_file_detailed(path, &source).source_map);
        }
    }

    /// Breakpoints can arrive before the program is known, so the lines are
    /// held and resolved once a source map exists.
    fn replay_pending_breakpoints(&mut self) {
        let lines = std::mem::take(&mut self.pending_lines);
        if lines.is_empty() {
            return;
        }
        let resolved: BTreeSet<String> = lines
            .iter()
            .filter_map(|line| node_for_line(self.source_map.as_ref(), *line))
            .collect();
        if let Ok(mut set) = self.breakpoints.lock() {
            set.extend(resolved);
        }
    }

    /// Replace the breakpoint set and tell the client which lines resolved to a
    /// statement. An unverified breakpoint is reported honestly rather than
    /// silently accepted and never hit.
    fn set_breakpoints(&mut self, body: &str) -> String {
        let lines = number_fields(body, "line");
        let mut verified = Vec::new();
        let mut nodes = BTreeSet::new();
        for line in &lines {
            match node_for_line(self.source_map.as_ref(), *line) {
                Some(node) => {
                    nodes.insert(node);
                    verified.push(format!("{{\"verified\":true,\"line\":{line}}}"));
                }
                None => {
                    if self.source_map.is_none() {
                        // Not yet resolvable; keep it for after `launch`.
                        self.pending_lines.push(*line);
                        verified.push(format!("{{\"verified\":true,\"line\":{line}}}"));
                    } else {
                        verified.push(format!(
                            "{{\"verified\":false,\"line\":{line},\"message\":\"no statement on this line\"}}"
                        ));
                    }
                }
            }
        }
        if self.source_map.is_some() {
            if let Ok(mut set) = self.breakpoints.lock() {
                *set = nodes;
            }
        }
        format!("{{\"breakpoints\":[{}]}}", verified.join(","))
    }

    fn respond<W: Write>(
        &mut self,
        writer: &mut W,
        request: &Request,
        body: &str,
    ) -> Result<(), String> {
        self.seq += 1;
        let message = format!(
            "{{\"seq\":{},\"type\":\"response\",\"request_seq\":{},\"success\":true,\"command\":{},\"body\":{}}}",
            self.seq,
            request.seq,
            json::string(&request.command),
            body
        );
        write_message(writer, &message)
    }

    fn event<W: Write>(&mut self, writer: &mut W, event: &str, body: &str) -> Result<(), String> {
        self.seq += 1;
        let message = format!(
            "{{\"seq\":{},\"type\":\"event\",\"event\":{},\"body\":{}}}",
            self.seq,
            json::string(event),
            body
        );
        write_message(writer, &message)
    }
}

/// Capabilities this adapter actually implements. Nothing is advertised that
/// is not wired up: a client that trusts a false capability produces a worse
/// experience than one that knows the limits.
const CAPABILITIES: &str = concat!(
    "{\"supportsConfigurationDoneRequest\":true,",
    "\"supportsEvaluateForHovers\":true,",
    "\"supportsTerminateRequest\":true,",
    "\"supportsSingleThreadExecutionRequests\":false,",
    "\"supportsConditionalBreakpoints\":false,",
    "\"supportsFunctionBreakpoints\":false,",
    "\"supportsStepBack\":false}"
);

/// The single logical thread a Forgentex run has.
const THREAD_ID: i64 = 1;

struct LaunchArgs {
    program: String,
    entry: String,
    input: String,
    live: bool,
    allow_effects: Option<Vec<String>>,
    fs_grants: Vec<String>,
}

impl LaunchArgs {
    fn parse(body: &str) -> Result<Self, String> {
        let program = string_field(body, "program")
            .ok_or_else(|| "launch requires a \"program\" path".to_string())?;
        let entry = string_field(body, "entry").unwrap_or_else(|| "main".to_string());
        let input = string_field(body, "input").unwrap_or_else(|| "{}".to_string());
        let live = body.contains("\"live\":true");
        let allow_effects = string_field(body, "allowEffects").map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        });
        let fs_grants = string_field(body, "grantFs")
            .map(|value| {
                value
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            program,
            entry,
            input,
            live,
            allow_effects,
            fs_grants,
        })
    }
}

/// Answer DAP requests while the session is paused, returning once the client
/// asks for execution to resume.
#[allow(clippy::too_many_arguments)]
fn handle_event<R: BufRead, W: Write>(
    event: &DebugEvent,
    reader: &mut R,
    writer: &mut W,
    seq: &mut i64,
    breakpoints: &Breakpoints,
    source_map: Option<&SourceMap>,
    program: &str,
) -> Result<DebugCommand, String> {
    let stop = match event {
        DebugEvent::Stopped(stop) => stop,
        // Nothing to ask the client about; the adapter drives these itself.
        DebugEvent::Evaluated { .. } | DebugEvent::Finished(_) => {
            return Ok(DebugCommand::Continue)
        }
    };

    *seq += 1;
    write_message(
        writer,
        &format!(
            "{{\"seq\":{seq},\"type\":\"event\",\"event\":\"stopped\",\"body\":{{\"reason\":\"step\",\"threadId\":{THREAD_ID},\"allThreadsStopped\":true}}}}"
        ),
    )?;

    loop {
        let Some(request) = read_message(reader)? else {
            // The client vanished; let the program finish rather than hang.
            return Ok(DebugCommand::Continue);
        };
        let respond = |writer: &mut W, seq: &mut i64, body: &str| -> Result<(), String> {
            *seq += 1;
            write_message(
                writer,
                &format!(
                    "{{\"seq\":{seq},\"type\":\"response\",\"request_seq\":{},\"success\":true,\"command\":{},\"body\":{body}}}",
                    request.seq,
                    json::string(&request.command)
                ),
            )
        };

        match request.command.as_str() {
            "threads" => respond(
                writer,
                seq,
                &format!("{{\"threads\":[{{\"id\":{THREAD_ID},\"name\":\"forgentex\"}}]}}"),
            )?,
            "stackTrace" => {
                let line = node_line(source_map, &stop.node).unwrap_or(0);
                let frame = format!(
                    "{{\"id\":1,\"name\":{},\"line\":{line},\"column\":1,\"source\":{{\"path\":{}}}}}",
                    json::string(&stop.function),
                    json::string(program)
                );
                respond(
                    writer,
                    seq,
                    &format!("{{\"stackFrames\":[{frame}],\"totalFrames\":1}}"),
                )?
            }
            "scopes" => respond(
                writer,
                seq,
                "{\"scopes\":[{\"name\":\"Locals\",\"variablesReference\":1,\"expensive\":false}]}",
            )?,
            "variables" => respond(writer, seq, &variables_body(stop))?,
            "evaluate" => {
                let expression = string_field(&request.body, "expression").unwrap_or_default();
                return Ok(DebugCommand::Evaluate(expression));
            }
            "setBreakpoints" => {
                // Mid-run breakpoint edits reach the interpreter through the
                // shared set, so they take effect on the next statement.
                let lines = number_fields(&request.body, "line");
                let mut nodes = BTreeSet::new();
                let mut verified = Vec::new();
                for line in &lines {
                    match node_for_line(source_map, *line) {
                        Some(node) => {
                            nodes.insert(node);
                            verified.push(format!("{{\"verified\":true,\"line\":{line}}}"));
                        }
                        None => verified.push(format!(
                            "{{\"verified\":false,\"line\":{line},\"message\":\"no statement on this line\"}}"
                        )),
                    }
                }
                if let Ok(mut set) = breakpoints.lock() {
                    *set = nodes;
                }
                respond(
                    writer,
                    seq,
                    &format!("{{\"breakpoints\":[{}]}}", verified.join(",")),
                )?
            }
            "continue" => {
                respond(writer, seq, "{\"allThreadsContinued\":true}")?;
                return Ok(DebugCommand::Continue);
            }
            "next" => {
                respond(writer, seq, "{}")?;
                return Ok(DebugCommand::Next);
            }
            "stepIn" => {
                respond(writer, seq, "{}")?;
                return Ok(DebugCommand::StepIn);
            }
            "stepOut" => {
                respond(writer, seq, "{}")?;
                return Ok(DebugCommand::StepOut);
            }
            "disconnect" | "terminate" => {
                respond(writer, seq, "{}")?;
                return Ok(DebugCommand::Quit);
            }
            _ => respond(writer, seq, "{}")?,
        }
    }
}

fn variables_body(stop: &DebugStop) -> String {
    let entries = stop
        .locals
        .iter()
        .map(|(name, value)| {
            format!(
                "{{\"name\":{},\"value\":{},\"variablesReference\":0}}",
                json::string(name),
                json::string(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"variables\":[{entries}]}}")
}

/// Sub-expression ids share a line with their statement, so they are excluded:
/// a breakpoint must land on something the tracer actually steps through.
const NON_STATEMENT_SEGMENTS: &[&str] = &[
    ".expr",
    ".cond",
    ".target",
    ".loop",
    ".param",
    ".field",
    ".variant",
    ".scrutinee",
    ".start",
    ".end",
    ".index",
    ".base",
    ".left",
    ".right",
    ".arg",
    ".ret",
];

fn is_statement_node(node: &str) -> bool {
    node.starts_with("@function.")
        && node.contains(".stmt.")
        && !NON_STATEMENT_SEGMENTS
            .iter()
            .any(|segment| node.contains(segment))
}

/// The statement node a client's line refers to. Where several statement nodes
/// share a line, the outermost wins, which is what a gutter click means.
fn node_for_line(map: Option<&SourceMap>, line: usize) -> Option<String> {
    let map = map?;
    map.nodes
        .iter()
        .filter(|entry| entry.span.start_line == line && is_statement_node(&entry.node))
        .map(|entry| entry.node.clone())
        .min_by_key(|node| (node.len(), node.clone()))
}

fn node_line(map: Option<&SourceMap>, node: &str) -> Option<usize> {
    map?.nodes
        .iter()
        .find(|entry| entry.node == node)
        .map(|entry| entry.span.start_line)
}

// ---------------------------------------------------------------- transport

fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Request>, String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("failed to read DAP header: {err}"))?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|err| format!("invalid Content-Length: {err}"))?,
            );
        }
    }
    let Some(length) = content_length else {
        return Err("missing Content-Length header".to_string());
    };
    let mut body = vec![0_u8; length];
    std::io::Read::read_exact(reader, &mut body)
        .map_err(|err| format!("failed to read DAP body: {err}"))?;
    let body = String::from_utf8(body).map_err(|err| err.to_string())?;
    Ok(Some(Request {
        seq: number_field(&body, "seq").unwrap_or(0),
        command: string_field(&body, "command").unwrap_or_default(),
        body,
    }))
}

pub(crate) fn write_message<W: Write>(writer: &mut W, message: &str) -> Result<(), String> {
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        message.len(),
        message
    )
    .and_then(|()| writer.flush())
    .map_err(|err| err.to_string())
}

// ------------------------------------------------------------ tiny json reads
//
// The adapter reads a handful of known fields out of client messages rather
// than carrying a general parser, matching how the LSP bridge reads its own.

pub(crate) fn string_field(source: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let start = source.find(&needle)? + needle.len();
    let rest = source[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

pub(crate) fn number_field(source: &str, field: &str) -> Option<i64> {
    let needle = format!("\"{field}\"");
    let start = source.find(&needle)? + needle.len();
    let rest = source[start..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect();
    digits.parse().ok()
}

/// Every value of a repeated numeric field, in order — used for the line list
/// inside a `setBreakpoints` request.
fn number_fields(source: &str, field: &str) -> Vec<usize> {
    let needle = format!("\"{field}\"");
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(found) = source[cursor..].find(&needle) {
        let start = cursor + found + needle.len();
        cursor = start;
        let rest = source[start..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(value) = digits.parse::<usize>() {
            out.push(value);
        }
    }
    out
}
