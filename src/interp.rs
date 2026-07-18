use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ast::{Capsule, Expr, FunctionDecl, MatchArm, Pattern, Program, Stmt, Type};
use crate::backend::{lower_program_to_run_ir, BackendIr, IrFunction, IrInstruction};
use crate::parser::parse_expr;
use crate::stdlib::canonical_simd_intrinsic;
use crate::targets::is_cpu_feature_probe;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Array(Vec<Value>),
    Struct(String, HashMap<String, Value>),
    Variant {
        enum_name: String,
        variant: String,
        value: Option<Box<Value>>,
    },
    /// A growable map (lang.3): insertion-ordered key/value pairs, so runs are
    /// deterministic. Lookups are linear - fine for the v0 interpreter.
    Map(Vec<(Value, Value)>),
    /// A function value (lang.4): a reference to a top-level `fn` by name — a
    /// name rather than a pointer, so it is copyable and serializes trivially.
    Function(String),
    /// A closure (lang.4): parameter names, an expression body, and the free
    /// variables captured **by value** at creation.
    Closure {
        params: Vec<String>,
        body: Box<Expr>,
        captured: HashMap<String, Value>,
    },
    /// A deterministic cooperative task. The callable and its one owned input
    /// are captured at spawn time, but execution is deferred until `task_await`.
    Task {
        state: TaskState,
        callable: Box<Value>,
        input: Box<Value>,
    },
    /// A bounded structured-concurrency scope. Children never escape as
    /// handles: the group is awaited or cancelled as one affine value.
    TaskGroup {
        state: TaskGroupState,
        capacity: usize,
        tasks: Vec<Value>,
    },
    /// A bounded, deterministic FIFO channel. The first concurrency slice is
    /// deliberately non-blocking: send reports `false` at capacity and recv
    /// returns `None` when no value is ready.
    Channel {
        capacity: usize,
        values: Vec<Value>,
        closed: bool,
    },
    Void,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Cancelled,
    Consumed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskGroupState {
    Active,
    Cancelled,
    Consumed,
}

const MAX_TASK_GROUP_TASKS: usize = 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    pub entry: String,
    pub return_value: Value,
    pub bindings: HashMap<String, Value>,
    pub status: String,
}

pub fn run_demo(program: &Program) -> Result<String, String> {
    let ir = lower_program_to_run_ir(program);
    run_demo_impl(program, Some(&ir))
}

pub fn run_demo_with_ir(program: &Program, ir: &BackendIr) -> Result<String, String> {
    run_demo_impl(program, Some(ir))
}

fn run_demo_impl(program: &Program, fir: Option<&BackendIr>) -> Result<String, String> {
    let interpreter = Interpreter {
        program,
        fir,
        io: None,
        net: None,
        capsules: None,
        tracer: None,
    };
    if program.find_function("mix_samples").is_some() {
        let dst = Value::Array(vec![
            Value::Int(1000),
            Value::Int(-20000),
            Value::Int(16000),
            Value::Int(32000),
        ]);
        let a = Value::Array(vec![
            Value::Int(1000),
            Value::Int(-20000),
            Value::Int(16000),
            Value::Int(32000),
        ]);
        let b = Value::Array(vec![
            Value::Int(5000),
            Value::Int(10000),
            Value::Int(-20000),
            Value::Int(1000),
        ]);
        let gain = Value::Float(0.5);
        let (_ret, env) = interpreter.execute_named("mix_samples", vec![dst, a, b, gain])?;
        let final_dst = env
            .get("dst")
            .ok_or_else(|| "mix_samples did not expose dst in demo environment".to_string())?;
        let expected = Value::Array(vec![
            Value::Int(5500),
            Value::Int(0),
            Value::Int(-12000),
            Value::Int(17000),
        ]);
        let status = if *final_dst == expected {
            "pass"
        } else {
            "fail"
        };
        return Ok(format!(
            "run mix_samples: {status}\ninput a: [1000, -20000, 16000, 32000]\ninput b: [5000, 10000, -20000, 1000]\ngain: 0.5\ndst: {}\nexpected: {}",
            value_to_text(final_dst),
            value_to_text(&expected)
        ));
    }
    if program.find_function("clamp_i16").is_some() {
        let (value, _env) = interpreter.execute_named("clamp_i16", vec![Value::Float(40000.0)])?;
        return Ok(format!("run clamp_i16(40000.0): {}", value_to_text(&value)));
    }
    Err("no runnable demo entrypoint found; expected mix_samples or clamp_i16".to_string())
}

pub fn run_entry_json(program: &Program, entry: &str, input: &str) -> Result<String, String> {
    let ir = lower_program_to_run_ir(program);
    run_entry_json_with_ir(program, &ir, entry, input)
}

pub fn run_entry_json_with_ir(
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    input: &str,
) -> Result<String, String> {
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        fir: Some(ir),
        io: None,
        net: None,
        capsules: None,
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(function)?;
    let (return_value, bindings) = interpreter.execute_function(function, args)?;
    let status = input.expectation_status(&return_value, &bindings)?;
    let result = RunResult {
        entry: entry.to_string(),
        return_value,
        bindings,
        status,
    };
    Ok(run_result_to_json(&result))
}

/// Run an entry function under a gated I/O context (v2.14). Returns the
/// entry's value, the recorded I/O transcript, and any execution error
/// (the transcript still captures the denied/failed op).
pub fn run_entry_with_io(
    program: &Program,
    entry: &str,
    args: Vec<Value>,
    grants: Vec<String>,
) -> (Option<Value>, Vec<IoOp>, Option<String>) {
    let ir = lower_program_to_run_ir(program);
    run_entry_with_io_impl(program, Some(&ir), entry, args, grants)
}

pub fn run_entry_with_io_with_ir(
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    args: Vec<Value>,
    grants: Vec<String>,
) -> (Option<Value>, Vec<IoOp>, Option<String>) {
    run_entry_with_io_impl(program, Some(ir), entry, args, grants)
}

fn run_entry_with_io_impl(
    program: &Program,
    fir: Option<&BackendIr>,
    entry: &str,
    args: Vec<Value>,
    grants: Vec<String>,
) -> (Option<Value>, Vec<IoOp>, Option<String>) {
    let interpreter = Interpreter {
        program,
        fir,
        io: Some(IoContext::new(grants)),
        net: None,
        capsules: None,
        tracer: None,
    };
    let result = interpreter.execute_named(entry, args);
    let ops = interpreter
        .io
        .as_ref()
        .map(IoContext::ops)
        .unwrap_or_default();
    match result {
        Ok((value, _)) => (Some(value), ops, None),
        Err(err) => (None, ops, Some(err)),
    }
}

/// Run an entry function under a gated network host (v2.15). Returns the
/// entry's value, the recorded network transcript, and any execution error.
pub fn run_entry_with_net(
    program: &Program,
    entry: &str,
    args: Vec<Value>,
    allow: Vec<String>,
) -> (Option<Value>, Vec<NetOp>, Option<String>) {
    let ir = lower_program_to_run_ir(program);
    run_entry_with_net_impl(program, Some(&ir), entry, args, allow)
}

pub fn run_entry_with_net_with_ir(
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    args: Vec<Value>,
    allow: Vec<String>,
) -> (Option<Value>, Vec<NetOp>, Option<String>) {
    run_entry_with_net_impl(program, Some(ir), entry, args, allow)
}

fn run_entry_with_net_impl(
    program: &Program,
    fir: Option<&BackendIr>,
    entry: &str,
    args: Vec<Value>,
    allow: Vec<String>,
) -> (Option<Value>, Vec<NetOp>, Option<String>) {
    let interpreter = Interpreter {
        program,
        fir,
        io: None,
        net: Some(NetContext::new(allow)),
        capsules: None,
        tracer: None,
    };
    let result = interpreter.execute_named(entry, args);
    let ops = interpreter
        .net
        .as_ref()
        .map(NetContext::ops)
        .unwrap_or_default();
    match result {
        Ok((value, _)) => (Some(value), ops, None),
        Err(err) => (None, ops, Some(err)),
    }
}

pub fn eval_expr_for_test(program: &Program, expr: &Expr) -> Result<Value, String> {
    let interpreter = Interpreter {
        program,
        fir: None,
        io: None,
        net: None,
        capsules: None,
        tracer: None,
    };
    let mut env = HashMap::new();
    interpreter.eval_expr(expr, &mut env)
}

/// Like [`run_entry_json`] but with the live capsule runtime enabled (rt.1), so
/// the program may call capsules with real runtime values. Used by `fgx run
/// --live`. `allow` optionally restricts which capsule effects are permitted
/// (`None` permits any). The audit transcript of capsule calls is appended to
/// the result.
pub fn run_entry_json_live(
    program: &Program,
    entry: &str,
    input: &str,
    allow: Option<Vec<String>>,
) -> Result<String, String> {
    let ir = lower_program_to_run_ir(program);
    run_entry_json_live_impl(program, Some(&ir), entry, input, allow)
}

pub fn run_entry_json_live_with_ir(
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    input: &str,
    allow: Option<Vec<String>>,
) -> Result<String, String> {
    run_entry_json_live_impl(program, Some(ir), entry, input, allow)
}

fn run_entry_json_live_impl(
    program: &Program,
    fir: Option<&BackendIr>,
    entry: &str,
    input: &str,
    allow: Option<Vec<String>>,
) -> Result<String, String> {
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        fir,
        io: None,
        net: None,
        capsules: Some(match allow {
            Some(allow) => CapsuleHost::with_effects(allow),
            None => CapsuleHost::new(),
        }),
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(function)?;
    let (return_value, bindings) = interpreter.execute_function(function, args)?;
    let status = input.expectation_status(&return_value, &bindings)?;
    let result = RunResult {
        entry: entry.to_string(),
        return_value,
        bindings,
        status,
    };
    let calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    Ok(run_result_with_capsules_to_json(&result, &calls))
}

fn run_result_with_capsules_to_json(result: &RunResult, calls: &[CapsuleCall]) -> String {
    let base = run_result_to_json(result);
    let transcript = capsule_calls_json(calls);
    // Splice a capsule_calls array in before the closing brace.
    let trimmed = base.trim_end();
    let body = trimmed.strip_suffix('}').unwrap_or(trimmed).trim_end();
    format!("{body},\n  \"capsule_calls\": [{transcript}]\n}}")
}

fn capsule_calls_json(calls: &[CapsuleCall]) -> String {
    calls
        .iter()
        .map(|call| {
            let native = call
                .native
                .as_ref()
                .map(|receipt| {
                    format!(
                        ", \"native\": {{\"schema\": {}, \"boundary_hash\": {}, \"toolchain\": {}, \"duration_ms\": {}, \"wall_limit_ms\": {}, \"owned_output_bytes\": {}, \"artifact\": {}, \"containment\": {}, \"cpu_limit_ms\": {}, \"memory_limit_bytes\": {}, \"runner_reused\": {}}}",
                        quoted_json(&receipt.schema),
                        quoted_json(&receipt.boundary_hash),
                        quoted_json(&receipt.toolchain),
                        receipt.duration_ms,
                        receipt
                            .wall_limit_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "null".to_string()),
                        receipt
                            .owned_output_bytes
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "null".to_string()),
                        receipt.artifact.as_ref().map(|artifact| format!(
                            "{{\"schema\": {}, \"media_type\": {}, \"bytes\": {}, \"sha256\": {}, \"path\": {}}}",
                            quoted_json(&artifact.schema),
                            quoted_json(&artifact.media_type),
                            artifact.bytes,
                            quoted_json(&artifact.sha256),
                            artifact.path.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string())
                        )).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.containment),
                        receipt.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.runner_reused
                    )
                })
                .unwrap_or_default();
            let native_failure = call
                .native_failure
                .as_ref()
                .map(|receipt| {
                    format!(
                        ", \"native_failure\": {{\"schema\": {}, \"kind\": {}, \"phase\": {}, \"duration_ms\": {}, \"wall_limit_ms\": {}, \"cpu_limit_ms\": {}, \"memory_limit_bytes\": {}, \"limits_applied\": {}, \"containment\": {}}}",
                        quoted_json(&receipt.schema),
                        quoted_json(&receipt.kind),
                        quoted_json(&receipt.phase),
                        receipt.duration_ms,
                        receipt.wall_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.limits_applied,
                        receipt.containment.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string())
                    )
                })
                .unwrap_or_default();
            format!(
                "{{\"capsule\": {}, \"language\": {}, \"decision\": {}, \"redacted_inputs\": [{}]{native}{native_failure}}}",
                quoted_json(&call.capsule),
                quoted_json(&call.language),
                quoted_json(&call.decision),
                call.redacted_inputs.iter().map(|name| quoted_json(name)).collect::<Vec<_>>().join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The hosted program runtime (dom.1): one run with every effect domain
/// available and individually gated - filesystem grants, a network allowlist,
/// and the live capsule runtime (optionally effect-restricted). Domains not
/// granted are present but deny-and-audit rather than absent, so a real program
/// composes fs + net + capsule work under one audited roof and the result JSON
/// carries all three transcripts. Used by `fgx run --live [--grant-fs ...]
/// [--grant-net ...]`.
pub fn run_entry_json_hosted(
    program: &Program,
    entry: &str,
    input: &str,
    capsule_allow: Option<Vec<String>>,
    fs_grants: Vec<String>,
    net_allow: Vec<String>,
) -> Result<String, String> {
    let ir = lower_program_to_run_ir(program);
    run_entry_json_hosted_impl(
        program,
        Some(&ir),
        entry,
        input,
        capsule_allow,
        fs_grants,
        net_allow,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_entry_json_hosted_with_ir(
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    input: &str,
    capsule_allow: Option<Vec<String>>,
    fs_grants: Vec<String>,
    net_allow: Vec<String>,
    warm: bool,
) -> Result<String, String> {
    run_entry_json_hosted_impl(
        program,
        Some(ir),
        entry,
        input,
        capsule_allow,
        fs_grants,
        net_allow,
        warm,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_entry_json_hosted_impl(
    program: &Program,
    fir: Option<&BackendIr>,
    entry: &str,
    input: &str,
    capsule_allow: Option<Vec<String>>,
    fs_grants: Vec<String>,
    net_allow: Vec<String>,
    warm: bool,
) -> Result<String, String> {
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        fir,
        io: Some(IoContext::new(fs_grants)),
        net: Some(NetContext::new(net_allow)),
        capsules: Some(
            match capsule_allow {
                Some(allow) => CapsuleHost::with_effects(allow),
                None => CapsuleHost::new(),
            }
            .with_warm(warm),
        ),
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(function)?;
    let (return_value, bindings) = interpreter.execute_function(function, args)?;
    let status = input.expectation_status(&return_value, &bindings)?;
    let result = RunResult {
        entry: entry.to_string(),
        return_value,
        bindings,
        status,
    };
    let calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    let io_ops = interpreter
        .io
        .as_ref()
        .map(IoContext::ops)
        .unwrap_or_default();
    let net_ops = interpreter
        .net
        .as_ref()
        .map(NetContext::ops)
        .unwrap_or_default();

    let io_json = io_ops
        .iter()
        .map(|op| {
            format!(
                "{{\"op\": {}, \"path\": {}, \"bytes\": {}, \"decision\": {}}}",
                quoted_json(&op.op),
                quoted_json(&op.path),
                op.bytes,
                quoted_json(&op.decision)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let net_json = net_ops
        .iter()
        .map(|op| {
            format!(
                "{{\"op\": {}, \"endpoint\": {}, \"bytes_out\": {}, \"bytes_in\": {}, \"decision\": {}}}",
                quoted_json(&op.op),
                quoted_json(&op.endpoint),
                op.bytes_out,
                op.bytes_in,
                quoted_json(&op.decision)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let base = run_result_to_json(&result);
    let trimmed = base.trim_end();
    let body = trimmed.strip_suffix('}').unwrap_or(trimmed).trim_end();
    Ok(format!(
        "{body},\n  \"capsule_calls\": [{}],\n  \"io_ops\": [{io_json}],\n  \"net_ops\": [{net_json}]\n}}",
        capsule_calls_json(&calls)
    ))
}

/// Like [`run_entry_with_capsules`] but with warm workers (rt.3): one
/// persistent process per capsule, reused across every call in the run.
pub fn run_entry_with_warm_capsules(
    program: &Program,
    entry: &str,
    args: Vec<Value>,
) -> (Option<Value>, Vec<CapsuleCall>, Option<String>) {
    let interpreter = Interpreter {
        program,
        fir: None,
        io: None,
        net: None,
        capsules: Some(CapsuleHost::new().with_warm(true)),
        tracer: None,
    };
    let result = interpreter.execute_named(entry, args);
    let calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    match result {
        Ok((value, _)) => (Some(value), calls, None),
        Err(err) => (None, calls, Some(err)),
    }
}

/// Run an entry function under the live capsule runtime (rt.1). A running
/// program may call capsules with real runtime values; returns the entry's
/// value, the audit transcript of capsule calls, and any execution error.
pub fn run_entry_with_capsules(
    program: &Program,
    entry: &str,
    args: Vec<Value>,
) -> (Option<Value>, Vec<CapsuleCall>, Option<String>) {
    let ir = lower_program_to_run_ir(program);
    let interpreter = Interpreter {
        program,
        fir: Some(&ir),
        io: None,
        net: None,
        capsules: Some(CapsuleHost::new()),
        tracer: None,
    };
    let result = interpreter.execute_named(entry, args);
    let calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    match result {
        Ok((value, _)) => (Some(value), calls, None),
        Err(err) => (None, calls, Some(err)),
    }
}

/// Like [`run_entry_with_capsules`] but the live runtime only permits the listed
/// capsule effects; a capsule declaring any other effect is denied before it
/// runs (`capsule_effect_denied`), and the denial is recorded in the transcript.
pub fn run_entry_with_capsule_effects(
    program: &Program,
    entry: &str,
    args: Vec<Value>,
    allow: Vec<String>,
) -> (Option<Value>, Vec<CapsuleCall>, Option<String>) {
    let ir = lower_program_to_run_ir(program);
    let interpreter = Interpreter {
        program,
        fir: Some(&ir),
        io: None,
        net: None,
        capsules: Some(CapsuleHost::with_effects(allow)),
        tracer: None,
    };
    let result = interpreter.execute_named(entry, args);
    let calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    match result {
        Ok((value, _)) => (Some(value), calls, None),
        Err(err) => (None, calls, Some(err)),
    }
}

/// Invoke one checked capsule through the live runtime without requiring a
/// Forgentex function wrapper. This is the narrow runtime hook used by the
/// deterministic shared-value pipeline: it preserves the normal effect gate
/// and capsule-call transcript while the pipeline owns stage ordering and
/// shared-value write-back.
pub fn invoke_capsule_live(
    program: &Program,
    capsule_name: &str,
    input: Value,
    allow: Option<Vec<String>>,
) -> (Result<Value, String>, Vec<CapsuleCall>) {
    let interpreter = Interpreter {
        program,
        fir: None,
        io: None,
        net: None,
        capsules: Some(match allow {
            Some(allow) => CapsuleHost::with_effects(allow),
            None => CapsuleHost::new(),
        }),
        tracer: None,
    };
    let result = match program
        .capsules()
        .find(|capsule| capsule.name == capsule_name)
    {
        Some(capsule) => {
            // Pipeline-owned artifacts may become immutable slice inputs, but
            // this placeholder source expression can never authorize mutable
            // slice copy-back.
            let source_args = [Expr::Int(0)];
            let mut env = HashMap::new();
            interpreter.call_capsule(capsule, &source_args, vec![input], &mut env)
        }
        None => Err(trap(
            "unknown_capsule",
            format!("unknown capsule '{capsule_name}'"),
        )),
    };
    let calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    (result, calls)
}

struct Interpreter<'a> {
    program: &'a Program,
    fir: Option<&'a BackendIr>,
    io: Option<IoContext>,
    net: Option<NetContext>,
    // The live capsule runtime (rt.1). `None` forbids live capsule calls, so
    // the default deterministic interpreter never spawns a host runtime.
    capsules: Option<CapsuleHost>,
    // The debugger capability: when present, every executed statement is
    // traced against its canonical node id (the same ids semantic patches
    // target). `None` on normal runs — zero tracing cost.
    tracer: Option<std::cell::RefCell<DebugTracer>>,
}

/// A single traced statement execution (`forgentex.debug_trace.v1`).
#[derive(Clone, Debug, PartialEq)]
pub struct DebugStep {
    pub seq: usize,
    pub node: String,
    pub kind: String,
    pub text: String,
    /// Bindings created or changed by this statement, as `(name, text value)`.
    pub changed: Vec<(String, String)>,
}

/// A function enter/exit record in the dynamic call tree.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugCall {
    pub seq: usize,
    pub event: String, // "enter" | "exit"
    pub function: String,
    pub detail: String, // args on enter, return value on exit
}

/// One unwinding frame of a trap post-mortem: the function, its locals at the
/// moment of failure, and the error that passed through it.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugFrame {
    pub function: String,
    pub locals: Vec<(String, String)>,
}

/// The tracing debugger (v0): records steps, the call tree, breakpoint state,
/// and trap post-mortem frames. See docs/debugger_plan.md.
pub struct DebugTracer {
    steps: Vec<DebugStep>,
    calls: Vec<DebugCall>,
    frames: Vec<DebugFrame>,
    breakpoint: Option<(String, Vec<(String, String)>)>,
    break_at: Option<String>,
    max_steps: usize,
    seq: usize,
}

impl DebugTracer {
    fn new(break_at: Option<String>, max_steps: usize) -> Self {
        Self {
            steps: Vec::new(),
            calls: Vec::new(),
            frames: Vec::new(),
            breakpoint: None,
            break_at,
            max_steps,
            seq: 0,
        }
    }

    fn snapshot(env: &HashMap<String, Value>) -> Vec<(String, String)> {
        let mut locals: Vec<(String, String)> = env
            .iter()
            .map(|(name, value)| (name.clone(), value_to_text(value)))
            .collect();
        locals.sort();
        locals
    }
}

fn stmt_kind(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::Let { .. } => "let",
        Stmt::Return(_) => "return",
        Stmt::Require(_) => "require",
        Stmt::If { .. } => "if",
        Stmt::For { .. } => "for",
        Stmt::While { .. } => "while",
        Stmt::Match { .. } => "match",
        Stmt::Assign { .. } => "assign",
        Stmt::Break => "break",
        Stmt::Continue => "continue",
        Stmt::Expr(_) => "expr",
        Stmt::Asm(_) => "asm",
    }
}

/// A one-line readable rendering of a statement for trace steps.
fn stmt_summary(stmt: &Stmt) -> String {
    use crate::format::format_expr;
    match stmt {
        Stmt::Let { name, expr, .. } => format!("let {name} = {}", format_expr(expr)),
        Stmt::Return(Some(expr)) => format!("return {}", format_expr(expr)),
        Stmt::Return(None) => "return".to_string(),
        Stmt::Require(expr) => format!("require {}", format_expr(expr)),
        Stmt::If { cond, .. } => format!("if {}", format_expr(cond)),
        Stmt::For {
            var, start, end, ..
        } => format!(
            "for {var} in {} .. {}",
            format_expr(start),
            format_expr(end)
        ),
        Stmt::While { cond, .. } => format!("while {}", format_expr(cond)),
        Stmt::Match { scrutinee, .. } => format!("match {}", format_expr(scrutinee)),
        Stmt::Assign { target, expr } => {
            format!("{} = {}", format_expr(target), format_expr(expr))
        }
        Stmt::Break => "break".to_string(),
        Stmt::Continue => "continue".to_string(),
        Stmt::Expr(expr) => format_expr(expr),
        Stmt::Asm(_) => "asm".to_string(),
    }
}

/// Options for a debug run. `max_steps` bounds the trace so a runaway loop
/// yields status `step_limit` instead of an unbounded dump.
pub struct DebugOptions {
    pub break_at: Option<String>,
    pub max_steps: usize,
}

impl Default for DebugOptions {
    fn default() -> Self {
        Self {
            break_at: None,
            max_steps: 10_000,
        }
    }
}

/// The result of a debug run (`forgentex.debug_trace.v1`): status, return
/// value, the step trace, the dynamic call tree, breakpoint state, and trap
/// post-mortem frames. Agents consume `to_json()`; humans read `to_text()`.
pub struct DebugReport {
    pub entry: String,
    pub status: String,
    pub return_text: Option<String>,
    pub error: Option<String>,
    pub steps: Vec<DebugStep>,
    pub calls: Vec<DebugCall>,
    pub frames: Vec<DebugFrame>,
    pub breakpoint: Option<(String, Vec<(String, String)>)>,
}

fn kv_object(pairs: &[(String, String)]) -> String {
    let body = pairs
        .iter()
        .map(|(name, value)| format!("{}: {}", quoted_json(name), quoted_json(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{body}}}")
}

impl DebugReport {
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n  \"schema\": \"forgentex.debug_trace.v1\",\n");
        out.push_str(&format!("  \"entry\": {},\n", quoted_json(&self.entry)));
        out.push_str(&format!("  \"status\": {},\n", quoted_json(&self.status)));
        out.push_str(&format!(
            "  \"return\": {},\n",
            self.return_text
                .as_ref()
                .map(|value| quoted_json(value))
                .unwrap_or_else(|| "null".to_string())
        ));
        out.push_str(&format!(
            "  \"error\": {},\n",
            self.error
                .as_ref()
                .map(|value| quoted_json(value))
                .unwrap_or_else(|| "null".to_string())
        ));
        out.push_str("  \"steps\": [");
        for (idx, step) in self.steps.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"seq\": {}, \"node\": {}, \"kind\": {}, \"text\": {}, \"changed\": {}}}",
                step.seq,
                quoted_json(&step.node),
                quoted_json(&step.kind),
                quoted_json(&step.text),
                kv_object(&step.changed)
            ));
        }
        out.push_str("\n  ],\n  \"calls\": [");
        for (idx, call) in self.calls.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"seq\": {}, \"event\": {}, \"function\": {}, \"detail\": {}}}",
                call.seq,
                quoted_json(&call.event),
                quoted_json(&call.function),
                quoted_json(&call.detail)
            ));
        }
        out.push_str("\n  ],\n");
        match &self.breakpoint {
            Some((node, locals)) => out.push_str(&format!(
                "  \"breakpoint\": {{\"node\": {}, \"locals\": {}}},\n",
                quoted_json(node),
                kv_object(locals)
            )),
            None => out.push_str("  \"breakpoint\": null,\n"),
        }
        out.push_str("  \"frames\": [");
        for (idx, frame) in self.frames.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"function\": {}, \"locals\": {}}}",
                quoted_json(&frame.function),
                kv_object(&frame.locals)
            ));
        }
        out.push_str("\n  ]\n}");
        out
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("debug {} status={}\n", self.entry, self.status));
        if let Some(value) = &self.return_text {
            out.push_str(&format!("return {value}\n"));
        }
        if let Some(error) = &self.error {
            out.push_str(&format!("error {error}\n"));
        }
        for step in &self.steps {
            let changed = if step.changed.is_empty() {
                String::new()
            } else {
                format!(
                    "  [{}]",
                    step.changed
                        .iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            out.push_str(&format!(
                "  {:>4}  {}  {}{changed}\n",
                step.seq, step.node, step.text
            ));
        }
        if let Some((node, locals)) = &self.breakpoint {
            out.push_str(&format!("breakpoint at {node}\n"));
            for (name, value) in locals {
                out.push_str(&format!("  {name} = {value}\n"));
            }
        }
        if !self.frames.is_empty() {
            out.push_str("stack (innermost first):\n");
            for frame in &self.frames {
                out.push_str(&format!("  fn {}\n", frame.function));
                for (name, value) in &frame.locals {
                    out.push_str(&format!("    {name} = {value}\n"));
                }
            }
        }
        out
    }
}

