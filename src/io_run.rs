//! Effect-gated I/O execution (v2.14).
//!
//! `core.io` builtins perform real filesystem work, but only under granted
//! capabilities: an `fs.read`/`fs.write` operation runs only if the matching
//! grant is present, otherwise it is denied (not silently skipped). Every
//! operation — allowed, denied, or errored — is recorded in a deterministic
//! transcript, so a run is replayable and testable. This moves the
//! effects/sandbox model from review-time to runtime enforcement for I/O.

use crate::ast::Program;
use crate::backend::BackendIr;
use crate::interp::{run_entry_with_io, run_entry_with_io_with_ir, IoOp, Value};
use crate::json;

#[derive(Clone, Debug, PartialEq)]
pub struct IoRunReport {
    pub schema: String,
    pub file: String,
    pub entry: String,
    pub grants: Vec<String>,
    pub status: String,
    pub ops: Vec<IoOp>,
    pub bytes_total: usize,
    pub transcript_hash: String,
    pub return_value: Option<String>,
    pub message: Option<String>,
}

fn fnv_hash(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Int(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Bool(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::Void => "void".to_string(),
        other => format!("{other:?}"),
    }
}

pub fn run(
    file: &str,
    program: &Program,
    entry: &str,
    args: Vec<String>,
    grants: Vec<String>,
) -> IoRunReport {
    run_impl(file, program, None, entry, args, grants)
}

pub fn run_with_ir(
    file: &str,
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    args: Vec<String>,
    grants: Vec<String>,
) -> IoRunReport {
    run_impl(file, program, Some(ir), entry, args, grants)
}

fn run_impl(
    file: &str,
    program: &Program,
    ir: Option<&BackendIr>,
    entry: &str,
    args: Vec<String>,
    grants: Vec<String>,
) -> IoRunReport {
    let values = args.into_iter().map(Value::String).collect();
    let (value, ops, error) = match ir {
        Some(ir) => run_entry_with_io_with_ir(program, ir, entry, values, grants.clone()),
        None => run_entry_with_io(program, entry, values, grants.clone()),
    };

    let bytes_total = ops
        .iter()
        .filter(|op| op.decision == "allow")
        .map(|op| op.bytes)
        .sum();
    let transcript = ops
        .iter()
        .map(|op| format!("{} {} {} {}", op.op, op.decision, op.path, op.bytes))
        .collect::<Vec<_>>()
        .join("\n");
    let denied = ops.iter().any(|op| op.decision == "deny");
    let status = if error.is_some() {
        if denied {
            "denied"
        } else {
            "error"
        }
    } else {
        "ok"
    };

    IoRunReport {
        schema: "forgentex.io_run.v1".to_string(),
        file: file.to_string(),
        entry: entry.to_string(),
        grants,
        status: status.to_string(),
        ops,
        bytes_total,
        transcript_hash: fnv_hash(&transcript),
        return_value: value.as_ref().map(render_value),
        message: error,
    }
}

impl IoRunReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex effect-gated I/O run\n");
        out.push_str(&format!(
            "schema={} file={} entry={} status={}\n",
            self.schema, self.file, self.entry, self.status
        ));
        out.push_str(&format!("  grants: [{}]\n", self.grants.join(", ")));
        for op in &self.ops {
            out.push_str(&format!(
                "  {:<6} {:<6} {} ({} bytes)\n",
                op.op, op.decision, op.path, op.bytes
            ));
        }
        out.push_str(&format!(
            "  bytes_total={} transcript_hash={}\n",
            self.bytes_total, self.transcript_hash
        ));
        if let Some(value) = &self.return_value {
            out.push_str(&format!("  return: {value}\n"));
        }
        if let Some(message) = &self.message {
            out.push_str(&format!("  note: {message}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let ops = self
            .ops
            .iter()
            .map(|op| {
                format!(
                    "{{ \"op\": {}, \"path\": {}, \"bytes\": {}, \"decision\": {} }}",
                    json::string(&op.op),
                    json::string(&op.path),
                    op.bytes,
                    json::string(&op.decision)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"entry\": {},\n  \"grants\": {},\n  \"status\": {},\n  \"ops\": [\n    {}\n  ],\n  \"bytes_total\": {},\n  \"transcript_hash\": {},\n  \"return_value\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.entry),
            json::string_array(&self.grants),
            json::string(&self.status),
            ops,
            self.bytes_total,
            json::string(&self.transcript_hash),
            json::opt_string(self.return_value.as_deref()),
            json::opt_string(self.message.as_deref()),
        )
    }
}
