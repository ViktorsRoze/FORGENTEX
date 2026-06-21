//! Effect-gated network execution (v2.15).
//!
//! `core.socket` builtins open real TCP sockets, but only to endpoints on a
//! host:port allowlist; a connection to an unlisted endpoint is denied (not
//! attempted). Every operation — allowed, denied, or errored — is recorded
//! in a deterministic transcript (op, endpoint, bytes out/in, decision), the
//! agent-to-agent transport substrate with runtime-enforced reach.

use crate::ast::Program;
use crate::interp::{run_entry_with_net, NetOp, Value};
use crate::json;

#[derive(Clone, Debug, PartialEq)]
pub struct NetRunReport {
    pub schema: String,
    pub file: String,
    pub entry: String,
    pub allow: Vec<String>,
    pub status: String,
    pub ops: Vec<NetOp>,
    pub bytes_out: usize,
    pub bytes_in: usize,
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
    allow: Vec<String>,
) -> NetRunReport {
    let values = args.into_iter().map(Value::String).collect();
    let (value, ops, error) = run_entry_with_net(program, entry, values, allow.clone());

    let bytes_out = ops
        .iter()
        .filter(|op| op.decision == "allow")
        .map(|op| op.bytes_out)
        .sum();
    let bytes_in = ops
        .iter()
        .filter(|op| op.decision == "allow")
        .map(|op| op.bytes_in)
        .sum();
    // The transcript excludes wall-clock-dependent fields, so it is stable.
    let transcript = ops
        .iter()
        .map(|op| {
            format!(
                "{} {} {} {} {}",
                op.op, op.decision, op.endpoint, op.bytes_out, op.bytes_in
            )
        })
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

    NetRunReport {
        schema: "forgentex.net_run.v1".to_string(),
        file: file.to_string(),
        entry: entry.to_string(),
        allow,
        status: status.to_string(),
        ops,
        bytes_out,
        bytes_in,
        transcript_hash: fnv_hash(&transcript),
        return_value: value.as_ref().map(render_value),
        message: error,
    }
}

impl NetRunReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex effect-gated network run\n");
        out.push_str(&format!(
            "schema={} file={} entry={} status={}\n",
            self.schema, self.file, self.entry, self.status
        ));
        out.push_str(&format!("  allow: [{}]\n", self.allow.join(", ")));
        for op in &self.ops {
            out.push_str(&format!(
                "  {:<8} {:<6} {} (out {} / in {})\n",
                op.op, op.decision, op.endpoint, op.bytes_out, op.bytes_in
            ));
        }
        out.push_str(&format!(
            "  bytes_out={} bytes_in={} transcript_hash={}\n",
            self.bytes_out, self.bytes_in, self.transcript_hash
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
                    "{{ \"op\": {}, \"endpoint\": {}, \"bytes_out\": {}, \"bytes_in\": {}, \"decision\": {} }}",
                    json::string(&op.op),
                    json::string(&op.endpoint),
                    op.bytes_out,
                    op.bytes_in,
                    json::string(&op.decision)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"entry\": {},\n  \"allow\": {},\n  \"status\": {},\n  \"ops\": [\n    {}\n  ],\n  \"bytes_out\": {},\n  \"bytes_in\": {},\n  \"transcript_hash\": {},\n  \"return_value\": {},\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.entry),
            json::string_array(&self.allow),
            json::string(&self.status),
            ops,
            self.bytes_out,
            self.bytes_in,
            json::string(&self.transcript_hash),
            json::opt_string(self.return_value.as_deref()),
            json::opt_string(self.message.as_deref()),
        )
    }
}