/// Run an entry function under the tracing debugger (debugger v0). Executes
/// the deterministic AST path; see docs/debugger_plan.md.
pub fn debug_entry(
    program: &Program,
    entry: &str,
    input: &str,
    options: DebugOptions,
) -> Result<DebugReport, String> {
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        fir: None,
        io: None,
        net: None,
        capsules: None,
        tracer: Some(std::cell::RefCell::new(DebugTracer::new(
            options.break_at,
            options.max_steps,
        ))),
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(function)?;
    let result = interpreter.execute_function(function, args);
    let tracer = interpreter
        .tracer
        .as_ref()
        .expect("debug interpreter has a tracer")
        .borrow();
    let (status, return_text, error) = match &result {
        Ok((value, _)) => ("completed", Some(value_to_text(value)), None),
        Err(err) if err.contains("breakpoint_hit") => ("breakpoint", None, None),
        Err(err) if err.contains("step_limit_exceeded") => ("step_limit", None, Some(err.clone())),
        Err(err) => ("trap", None, Some(err.clone())),
    };
    Ok(DebugReport {
        entry: entry.to_string(),
        status: status.to_string(),
        return_text,
        error,
        steps: tracer.steps.clone(),
        calls: tracer.calls.clone(),
        frames: tracer.frames.clone(),
        breakpoint: tracer.breakpoint.clone(),
    })
}

/// The live capsule runtime capability: permits calling capsules with real
/// runtime values and records an audit transcript of every call. An optional
/// effect allowlist gates calls at the effect level - a capsule that declares an
/// effect the caller did not grant is denied before it runs.
pub struct CapsuleHost {
    // `None` permits any effect; `Some(set)` permits only the listed effects.
    allow: Option<std::collections::BTreeSet<String>>,
    calls: std::cell::RefCell<Vec<CapsuleCall>>,
    // rt.3: reuse one persistent worker per capsule instead of spawning a
    // process per call. Opt-in this slice (docs/rt3_warm_workers_plan.md);
    // workers die with the host (Drop kills the children).
    warm: bool,
    workers: std::cell::RefCell<HashMap<String, crate::capsule_worker::CapsuleWorker>>,
    native_runners: std::cell::RefCell<crate::ffi::NativeRunnerCache>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleCall {
    pub capsule: String,
    pub language: String,
    pub inputs: String,
    pub redacted_inputs: Vec<String>,
    pub output: String,
    pub decision: String,
    pub native: Option<crate::ffi::NativeReceipt>,
    pub native_failure: Option<crate::ffi::NativeFailureReceipt>,
}

struct CapsuleAuditInputs {
    text: String,
    redacted: Vec<String>,
}

fn capsule_audit_inputs(capsule: &Capsule, args: &[Value]) -> CapsuleAuditInputs {
    let policy = crate::sandbox::analyze_capsule(capsule).redacted_inputs;
    let redacted_inputs = capsule
        .inputs
        .iter()
        .filter(|binding| policy.contains(&binding.name))
        .map(|binding| binding.name.clone())
        .collect::<Vec<_>>();
    let rendered = args
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if capsule
                .inputs
                .get(index)
                .is_some_and(|binding| redacted_inputs.contains(&binding.name))
            {
                "{\"redacted\":true}".to_string()
            } else {
                value_to_json(value)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    CapsuleAuditInputs {
        text: format!("[{rendered}]"),
        redacted: redacted_inputs,
    }
}

fn capsule_error_for_caller(error: &str, redacted_inputs: &[String]) -> String {
    if redacted_inputs.is_empty() {
        return error.to_string();
    }
    let stable_kind = error
        .split_once(':')
        .map(|(kind, _)| kind)
        .filter(|kind| {
            kind.starts_with("native_")
                && kind.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')
        })
        .unwrap_or("capsule_error");
    format!("{stable_kind}: diagnostic redacted by input policy")
}

impl Default for CapsuleHost {
    fn default() -> Self {
        Self::new()
    }
}

impl CapsuleHost {
    /// A permissive host: any capsule effect is allowed.
    pub fn new() -> Self {
        Self {
            allow: None,
            calls: std::cell::RefCell::new(Vec::new()),
            warm: false,
            workers: std::cell::RefCell::new(HashMap::new()),
            native_runners: std::cell::RefCell::new(crate::ffi::NativeRunnerCache::default()),
        }
    }

    /// Enable warm workers (rt.3): one persistent process per capsule,
    /// reused across calls in this run.
    pub fn with_warm(mut self, warm: bool) -> Self {
        self.warm = warm;
        self
    }

    /// A host that permits only the listed effects; a capsule declaring any
    /// other effect is denied before it runs.
    pub fn with_effects<I: IntoIterator<Item = String>>(allow: I) -> Self {
        Self {
            allow: Some(allow.into_iter().collect()),
            calls: std::cell::RefCell::new(Vec::new()),
            warm: false,
            workers: std::cell::RefCell::new(HashMap::new()),
            native_runners: std::cell::RefCell::new(crate::ffi::NativeRunnerCache::default()),
        }
    }

    /// One warm call through this host's worker pool: spawn the capsule's
    /// worker on first use (sandbox-gated), then dispatch over the pipe. The
    /// enveloped response line is returned raw; the caller parses it.
    /// One warm call through this host's worker pool, with auto-respawn: if
    /// the worker died (a *transport* failure — broken pipe / EOF, not a
    /// capsule `{"err": …}`), replace it and retry the call once. A capsule's
    /// own error is deterministic and is never retried.
    pub(crate) fn warm_call(&self, capsule: &Capsule, json_args: &str) -> Result<String, String> {
        match self.warm_call_once(capsule, json_args) {
            Ok(response) => Ok(response),
            // Transport errors all say "warm worker …"; spawn/sandbox errors
            // do not (retrying those would fail identically).
            Err(first) if first.contains("warm worker") => {
                self.workers.borrow_mut().remove(&capsule.name);
                self.warm_call_once(capsule, json_args)
                    .map_err(|retry| format!("{retry} (after respawn; first failure: {first})"))
            }
            Err(err) => Err(err),
        }
    }

    /// A single attempt: spawn the capsule's worker on first use
    /// (sandbox-gated), then dispatch over the pipe.
    fn warm_call_once(&self, capsule: &Capsule, json_args: &str) -> Result<String, String> {
        let mut workers = self.workers.borrow_mut();
        if !workers.contains_key(&capsule.name) {
            let worker = crate::capsule_worker::CapsuleWorker::spawn(capsule)?;
            workers.insert(capsule.name.clone(), worker);
        }
        workers
            .get_mut(&capsule.name)
            .expect("worker just ensured")
            .call(json_args)
    }

    /// The capsule's declared effects that the caller did not grant.
    fn denied_effects(&self, effects: &[String]) -> Vec<String> {
        match &self.allow {
            None => Vec::new(),
            Some(allow) => effects
                .iter()
                .filter(|effect| !allow.contains(*effect))
                .cloned()
                .collect(),
        }
    }

    pub fn calls(&self) -> Vec<CapsuleCall> {
        self.calls.borrow().clone()
    }

    fn record(
        &self,
        capsule: &str,
        language: &str,
        audit: &CapsuleAuditInputs,
        output: &str,
        decision: &str,
    ) {
        self.record_with_native(capsule, language, audit, output, decision, None);
    }

    fn record_native(
        &self,
        capsule: &str,
        language: &str,
        audit: &CapsuleAuditInputs,
        output: &str,
        decision: &str,
        receipt: Option<crate::ffi::NativeReceipt>,
    ) {
        self.record_with_native(capsule, language, audit, output, decision, receipt);
    }

    fn record_with_native(
        &self,
        capsule: &str,
        language: &str,
        audit: &CapsuleAuditInputs,
        output: &str,
        decision: &str,
        native: Option<crate::ffi::NativeReceipt>,
    ) {
        let output = if decision == "error" && !audit.redacted.is_empty() {
            "capsule diagnostic redacted by input policy"
        } else {
            output
        };
        self.calls.borrow_mut().push(CapsuleCall {
            capsule: capsule.to_string(),
            language: language.to_string(),
            inputs: audit.text.clone(),
            redacted_inputs: audit.redacted.clone(),
            output: output.to_string(),
            decision: decision.to_string(),
            native,
            native_failure: None,
        });
    }

    fn record_native_failure(
        &self,
        capsule: &str,
        language: &str,
        audit: &CapsuleAuditInputs,
        output: &str,
        receipt: crate::ffi::NativeFailureReceipt,
    ) {
        let output = if audit.redacted.is_empty() {
            output
        } else {
            "capsule diagnostic redacted by input policy"
        };
        self.calls.borrow_mut().push(CapsuleCall {
            capsule: capsule.to_string(),
            language: language.to_string(),
            inputs: audit.text.clone(),
            redacted_inputs: audit.redacted.clone(),
            output: output.to_string(),
            decision: "error".to_string(),
            native: None,
            native_failure: Some(receipt),
        });
    }
}

/// A single real filesystem operation, recorded for the run transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct IoOp {
    pub op: String,
    pub path: String,
    pub bytes: usize,
    pub decision: String,
}

/// Runtime I/O host for `core_io_*` builtins: enforces granted capabilities
/// and records a deterministic transcript of every operation (v2.14).
pub struct IoContext {
    grants: std::collections::BTreeSet<String>,
    ops: std::cell::RefCell<Vec<IoOp>>,
}

impl IoContext {
    pub fn new<I: IntoIterator<Item = String>>(grants: I) -> Self {
        Self {
            grants: grants.into_iter().collect(),
            ops: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn ops(&self) -> Vec<IoOp> {
        self.ops.borrow().clone()
    }

    fn record(&self, op: &str, path: &str, bytes: usize, decision: &str) {
        self.ops.borrow_mut().push(IoOp {
            op: op.to_string(),
            path: path.to_string(),
            bytes,
            decision: decision.to_string(),
        });
    }

    fn read_file(&self, path: &str) -> Result<String, String> {
        if !self.grants.contains("fs.read") {
            self.record("read", path, 0, "deny");
            return Err(trap("io_denied", format!("fs.read not granted for {path}")));
        }
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                self.record("read", path, contents.len(), "allow");
                Ok(contents)
            }
            Err(err) => {
                self.record("read", path, 0, "error");
                Err(trap("io_error", format!("read {path}: {err}")))
            }
        }
    }

    fn write_file(&self, path: &str, contents: &str) -> Result<usize, String> {
        if !self.grants.contains("fs.write") {
            self.record("write", path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("fs.write not granted for {path}"),
            ));
        }
        match std::fs::write(path, contents) {
            Ok(()) => {
                self.record("write", path, contents.len(), "allow");
                Ok(contents.len())
            }
            Err(err) => {
                self.record("write", path, 0, "error");
                Err(trap("io_error", format!("write {path}: {err}")))
            }
        }
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<usize, String> {
        let path = format!("{src} -> {dst}");
        if !self.grants.contains("fs.read") || !self.grants.contains("fs.write") {
            self.record("copy", &path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("copy {path} requires fs.read and fs.write"),
            ));
        }
        match std::fs::copy(src, dst) {
            Ok(bytes) => {
                self.record("copy", &path, bytes as usize, "allow");
                Ok(bytes as usize)
            }
            Err(err) => {
                self.record("copy", &path, 0, "error");
                Err(trap("io_error", format!("copy {path}: {err}")))
            }
        }
    }
}

/// A single network operation, recorded for the run transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct NetOp {
    pub op: String,
    pub endpoint: String,
    pub bytes_out: usize,
    pub bytes_in: usize,
    pub decision: String,
}

/// Runtime network host for `core_socket_*` builtins: enforces a host:port
/// allowlist and records a deterministic transcript of every operation
/// (v2.15). Uses real TCP sockets; the allowlist is the gate.
pub struct NetContext {
    allow: std::collections::BTreeSet<String>,
    ops: std::cell::RefCell<Vec<NetOp>>,
}

impl NetContext {
    pub fn new<I: IntoIterator<Item = String>>(allow: I) -> Self {
        Self {
            allow: allow.into_iter().collect(),
            ops: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn ops(&self) -> Vec<NetOp> {
        self.ops.borrow().clone()
    }

    fn record(&self, op: &str, endpoint: &str, bytes_out: usize, bytes_in: usize, decision: &str) {
        self.ops.borrow_mut().push(NetOp {
            op: op.to_string(),
            endpoint: endpoint.to_string(),
            bytes_out,
            bytes_in,
            decision: decision.to_string(),
        });
    }

    fn allowed(&self, endpoint: &str) -> bool {
        self.allow.contains(endpoint)
    }

    fn connect(&self, op: &str, endpoint: &str) -> Result<std::net::TcpStream, String> {
        use std::time::Duration;
        if !self.allowed(endpoint) {
            self.record(op, endpoint, 0, 0, "deny");
            return Err(trap(
                "net_denied",
                format!("connect to {endpoint} is not in the allowlist"),
            ));
        }
        match std::net::TcpStream::connect(endpoint) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                Ok(stream)
            }
            Err(err) => {
                self.record(op, endpoint, 0, 0, "error");
                Err(trap("net_error", format!("connect {endpoint}: {err}")))
            }
        }
    }

    /// Connect, send the payload, then read the response to completion.
    fn tcp_request(&self, endpoint: &str, payload: &str) -> Result<String, String> {
        use std::io::{Read, Write};
        use std::net::Shutdown;
        let mut stream = self.connect("request", endpoint)?;
        if let Err(err) = stream.write_all(payload.as_bytes()) {
            self.record("request", endpoint, 0, 0, "error");
            return Err(trap("net_error", format!("write {endpoint}: {err}")));
        }
        let _ = stream.shutdown(Shutdown::Write);
        let mut buf = Vec::new();
        if let Err(err) = stream.read_to_end(&mut buf) {
            self.record("request", endpoint, payload.len(), 0, "error");
            return Err(trap("net_error", format!("read {endpoint}: {err}")));
        }
        let response = String::from_utf8_lossy(&buf).to_string();
        self.record("request", endpoint, payload.len(), response.len(), "allow");
        Ok(response)
    }

    /// Connect and send the payload (fire-and-forget).
    fn tcp_send(&self, endpoint: &str, payload: &str) -> Result<usize, String> {
        use std::io::Write;
        let mut stream = self.connect("send", endpoint)?;
        if let Err(err) = stream.write_all(payload.as_bytes()) {
            self.record("send", endpoint, 0, 0, "error");
            return Err(trap("net_error", format!("write {endpoint}: {err}")));
        }
        self.record("send", endpoint, payload.len(), 0, "allow");
        Ok(payload.len())
    }
}

impl<'a> Interpreter<'a> {
    fn require_io(&self) -> Result<&IoContext, String> {
        self.io
            .as_ref()
            .ok_or_else(|| trap("io_unavailable", "core_io builtins require `fgx io-run`"))
    }

    fn require_net(&self) -> Result<&NetContext, String> {
        self.net.as_ref().ok_or_else(|| {
            trap(
                "net_unavailable",
                "core_socket builtins require `fgx net-run`",
            )
        })
    }

    fn execute_named(
        &self,
        name: &str,
        args: Vec<Value>,
    ) -> Result<(Value, HashMap<String, Value>), String> {
        let function = self
            .program
            .find_function(name)
            .ok_or_else(|| format!("unknown function '{name}'"))?;
        self.execute_function(function, args)
    }

    /// Invoke a capsule with live runtime values (rt.1). Gated by the live
    /// capsule runtime capability: without it the call traps, so the default
    /// deterministic interpreter never spawns a host runtime. Marshals the
    /// arguments across the typed boundary as JSON, dispatches to the host
    /// runtime for the capsule's language, records an audit entry, and parses
    /// the result back into a value.
    fn call_capsule(
        &self,
        capsule: &Capsule,
        source_args: &[Expr],
        args: Vec<Value>,
        env: &mut HashMap<String, Value>,
    ) -> Result<Value, String> {
        let host = self.capsules.as_ref().ok_or_else(|| {
            trap(
                "capsule_call_not_permitted",
                format!(
                    "calling capsule '{}' requires the live capsule runtime",
                    capsule.name
                ),
            )
        })?;
        // A JSON argument array is the audit form and the wire form for the
        // stdlib-JSON hosts; type-directed hosts get plain per-argument strings.
        let inputs = format!(
            "[{}]",
            args.iter().map(value_to_json).collect::<Vec<_>>().join(",")
        );
        let audit = capsule_audit_inputs(capsule, &args);
        // Effect-level gate: a capsule may only use effects the caller granted.
        // Enforced before dispatch, so a denied call never reaches a host.
        let denied = host.denied_effects(&capsule.effects);
        if !denied.is_empty() {
            let reason = format!("effects not permitted: {}", denied.join(", "));
            host.record(&capsule.name, &capsule.language, &audit, &reason, "denied");
            return Err(trap(
                "capsule_effect_denied",
                format!(
                    "capsule '{}' requires effects the caller did not grant: {}",
                    capsule.name,
                    denied.join(", ")
                ),
            ));
        }
        // rt.3 warm path: dispatch through the persistent worker instead of
        // spawning a process. The response envelope distinguishes a capsule
        // exception ({"err": …}) from a result ({"ok": …}) so a failing call
        // never kills the worker.
        if host.warm && crate::capsule_worker::supports_language(&capsule.language) {
            match host
                .warm_call(capsule, &inputs)
                .and_then(|raw| parse_worker_envelope(&raw))
            {
                Ok(value) => {
                    host.record(
                        &capsule.name,
                        &capsule.language,
                        &audit,
                        &value_to_json(&value),
                        "allow",
                    );
                    return Ok(retag_struct_output(capsule, value));
                }
                Err(err) => {
                    host.record(&capsule.name, &capsule.language, &audit, &err, "error");
                    let err = capsule_error_for_caller(&err, &audit.redacted);
                    return Err(trap(
                        "capsule_call_failed",
                        format!("capsule '{}': {err}", capsule.name),
                    ));
                }
            }
        }
        if matches!(
            capsule.language.as_str(),
            "c" | "cpp" | "c++" | "rust" | "zig"
        ) {
            return self.call_native_capsule(host, capsule, source_args, args, env, &audit);
        }
        let dispatch = match capsule.language.as_str() {
            "python" => crate::python_run::invoke_live(capsule, &inputs),
            "node" | "typescript" => crate::node_run::invoke_live(capsule, &inputs),
            "go" => crate::go_run::invoke_live(capsule, &inputs),
            "java" => {
                let plain = args.iter().map(value_to_arg).collect::<Vec<_>>();
                crate::jvm_run::invoke_live(capsule, &plain)
            }
            // Same type-directed boundary as the JVM. Wired but unverified on a
            // box without a .NET SDK (see dotnet_run::invoke_live).
            "csharp" => {
                let plain = args.iter().map(value_to_arg).collect::<Vec<_>>();
                crate::dotnet_run::invoke_live(capsule, &plain)
            }
            other => Err(format!(
                "live invocation of '{other}' capsules is not wired yet"
            )),
        };
        match dispatch {
            Ok(output) => {
                host.record(&capsule.name, &capsule.language, &audit, &output, "allow");
                let json = JsonParser::new(&output).parse().map_err(|err| {
                    trap(
                        "capsule_bad_output",
                        format!("capsule '{}' returned invalid JSON: {err}", capsule.name),
                    )
                })?;
                let value = value_from_json(&json)?;
                // A capsule may bundle several values into a struct output; tag
                // the parsed record with the declared output type so it is the
                // program's struct, not the generic JSON object.
                Ok(retag_struct_output(capsule, value))
            }
            Err(err) => {
                host.record(&capsule.name, &capsule.language, &audit, &err, "error");
                let err = capsule_error_for_caller(&err, &audit.redacted);
                Err(trap(
                    "capsule_call_failed",
                    format!("capsule '{}': {err}", capsule.name),
                ))
            }
        }
    }

    fn call_native_capsule(
        &self,
        host: &CapsuleHost,
        capsule: &Capsule,
        source_args: &[Expr],
        args: Vec<Value>,
        env: &mut HashMap<String, Value>,
        audit: &CapsuleAuditInputs,
    ) -> Result<Value, String> {
        validate_native_mutable_targets(capsule, source_args)?;
        let native_args = args
            .iter()
            .zip(&capsule.inputs)
            .map(|(value, binding)| value_to_native_arg(self.program, value, &binding.ty))
            .collect::<Result<Vec<_>, _>>();
        let invocation = match native_args {
            Ok(native_args) => {
                let result = if host.warm {
                    crate::ffi::invoke_live_cached_audited(
                        self.program,
                        capsule,
                        &native_args,
                        &mut host.native_runners.borrow_mut(),
                    )
                } else {
                    crate::ffi::invoke_live_audited(self.program, capsule, &native_args)
                };
                result.map_err(|failure| {
                    let failure = *failure;
                    (failure.message, Some(failure.receipt))
                })
            }
            Err(err) => Err((err, None)),
        };
        match invocation {
            Ok(invocation) => {
                let json = JsonParser::new(&invocation.output).parse().map_err(|err| {
                    trap(
                        "capsule_bad_output",
                        format!("capsule '{}' returned invalid JSON: {err}", capsule.name),
                    )
                })?;
                let value = retag_struct_output(capsule, value_from_json(&json)?);
                // The complete scalar result is decoded before mutable state
                // is replaced. Together with FFI envelope validation, a bad
                // native response is atomic from the interpreter's view.
                apply_native_mutations(capsule, source_args, &invocation.mutations, env)?;
                host.record_native(
                    &capsule.name,
                    &capsule.language,
                    audit,
                    &invocation.output,
                    "allow",
                    invocation.receipt.clone(),
                );
                Ok(value)
            }
            Err((err, receipt)) => {
                if let Some(receipt) = receipt {
                    host.record_native_failure(
                        &capsule.name,
                        &capsule.language,
                        audit,
                        &err,
                        receipt,
                    );
                } else {
                    host.record(&capsule.name, &capsule.language, audit, &err, "error");
                }
                let err = capsule_error_for_caller(&err, &audit.redacted);
                Err(trap(
                    "capsule_call_failed",
                    format!("capsule '{}': {err}", capsule.name),
                ))
            }
        }
    }

    fn execute_function(
        &self,
        function: &FunctionDecl,
        args: Vec<Value>,
    ) -> Result<(Value, HashMap<String, Value>), String> {
        if self.fir.is_some() {
            let ir_function = self.ir_function(function).ok_or_else(|| {
                trap(
                    "fir_missing_function",
                    format!("FIR function missing for '{}'", function.name),
                )
            })?;
            return self.execute_ir_function(function, ir_function, args);
        }
        if function.unsafe_fn || function.has_asm() {
            return Err(format!(
                "function '{}' contains unsafe/assembly code and is review-only in v0",
                function.name
            ));
        }
        if function.params.len() != args.len() {
            return Err(format!(
                "function '{}' expects {} args, got {}",
                function.name,
                function.params.len(),
                args.len()
            ));
        }
        let mut env = HashMap::new();
        for (param, value) in function.params.iter().zip(args) {
            env.insert(param.name.clone(), value);
        }
        if let Some(tracer) = &self.tracer {
            let mut t = tracer.borrow_mut();
            t.seq += 1;
            let seq = t.seq;
            let detail = function
                .params
                .iter()
                .map(|param| {
                    let value = env
                        .get(&param.name)
                        .map(value_to_text)
                        .unwrap_or_else(|| "?".to_string());
                    format!("{}={}", param.name, value)
                })
                .collect::<Vec<_>>()
                .join(", ");
            t.calls.push(DebugCall {
                seq,
                event: "enter".to_string(),
                function: function.name.clone(),
                detail,
            });
        }
        let outcome = self.run_body(&function.body, &mut env, &format!("{}.stmt", function.id));
        match outcome {
            Ok(control) => {
                let value = match control {
                    Control::Return(value) => value,
                    _ => Value::Void,
                };
                if let Some(tracer) = &self.tracer {
                    let mut t = tracer.borrow_mut();
                    t.seq += 1;
                    let seq = t.seq;
                    t.calls.push(DebugCall {
                        seq,
                        event: "exit".to_string(),
                        function: function.name.clone(),
                        detail: value_to_text(&value),
                    });
                }
                Ok((value, env))
            }
            Err(err) => {
                // Trap post-mortem: each unwinding frame snapshots its locals,
                // innermost first — a full stack picture at the failure.
                if let Some(tracer) = &self.tracer {
                    tracer.borrow_mut().frames.push(DebugFrame {
                        function: function.name.clone(),
                        locals: DebugTracer::snapshot(&env),
                    });
                }
                Err(err)
            }
        }
    }

    fn execute_ir_function(
        &self,
        function: &FunctionDecl,
        ir_function: &IrFunction,
        args: Vec<Value>,
    ) -> Result<(Value, HashMap<String, Value>), String> {
        ensure_ir_function_ready(ir_function)?;
        if function.params.len() != args.len() {
            return Err(format!(
                "function '{}' expects {} args, got {}",
                function.name,
                function.params.len(),
                args.len()
            ));
        }
        let mut env = HashMap::new();
        for (param, value) in function.params.iter().zip(args) {
            env.insert(param.name.clone(), value);
        }
        let block = ir_function
            .blocks
            .iter()
            .find(|block| block.label == "entry")
            .ok_or_else(|| trap("fir_missing_block", "FIR function missing entry block"))?;
        let mut cursor = 0;
        let (body, stop) = parse_ir_body(&block.instructions, &mut cursor, &[])?;
        match stop {
            FirParseStop::End => {}
            FirParseStop::Terminator(op) => {
                return Err(trap(
                    "fir_malformed_body",
                    format!("unexpected top-level FIR terminator '{op}'"),
                ));
            }
        }
        if let Control::Return(value) =
            self.run_body(&body, &mut env, &format!("{}.stmt", function.id))?
        {
            return Ok((value, env));
        }
        Ok((Value::Void, env))
    }

    fn ir_function(&self, function: &FunctionDecl) -> Option<&'a IrFunction> {
        self.fir?
            .modules
            .iter()
            .flat_map(|module| module.functions.iter())
            .find(|ir_function| ir_function.id == function.id || ir_function.name == function.name)
    }

    /// Run a block, stopping at the first statement that does not complete
    /// normally and propagating that control (return/break/continue) upward.
    fn run_body(
        &self,
        body: &[Stmt],
        env: &mut HashMap<String, Value>,
        path: &str,
    ) -> Result<Control, String> {
        for (idx, stmt) in body.iter().enumerate() {
            match self.exec_stmt(stmt, env, &format!("{path}.{idx}"))? {
                Control::Continue => {}
                other => return Ok(other),
            }
        }
        Ok(Control::Continue)
    }

    /// Execute one statement, tracing it when the debugger capability is
    /// present. `node` is the statement's canonical id — the same id semantic
    /// patches target — threaded down from the function root.
    fn exec_stmt(
        &self,
        stmt: &Stmt,
        env: &mut HashMap<String, Value>,
        node: &str,
    ) -> Result<Control, String> {
        let Some(tracer) = &self.tracer else {
            return self.exec_stmt_inner(stmt, env, node);
        };
        {
            let mut t = tracer.borrow_mut();
            // A breakpoint stops BEFORE the node executes, with full locals.
            if t.break_at.as_deref() == Some(node) {
                t.breakpoint = Some((node.to_string(), DebugTracer::snapshot(env)));
                return Err(trap("breakpoint_hit", format!("breakpoint at {node}")));
            }
            if t.steps.len() >= t.max_steps {
                return Err(trap(
                    "step_limit_exceeded",
                    format!("debug trace exceeded {} steps", t.max_steps),
                ));
            }
        }
        // Containers are recorded pre-order with no delta (their children
        // report their own changes); leaves are recorded post-execution with
        // the bindings they created or changed.
        let is_container = matches!(
            stmt,
            Stmt::If { .. } | Stmt::For { .. } | Stmt::While { .. } | Stmt::Match { .. }
        );
        if is_container {
            let mut t = tracer.borrow_mut();
            t.seq += 1;
            let seq = t.seq;
            t.steps.push(DebugStep {
                seq,
                node: node.to_string(),
                kind: stmt_kind(stmt).to_string(),
                text: stmt_summary(stmt),
                changed: Vec::new(),
            });
            drop(t);
            return self.exec_stmt_inner(stmt, env, node);
        }
        let before = env.clone();
        let result = self.exec_stmt_inner(stmt, env, node);
        let mut t = tracer.borrow_mut();
        t.seq += 1;
        let seq = t.seq;
        let mut changed: Vec<(String, String)> = env
            .iter()
            .filter(|(name, value)| before.get(*name) != Some(*value))
            .map(|(name, value)| (name.clone(), value_to_text(value)))
            .collect();
        changed.sort();
        t.steps.push(DebugStep {
            seq,
            node: node.to_string(),
            kind: stmt_kind(stmt).to_string(),
            text: stmt_summary(stmt),
            changed,
        });
        result
    }

    fn exec_stmt_inner(
        &self,
        stmt: &Stmt,
        env: &mut HashMap<String, Value>,
        node: &str,
    ) -> Result<Control, String> {
        let _ = node;
        match stmt {
            Stmt::Let { name, expr, .. } => {
                let value = self.eval_expr(expr, env)?;
                env.insert(name.clone(), value);
                Ok(Control::Continue)
            }
            Stmt::Return(Some(expr)) => Ok(Control::Return(self.eval_expr(expr, env)?)),
            Stmt::Return(None) => Ok(Control::Return(Value::Void)),
            Stmt::Break => Ok(Control::Break),
            Stmt::Continue => Ok(Control::LoopContinue),
            Stmt::Match { scrutinee, arms } => {
                let value = self.eval_expr(scrutinee, env)?;
                for (arm_idx, arm) in arms.iter().enumerate() {
                    if let Some(bindings) = match_pattern(&arm.pattern, &value) {
                        // Bind the payload (if any) for the arm only, run the
                        // body in the live env so assignments to outer locals
                        // persist, then restore the shadowed names.
                        let saved: Vec<(String, Option<Value>)> = bindings
                            .iter()
                            .map(|(name, _)| (name.clone(), env.get(name).cloned()))
                            .collect();
                        for (name, bound) in bindings {
                            env.insert(name, bound);
                        }
                        let control =
                            self.run_body(&arm.body, env, &format!("{node}.arm.{arm_idx}"));
                        for (name, previous) in saved {
                            match previous {
                                Some(value) => {
                                    env.insert(name, value);
                                }
                                None => {
                                    env.remove(&name);
                                }
                            }
                        }
                        // Propagate return/break/continue out of the arm.
                        return control;
                    }
                }
                // The type checker proves matches exhaustive; a miss here means
                // an unchecked program reached an uncovered variant.
                Err(trap(
                    "non_exhaustive_match",
                    format!("no match arm covered {value:?}"),
                ))
            }
            Stmt::Require(expr) => {
                if !self.eval_expr(expr, env)?.as_bool()? {
                    return Err(trap("require_failed", format!("require failed: {expr:?}")));
                }
                Ok(Control::Continue)
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let (branch, label) = if self.eval_expr(cond, env)?.as_bool()? {
                    (then_body, "then")
                } else {
                    (else_body, "else")
                };
                // Propagate return/break/continue out of the branch.
                self.run_body(branch, env, &format!("{node}.{label}"))
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                let start = self.eval_expr(start, env)?.as_i64()?;
                let end = self.eval_expr(end, env)?.as_i64()?;
                if end < start {
                    return Err(trap(
                        "invalid_range",
                        format!("loop end {end} is before start {start}"),
                    ));
                }
                for index in start..end {
                    env.insert(var.clone(), Value::Int(index));
                    match self.run_body(body, env, &format!("{node}.body"))? {
                        Control::Return(value) => return Ok(Control::Return(value)),
                        Control::Break => break,
                        Control::Continue | Control::LoopContinue => {}
                    }
                }
                Ok(Control::Continue)
            }
            Stmt::While { cond, body } => {
                // A bounded iteration guard keeps the interpreter from hanging
                // on a loop that never terminates (e.g. `while true` with no
                // break).
                const MAX_ITERATIONS: u64 = 10_000_000;
                let mut iterations = 0u64;
                while self.eval_expr(cond, env)?.as_bool()? {
                    match self.run_body(body, env, &format!("{node}.body"))? {
                        Control::Return(value) => return Ok(Control::Return(value)),
                        Control::Break => break,
                        Control::Continue | Control::LoopContinue => {}
                    }
                    iterations += 1;
                    if iterations >= MAX_ITERATIONS {
                        return Err(trap(
                            "loop_limit_exceeded",
                            format!("while loop exceeded {MAX_ITERATIONS} iterations"),
                        ));
                    }
                }
                Ok(Control::Continue)
            }
            Stmt::Assign { target, expr } => {
                let value = self.eval_expr(expr, env)?;
                self.assign(target, value, env)?;
                Ok(Control::Continue)
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr, env)?;
                Ok(Control::Continue)
            }
            Stmt::Asm(_) => Err(trap(
                "review_only_assembly",
                "assembly is review-only and cannot run in v1",
            )),
        }
    }

    fn assign(
        &self,
        target: &Expr,
        value: Value,
        env: &mut HashMap<String, Value>,
    ) -> Result<(), String> {
        match target {
            Expr::Var(name) => {
                env.insert(name.clone(), value);
                Ok(())
            }
            Expr::Index { base, index } => {
                let index = checked_index(self.eval_expr(index, env)?.as_i64()?)?;
                let Expr::Var(name) = base.as_ref() else {
                    return Err(trap(
                        "unsupported_assignment",
                        "only direct slice index assignment is supported",
                    ));
                };
                let array = env.get_mut(name).ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown array target '{name}'"))
                })?;
                let Value::Array(values) = array else {
                    return Err(trap(
                        "type_error",
                        format!("target '{name}' is not an array"),
                    ));
                };
                if index >= values.len() {
                    return Err(trap(
                        "index_out_of_bounds",
                        format!(
                            "index {index} out of bounds for '{name}' length {}",
                            values.len()
                        ),
                    ));
                }
                values[index] = value;
                Ok(())
            }
            _ => Err(trap(
                "unsupported_assignment",
                "unsupported assignment target in interpreter",
            )),
        }
    }

    fn eval_expr(&self, expr: &Expr, env: &mut HashMap<String, Value>) -> Result<Value, String> {
        match expr {
            Expr::Int(value) => Ok(Value::Int(*value)),
            Expr::Float(value) => Ok(Value::Float(*value)),
            Expr::Bool(value) => Ok(Value::Bool(*value)),
            Expr::String(value) => Ok(Value::String(value.clone())),
            Expr::TraitConst {
                trait_name,
                name,
                for_type,
            } => {
                let value = self
                    .program
                    .trait_impls()
                    .find(|implementation| {
                        implementation.trait_name == *trait_name
                            && impl_type_pattern_matches(
                                &implementation.for_type,
                                for_type,
                                &implementation.type_params,
                            )
                    })
                    .and_then(|implementation| {
                        implementation
                            .constants
                            .iter()
                            .find(|constant| constant.name == *name)
                    })
                    .map(|constant| constant.value.clone())
                    .ok_or_else(|| {
                        trap(
                            "trait_const_impl_missing",
                            format!(
                                "trait constant '{trait_name}::{name}' is not implemented for '{}'",
                                for_type.to_human()
                            ),
                        )
                    })?;
                self.eval_expr(&value, env)
            }
            Expr::Var(name) if name.contains("::") => {
                // An enum variant value, e.g. `Color::Red`.
                let (enum_name, variant) = name.split_once("::").unwrap();
                Ok(Value::Variant {
                    enum_name: enum_name.to_string(),
                    variant: variant.to_string(),
                    value: None,
                })
            }
            Expr::Var(name) => match env.get(name) {
                Some(value) => Ok(value.clone()),
                // A bare reference to a top-level function is a function value
                // (lang.4). Locals shadow, so this only fires for non-locals.
                None if self.program.find_function(name).is_some() => {
                    Ok(Value::Function(name.clone()))
                }
                None => Err(trap("unknown_symbol", format!("unknown symbol '{name}'"))),
            },
            Expr::Lambda { params, body } => {
                // A closure captures its free variables BY VALUE at creation:
                // later mutation of the originals is invisible to it (lang.4).
                let mut free = std::collections::BTreeSet::new();
                collect_var_names(body, &mut free);
                for param in params {
                    free.remove(&param.name);
                }
                let mut captured = HashMap::new();
                for name in free {
                    if let Some(value) = env.get(&name) {
                        captured.insert(name, value.clone());
                    }
                }
                Ok(Value::Closure {
                    params: params.iter().map(|param| param.name.clone()).collect(),
                    body: body.clone(),
                    captured,
                })
            }
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr, env)?;
                match op.as_str() {
                    "-" => match value {
                        Value::Int(value) => Ok(Value::Int(-value)),
                        Value::Float(value) => Ok(Value::Float(-value)),
                        _ => Err(trap("type_error", "unary minus requires numeric value")),
                    },
                    "!" => Ok(Value::Bool(!value.as_bool()?)),
                    _ => Err(trap(
                        "unknown_operator",
                        format!("unknown unary operator '{op}'"),
                    )),
                }
            }
            Expr::Binary { op, left, right } => {
                let left = self.eval_expr(left, env)?;
                let right = self.eval_expr(right, env)?;
                eval_binary(op, left, right)
            }
            Expr::Call { name, args } => {
                if is_cpu_feature_probe(name) || name == "aligned" {
                    return Ok(Value::Bool(false));
                }
                if let Some(value) = self.eval_runtime_intrinsic(name, args, env)? {
                    return Ok(value);
                }
                if let Some((trait_name, method_name)) = name.split_once("::") {
                    if self.program.traits().any(|decl| decl.name == trait_name) {
                        let values = args
                            .iter()
                            .map(|arg| self.eval_expr(arg, env))
                            .collect::<Result<Vec<_>, _>>()?;
                        return self.call_trait_method(trait_name, method_name, args, values, env);
                    }
                }
                // A payload-carrying enum variant, e.g. `Shape::Circle(5)`.
                if let Some((enum_name, variant)) = name.split_once("::") {
                    let payload = match args.first() {
                        Some(arg) => Some(Box::new(self.eval_expr(arg, env)?)),
                        None => None,
                    };
                    return Ok(Value::Variant {
                        enum_name: enum_name.to_string(),
                        variant: variant.to_string(),
                        value: payload,
                    });
                }
                let values = args
                    .iter()
                    .map(|arg| self.eval_expr(arg, env))
                    .collect::<Result<Vec<_>, _>>()?;
                // Indirect call through a local (lang.4): `f(x)` where `f` is a
                // function value or closure. Locals shadow capsules and
                // program functions.
                if let Some(bound) = env.get(name) {
                    match bound.clone() {
                        Value::Function(target) => {
                            let Some(function) = self.program.find_function(&target) else {
                                return Err(trap(
                                    "unknown_function",
                                    format!("function value names unknown function '{target}'"),
                                ));
                            };
                            let (value, callee_env) = self.execute_function(function, values)?;
                            propagate_mutable_args(function, args, &callee_env, env);
                            return Ok(value);
                        }
                        Value::Closure {
                            params,
                            body,
                            captured,
                        } => {
                            if params.len() != values.len() {
                                return Err(trap(
                                    "call_arity",
                                    format!(
                                        "closure '{name}' expects {} argument(s), got {}",
                                        params.len(),
                                        values.len()
                                    ),
                                ));
                            }
                            // The closure runs against its captured environment
                            // plus its parameters — not the caller's scope.
                            let mut call_env = captured;
                            for (param, value) in params.iter().zip(values) {
                                call_env.insert(param.clone(), value);
                            }
                            return self.eval_expr(&body, &mut call_env);
                        }
                        _ => {
                            return Err(trap(
                                "not_callable",
                                format!("'{name}' is not a function value"),
                            ));
                        }
                    }
                }
                if let Some(capsule) = self.program.capsules().find(|c| c.name == *name) {
                    return self.call_capsule(capsule, args, values, env);
                }
                if self.program.structs().any(|decl| decl.name == *name) {
                    let decl = self
                        .program
                        .structs()
                        .find(|decl| decl.name == *name)
                        .unwrap();
                    let mut fields = HashMap::new();
                    for (field, value) in decl.fields.iter().zip(values) {
                        fields.insert(field.name.clone(), value);
                    }
                    return Ok(Value::Struct(name.clone(), fields));
                }
                let Some(function) = self.program.find_function(name) else {
                    return Err(trap(
                        "unknown_function",
                        format!("unknown function '{name}'"),
                    ));
                };
                let (value, callee_env) = self.execute_function(function, values)?;
                propagate_mutable_args(function, args, &callee_env, env);
                Ok(value)
            }
            Expr::Cast { ty, expr } => {
                let value = self.eval_expr(expr, env)?;
                cast_value(ty, value)
            }
            Expr::Field { base, field } => {
                let value = self.eval_expr(base, env)?;
                match value {
                    Value::Array(values) if field == "len" => Ok(Value::Int(values.len() as i64)),
                    Value::String(text) if field == "len" => Ok(Value::Int(text.len() as i64)),
                    Value::Map(entries) if field == "len" => Ok(Value::Int(entries.len() as i64)),
                    Value::Struct(_, fields) => fields.get(field).cloned().ok_or_else(|| {
                        trap("unknown_field", format!("unknown struct field '{field}'"))
                    }),
                    _ => Err(trap(
                        "unknown_field",
                        format!("value has no field '{field}'"),
                    )),
                }
            }
            Expr::Index { base, index } => {
                let value = self.eval_expr(base, env)?;
                let index = checked_index(self.eval_expr(index, env)?.as_i64()?)?;
                match value {
                    Value::Array(values) => values.get(index).cloned().ok_or_else(|| {
                        trap(
                            "index_out_of_bounds",
                            format!("index {index} out of bounds for length {}", values.len()),
                        )
                    }),
                    _ => Err(trap("type_error", "indexed expression is not an array")),
                }
            }
        }
    }

    fn call_trait_method(
        &self,
        trait_name: &str,
        method_name: &str,
        source_args: &[Expr],
        values: Vec<Value>,
        caller_env: &mut HashMap<String, Value>,
    ) -> Result<Value, String> {
        let Some(trait_decl) = self.program.traits().find(|decl| decl.name == trait_name) else {
            return Err(trap(
                "unknown_trait",
                format!("unknown trait '{trait_name}'"),
            ));
        };
        let Some(method) = trait_decl
            .methods
            .iter()
            .find(|method| method.name == method_name)
        else {
            return Err(trap(
                "unknown_trait_method_call",
                format!("trait '{trait_name}' has no required method '{method_name}'"),
            ));
        };

        let candidates = self
            .program
            .trait_impls()
            .filter(|implementation| implementation.trait_name == trait_name)
            .filter_map(|implementation| {
                let mapping = implementation
                    .methods
                    .iter()
                    .find(|mapping| mapping.method == method_name);
                let target = mapping
                    .map(|mapping| mapping.function.as_str())
                    .or(method.default_function.as_deref())?;
                let function = self.program.find_function(target)?;
                let signature_matches = method.params.len() == values.len()
                    && method.params.iter().zip(&values).all(|(param, value)| {
                        let ty = substitute_trait_contract_type(&param.ty, implementation);
                        value_matches_type_pattern(value, &ty, &implementation.type_params)
                    });
                signature_matches.then_some(function)
            })
            .collect::<Vec<_>>();

        let function = match candidates.as_slice() {
            [] => {
                return Err(trap(
                    "trait_method_impl_missing",
                    format!(
                        "no runtime implementation matches trait method '{trait_name}::{method_name}'"
                    ),
                ));
            }
            [function] => *function,
            _ => {
                return Err(trap(
                    "trait_method_dispatch_ambiguous",
                    format!(
                        "multiple runtime implementations match trait method '{trait_name}::{method_name}'"
                    ),
                ));
            }
        };
        let (value, callee_env) = self.execute_function(function, values)?;
        propagate_mutable_args(function, source_args, &callee_env, caller_env);
        Ok(value)
    }

    fn eval_runtime_intrinsic(
        &self,
        name: &str,
        args: &[Expr],
        env: &mut HashMap<String, Value>,
    ) -> Result<Option<Value>, String> {
        match name {
            "task_spawn" => {
                expect_arity(name, args, 2)?;
                let callable = self.eval_expr(&args[0], env)?;
                if !matches!(callable, Value::Function(_) | Value::Closure { .. }) {
                    return Err(trap(
                        "task_not_callable",
                        "task_spawn expects a one-argument function value",
                    ));
                }
                let input = self.eval_expr(&args[1], env)?;
                Ok(Some(Value::Task {
                    state: TaskState::Pending,
                    callable: Box::new(callable),
                    input: Box::new(input),
                }))
            }
            "task_await" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_await expects a task-handle variable",
                    ));
                };
                let task = env.get(var).cloned().ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown task handle '{var}'"))
                })?;
                let Value::Task {
                    state,
                    callable,
                    input,
                } = task
                else {
                    return Err(trap(
                        "type_error",
                        format!("task_await target '{var}' is not a task"),
                    ));
                };
                match state {
                    TaskState::Cancelled => Err(trap(
                        "task_cancelled",
                        format!("task '{var}' was cancelled before it ran"),
                    )),
                    TaskState::Consumed => Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was already awaited or cancelled"),
                    )),
                    TaskState::Pending => {
                        env.insert(
                            var.clone(),
                            Value::Task {
                                state: TaskState::Consumed,
                                callable: callable.clone(),
                                input: input.clone(),
                            },
                        );
                        Ok(Some(self.execute_task_callable(*callable, *input)?))
                    }
                }
            }
            "task_cancel" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_cancel expects a task-handle variable",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Task { state, .. }) if *state == TaskState::Pending => {
                        *state = TaskState::Cancelled;
                        Ok(Some(Value::Bool(true)))
                    }
                    Some(Value::Task { .. }) => Ok(Some(Value::Bool(false))),
                    Some(_) => Err(trap(
                        "type_error",
                        format!("task_cancel target '{var}' is not a task"),
                    )),
                    None => Err(trap(
                        "unknown_symbol",
                        format!("unknown task handle '{var}'"),
                    )),
                }
            }
            "task_group_new" => {
                expect_arity(name, args, 1)?;
                let capacity = self.eval_expr(&args[0], env)?.as_i64()?;
                let capacity = usize::try_from(capacity)
                    .ok()
                    .filter(|value| (1..=MAX_TASK_GROUP_TASKS).contains(value))
                    .ok_or_else(|| {
                        trap(
                            "task_group_capacity",
                            format!(
                                "task group capacity must be between 1 and {MAX_TASK_GROUP_TASKS}"
                            ),
                        )
                    })?;
                Ok(Some(Value::TaskGroup {
                    state: TaskGroupState::Active,
                    capacity,
                    tasks: Vec::new(),
                }))
            }
            "task_group_spawn" => {
                expect_arity(name, args, 3)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_spawn expects a task-group variable",
                    ));
                };
                let full = match env.get(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        capacity,
                        tasks,
                    }) => tasks.len() >= *capacity,
                    Some(Value::TaskGroup { .. }) => {
                        return Err(trap(
                            "task_group_closed",
                            format!("task group '{var}' was already awaited or cancelled"),
                        ))
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_group_spawn target '{var}' is not a task group"),
                        ))
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task group '{var}'"),
                        ))
                    }
                };
                let callable = self.eval_expr(&args[1], env)?;
                if !matches!(callable, Value::Function(_) | Value::Closure { .. }) {
                    return Err(trap(
                        "task_not_callable",
                        "task_group_spawn expects a one-argument function value",
                    ));
                }
                let input = self.eval_expr(&args[2], env)?;
                if full {
                    return Ok(Some(Value::Bool(false)));
                }
                let Some(Value::TaskGroup { tasks, .. }) = env.get_mut(var) else {
                    unreachable!("task group was validated before evaluating child values")
                };
                tasks.push(Value::Task {
                    state: TaskState::Pending,
                    callable: Box::new(callable),
                    input: Box::new(input),
                });
                Ok(Some(Value::Bool(true)))
            }
            "task_group_await_all" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_await_all expects a task-group variable",
                    ));
                };
                let group = env
                    .get(var)
                    .cloned()
                    .ok_or_else(|| trap("unknown_symbol", format!("unknown task group '{var}'")))?;
                let Value::TaskGroup { state, tasks, .. } = group else {
                    return Err(trap(
                        "type_error",
                        format!("task_group_await_all target '{var}' is not a task group"),
                    ));
                };
                if state != TaskGroupState::Active {
                    return Err(trap(
                        "task_group_closed",
                        format!("task group '{var}' was already awaited or cancelled"),
                    ));
                }
                if let Some(Value::TaskGroup { state, .. }) = env.get_mut(var) {
                    *state = TaskGroupState::Consumed;
                }
                let mut results = Vec::with_capacity(tasks.len());
                for task in tasks {
                    let Value::Task {
                        state: TaskState::Pending,
                        callable,
                        input,
                    } = task
                    else {
                        return Err(trap(
                            "task_group_child_state",
                            "task group contained a non-pending child",
                        ));
                    };
                    results.push(self.execute_task_callable(*callable, *input)?);
                }
                Ok(Some(Value::Array(results)))
            }
            "task_group_cancel" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_cancel expects a task-group variable",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::TaskGroup { state, tasks, .. })
                        if *state == TaskGroupState::Active =>
                    {
                        let mut cancelled = 0usize;
                        for task in tasks {
                            if let Value::Task { state, .. } = task {
                                if *state == TaskState::Pending {
                                    *state = TaskState::Cancelled;
                                    cancelled += 1;
                                }
                            }
                        }
                        *state = TaskGroupState::Cancelled;
                        Ok(Some(Value::Int(cancelled as i64)))
                    }
                    Some(Value::TaskGroup { .. }) => Ok(Some(Value::Int(0))),
                    Some(_) => Err(trap(
                        "type_error",
                        format!("task_group_cancel target '{var}' is not a task group"),
                    )),
                    None => Err(trap(
                        "unknown_symbol",
                        format!("unknown task group '{var}'"),
                    )),
                }
            }
            "channel_new" => {
                expect_arity(name, args, 1)?;
                let capacity = self.eval_expr(&args[0], env)?.as_i64()?;
                let capacity = usize::try_from(capacity)
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        trap(
                            "channel_capacity",
                            "channel capacity must be greater than zero",
                        )
                    })?;
                Ok(Some(Value::Channel {
                    capacity,
                    values: Vec::new(),
                    closed: false,
                }))
            }
            "channel_send" => {
                expect_arity(name, args, 2)?;
                let value = self.eval_expr(&args[1], env)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "channel_target",
                        "channel_send expects a channel variable",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Channel {
                        capacity,
                        values,
                        closed,
                    }) => {
                        if *closed || values.len() >= *capacity {
                            Ok(Some(Value::Bool(false)))
                        } else {
                            values.push(value);
                            Ok(Some(Value::Bool(true)))
                        }
                    }
                    Some(_) => Err(trap(
                        "type_error",
                        format!("channel_send target '{var}' is not a channel"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown channel '{var}'"))),
                }
            }
            "channel_recv" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "channel_target",
                        "channel_recv expects a channel variable",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Channel { values, .. }) if !values.is_empty() => {
                        Ok(Some(Value::Variant {
                            enum_name: "Option".to_string(),
                            variant: "Some".to_string(),
                            value: Some(Box::new(values.remove(0))),
                        }))
                    }
                    Some(Value::Channel { .. }) => Ok(Some(Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "None".to_string(),
                        value: None,
                    })),
                    Some(_) => Err(trap(
                        "type_error",
                        format!("channel_recv target '{var}' is not a channel"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown channel '{var}'"))),
                }
            }
            "channel_close" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "channel_target",
                        "channel_close expects a channel variable",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Channel { closed, .. }) => {
                        *closed = true;
                        Ok(Some(Value::Void))
                    }
                    Some(_) => Err(trap(
                        "type_error",
                        format!("channel_close target '{var}' is not a channel"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown channel '{var}'"))),
                }
            }
            "Some" | "core_option_some_u8" => {
                expect_arity(name, args, 1)?;
                Ok(Some(Value::Variant {
                    enum_name: "Option".to_string(),
                    variant: "Some".to_string(),
                    value: Some(Box::new(self.eval_expr(&args[0], env)?)),
                }))
            }
            "None" | "core_option_none_u8" => {
                expect_arity(name, args, 0)?;
                Ok(Some(Value::Variant {
                    enum_name: "Option".to_string(),
                    variant: "None".to_string(),
                    value: None,
                }))
            }
            // Growable Vec<T> (lang.3), backed by the array value. `push`/`pop`
            // mutate the vec in place, so they take the vec as a variable.
            "vec_new" => {
                expect_arity(name, args, 0)?;
                Ok(Some(Value::Array(Vec::new())))
            }
            "push" => {
                expect_arity(name, args, 2)?;
                let item = self.eval_expr(&args[1], env)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "push_target",
                        "push expects a variable as its first argument",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Array(items)) => {
                        items.push(item);
                        Ok(Some(Value::Void))
                    }
                    Some(_) => Err(trap(
                        "type_error",
                        format!("push target '{var}' is not a vec"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown symbol '{var}'"))),
                }
            }
            "pop" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap("pop_target", "pop expects a variable as its argument"));
                };
                match env.get_mut(var) {
                    Some(Value::Array(items)) => Ok(Some(match items.pop() {
                        Some(value) => Value::Variant {
                            enum_name: "Option".to_string(),
                            variant: "Some".to_string(),
                            value: Some(Box::new(value)),
                        },
                        None => Value::Variant {
                            enum_name: "Option".to_string(),
                            variant: "None".to_string(),
                            value: None,
                        },
                    })),
                    Some(_) => Err(trap(
                        "type_error",
                        format!("pop target '{var}' is not a vec"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown symbol '{var}'"))),
                }
            }
            // String library (lang.3): concatenate, measure, and stringify.
            "str_concat" => {
                expect_arity(name, args, 2)?;
                let left = self.eval_expr(&args[0], env)?.as_str_value()?;
                let right = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(Value::String(format!("{left}{right}"))))
            }
            "str_len" => {
                expect_arity(name, args, 1)?;
                let text = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::Int(text.len() as i64)))
            }
            "int_to_str" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()?;
                Ok(Some(Value::String(value.to_string())))
            }
            "str_slice" => {
                expect_arity(name, args, 3)?;
                let text = self.eval_expr(&args[0], env)?.as_str_value()?;
                let start = checked_index(self.eval_expr(&args[1], env)?.as_i64()?)?;
                let end = checked_index(self.eval_expr(&args[2], env)?.as_i64()?)?;
                match text.get(start..end) {
                    Some(slice) => Ok(Some(Value::String(slice.to_string()))),
                    None => Err(trap(
                        "slice_out_of_bounds",
                        format!(
                            "byte range {start}..{end} is invalid for string of length {}",
                            text.len()
                        ),
                    )),
                }
            }
            "str_find" => {
                expect_arity(name, args, 2)?;
                let text = self.eval_expr(&args[0], env)?.as_str_value()?;
                let needle = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(match text.find(&needle) {
                    Some(index) => Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "Some".to_string(),
                        value: Some(Box::new(Value::Int(index as i64))),
                    },
                    None => Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "None".to_string(),
                        value: None,
                    },
                }))
            }
            "str_contains" => {
                expect_arity(name, args, 2)?;
                let text = self.eval_expr(&args[0], env)?.as_str_value()?;
                let needle = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(Value::Bool(text.contains(&needle))))
            }
            "str_split" => {
                expect_arity(name, args, 2)?;
                let text = self.eval_expr(&args[0], env)?.as_str_value()?;
                let sep = self.eval_expr(&args[1], env)?.as_str_value()?;
                if sep.is_empty() {
                    return Err(trap(
                        "empty_separator",
                        "str_split separator must be non-empty",
                    ));
                }
                Ok(Some(Value::Array(
                    text.split(&sep)
                        .map(|part| Value::String(part.to_string()))
                        .collect(),
                )))
            }
            // Growable Map[K, V] (lang.3). Like push/pop, the mutators take the
            // map as a variable and update the binding in place.
            "map_new" => {
                expect_arity(name, args, 0)?;
                Ok(Some(Value::Map(Vec::new())))
            }
            "map_set" => {
                expect_arity(name, args, 3)?;
                let key = self.eval_expr(&args[1], env)?;
                let value = self.eval_expr(&args[2], env)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "map_target",
                        "map_set expects a variable as its first argument",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Map(entries)) => {
                        match entries.iter_mut().find(|(k, _)| *k == key) {
                            Some((_, existing)) => *existing = value,
                            None => entries.push((key, value)),
                        }
                        Ok(Some(Value::Void))
                    }
                    Some(_) => Err(trap(
                        "type_error",
                        format!("map_set target '{var}' is not a map"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown symbol '{var}'"))),
                }
            }
            "map_get" => {
                expect_arity(name, args, 2)?;
                let map = self.eval_expr(&args[0], env)?;
                let key = self.eval_expr(&args[1], env)?;
                let Value::Map(entries) = map else {
                    return Err(trap("type_error", "map_get expects a map"));
                };
                Ok(Some(match entries.iter().find(|(k, _)| *k == key) {
                    Some((_, value)) => Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "Some".to_string(),
                        value: Some(Box::new(value.clone())),
                    },
                    None => Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "None".to_string(),
                        value: None,
                    },
                }))
            }
            "map_has" => {
                expect_arity(name, args, 2)?;
                let map = self.eval_expr(&args[0], env)?;
                let key = self.eval_expr(&args[1], env)?;
                let Value::Map(entries) = map else {
                    return Err(trap("type_error", "map_has expects a map"));
                };
                Ok(Some(Value::Bool(entries.iter().any(|(k, _)| *k == key))))
            }
            "map_remove" => {
                expect_arity(name, args, 2)?;
                let key = self.eval_expr(&args[1], env)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "map_target",
                        "map_remove expects a variable as its first argument",
                    ));
                };
                match env.get_mut(var) {
                    Some(Value::Map(entries)) => {
                        Ok(Some(match entries.iter().position(|(k, _)| *k == key) {
                            Some(index) => {
                                let (_, value) = entries.remove(index);
                                Value::Variant {
                                    enum_name: "Option".to_string(),
                                    variant: "Some".to_string(),
                                    value: Some(Box::new(value)),
                                }
                            }
                            None => Value::Variant {
                                enum_name: "Option".to_string(),
                                variant: "None".to_string(),
                                value: None,
                            },
                        }))
                    }
                    Some(_) => Err(trap(
                        "type_error",
                        format!("map_remove target '{var}' is not a map"),
                    )),
                    None => Err(trap("unknown_symbol", format!("unknown symbol '{var}'"))),
                }
            }
            "Ok" | "core_result_ok_u8" => {
                expect_arity(name, args, 1)?;
                Ok(Some(Value::Variant {
                    enum_name: "Result".to_string(),
                    variant: "Ok".to_string(),
                    value: Some(Box::new(self.eval_expr(&args[0], env)?)),
                }))
            }
            "Err" | "core_result_err_u8" => {
                expect_arity(name, args, 1)?;
                Ok(Some(Value::Variant {
                    enum_name: "Result".to_string(),
                    variant: "Err".to_string(),
                    value: Some(Box::new(self.eval_expr(&args[0], env)?)),
                }))
            }
            "is_some"
            | "is_none"
            | "is_ok"
            | "is_err"
            | "core_option_is_some_u8"
            | "core_option_is_none_u8"
            | "core_result_is_ok_u8"
            | "core_result_is_err_u8" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?;
                let canonical = match name {
                    "core_option_is_some_u8" => "is_some",
                    "core_option_is_none_u8" => "is_none",
                    "core_result_is_ok_u8" => "is_ok",
                    "core_result_is_err_u8" => "is_err",
                    other => other,
                };
                let result = match (canonical, value) {
                    (
                        "is_some",
                        Value::Variant {
                            enum_name, variant, ..
                        },
                    ) if enum_name == "Option" => variant == "Some",
                    (
                        "is_none",
                        Value::Variant {
                            enum_name, variant, ..
                        },
                    ) if enum_name == "Option" => variant == "None",
                    (
                        "is_ok",
                        Value::Variant {
                            enum_name, variant, ..
                        },
                    ) if enum_name == "Result" => variant == "Ok",
                    (
                        "is_err",
                        Value::Variant {
                            enum_name, variant, ..
                        },
                    ) if enum_name == "Result" => variant == "Err",
                    _ => {
                        return Err(trap(
                            "type_error",
                            format!("{name} requires matching Option/Result value"),
                        ))
                    }
                };
                Ok(Some(Value::Bool(result)))
            }
            "unwrap_or" | "core_option_unwrap_or_u8" | "core_result_unwrap_or_u8" => {
                expect_arity(name, args, 2)?;
                let value = self.eval_expr(&args[0], env)?;
                let fallback = self.eval_expr(&args[1], env)?;
                let unwrapped = match value {
                    Value::Variant {
                        enum_name,
                        variant,
                        value,
                    } if (enum_name == "Option" && variant == "Some")
                        || (enum_name == "Result" && variant == "Ok") =>
                    {
                        value.map(|value| *value).unwrap_or(Value::Void)
                    }
                    Value::Variant { .. } => fallback,
                    _ => {
                        return Err(trap(
                            "type_error",
                            "unwrap_or requires Option or Result value",
                        ))
                    }
                };
                Ok(Some(unwrapped))
            }
            "core_mem_is_aligned_i16" => {
                expect_arity(name, args, 2)?;
                Ok(Some(Value::Bool(false)))
            }
            "core_slice_len_i16" | "core_slice_len_u8" | "core_slice_len_mut_i16" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?;
                let Value::Array(values) = value else {
                    return Err(trap("type_error", format!("{name} requires a slice value")));
                };
                Ok(Some(Value::Int(values.len() as i64)))
            }
            "core_slice_require_same_len_i16" | "core_slice_require_mut_same_len_i16" => {
                expect_arity(name, args, 2)?;
                let left = self.eval_expr(&args[0], env)?;
                let right = self.eval_expr(&args[1], env)?;
                let (Value::Array(left), Value::Array(right)) = (left, right) else {
                    return Err(trap("type_error", format!("{name} requires slice values")));
                };
                if left.len() != right.len() {
                    return Err(trap(
                        "require_failed",
                        format!("{name} length mismatch {} != {}", left.len(), right.len()),
                    ));
                }
                Ok(Some(Value::Void))
            }
            "core_slice_copy_i16" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(dst_name) = &args[0] else {
                    return Err(trap(
                        "unsupported_assignment",
                        "core_slice_copy_i16 destination must be a direct variable",
                    ));
                };
                let source = self.eval_expr(&args[1], env)?;
                let Value::Array(source) = source else {
                    return Err(trap(
                        "type_error",
                        "core_slice_copy_i16 source is not a slice",
                    ));
                };
                let dst = env.get_mut(dst_name).ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown symbol '{dst_name}'"))
                })?;
                let Value::Array(values) = dst else {
                    return Err(trap("type_error", format!("{dst_name} is not a slice")));
                };
                if values.len() != source.len() {
                    return Err(trap(
                        "require_failed",
                        format!(
                            "core_slice_copy_i16 length mismatch {} != {}",
                            values.len(),
                            source.len()
                        ),
                    ));
                }
                *values = source;
                Ok(Some(Value::Void))
            }
            "core_math_clamp_i16" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_f64()?;
                Ok(Some(Value::Int(clamp_i16(value))))
            }
            "core_math_saturating_add_i16" => {
                expect_arity(name, args, 2)?;
                let left = self.eval_expr(&args[0], env)?.as_i64()?;
                let right = self.eval_expr(&args[1], env)?.as_i64()?;
                Ok(Some(Value::Int((left + right).clamp(-32768, 32767))))
            }
            "core_math_checked_add_i32"
            | "core_math_checked_sub_i32"
            | "core_math_checked_mul_i32" => {
                expect_arity(name, args, 2)?;
                let left = self.eval_expr(&args[0], env)?.as_i64()?;
                let right = self.eval_expr(&args[1], env)?.as_i64()?;
                let value = match name {
                    "core_math_checked_add_i32" => left.checked_add(right),
                    "core_math_checked_sub_i32" => left.checked_sub(right),
                    "core_math_checked_mul_i32" => left.checked_mul(right),
                    _ => None,
                }
                .ok_or_else(|| trap("integer_overflow", format!("{name} overflow")))?;
                Ok(Some(Value::Int(checked_i32(value, name)?)))
            }
            "core_bits_extract_u8" => {
                expect_arity(name, args, 3)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()? as u8;
                let shift = self.eval_expr(&args[1], env)?.as_i64()?;
                let mask = self.eval_expr(&args[2], env)?.as_i64()? as u8;
                if !(0..8).contains(&shift) {
                    return Err(trap("invalid_shift", format!("invalid u8 shift {shift}")));
                }
                Ok(Some(Value::Int(i64::from((value >> shift) & mask))))
            }
            "core_bits_popcount_u32" | "core_bits_clz_u32" | "core_bits_ctz_u32" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()? as u32;
                let out = match name {
                    "core_bits_popcount_u32" => value.count_ones(),
                    "core_bits_clz_u32" => value.leading_zeros(),
                    "core_bits_ctz_u32" => value.trailing_zeros(),
                    _ => unreachable!(),
                };
                Ok(Some(Value::Int(i64::from(out))))
            }
            "core_bits_rotl_u32" => {
                expect_arity(name, args, 2)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()? as u32;
                let amount = self.eval_expr(&args[1], env)?.as_i64()? as u32;
                Ok(Some(Value::Int(i64::from(value.rotate_left(amount)))))
            }
            "core_io_read_file" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::String(io.read_file(&path)?)))
            }
            "core_io_write_file" => {
                expect_arity(name, args, 2)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let contents = self.eval_expr(&args[1], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.write_file(&path, &contents)? as i64)))
            }
            "core_io_copy_file" => {
                expect_arity(name, args, 2)?;
                let src = self.eval_expr(&args[0], env)?.as_str_value()?;
                let dst = self.eval_expr(&args[1], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.copy_file(&src, &dst)? as i64)))
            }
            "core_hash_sha256" => {
                expect_arity(name, args, 1)?;
                let data = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(crate::crypto::sha256_hex(
                    data.as_bytes(),
                ))))
            }
            "core_hash_hmac_sha256" => {
                expect_arity(name, args, 2)?;
                let key = self.eval_expr(&args[0], env)?.as_str_value()?;
                let msg = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(Value::String(crate::crypto::hmac_sha256_hex(
                    key.as_bytes(),
                    msg.as_bytes(),
                ))))
            }
            "core_hash_ct_eq" => {
                expect_arity(name, args, 2)?;
                let a = self.eval_expr(&args[0], env)?.as_str_value()?;
                let b = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(Value::Bool(crate::crypto::constant_time_eq(
                    a.as_bytes(),
                    b.as_bytes(),
                ))))
            }
            "core_socket_tcp_request" => {
                expect_arity(name, args, 2)?;
                let endpoint = self.eval_expr(&args[0], env)?.as_str_value()?;
                let payload = self.eval_expr(&args[1], env)?.as_str_value()?;
                let net = self.require_net()?;
                Ok(Some(Value::String(net.tcp_request(&endpoint, &payload)?)))
            }
            "core_socket_tcp_send" => {
                expect_arity(name, args, 2)?;
                let endpoint = self.eval_expr(&args[0], env)?.as_str_value()?;
                let payload = self.eval_expr(&args[1], env)?.as_str_value()?;
                let net = self.require_net()?;
                Ok(Some(Value::Int(net.tcp_send(&endpoint, &payload)? as i64)))
            }
            "core_fs_scope_read"
            | "core_runtime_fs_scope_read"
            | "core_fs_scope_write"
            | "core_runtime_fs_scope_write" => {
                expect_arity(name, args, 1)?;
                let scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::Bool(!scope.trim().is_empty())))
            }
            "core_fs_read_text" | "core_runtime_fs_read_text" => {
                expect_arity(name, args, 2)?;
                let scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                let path = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(ok_string(mock_file_contents(&scope, &path))))
            }
            "core_fs_write_text" | "core_runtime_fs_write_text" => {
                expect_arity(name, args, 3)?;
                let _scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                let _path = self.eval_expr(&args[1], env)?.as_str_value()?;
                let contents = self.eval_expr(&args[2], env)?.as_str_value()?;
                Ok(Some(ok_usize(contents.len())))
            }
            "core_fs_copy_text" | "core_runtime_fs_copy_text" => {
                expect_arity(name, args, 3)?;
                let scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                let source = self.eval_expr(&args[1], env)?.as_str_value()?;
                let _dest = self.eval_expr(&args[2], env)?.as_str_value()?;
                Ok(Some(ok_usize(mock_file_contents(&scope, &source).len())))
            }
            "core_net_http_get" | "core_runtime_net_http_get" => {
                expect_arity(name, args, 2)?;
                let scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                let url = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(ok_string(format!("pong:{scope}:{url}"))))
            }
            "core_net_http_post" | "core_runtime_net_http_post" => {
                expect_arity(name, args, 3)?;
                let scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                let url = self.eval_expr(&args[1], env)?.as_str_value()?;
                let body = self.eval_expr(&args[2], env)?.as_str_value()?;
                Ok(Some(ok_string(format!(
                    "accepted:{scope}:{url}:{}",
                    body.len()
                ))))
            }
            "core_net_tcp_send"
            | "core_runtime_net_tcp_send"
            | "core_net_udp_send"
            | "core_runtime_net_udp_send"
            | "core_net_websocket_send"
            | "core_runtime_net_websocket_send" => {
                expect_arity(name, args, 3)?;
                let _scope = self.eval_expr(&args[0], env)?.as_str_value()?;
                let _endpoint = self.eval_expr(&args[1], env)?.as_str_value()?;
                let payload = self.eval_expr(&args[2], env)?.as_str_value()?;
                Ok(Some(ok_usize(payload.len())))
            }
            "core_serialize_text_len"
            | "core_runtime_serialize_text_len"
            | "core_stream_count_bytes"
            | "core_runtime_stream_count_bytes" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::Int(value.len() as i64)))
            }
            "core_crypto_mock_secret_key" | "core_runtime_crypto_mock_secret_key" => {
                expect_arity(name, args, 1)?;
                let label = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "secret:ed25519:{}",
                    stable_hash(&label)
                ))))
            }
            "core_crypto_public_key_ed25519" | "core_runtime_crypto_public_key_ed25519" => {
                expect_arity(name, args, 1)?;
                let secret = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "public:ed25519:{}",
                    stable_hash(&secret)
                ))))
            }
            "core_crypto_hash_blake3" | "core_runtime_crypto_hash_blake3" => {
                expect_arity(name, args, 1)?;
                let message = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "hash:blake3:{}:{}",
                    message.len(),
                    stable_hash(&message)
                ))))
            }
            "core_crypto_sign_ed25519" | "core_runtime_crypto_sign_ed25519" => {
                expect_arity(name, args, 2)?;
                let secret = self.eval_expr(&args[0], env)?.as_str_value()?;
                let message = self.eval_expr(&args[1], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "sig:ed25519:{}",
                    stable_hash(&format!("{secret}:{message}"))
                ))))
            }
            "core_crypto_verify_ed25519" | "core_runtime_crypto_verify_ed25519" => {
                expect_arity(name, args, 3)?;
                let _public_key = self.eval_expr(&args[0], env)?.as_str_value()?;
                let _message = self.eval_expr(&args[1], env)?.as_str_value()?;
                let signature = self.eval_expr(&args[2], env)?.as_str_value()?;
                Ok(Some(Value::Bool(signature.starts_with("sig:ed25519:"))))
            }
            "core_crypto_nonce" | "core_runtime_crypto_nonce" => {
                expect_arity(name, args, 1)?;
                let label = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "nonce:{}:{}",
                    label,
                    stable_hash(&label)
                ))))
            }
            "core_crypto_export_secret_key" | "core_runtime_crypto_export_secret_key" => {
                expect_arity(name, args, 1)?;
                self.eval_expr(&args[0], env).map(Some)
            }
            "core_crypto_clone_secret_key" | "core_runtime_crypto_clone_secret_key" => {
                expect_arity(name, args, 1)?;
                self.eval_expr(&args[0], env).map(Some)
            }
            "core_agent_log_secret" | "core_runtime_agent_log_secret" => {
                expect_arity(name, args, 1)?;
                let _secret = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::Void))
            }
            "core_agent_token_amount" | "core_runtime_agent_token_amount" => {
                expect_arity(name, args, 2)?;
                let symbol = self.eval_expr(&args[0], env)?.as_str_value()?;
                let amount = self.eval_expr(&args[1], env)?.as_i64()?;
                Ok(Some(Value::String(format!("token:mock:{symbol}:{amount}"))))
            }
            "core_agent_payment_receipt" | "core_runtime_agent_payment_receipt" => {
                expect_arity(name, args, 4)?;
                let payer = self.eval_expr(&args[0], env)?.as_str_value()?;
                let payee = self.eval_expr(&args[1], env)?.as_str_value()?;
                let amount = self.eval_expr(&args[2], env)?.as_str_value()?;
                let memo_hash = self.eval_expr(&args[3], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "receipt:mock:{}",
                    stable_hash(&format!("{payer}:{payee}:{amount}:{memo_hash}"))
                ))))
            }
            "core_agent_receipt_ok" | "core_runtime_agent_receipt_ok" => {
                expect_arity(name, args, 1)?;
                let receipt = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::Bool(receipt.starts_with("receipt:mock:"))))
            }
            "core_agent_signed_patch_hash" | "core_runtime_agent_signed_patch_hash" => {
                expect_arity(name, args, 1)?;
                let patch = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(format!(
                    "patch-hash:blake3:{}",
                    stable_hash(&patch)
                ))))
            }
            _ if canonical_simd_intrinsic(name).is_some() => {
                Ok(Some(self.eval_simd_intrinsic(name, args, env)?))
            }
            _ => Ok(None),
        }
    }

    fn execute_task_callable(&self, callable: Value, input: Value) -> Result<Value, String> {
        match callable {
            Value::Function(target) => {
                let function = self.program.find_function(&target).ok_or_else(|| {
                    trap(
                        "unknown_function",
                        format!("task names unknown function '{target}'"),
                    )
                })?;
                if function.params.len() != 1 {
                    return Err(trap(
                        "task_arity",
                        format!("task function '{target}' must take exactly one argument"),
                    ));
                }
                self.execute_function(function, vec![input])
                    .map(|(value, _)| value)
            }
            Value::Closure {
                params,
                body,
                mut captured,
            } => {
                if params.len() != 1 {
                    return Err(trap(
                        "task_arity",
                        "task closure must take exactly one argument",
                    ));
                }
                captured.insert(params[0].clone(), input);
                self.eval_expr(&body, &mut captured)
            }
            _ => Err(trap(
                "task_not_callable",
                "task contains a value that is not callable",
            )),
        }
    }

    fn eval_simd_intrinsic(
        &self,
        name: &str,
        args: &[Expr],
        env: &mut HashMap<String, Value>,
    ) -> Result<Value, String> {
        let canonical = canonical_simd_intrinsic(name).unwrap_or_else(|| name.to_string());
        let name = canonical.as_str();
        if let Some(lanes) = simd_lanes_from_suffix(name.strip_prefix("simd_load_").unwrap_or("")) {
            expect_arity(name, args, 2)?;
            let source = self.eval_expr(&args[0], env)?;
            let index = checked_index(self.eval_expr(&args[1], env)?.as_i64()?)?;
            let Value::Array(values) = source else {
                return Err(trap("type_error", format!("{name} source is not an array")));
            };
            if index + lanes > values.len() {
                return Err(trap(
                    "index_out_of_bounds",
                    format!("{name} reads lanes {index}..{}", index + lanes),
                ));
            }
            return Ok(Value::Array(values[index..index + lanes].to_vec()));
        }
        if let Some(lanes) = simd_lanes_from_suffix(name.strip_prefix("simd_store_").unwrap_or(""))
        {
            expect_arity(name, args, 3)?;
            let Expr::Var(dst_name) = &args[0] else {
                return Err(trap(
                    "unsupported_assignment",
                    format!("{name} destination must be a direct variable"),
                ));
            };
            let index = checked_index(self.eval_expr(&args[1], env)?.as_i64()?)?;
            let vector = self.eval_expr(&args[2], env)?;
            let Value::Array(lanes_value) = vector else {
                return Err(trap("type_error", format!("{name} value is not a vector")));
            };
            if lanes_value.len() != lanes {
                return Err(trap(
                    "type_error",
                    format!("{name} expects {lanes} lanes, got {}", lanes_value.len()),
                ));
            }
            let dst = env
                .get_mut(dst_name)
                .ok_or_else(|| trap("unknown_symbol", format!("unknown symbol '{dst_name}'")))?;
            let Value::Array(values) = dst else {
                return Err(trap("type_error", format!("{dst_name} is not an array")));
            };
            if index + lanes > values.len() {
                return Err(trap(
                    "index_out_of_bounds",
                    format!("{name} writes lanes {index}..{}", index + lanes),
                ));
            }
            values[index..(lanes + index)].clone_from_slice(&lanes_value[..lanes]);
            return Ok(Value::Void);
        }
        if simd_lanes_from_suffix(name.strip_prefix("simd_vec_").unwrap_or("")).is_some() {
            let values = args
                .iter()
                .map(|arg| self.eval_expr(arg, env))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Value::Array(values));
        }
        if let Some(lanes) = simd_lanes_from_suffix(name.strip_prefix("simd_splat_").unwrap_or(""))
        {
            expect_arity(name, args, 1)?;
            let value = self.eval_expr(&args[0], env)?;
            return Ok(Value::Array(vec![value; lanes]));
        }
        if name.starts_with("simd_widen_") || name.starts_with("simd_narrow_") {
            expect_arity(name, args, 1)?;
            return self.eval_expr(&args[0], env);
        }
        match name {
            "simd_add" | "simd_mul" | "simd_saturating_add" => {
                expect_arity(name, args, 2)?;
                let left = self.eval_expr(&args[0], env)?;
                let right = self.eval_expr(&args[1], env)?;
                simd_binary(name, left, right)
            }
            "simd_eq" => {
                expect_arity(name, args, 2)?;
                let left = self.eval_expr(&args[0], env)?;
                let right = self.eval_expr(&args[1], env)?;
                simd_eq(left, right)
            }
            "simd_select" => {
                expect_arity(name, args, 3)?;
                let mask = self.eval_expr(&args[0], env)?;
                let left = self.eval_expr(&args[1], env)?;
                let right = self.eval_expr(&args[2], env)?;
                simd_select(mask, left, right)
            }
            "simd_reduce_add" | "simd_reduce_mul" => {
                expect_arity(name, args, 1)?;
                let vector = self.eval_expr(&args[0], env)?;
                simd_reduce(name, vector)
            }
            "simd_shuffle" => {
                if args.len() < 2 {
                    return Err(trap("call_arity", "simd_shuffle expects lane indices"));
                }
                let vector = self.eval_expr(&args[0], env)?;
                let Value::Array(values) = vector else {
                    return Err(trap("type_error", "simd_shuffle requires a vector"));
                };
                let mut out = Vec::new();
                for index_expr in &args[1..] {
                    let index = checked_index(self.eval_expr(index_expr, env)?.as_i64()?)?;
                    out.push(values.get(index).cloned().ok_or_else(|| {
                        trap("index_out_of_bounds", format!("shuffle lane {index}"))
                    })?);
                }
                Ok(Value::Array(out))
            }
            _ => Err(trap(
                "unknown_function",
                format!("unknown SIMD intrinsic '{name}'"),
            )),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FirParseStop {
    End,
    Terminator(String),
}

fn parse_ir_body(
    instructions: &[IrInstruction],
    cursor: &mut usize,
    terminators: &[&str],
) -> Result<(Vec<Stmt>, FirParseStop), String> {
    let mut body = Vec::new();
    while *cursor < instructions.len() {
        let op = instructions[*cursor].op.as_str();
        if terminators.contains(&op) {
            return Ok((body, FirParseStop::Terminator(op.to_string())));
        }
        body.push(parse_ir_stmt(instructions, cursor)?);
    }
    Ok((body, FirParseStop::End))
}

fn parse_ir_stmt(instructions: &[IrInstruction], cursor: &mut usize) -> Result<Stmt, String> {
    let instruction = instructions.get(*cursor).ok_or_else(|| {
        trap(
            "fir_malformed_body",
            "FIR cursor moved past instruction stream",
        )
    })?;
    match instruction.op.as_str() {
        "let" => {
            let [name, expr_text] = instruction.args.as_slice() else {
                return Err(malformed_ir(instruction, *cursor));
            };
            *cursor += 1;
            Ok(Stmt::Let {
                name: name.clone(),
                ty: instruction.ty.as_deref().map(Type::parse),
                expr: parse_ir_expr(expr_text)?,
            })
        }
        "return" => match instruction.args.as_slice() {
            [] => {
                *cursor += 1;
                Ok(Stmt::Return(None))
            }
            [expr_text] => {
                *cursor += 1;
                Ok(Stmt::Return(Some(parse_ir_expr(expr_text)?)))
            }
            _ => Err(malformed_ir(instruction, *cursor)),
        },
        "require" => {
            let [expr_text] = instruction.args.as_slice() else {
                return Err(malformed_ir(instruction, *cursor));
            };
            *cursor += 1;
            Ok(Stmt::Require(parse_ir_expr(expr_text)?))
        }
        "assign" => {
            let [target_text, expr_text] = instruction.args.as_slice() else {
                return Err(malformed_ir(instruction, *cursor));
            };
            *cursor += 1;
            Ok(Stmt::Assign {
                target: parse_ir_expr(target_text)?,
                expr: parse_ir_expr(expr_text)?,
            })
        }
        "expr" => {
            let [expr_text] = instruction.args.as_slice() else {
                return Err(malformed_ir(instruction, *cursor));
            };
            *cursor += 1;
            Ok(Stmt::Expr(parse_ir_expr(expr_text)?))
        }
        "break" => {
            if !instruction.args.is_empty() {
                return Err(malformed_ir(instruction, *cursor));
            }
            *cursor += 1;
            Ok(Stmt::Break)
        }
        "continue" => {
            if !instruction.args.is_empty() {
                return Err(malformed_ir(instruction, *cursor));
            }
            *cursor += 1;
            Ok(Stmt::Continue)
        }
        "if" => parse_ir_if(instructions, cursor),
        "for" => parse_ir_for(instructions, cursor),
        "while" => parse_ir_while(instructions, cursor),
        "match" => parse_ir_match(instructions, cursor),
        _ => Err(format!(
            "trap: fir_unsupported_op: unsupported or malformed FIR op '{}' at instruction {}",
            instruction.op, *cursor
        )),
    }
}

fn parse_ir_if(instructions: &[IrInstruction], cursor: &mut usize) -> Result<Stmt, String> {
    let instruction = &instructions[*cursor];
    let [cond_text, _then_len] = instruction.args.as_slice() else {
        return Err(malformed_ir(instruction, *cursor));
    };
    let cond = parse_ir_expr(cond_text)?;
    *cursor += 1;
    let (then_body, stop) = parse_ir_body(instructions, cursor, &["else", "endif"])?;
    let else_body = match stop {
        FirParseStop::Terminator(op) if op == "else" => {
            *cursor += 1;
            let (else_body, else_stop) = parse_ir_body(instructions, cursor, &["endif"])?;
            match else_stop {
                FirParseStop::Terminator(op) if op == "endif" => {
                    *cursor += 1;
                    else_body
                }
                _ => return Err(missing_terminator("if", "endif")),
            }
        }
        FirParseStop::Terminator(op) if op == "endif" => {
            *cursor += 1;
            Vec::new()
        }
        _ => return Err(missing_terminator("if", "endif")),
    };
    Ok(Stmt::If {
        cond,
        then_body,
        else_body,
    })
}

fn parse_ir_for(instructions: &[IrInstruction], cursor: &mut usize) -> Result<Stmt, String> {
    let instruction = &instructions[*cursor];
    let [var, start_text, end_text, _body_len] = instruction.args.as_slice() else {
        return Err(malformed_ir(instruction, *cursor));
    };
    *cursor += 1;
    let (body, stop) = parse_ir_body(instructions, cursor, &["endfor"])?;
    match stop {
        FirParseStop::Terminator(op) if op == "endfor" => {
            *cursor += 1;
            Ok(Stmt::For {
                var: var.clone(),
                start: parse_ir_expr(start_text)?,
                end: parse_ir_expr(end_text)?,
                body,
            })
        }
        _ => Err(missing_terminator("for", "endfor")),
    }
}

fn parse_ir_while(instructions: &[IrInstruction], cursor: &mut usize) -> Result<Stmt, String> {
    let instruction = &instructions[*cursor];
    let [cond_text, _body_len] = instruction.args.as_slice() else {
        return Err(malformed_ir(instruction, *cursor));
    };
    *cursor += 1;
    let (body, stop) = parse_ir_body(instructions, cursor, &["endwhile"])?;
    match stop {
        FirParseStop::Terminator(op) if op == "endwhile" => {
            *cursor += 1;
            Ok(Stmt::While {
                cond: parse_ir_expr(cond_text)?,
                body,
            })
        }
        _ => Err(missing_terminator("while", "endwhile")),
    }
}

fn parse_ir_match(instructions: &[IrInstruction], cursor: &mut usize) -> Result<Stmt, String> {
    let instruction = &instructions[*cursor];
    let [scrutinee_text, _arms] = instruction.args.as_slice() else {
        return Err(malformed_ir(instruction, *cursor));
    };
    *cursor += 1;
    let mut arms = Vec::new();
    loop {
        let case = instructions
            .get(*cursor)
            .ok_or_else(|| missing_terminator("match", "endmatch"))?;
        if case.op == "endmatch" {
            *cursor += 1;
            break;
        }
        if case.op != "case" {
            return Err(malformed_ir(case, *cursor));
        }
        let [pattern_text] = case.args.as_slice() else {
            return Err(malformed_ir(case, *cursor));
        };
        let pattern = parse_ir_pattern(pattern_text)?;
        *cursor += 1;
        let (body, stop) = parse_ir_body(instructions, cursor, &["case", "endmatch"])?;
        arms.push(MatchArm { pattern, body });
        match stop {
            FirParseStop::Terminator(op) if op == "case" => {}
            FirParseStop::Terminator(op) if op == "endmatch" => {
                *cursor += 1;
                break;
            }
            _ => return Err(missing_terminator("match", "endmatch")),
        }
    }
    Ok(Stmt::Match {
        scrutinee: parse_ir_expr(scrutinee_text)?,
        arms,
    })
}

fn ensure_ir_function_ready(function: &IrFunction) -> Result<(), String> {
    if let Some(contract) = function
        .backend_contracts
        .iter()
        .find(|contract| contract.backend == "interp")
    {
        if contract.status == "ready" {
            return Ok(());
        }
        let reasons = if contract.blockers.is_empty() {
            "unknown".to_string()
        } else {
            contract.blockers.join(",")
        };
        return Err(format!(
            "FIR contract blocks backend 'interp': {} blockers=[{}]",
            function.name, reasons
        ));
    }
    Err(format!(
        "FIR contract blocks backend 'interp': {} blockers=[missing_contract]",
        function.name
    ))
}

fn parse_ir_expr(text: &str) -> Result<Expr, String> {
    parse_expr(text).map_err(|err| {
        trap(
            "fir_expr_parse",
            format!("could not parse FIR expression '{text}': {err}"),
        )
    })
}

fn parse_ir_pattern(text: &str) -> Result<Pattern, String> {
    let text = text.trim();
    if text == "_" {
        return Ok(Pattern::Wildcard);
    }
    let (path, binding) = if let Some(open) = text.find('(') {
        let path = text[..open].trim().to_string();
        let name = text[open + 1..]
            .strip_suffix(')')
            .ok_or_else(|| trap("fir_pattern_parse", format!("invalid FIR pattern '{text}'")))?
            .trim()
            .to_string();
        (path, Some(name))
    } else {
        (text.to_string(), None)
    };
    if !path.contains("::") {
        return Err(trap(
            "fir_pattern_parse",
            format!("invalid FIR pattern '{text}'"),
        ));
    }
    Ok(Pattern::Variant { path, binding })
}

fn malformed_ir(instruction: &IrInstruction, cursor: usize) -> String {
    trap(
        "fir_malformed_body",
        format!(
            "unsupported or malformed FIR op '{}' at instruction {}",
            instruction.op, cursor
        ),
    )
}

fn missing_terminator(op: &str, terminator: &str) -> String {
    trap(
        "fir_malformed_body",
        format!("FIR '{op}' missing terminator '{terminator}'"),
    )
}

enum Control {
    /// Normal completion: proceed to the next statement.
    Continue,
    Return(Value),
    /// `break`: unwind to the nearest enclosing loop and exit it.
    Break,
    /// `continue`: unwind to the nearest enclosing loop and start its next
    /// iteration.
    LoopContinue,
}

/// When a capsule's single output is a named struct, retag a struct result with
/// that name so field access and downstream typing see the declared type rather
/// than the generic `Object` record produced from the JSON.
/// Every variable name read by an expression, with lambda parameters excluded
/// inside their own bodies — the free-variable set used for capture (lang.4).
fn collect_var_names(expr: &Expr, out: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::Var(name) => {
            out.insert(name.clone());
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => collect_var_names(expr, out),
        Expr::Binary { left, right, .. } => {
            collect_var_names(left, out);
            collect_var_names(right, out);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_var_names(arg, out);
            }
        }
        Expr::Field { base, .. } => collect_var_names(base, out),
        Expr::Index { base, index } => {
            collect_var_names(base, out);
            collect_var_names(index, out);
        }
        Expr::Lambda { params, body } => {
            let mut inner = std::collections::BTreeSet::new();
            collect_var_names(body, &mut inner);
            for param in params {
                inner.remove(&param.name);
            }
            out.extend(inner);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
    }
}

/// Parse a warm worker's response envelope: `{"ok": <value>}` yields the
/// value; `{"err": "<message>"}` is the capsule's own exception.
fn parse_worker_envelope(raw: &str) -> Result<Value, String> {
    let json = JsonParser::new(raw)
        .parse()
        .map_err(|err| format!("worker returned invalid JSON: {err}"))?;
    let JsonValue::Object(mut object) = json else {
        return Err("worker response is not an envelope object".to_string());
    };
    if let Some(JsonValue::String(message)) = object.get("err") {
        return Err(format!("capsule raised: {message}"));
    }
    match object.remove("ok") {
        Some(value) => value_from_json(&value),
        None => Err("worker envelope has neither ok nor err".to_string()),
    }
}

fn retag_struct_output(capsule: &Capsule, value: Value) -> Value {
    if let (Value::Struct(_, fields), [output]) = (&value, capsule.outputs.as_slice()) {
        if let Type::Named(name) = &output.ty {
            return Value::Struct(name.clone(), fields.clone());
        }
    }
    value
}

/// Returns the bindings to introduce for the arm if `pattern` matches `value`,
/// or `None` if it does not. A wildcard or data-less variant binds nothing.
fn match_pattern(pattern: &Pattern, value: &Value) -> Option<Vec<(String, Value)>> {
    match pattern {
        Pattern::Wildcard => Some(Vec::new()),
        Pattern::Variant { path, binding } => match value {
            Value::Variant {
                enum_name,
                variant,
                value: payload,
            } if *path == format!("{enum_name}::{variant}") => {
                let mut binds = Vec::new();
                if let (Some(name), Some(payload)) = (binding, payload) {
                    binds.push((name.clone(), (**payload).clone()));
                }
                Some(binds)
            }
            _ => None,
        },
    }
}

fn eval_binary(op: &str, left: Value, right: Value) -> Result<Value, String> {
    match op {
        // Enum variants (and bools) compare by structural equality; everything
        // else falls back to numeric comparison.
        "==" if !left.is_number() || !right.is_number() => Ok(Value::Bool(left == right)),
        "!=" if !left.is_number() || !right.is_number() => Ok(Value::Bool(left != right)),
        "==" => Ok(Value::Bool(compare_numbers(&left, &right, |a, b| a == b)?)),
        "!=" => Ok(Value::Bool(compare_numbers(&left, &right, |a, b| a != b)?)),
        "<" => Ok(Value::Bool(compare_numbers(&left, &right, |a, b| a < b)?)),
        ">" => Ok(Value::Bool(compare_numbers(&left, &right, |a, b| a > b)?)),
        "<=" => Ok(Value::Bool(compare_numbers(&left, &right, |a, b| a <= b)?)),
        ">=" => Ok(Value::Bool(compare_numbers(&left, &right, |a, b| a >= b)?)),
        "&&" => Ok(Value::Bool(left.as_bool()? && right.as_bool()?)),
        "||" => Ok(Value::Bool(left.as_bool()? || right.as_bool()?)),
        "+" => numeric_op(left, right, i64::checked_add, |a, b| Ok(a + b)),
        "-" => numeric_op(left, right, i64::checked_sub, |a, b| Ok(a - b)),
        "*" => numeric_op(left, right, i64::checked_mul, |a, b| Ok(a * b)),
        "/" => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(trap("division_by_zero", "integer division by zero"))
            }
            (Value::Int(a), Value::Int(b)) => a
                .checked_div(b)
                .map(Value::Int)
                .ok_or_else(|| trap("integer_overflow", "checked integer division failed")),
            (a, b) => {
                let a = a.as_f64()?;
                let b = b.as_f64()?;
                if b == 0.0 {
                    Err(trap("division_by_zero", "float division by zero"))
                } else {
                    Ok(Value::Float(a / b))
                }
            }
        },
        "%" => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(trap("division_by_zero", "integer modulo by zero"))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
            _ => Err(trap("type_error", "modulo requires integer values")),
        },
        "<<" => match (left, right) {
            (Value::Int(a), Value::Int(b)) if (0..64).contains(&b) => Ok(Value::Int(a << b)),
            (Value::Int(_), Value::Int(b)) => {
                Err(trap("invalid_shift", format!("invalid shift amount {b}")))
            }
            _ => Err(trap("type_error", "shift requires integer values")),
        },
        ">>" => match (left, right) {
            (Value::Int(a), Value::Int(b)) if (0..64).contains(&b) => Ok(Value::Int(a >> b)),
            (Value::Int(_), Value::Int(b)) => {
                Err(trap("invalid_shift", format!("invalid shift amount {b}")))
            }
            _ => Err(trap("type_error", "shift requires integer values")),
        },
        "&" => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
            _ => Err(trap("type_error", "bitwise and requires integer values")),
        },
        "|" => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            _ => Err(trap("type_error", "bitwise or requires integer values")),
        },
        _ => Err(trap(
            "unknown_operator",
            format!("unknown binary operator '{op}'"),
        )),
    }
}

fn numeric_op(
    left: Value,
    right: Value,
    int_op: impl FnOnce(i64, i64) -> Option<i64>,
    float_op: impl FnOnce(f64, f64) -> Result<f64, String>,
) -> Result<Value, String> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => int_op(a, b)
            .map(Value::Int)
            .ok_or_else(|| trap("integer_overflow", "checked integer arithmetic failed")),
        (a, b) => Ok(Value::Float(float_op(a.as_f64()?, b.as_f64()?)?)),
    }
}

fn compare_numbers(
    left: &Value,
    right: &Value,
    op: impl FnOnce(f64, f64) -> bool,
) -> Result<bool, String> {
    Ok(op(left.as_f64()?, right.as_f64()?))
}

fn cast_value(ty: &Type, value: Value) -> Result<Value, String> {
    match ty {
        Type::Bool => Ok(Value::Bool(value.as_bool()?)),
        Type::Primitive(name) if name == "f32" || name == "f64" => {
            Ok(Value::Float(value.as_f64()?))
        }
        Type::Primitive(name) if name == "str" => Ok(Value::String(value.as_str_value()?)),
        Type::Primitive(_) => Ok(Value::Int(value.as_i64()?)),
        _ => Ok(value),
    }
}

impl Value {
    fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_))
    }

    fn as_i64(&self) -> Result<i64, String> {
        match self {
            Value::Int(value) => Ok(*value),
            Value::Float(value) => Ok(*value as i64),
            _ => Err(trap(
                "type_error",
                format!("expected numeric value, got {self:?}"),
            )),
        }
    }

    fn as_f64(&self) -> Result<f64, String> {
        match self {
            Value::Int(value) => Ok(*value as f64),
            Value::Float(value) => Ok(*value),
            _ => Err(trap(
                "type_error",
                format!("expected numeric value, got {self:?}"),
            )),
        }
    }

    fn as_bool(&self) -> Result<bool, String> {
        match self {
            Value::Bool(value) => Ok(*value),
            _ => Err(trap(
                "type_error",
                format!("expected bool value, got {self:?}"),
            )),
        }
    }

    fn as_str_value(&self) -> Result<String, String> {
        match self {
            Value::String(value) => Ok(value.clone()),
            _ => Err(trap(
                "type_error",
                format!("expected string value, got {self:?}"),
            )),
        }
    }
}

fn ok_string(value: String) -> Value {
    Value::Variant {
        enum_name: "Result".to_string(),
        variant: "Ok".to_string(),
        value: Some(Box::new(Value::String(value))),
    }
}

fn ok_usize(value: usize) -> Value {
    Value::Variant {
        enum_name: "Result".to_string(),
        variant: "Ok".to_string(),
        value: Some(Box::new(Value::Int(value as i64))),
    }
}

fn mock_file_contents(scope: &str, path: &str) -> String {
    format!("mock-file:{scope}:{path}")
}

fn stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn value_to_text(value: &Value) -> String {
    match value {
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::String(value) => format!("\"{value}\""),
        Value::Void => "void".to_string(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(value_to_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Struct(name, fields) => {
            let mut entries = fields
                .iter()
                .map(|(name, value)| format!("{name}: {}", value_to_text(value)))
                .collect::<Vec<_>>();
            entries.sort();
            format!("{name} {{ {} }}", entries.join(", "))
        }
        Value::Variant {
            enum_name,
            variant,
            value,
        } => match value {
            Some(value) => format!("{enum_name}.{variant}({})", value_to_text(value)),
            None => format!("{enum_name}.{variant}"),
        },
        Value::Map(entries) => format!(
            "map {{ {} }}",
            entries
                .iter()
                .map(|(key, value)| format!("{}: {}", value_to_text(key), value_to_text(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Function(name) => format!("fn {name}"),
        Value::Closure { params, .. } => format!("closure(|{}|)", params.join(", ")),
        Value::Task { state, .. } => format!("task({})", task_state_name(state)),
        Value::TaskGroup {
            state,
            capacity,
            tasks,
        } => format!(
            "task_group(state={}, children={}, capacity={capacity})",
            task_group_state_name(state),
            tasks.len()
        ),
        Value::Channel {
            capacity,
            values,
            closed,
        } => format!(
            "channel(len={}, capacity={}, closed={closed})",
            values.len(),
            capacity
        ),
    }
}

fn propagate_mutable_args(
    function: &FunctionDecl,
    args: &[Expr],
    callee_env: &HashMap<String, Value>,
    caller_env: &mut HashMap<String, Value>,
) {
    for (param, arg) in function.params.iter().zip(args.iter()) {
        if !param.ty.is_mutable_slice() {
            continue;
        }
        let Expr::Var(name) = arg else {
            continue;
        };
        if let Some(value) = callee_env.get(&param.name) {
            caller_env.insert(name.clone(), value.clone());
        }
    }
}

fn substitute_trait_contract_type(ty: &Type, implementation: &crate::ast::TraitImplDecl) -> Type {
    fn substitute(
        ty: &Type,
        implementation: &crate::ast::TraitImplDecl,
        visiting: &mut BTreeSet<String>,
    ) -> Type {
        match ty {
            Type::Named(name) if name == "Self" => implementation.for_type.clone(),
            Type::Named(name) if name.starts_with("Self::") => {
                if !visiting.insert(name.clone()) {
                    return ty.clone();
                }
                let resolved = implementation
                    .associated_types
                    .iter()
                    .find(|binding| name == &format!("Self::{}", binding.name))
                    .map(|binding| substitute(&binding.ty, implementation, visiting))
                    .unwrap_or_else(|| ty.clone());
                visiting.remove(name);
                resolved
            }
            Type::Generic { name, args } => Type::Generic {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| substitute(arg, implementation, visiting))
                    .collect(),
            },
            Type::Array { elem, len } => Type::Array {
                elem: Box::new(substitute(elem, implementation, visiting)),
                len: *len,
            },
            Type::Slice { mutable, elem } => Type::Slice {
                mutable: *mutable,
                elem: Box::new(substitute(elem, implementation, visiting)),
            },
            Type::Pointer { mutable, elem } => Type::Pointer {
                mutable: *mutable,
                elem: Box::new(substitute(elem, implementation, visiting)),
            },
            other => other.clone(),
        }
    }

    substitute(ty, implementation, &mut BTreeSet::new())
}

fn impl_type_pattern_matches(pattern: &Type, found: &Type, type_params: &[String]) -> bool {
    if matches!(pattern, Type::Named(name) if type_params.contains(name)) {
        return !matches!(found, Type::Unknown);
    }
    match (pattern, found) {
        (
            Type::Generic {
                name: pattern_name,
                args: pattern_args,
            },
            Type::Generic {
                name: found_name,
                args: found_args,
            },
        ) => {
            pattern_name == found_name
                && pattern_args.len() == found_args.len()
                && pattern_args
                    .iter()
                    .zip(found_args)
                    .all(|(pattern, found)| impl_type_pattern_matches(pattern, found, type_params))
        }
        (
            Type::Array {
                elem: pattern_elem,
                len: pattern_len,
            },
            Type::Array {
                elem: found_elem,
                len: found_len,
            },
        ) => {
            pattern_len == found_len
                && impl_type_pattern_matches(pattern_elem, found_elem, type_params)
        }
        (
            Type::Slice {
                mutable: pattern_mutable,
                elem: pattern_elem,
            },
            Type::Slice {
                mutable: found_mutable,
                elem: found_elem,
            },
        ) => {
            (*pattern_mutable == *found_mutable || (!pattern_mutable && *found_mutable))
                && impl_type_pattern_matches(pattern_elem, found_elem, type_params)
        }
        (
            Type::Pointer {
                mutable: pattern_mutable,
                elem: pattern_elem,
            },
            Type::Pointer {
                mutable: found_mutable,
                elem: found_elem,
            },
        ) => {
            pattern_mutable == found_mutable
                && impl_type_pattern_matches(pattern_elem, found_elem, type_params)
        }
        _ => pattern == found,
    }
}

fn value_matches_type_pattern(value: &Value, ty: &Type, type_params: &[String]) -> bool {
    if matches!(ty, Type::Named(name) if type_params.contains(name)) {
        return true;
    }
    match ty {
        Type::Generic { name, args } => match (name.as_str(), value) {
            ("Vec", Value::Array(values)) => args.first().is_some_and(|ty| {
                values
                    .iter()
                    .all(|value| value_matches_type_pattern(value, ty, type_params))
            }),
            ("Map", Value::Map(entries)) if args.len() == 2 => {
                entries.iter().all(|(key, value)| {
                    value_matches_type_pattern(key, &args[0], type_params)
                        && value_matches_type_pattern(value, &args[1], type_params)
                })
            }
            _ => value_matches_type(value, ty),
        },
        Type::Array { elem, len } => match value {
            Value::Array(values) => {
                values.len() == *len
                    && values
                        .iter()
                        .all(|value| value_matches_type_pattern(value, elem, type_params))
            }
            _ => false,
        },
        Type::Slice { elem, .. } => match value {
            Value::Array(values) => values
                .iter()
                .all(|value| value_matches_type_pattern(value, elem, type_params)),
            _ => false,
        },
        _ => value_matches_type(value, ty),
    }
}

fn value_matches_type(value: &Value, ty: &Type) -> bool {
    match ty {
        Type::Bool => matches!(value, Value::Bool(_)),
        Type::Primitive(name) if name == "str" => matches!(value, Value::String(_)),
        Type::Primitive(_) if ty.is_integer() => matches!(value, Value::Int(_)),
        Type::Primitive(_) if ty.is_float() => matches!(value, Value::Float(_)),
        Type::Primitive(_) => false,
        Type::Named(name) => match value {
            Value::Struct(type_name, _) => type_name == name,
            Value::Variant { enum_name, .. } => enum_name == name,
            _ => false,
        },
        Type::Generic { name, args } => match (name.as_str(), value) {
            ("Vec", Value::Array(values)) => args
                .first()
                .is_some_and(|ty| values.iter().all(|value| value_matches_type(value, ty))),
            ("Map", Value::Map(entries)) if args.len() == 2 => {
                entries.iter().all(|(key, value)| {
                    value_matches_type(key, &args[0]) && value_matches_type(value, &args[1])
                })
            }
            ("Option" | "Result", Value::Variant { enum_name, .. }) => enum_name == name,
            ("Fn", Value::Function(_) | Value::Closure { .. }) => true,
            ("Task", Value::Task { .. }) | ("Channel", Value::Channel { .. }) => true,
            _ => false,
        },
        Type::Array { elem, len } => match value {
            Value::Array(values) => {
                values.len() == *len && values.iter().all(|value| value_matches_type(value, elem))
            }
            _ => false,
        },
        Type::Slice { elem, .. } => match value {
            Value::Array(values) => values.iter().all(|value| value_matches_type(value, elem)),
            _ => false,
        },
        Type::Pointer { .. } => false,
        Type::Void => matches!(value, Value::Void),
        Type::Unknown => true,
    }
}

fn validate_native_mutable_targets(capsule: &Capsule, source_args: &[Expr]) -> Result<(), String> {
    for (index, binding) in capsule.inputs.iter().enumerate() {
        if !matches!(binding.ty, Type::Slice { mutable: true, .. }) {
            continue;
        }
        match source_args.get(index) {
            Some(Expr::Var(_)) => {}
            _ => {
                return Err(trap(
                    "native_capsule_mut_target",
                    format!(
                        "mutable native input '{}' must be a direct variable for copy-back",
                        binding.name
                    ),
                ))
            }
        }
    }
    Ok(())
}

fn apply_native_mutations(
    capsule: &Capsule,
    source_args: &[Expr],
    mutations: &[crate::ffi::NativeMutation],
    env: &mut HashMap<String, Value>,
) -> Result<(), String> {
    for mutation in mutations {
        let binding = capsule.inputs.get(mutation.input_index).ok_or_else(|| {
            trap(
                "native_capsule_bad_output",
                format!(
                    "native mutation references missing input {}",
                    mutation.input_index
                ),
            )
        })?;
        if !matches!(binding.ty, Type::Slice { mutable: true, .. }) {
            return Err(trap(
                "native_capsule_bad_output",
                format!(
                    "native mutation references non-mutable input '{}'",
                    binding.name
                ),
            ));
        }
        let Expr::Var(name) = source_args.get(mutation.input_index).ok_or_else(|| {
            trap(
                "native_capsule_mut_target",
                format!("missing mutable source argument for '{}'", binding.name),
            )
        })?
        else {
            return Err(trap(
                "native_capsule_mut_target",
                format!(
                    "mutable native input '{}' must be a direct variable for copy-back",
                    binding.name
                ),
            ));
        };
        let values = mutation
            .values
            .iter()
            .map(capsule_scalar_to_value)
            .collect::<Result<Vec<_>, _>>()?;
        env.insert(name.clone(), Value::Array(values));
    }
    Ok(())
}

fn capsule_scalar_to_value(value: &crate::ast::CapsuleValue) -> Result<Value, String> {
    match value {
        crate::ast::CapsuleValue::Int(value) => Ok(Value::Int(*value)),
        crate::ast::CapsuleValue::Float(value) => Ok(Value::Float(*value)),
        crate::ast::CapsuleValue::Bool(value) => Ok(Value::Bool(*value)),
        crate::ast::CapsuleValue::Str(_) => Err(trap(
            "native_capsule_bad_output",
            "string values are not supported in the scalar native slice boundary",
        )),
    }
}

fn checked_index(value: i64) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| trap("negative_index", format!("negative index {value}")))
}

fn checked_i32(value: i64, name: &str) -> Result<i64, String> {
    if value < i64::from(i32::MIN) || value > i64::from(i32::MAX) {
        Err(trap(
            "integer_overflow",
            format!("{name} exceeded i32 range"),
        ))
    } else {
        Ok(value)
    }
}

fn clamp_i16(value: f64) -> i64 {
    if value > f64::from(i16::MAX) {
        i64::from(i16::MAX)
    } else if value < f64::from(i16::MIN) {
        i64::from(i16::MIN)
    } else {
        value as i64
    }
}

fn expect_arity(name: &str, args: &[Expr], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(trap(
            "call_arity",
            format!("{name} expects {expected} args, got {}", args.len()),
        ))
    }
}

fn simd_lanes_from_suffix(suffix: &str) -> Option<usize> {
    let (_, lanes) = suffix.rsplit_once('x')?;
    lanes.parse::<usize>().ok().filter(|lanes| *lanes > 0)
}

fn simd_binary(name: &str, left: Value, right: Value) -> Result<Value, String> {
    let (Value::Array(left), Value::Array(right)) = (left, right) else {
        return Err(trap("type_error", format!("{name} requires vector values")));
    };
    if left.len() != right.len() {
        return Err(trap("type_error", format!("{name} lane counts differ")));
    }
    let values = left
        .into_iter()
        .zip(right)
        .map(|(left, right)| match name {
            "simd_add" => numeric_op(left, right, i64::checked_add, |a, b| Ok(a + b)),
            "simd_mul" => numeric_op(left, right, i64::checked_mul, |a, b| Ok(a * b)),
            "simd_saturating_add" => Ok(Value::Int(left.as_i64()?.saturating_add(right.as_i64()?))),
            _ => Err(trap(
                "unknown_function",
                format!("unknown SIMD intrinsic '{name}'"),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(values))
}

fn simd_eq(left: Value, right: Value) -> Result<Value, String> {
    let (Value::Array(left), Value::Array(right)) = (left, right) else {
        return Err(trap("type_error", "simd_eq requires vector values"));
    };
    if left.len() != right.len() {
        return Err(trap("type_error", "simd_eq lane counts differ"));
    }
    Ok(Value::Array(
        left.into_iter()
            .zip(right)
            .map(|(left, right)| Value::Bool(left == right))
            .collect(),
    ))
}

fn simd_select(mask: Value, left: Value, right: Value) -> Result<Value, String> {
    let (Value::Array(mask), Value::Array(left), Value::Array(right)) = (mask, left, right) else {
        return Err(trap(
            "type_error",
            "simd_select requires mask and vector values",
        ));
    };
    if mask.len() != left.len() || left.len() != right.len() {
        return Err(trap("type_error", "simd_select lane counts differ"));
    }
    let mut out = Vec::new();
    for ((mask, left), right) in mask.into_iter().zip(left).zip(right) {
        out.push(if mask.as_bool()? { left } else { right });
    }
    Ok(Value::Array(out))
}

fn simd_reduce(name: &str, vector: Value) -> Result<Value, String> {
    let Value::Array(values) = vector else {
        return Err(trap("type_error", format!("{name} requires a vector")));
    };
    let mut iter = values.into_iter();
    let Some(first) = iter.next() else {
        return Err(trap(
            "type_error",
            format!("{name} requires at least one lane"),
        ));
    };
    iter.try_fold(first, |acc, value| match name {
        "simd_reduce_add" => numeric_op(acc, value, i64::checked_add, |a, b| Ok(a + b)),
        "simd_reduce_mul" => numeric_op(acc, value, i64::checked_mul, |a, b| Ok(a * b)),
        _ => Err(trap(
            "unknown_function",
            format!("unknown SIMD intrinsic '{name}'"),
        )),
    })
}

fn trap(code: &str, message: impl Into<String>) -> String {
    format!("trap: {code}: {}", message.into())
}

struct RunInput {
    args: JsonValue,
    expect: Option<RunExpectation>,
}

struct RunExpectation {
    return_value: Option<Value>,
    bindings: HashMap<String, Value>,
}

impl RunInput {
    fn parse(source: &str) -> Result<Self, String> {
        let source = source.trim();
        let json = if source.is_empty() {
            JsonValue::Object(BTreeMap::new())
        } else {
            JsonParser::new(source).parse()?
        };
        match json {
            JsonValue::Array(_) => Ok(Self {
                args: json,
                expect: None,
            }),
            JsonValue::Object(mut object) => {
                let args = object
                    .remove("args")
                    .unwrap_or_else(|| JsonValue::Object(BTreeMap::new()));
                let expect = object.remove("expect").map(parse_expectation).transpose()?;
                Ok(Self { args, expect })
            }
            _ => Err(trap(
                "invalid_input",
                "json input must be an array or object with args",
            )),
        }
    }

    fn args_for(&self, function: &FunctionDecl) -> Result<Vec<Value>, String> {
        match &self.args {
            JsonValue::Array(values) => {
                if values.len() != function.params.len() {
                    return Err(trap(
                        "input_arity",
                        format!(
                            "entry '{}' expects {} args, got {}",
                            function.name,
                            function.params.len(),
                            values.len()
                        ),
                    ));
                }
                values.iter().map(value_from_json).collect()
            }
            JsonValue::Object(values) => function
                .params
                .iter()
                .map(|param| {
                    values
                        .get(&param.name)
                        .ok_or_else(|| {
                            trap(
                                "missing_argument",
                                format!("json input missing argument '{}'", param.name),
                            )
                        })
                        .and_then(value_from_json)
                })
                .collect(),
            _ => Err(trap("invalid_input", "args must be an array or object")),
        }
    }

    fn expectation_status(
        &self,
        return_value: &Value,
        bindings: &HashMap<String, Value>,
    ) -> Result<String, String> {
        let Some(expect) = &self.expect else {
            return Ok("ok".to_string());
        };
        if let Some(expected_return) = &expect.return_value {
            if expected_return != return_value {
                return Err(trap(
                    "expectation_failed",
                    format!(
                        "return expected {}, found {}",
                        value_to_text(expected_return),
                        value_to_text(return_value)
                    ),
                ));
            }
        }
        for (name, expected) in &expect.bindings {
            let found = bindings
                .get(name)
                .ok_or_else(|| trap("expectation_failed", format!("binding '{name}' missing")))?;
            if found != expected {
                return Err(trap(
                    "expectation_failed",
                    format!(
                        "binding '{name}' expected {}, found {}",
                        value_to_text(expected),
                        value_to_text(found)
                    ),
                ));
            }
        }
        Ok("pass".to_string())
    }
}

fn parse_expectation(value: JsonValue) -> Result<RunExpectation, String> {
    let JsonValue::Object(mut object) = value else {
        return Err(trap("invalid_input", "expect must be an object"));
    };
    let return_value = object
        .remove("return")
        .map(|value| value_from_json(&value))
        .transpose()?;
    let bindings = match object.remove("bindings") {
        Some(JsonValue::Object(values)) => values
            .iter()
            .map(|(name, value)| Ok((name.clone(), value_from_json(value)?)))
            .collect::<Result<HashMap<_, _>, String>>()?,
        Some(_) => return Err(trap("invalid_input", "expect.bindings must be an object")),
        None => HashMap::new(),
    };
    Ok(RunExpectation {
        return_value,
        bindings,
    })
}

fn value_from_json(value: &JsonValue) -> Result<Value, String> {
    match value {
        JsonValue::Null => Ok(Value::Void),
        JsonValue::Bool(value) => Ok(Value::Bool(*value)),
        JsonValue::Int(value) => Ok(Value::Int(*value)),
        JsonValue::Float(value) => Ok(Value::Float(*value)),
        JsonValue::String(value) => Ok(Value::String(value.clone())),
        JsonValue::Array(values) => values
            .iter()
            .map(value_from_json)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        JsonValue::Object(values) => {
            if let Some(value) = variant_from_json(values, "Option", "Some")? {
                return Ok(value);
            }
            if let Some(value) = unit_variant_from_json(values, "Option", "None") {
                return Ok(value);
            }
            if let Some(value) = variant_from_json(values, "Result", "Ok")? {
                return Ok(value);
            }
            if let Some(value) = variant_from_json(values, "Result", "Err")? {
                return Ok(value);
            }
            let struct_name = values
                .get("$struct")
                .and_then(|value| match value {
                    JsonValue::String(name) => Some(name.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Object".to_string());
            let field_source = match values.get("fields") {
                Some(JsonValue::Object(fields)) => fields,
                _ => values,
            };
            let mut fields = HashMap::new();
            for (name, value) in field_source {
                if name == "$struct" || name == "fields" {
                    continue;
                }
                fields.insert(name.clone(), value_from_json(value)?);
            }
            Ok(Value::Struct(struct_name, fields))
        }
    }
}

fn variant_from_json(
    values: &BTreeMap<String, JsonValue>,
    enum_name: &str,
    variant: &str,
) -> Result<Option<Value>, String> {
    let Some(value) = values.get(variant) else {
        return Ok(None);
    };
    Ok(Some(Value::Variant {
        enum_name: enum_name.to_string(),
        variant: variant.to_string(),
        value: Some(Box::new(value_from_json(value)?)),
    }))
}

fn unit_variant_from_json(
    values: &BTreeMap<String, JsonValue>,
    enum_name: &str,
    variant: &str,
) -> Option<Value> {
    values.get(variant)?;
    Some(Value::Variant {
        enum_name: enum_name.to_string(),
        variant: variant.to_string(),
        value: None,
    })
}

fn run_result_to_json(result: &RunResult) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"entry\": {},\n", quoted_json(&result.entry)));
    out.push_str(&format!("  \"status\": {},\n", quoted_json(&result.status)));
    out.push_str(&format!(
        "  \"return\": {},\n",
        value_to_json(&result.return_value)
    ));
    out.push_str("  \"bindings\": {");
    let mut entries = result.bindings.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    for (idx, (name, value)) in entries.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "\n    {}: {}",
            quoted_json(name),
            value_to_json(value)
        ));
    }
    if !entries.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("}\n}");
    out
}

/// Plain-string marshalling for type-directed boundaries (e.g. the JVM, which
/// parses each CLI argument into its declared type). Unlike [`value_to_json`]
/// it does not quote strings or normalize floats; arrays/structs fall back to
/// JSON since those have no plain scalar form.
fn value_to_arg(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        other => value_to_json(other),
    }
}

/// Marshal an interpreter value into the native C ABI representation. Native
/// call v1.2 supports copy-in/copy-back scalar mutable slices; structs, maps,
/// and owned output buffers remain deliberately blocked until their ownership
/// contracts are explicit.
fn value_to_native_arg(
    program: &Program,
    value: &Value,
    ty: &Type,
) -> Result<crate::ffi::NativeValue, String> {
    match (value, ty) {
        (Value::Array(values), Type::Slice { mutable: false, .. }) => values
            .iter()
            .map(value_to_capsule_scalar)
            .collect::<Result<Vec<_>, _>>()
            .map(crate::ffi::NativeValue::ReadSlice),
        (Value::Array(values), Type::Slice { mutable: true, .. }) => values
            .iter()
            .map(value_to_capsule_scalar)
            .collect::<Result<Vec<_>, _>>()
            .map(crate::ffi::NativeValue::MutSlice),
        (Value::Array(_), _) => Err(trap(
            "native_capsule_boundary_unsupported",
            format!(
                "array value does not match native input type {}",
                ty.to_agent()
            ),
        )),
        (_, Type::Slice { .. }) => Err(trap(
            "native_capsule_argument_type",
            format!(
                "native slice input {} requires an array value",
                ty.to_agent()
            ),
        )),
        (Value::Struct(actual, values), Type::Named(expected)) => {
            if actual != expected {
                return Err(trap(
                    "native_capsule_argument_type",
                    format!("native struct input expects {}, got {}", expected, actual),
                ));
            }
            let decl = program
                .structs()
                .find(|decl| decl.name == *expected)
                .ok_or_else(|| {
                    trap(
                        "native_capsule_boundary_unsupported",
                        format!("native struct input '{}' is not declared", expected),
                    )
                })?;
            if values.len() != decl.fields.len()
                || values
                    .keys()
                    .any(|name| !decl.fields.iter().any(|field| field.name == *name))
            {
                return Err(trap(
                    "native_capsule_argument_type",
                    format!("native struct input '{}' has unexpected fields", expected),
                ));
            }
            decl.fields
                .iter()
                .map(|field| {
                    values.get(&field.name).ok_or_else(|| {
                        trap(
                            "native_capsule_argument_type",
                            format!(
                                "native struct input '{}' is missing field '{}'",
                                expected, field.name
                            ),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(value_to_capsule_scalar)
                .collect::<Result<Vec<_>, _>>()
                .map(crate::ffi::NativeValue::Struct)
        }
        (Value::Struct(_, _), _) => Err(trap(
            "native_capsule_argument_type",
            format!(
                "struct value does not match native input type {}",
                ty.to_agent()
            ),
        )),
        (value, _) => value_to_capsule_scalar(value).map(crate::ffi::NativeValue::Scalar),
    }
}

fn value_to_capsule_scalar(value: &Value) -> Result<crate::ast::CapsuleValue, String> {
    match value {
        Value::Int(value) => Ok(crate::ast::CapsuleValue::Int(*value)),
        Value::Float(value) => Ok(crate::ast::CapsuleValue::Float(*value)),
        Value::Bool(value) => Ok(crate::ast::CapsuleValue::Bool(*value)),
        Value::String(value) => Ok(crate::ast::CapsuleValue::Str(value.clone())),
        other => Err(format!(
            "native_capsule_boundary_unsupported: cannot marshal {} across a scalar C boundary",
            value_to_text(other)
        )),
    }
}

pub fn value_to_json(value: &Value) -> String {
    match value {
        Value::Void => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => {
            let mut text = value.to_string();
            if !text.contains('.') {
                text.push_str(".0");
            }
            text
        }
        Value::String(value) => quoted_json(value),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(value_to_json)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Struct(name, fields) => {
            let mut entries = fields.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut out = format!("{{\"$struct\": {}", quoted_json(name));
            for (field, value) in entries {
                out.push_str(&format!(
                    ", {}: {}",
                    quoted_json(field),
                    value_to_json(value)
                ));
            }
            out.push('}');
            out
        }
        Value::Variant {
            enum_name: _,
            variant,
            value,
        } => match value {
            Some(value) => format!("{{{}: {}}}", quoted_json(variant), value_to_json(value)),
            None => format!("{{{}: null}}", quoted_json(variant)),
        },
        Value::Map(entries) => {
            // A JSON object; string keys verbatim, other keys via their text form.
            let body = entries
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        Value::String(text) => text.clone(),
                        other => value_to_text(other),
                    };
                    format!("{}: {}", quoted_json(&key), value_to_json(value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{body}}}")
        }
        Value::Function(name) => format!("{{\"$fn\": {}}}", quoted_json(name)),
        Value::Closure { params, .. } => {
            format!("{{\"$closure\": {}}}", quoted_json(&params.join(",")))
        }
        Value::Task { state, .. } => {
            format!("{{\"$task\": {}}}", quoted_json(task_state_name(state)))
        }
        Value::TaskGroup {
            state,
            capacity,
            tasks,
        } => format!(
            "{{\"$task_group\":{{\"state\":{},\"capacity\":{capacity},\"children\":{}}}}}",
            quoted_json(task_group_state_name(state)),
            tasks.len()
        ),
        Value::Channel {
            capacity,
            values,
            closed,
        } => format!(
            "{{\"$channel\":{{\"capacity\":{capacity},\"closed\":{closed},\"values\":[{}]}}}}",
            values
                .iter()
                .map(value_to_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn task_state_name(state: &TaskState) -> &'static str {
    match state {
        TaskState::Pending => "pending",
        TaskState::Cancelled => "cancelled",
        TaskState::Consumed => "consumed",
    }
}

fn task_group_state_name(state: &TaskGroupState) -> &'static str {
    match state {
        TaskGroupState::Active => "active",
        TaskGroupState::Cancelled => "cancelled",
        TaskGroupState::Consumed => "consumed",
    }
}

fn quoted_json(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

#[derive(Clone, Debug, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

struct JsonParser<'a> {
    source: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
        }
    }

    fn parse(mut self) -> Result<JsonValue, String> {
        let value = self.parse_value()?;
        self.skip_ws();
        if self.pos != self.source.len() {
            return Err(trap("invalid_json", "trailing characters in json input"));
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => self.parse_literal(b"null", JsonValue::Null),
            Some(b't') => self.parse_literal(b"true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal(b"false", JsonValue::Bool(false)),
            Some(b'"') => self.parse_string().map(JsonValue::String),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(trap("invalid_json", "expected json value")),
        }
    }

    fn parse_literal(&mut self, literal: &[u8], value: JsonValue) -> Result<JsonValue, String> {
        if self.source.get(self.pos..self.pos + literal.len()) == Some(literal) {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err(trap("invalid_json", "invalid literal"))
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(ch) = self.peek() {
                if ch == b'"' || ch == b'\\' {
                    break;
                }
                if ch < 0x20 {
                    return Err(trap("invalid_json", "unescaped control in string"));
                }
                self.pos += 1;
            }
            let segment = std::str::from_utf8(&self.source[start..self.pos])
                .map_err(|_| trap("invalid_json", "string is not valid UTF-8"))?;
            out.push_str(segment);
            match self.next() {
                Some(b'"') => return Ok(out),
                Some(b'\\') => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| trap("invalid_json", "unterminated escape"))?;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        _ => return Err(trap("invalid_json", "unsupported escape sequence")),
                    }
                }
                None => return Err(trap("invalid_json", "unterminated string")),
                _ => unreachable!("string scan stops only at a quote or backslash"),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            values.push(self.parse_value()?);
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect(b'{')?;
        let mut values = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Object(values))
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        self.consume(b'-');
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.consume(b'.') {
            is_float = true;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            let _ = self.consume(b'+') || self.consume(b'-');
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.source[start..self.pos])
            .map_err(|_| trap("invalid_json", "number is not utf8"))?;
        if is_float {
            text.parse::<f64>()
                .map(JsonValue::Float)
                .map_err(|_| trap("invalid_json", format!("invalid float '{text}'")))
        } else {
            text.parse::<i64>()
                .map(JsonValue::Int)
                .map_err(|_| trap("invalid_json", format!("invalid integer '{text}'")))
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(trap(
                "invalid_json",
                format!("expected '{}'", char::from(expected)),
            ))
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }
}
