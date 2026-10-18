use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::ast::{Capsule, Expr, FunctionDecl, MatchArm, Pattern, Program, Stmt, Type};
use crate::backend::{lower_program_to_run_ir, BackendIr, IrFunction, IrInstruction};
use crate::parser::parse_expr;
use crate::stdlib::canonical_simd_intrinsic;
use crate::targets::is_cpu_feature_probe;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    /// An exact unsigned runtime carrier for native `u64` and 64-bit `usize`
    /// values above the signed integer range.
    UInt(u64),
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
    /// A bounded one-shot cooperative wake source. Clones share signal and
    /// waiter state. The token is a copyable shared capability; the tasks it
    /// releases remain affine.
    WakeToken {
        id: usize,
        signaled: std::rc::Rc<std::cell::Cell<bool>>,
        cancelled: std::rc::Rc<std::cell::Cell<bool>>,
        waiters: std::rc::Rc<std::cell::Cell<usize>>,
        timer_queue: Option<std::rc::Rc<std::cell::RefCell<Vec<TimerRegistration>>>>,
    },
    /// A copyable logical clock. Clones share the same tick and bounded timer
    /// queue; advancing it never consults host wall time.
    AsyncClock {
        id: usize,
        now: std::rc::Rc<std::cell::Cell<usize>>,
        timers: std::rc::Rc<std::cell::RefCell<Vec<TimerRegistration>>>,
    },
    /// A copyable fixed-rate logical interval. Each `timer_interval_next` call
    /// reserves the next cadence deadline and returns an ordinary one-shot
    /// wake token, so tick delivery retains the existing token semantics.
    IntervalTimer {
        id: usize,
        now: std::rc::Rc<std::cell::Cell<usize>>,
        timers: std::rc::Rc<std::cell::RefCell<Vec<TimerRegistration>>>,
        period: usize,
        next_deadline: std::rc::Rc<std::cell::Cell<Option<usize>>>,
        open: std::rc::Rc<std::cell::Cell<bool>>,
    },
    /// A borrowed, heterogeneous reactor observation. Arm values are affine,
    /// but they retain only shared readiness state: the task or channel handle
    /// remains available after the arm set is consumed.
    ReadinessArm {
        source: ReadinessSource,
    },
    /// A deterministic cooperative task. The callable and its one owned input
    /// are captured at spawn time, but execution is deferred until `task_await`.
    Task {
        state: TaskState,
        ready: std::rc::Rc<std::cell::Cell<bool>>,
        open: std::rc::Rc<std::cell::Cell<bool>>,
        callable: Box<Value>,
        input: Box<Value>,
        continuations: Vec<Value>,
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
        readable: std::rc::Rc<std::cell::Cell<bool>>,
        writable: std::rc::Rc<std::cell::Cell<bool>>,
    },
    Void,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TaskState {
    Parked,
    Waiting(WakeRegistration),
    Pending,
    Cancelled,
    Consumed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WakeRegistration {
    token_id: usize,
    signaled: std::rc::Rc<std::cell::Cell<bool>>,
    waiters: std::rc::Rc<std::cell::Cell<usize>>,
    timer_queue: Option<std::rc::Rc<std::cell::RefCell<Vec<TimerRegistration>>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReadinessSource {
    Token {
        target: String,
        id: usize,
        signaled: std::rc::Rc<std::cell::Cell<bool>>,
        cancelled: std::rc::Rc<std::cell::Cell<bool>>,
        waiters: std::rc::Rc<std::cell::Cell<usize>>,
        timer_queue: Option<std::rc::Rc<std::cell::RefCell<Vec<TimerRegistration>>>>,
    },
    Task {
        target: String,
        ready: std::rc::Rc<std::cell::Cell<bool>>,
        open: std::rc::Rc<std::cell::Cell<bool>>,
        waiting: Option<WakeRegistration>,
    },
    Channel {
        target: String,
        readable: std::rc::Rc<std::cell::Cell<bool>>,
    },
    ChannelSend {
        target: String,
        writable: std::rc::Rc<std::cell::Cell<bool>>,
    },
    FileRead {
        target: String,
        path: PathBuf,
    },
    FileCreate {
        target: String,
        path: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimerRegistration {
    deadline: usize,
    sequence: usize,
    token_id: usize,
    signaled: std::rc::Rc<std::cell::Cell<bool>>,
    cancelled: std::rc::Rc<std::cell::Cell<bool>>,
    waiters: std::rc::Rc<std::cell::Cell<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskGroupState {
    Active,
    Cancelled,
    Consumed,
}

const MAX_TASK_GROUP_TASKS: usize = 1024;
const MAX_TASK_CONTINUATIONS: usize = 1024;
const MAX_WAKE_TOKEN_WAITERS: usize = 1024;
const MAX_WAKE_TOKEN_SELECT_ARMS: usize = 1024;
const MAX_CLOCK_TIMERS: usize = 1024;
const MAX_CLOCK_TICK: usize = 1_000_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    pub entry: String,
    pub return_value: Value,
    pub bindings: HashMap<String, Value>,
    pub status: String,
}

/// One deterministic cooperative-runtime transition. Events are emitted in
/// execution order and describe decisions instead of wall-clock timing, so
/// receipts remain stable across hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConcurrencyEvent {
    pub sequence: usize,
    pub operation: String,
    pub target: String,
    pub decision: String,
    pub count: usize,
    pub capacity: Option<usize>,
}

#[derive(Default)]
struct ConcurrencyTrace {
    events: std::cell::RefCell<Vec<ConcurrencyEvent>>,
    next_wake_token: std::cell::Cell<usize>,
    next_timer_sequence: std::cell::Cell<usize>,
    next_clock: std::cell::Cell<usize>,
    next_interval: std::cell::Cell<usize>,
}

impl ConcurrencyTrace {
    fn record(
        &self,
        operation: &str,
        target: &str,
        decision: &str,
        count: usize,
        capacity: Option<usize>,
    ) {
        let mut events = self.events.borrow_mut();
        let sequence = events.len();
        events.push(ConcurrencyEvent {
            sequence,
            operation: operation.to_string(),
            target: target.to_string(),
            decision: decision.to_string(),
            count,
            capacity,
        });
    }

    fn events(&self) -> Vec<ConcurrencyEvent> {
        self.events.borrow().clone()
    }

    fn allocate_wake_token(&self) -> usize {
        let id = self.next_wake_token.get();
        self.next_wake_token.set(id + 1);
        id
    }

    fn allocate_timer_sequence(&self) -> usize {
        let sequence = self.next_timer_sequence.get();
        self.next_timer_sequence.set(sequence + 1);
        sequence
    }

    fn allocate_clock(&self) -> usize {
        let id = self.next_clock.get();
        self.next_clock.set(id + 1);
        id
    }

    fn allocate_interval(&self) -> usize {
        let id = self.next_interval.get();
        self.next_interval.set(id + 1);
        id
    }
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir,
        io: None,
        net: None,
        capsules: None,
        concurrency: ConcurrencyTrace::default(),
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

/// What an entry produced, in the values the interpreter actually held: the
/// arguments it was called with, its return value, and the bindings that
/// survived the call.
pub(crate) type EntryValues = (Vec<Value>, Value, HashMap<String, Value>);

/// Run an entry the way [`run_entry_json`] does, but hand back the values
/// instead of their JSON projection.
///
/// The backend differential test needs the *arguments* it fed the interpreter
/// (to build an equivalent C call) and the resulting bindings (to compare the
/// mutations both sides made), and recovering those by re-parsing the JSON
/// would compare a rendering rather than the values themselves.
pub(crate) fn run_entry_values(
    program: &Program,
    entry: &str,
    input: &str,
) -> Result<EntryValues, String> {
    let ir = lower_program_to_run_ir(program);
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: Some(&ir),
        io: None,
        net: None,
        capsules: None,
        concurrency: ConcurrencyTrace::default(),
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(program, function)?;
    let (return_value, bindings) = interpreter.execute_function(function, args.clone())?;
    Ok((args, return_value, bindings))
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: Some(ir),
        io: None,
        net: None,
        capsules: None,
        concurrency: ConcurrencyTrace::default(),
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(program, function)?;
    let (return_value, bindings) = interpreter.execute_function(function, args)?;
    let status = input.expectation_status(&return_value, &bindings)?;
    let result = RunResult {
        entry: entry.to_string(),
        return_value,
        bindings,
        status,
    };
    Ok(append_concurrency_receipt(
        run_result_to_json(&result),
        &interpreter.concurrency.events(),
    ))
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

pub fn run_entry_with_scoped_io_with_ir(
    program: &Program,
    ir: &BackendIr,
    entry: &str,
    args: Vec<Value>,
    policy: IoPolicy,
) -> (
    Option<Value>,
    Vec<IoOp>,
    IoUsage,
    Vec<IoArtifactReceipt>,
    Option<String>,
) {
    run_entry_with_io_context(
        program,
        Some(ir),
        entry,
        args,
        IoContext::from_policy(policy),
    )
}

fn run_entry_with_io_impl(
    program: &Program,
    fir: Option<&BackendIr>,
    entry: &str,
    args: Vec<Value>,
    grants: Vec<String>,
) -> (Option<Value>, Vec<IoOp>, Option<String>) {
    let (value, ops, _usage, _artifacts, error) =
        run_entry_with_io_context(program, fir, entry, args, IoContext::new(grants));
    (value, ops, error)
}

fn run_entry_with_io_context(
    program: &Program,
    fir: Option<&BackendIr>,
    entry: &str,
    args: Vec<Value>,
    io: IoContext,
) -> (
    Option<Value>,
    Vec<IoOp>,
    IoUsage,
    Vec<IoArtifactReceipt>,
    Option<String>,
) {
    let interpreter = Interpreter {
        program,
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir,
        io: Some(io),
        net: None,
        capsules: None,
        concurrency: ConcurrencyTrace::default(),
        tracer: None,
    };
    let result = interpreter.execute_named(entry, args);
    let ops = interpreter
        .io
        .as_ref()
        .map(IoContext::ops)
        .unwrap_or_default();
    let usage = interpreter
        .io
        .as_ref()
        .map(IoContext::usage)
        .unwrap_or_default();
    let artifacts = interpreter
        .io
        .as_ref()
        .map(IoContext::artifacts)
        .unwrap_or_default();
    match result {
        Ok((value, _)) => (Some(value), ops, usage, artifacts, None),
        Err(err) => (None, ops, usage, artifacts, Some(err)),
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir,
        io: None,
        net: Some(NetContext::new(allow)),
        capsules: None,
        concurrency: ConcurrencyTrace::default(),
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: None,
        io: None,
        net: None,
        capsules: None,
        concurrency: ConcurrencyTrace::default(),
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir,
        io: None,
        net: None,
        capsules: Some(match allow {
            Some(allow) => CapsuleHost::with_effects(allow),
            None => CapsuleHost::new(),
        }),
        concurrency: ConcurrencyTrace::default(),
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(program, function)?;
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
    Ok(append_concurrency_receipt(
        run_result_with_capsules_to_json(&result, &calls),
        &interpreter.concurrency.events(),
    ))
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
                        ", \"native\": {{\"schema\": {}, \"boundary_hash\": {}, \"toolchain\": {}, \"toolchain_semver\": {}, \"toolchain_requirement\": {}, \"duration_ms\": {}, \"wall_limit_ms\": {}, \"wall_limit_source\": {}, \"compile_wall_limit_ms\": {}, \"compile_containment\": {}, \"borrowed_input_bytes\": {}, \"owned_output_bytes\": {}, \"artifact\": {}, \"containment\": {}, \"cpu_limit_ms\": {}, \"cpu_limit_source\": {}, \"memory_limit_bytes\": {}, \"memory_limit_source\": {}, \"peak_tree_memory_bytes\": {}, \"process_limit\": {}, \"process_limit_source\": {}, \"peak_tree_processes\": {}, \"output_limit_bytes\": {}, \"output_limit_source\": {}, \"stdout_bytes\": {}, \"stderr_bytes\": {}, \"runner_reused\": {}, \"runner_artifact_sha256\": {}, \"runner_cache_status\": {}, \"runner_input_transport\": {}, \"runner_execution_model\": {}, \"runner_environment_policy\": {}, \"runner_working_directory_policy\": {}, \"runner_scratch_access_policy\": {}, \"windows_ui_restrictions\": {}, \"ui_isolation_source\": {}, \"windows_restricted_token\": {}, \"windows_restricting_sid_count\": {}, \"windows_integrity_level\": {}, \"windows_appcontainer\": {}, \"windows_appcontainer_capability_count\": {}, \"windows_appcontainer_profile\": {}, \"runner_process_identity_policy\": {}, \"runner_capability_profile\": {}, \"filesystem_isolation\": {}, \"network_isolation\": {}, \"io_limit_bytes\": {}, \"io_limit_source\": {}, \"job_read_transfer_bytes\": {}, \"job_write_transfer_bytes\": {}, \"job_other_transfer_bytes\": {}, \"gpu\": {}}}",
                        quoted_json(&receipt.schema),
                        quoted_json(&receipt.boundary_hash),
                        quoted_json(&receipt.toolchain),
                        receipt
                            .toolchain_semver
                            .as_deref()
                            .map(quoted_json)
                            .unwrap_or_else(|| "null".to_string()),
                        receipt
                            .toolchain_requirement
                            .as_deref()
                            .map(quoted_json)
                            .unwrap_or_else(|| "null".to_string()),
                        receipt.duration_ms,
                        receipt
                            .wall_limit_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.wall_limit_source),
                        receipt.compile_wall_limit_ms,
                        quoted_json(&receipt.compile_containment),
                        receipt
                            .borrowed_input_bytes
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
                        quoted_json(&receipt.cpu_limit_source),
                        receipt.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.memory_limit_source),
                        receipt.peak_tree_memory_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.process_limit.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.process_limit_source),
                        receipt.peak_tree_processes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.output_limit_bytes,
                        quoted_json(&receipt.output_limit_source),
                        receipt.stdout_bytes,
                        receipt.stderr_bytes,
                        receipt.runner_reused,
                        quoted_json(&receipt.runner_artifact_sha256),
                        quoted_json(&receipt.runner_cache_status),
                        quoted_json(&receipt.runner_input_transport),
                        quoted_json(&receipt.runner_execution_model),
                        quoted_json(&receipt.runner_environment_policy),
                        quoted_json(&receipt.runner_working_directory_policy),
                        quoted_json(&receipt.runner_scratch_access_policy),
                        receipt.windows_ui_restrictions.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.ui_isolation_source),
                        receipt.windows_restricted_token.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.windows_restricting_sid_count.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.windows_integrity_level.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string()),
                        receipt.windows_appcontainer.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.windows_appcontainer_capability_count.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.windows_appcontainer_profile.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.runner_process_identity_policy),
                        quoted_json(&receipt.runner_capability_profile),
                        quoted_json(&receipt.filesystem_isolation),
                        quoted_json(&receipt.network_isolation),
                        receipt.io_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.io_limit_source),
                        receipt.job_read_transfer_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.job_write_transfer_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.job_other_transfer_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        receipt.gpu.as_ref().map(|gpu| format!(
                            "{{\"target\": {}, \"device_index\": {}, \"device_name\": {}, \"compute_capability\": {}, \"driver_version\": {}, \"visible_devices\": {}, \"host_memory_limit\": {}, \"jit_cache\": {}}}",
                            quoted_json(&gpu.target),
                            gpu.device_index,
                            quoted_json(&gpu.device_name),
                            quoted_json(&gpu.compute_capability),
                            quoted_json(&gpu.driver_version),
                            quoted_json(&gpu.visible_devices),
                            quoted_json(&gpu.host_memory_limit),
                            quoted_json(&gpu.jit_cache)
                        )).unwrap_or_else(|| "null".to_string())
                    )
                })
                .unwrap_or_default();
            let managed = call
                .managed
                .as_ref()
                .map(|receipt| {
                    let enforced = receipt
                        .enforced
                        .iter()
                        .map(|(key, value, source)| {
                            format!(
                                "{{\"limit\": {}, \"value\": {}, \"source\": {}}}",
                                quoted_json(key),
                                quoted_json(value),
                                quoted_json(source)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let waived = receipt
                        .waived
                        .iter()
                        .map(|item| quoted_json(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let granted_env = receipt
                        .granted_env
                        .iter()
                        .map(|item| quoted_json(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        ", \"managed\": {{\"schema\": \"forgentex.managed_receipt.v1\", \"profile\": {}, \"warm\": {}, \"enforced\": [{enforced}], \"waived\": [{waived}], \"environment\": \"explicit_allowlist\", \"granted_env\": [{granted_env}], \"diagnostics\": {}, \"filesystem_isolation\": \"none\", \"network_isolation\": \"none\"}}",
                        quoted_json(&receipt.profile),
                        receipt.warm,
                        quoted_json(&receipt.diagnostics)
                    )
                })
                .unwrap_or_default();
            let native_failure = call
                .native_failure
                .as_ref()
                .map(|receipt| {
                    format!(
                        ", \"native_failure\": {{\"schema\": {}, \"kind\": {}, \"phase\": {}, \"duration_ms\": {}, \"wall_limit_ms\": {}, \"wall_limit_source\": {}, \"compile_wall_limit_ms\": {}, \"cpu_limit_ms\": {}, \"cpu_limit_source\": {}, \"memory_limit_bytes\": {}, \"memory_limit_source\": {}, \"process_limit\": {}, \"process_limit_source\": {}, \"output_limit_bytes\": {}, \"output_limit_source\": {}, \"limits_applied\": {}, \"containment\": {}, \"runner_environment_policy\": {}, \"runner_working_directory_policy\": {}, \"io_limit_bytes\": {}, \"io_limit_source\": {}}}",
                        quoted_json(&receipt.schema),
                        quoted_json(&receipt.kind),
                        quoted_json(&receipt.phase),
                        receipt.duration_ms,
                        receipt.wall_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.wall_limit_source),
                        receipt.compile_wall_limit_ms,
                        receipt.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.cpu_limit_source),
                        receipt.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.memory_limit_source),
                        receipt.process_limit.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.process_limit_source),
                        receipt.output_limit_bytes,
                        quoted_json(&receipt.output_limit_source),
                        receipt.limits_applied,
                        receipt.containment.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string()),
                        receipt.runner_environment_policy.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string()),
                        receipt.runner_working_directory_policy.as_deref().map(quoted_json).unwrap_or_else(|| "null".to_string()),
                        receipt.io_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                        quoted_json(&receipt.io_limit_source)
                    )
                })
                .unwrap_or_default();
            format!(
                "{{\"capsule\": {}, \"language\": {}, \"decision\": {}, \"redacted_inputs\": [{}]{native}{native_failure}{managed}}}",
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
        CapsulePolicy {
            warm: false,
            ..CapsulePolicy::default()
        },
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
    policy: CapsulePolicy,
) -> Result<String, String> {
    run_entry_json_hosted_impl(
        program,
        Some(ir),
        entry,
        input,
        capsule_allow,
        fs_grants,
        net_allow,
        policy,
    )
}

/// How a hosted run executes capsules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsulePolicy {
    /// One persistent worker per capsule (rt.3). Used only for capsules that
    /// declare no limits, since a shared worker cannot apply per-call limits.
    pub warm: bool,
    /// The operator's managed-capsule policy, including whether the trusted
    /// profile is granted.
    pub managed: crate::managed_exec::ManagedPolicy,
}

impl Default for CapsulePolicy {
    fn default() -> Self {
        Self {
            warm: true,
            managed: crate::managed_exec::ManagedPolicy::default(),
        }
    }
}

/// Languages that run through a host runtime rather than a compiled runner.
fn is_managed_language(language: &str) -> bool {
    matches!(
        language,
        "python" | "node" | "typescript" | "go" | "java" | "csharp" | "julia" | "moonbit"
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
    policy: CapsulePolicy,
) -> Result<String, String> {
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir,
        io: Some(IoContext::new(fs_grants)),
        net: Some(NetContext::new(net_allow)),
        capsules: Some(
            match capsule_allow {
                Some(allow) => CapsuleHost::with_effects(allow),
                None => CapsuleHost::new(),
            }
            .with_warm(policy.warm)
            .with_managed_policy(policy.managed),
        ),
        concurrency: ConcurrencyTrace::default(),
        tracer: None,
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(program, function)?;
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
    let json = format!(
        "{body},\n  \"capsule_calls\": [{}],\n  \"io_ops\": [{io_json}],\n  \"net_ops\": [{net_json}]\n}}",
        capsule_calls_json(&calls)
    );
    Ok(append_concurrency_receipt(
        json,
        &interpreter.concurrency.events(),
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: None,
        io: None,
        net: None,
        capsules: Some(CapsuleHost::new().with_warm(true)),
        concurrency: ConcurrencyTrace::default(),
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: Some(&ir),
        io: None,
        net: None,
        capsules: Some(CapsuleHost::new()),
        concurrency: ConcurrencyTrace::default(),
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
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: Some(&ir),
        io: None,
        net: None,
        capsules: Some(CapsuleHost::with_effects(allow)),
        concurrency: ConcurrencyTrace::default(),
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
    invoke_capsule_live_args(program, capsule_name, vec![input], allow)
}

/// Multi-input form of `invoke_capsule_live`, used by deterministic pipeline
/// fan-in. Argument order remains the capsule declaration order.
pub fn invoke_capsule_live_args(
    program: &Program,
    capsule_name: &str,
    inputs: Vec<Value>,
    allow: Option<Vec<String>>,
) -> (Result<Value, String>, Vec<CapsuleCall>) {
    let (result, calls) =
        invoke_capsule_live_args_inner(program, capsule_name, inputs, allow, false);
    (result.map(|outcome| outcome.output), calls)
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleLiveMutation {
    pub input_index: usize,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapsuleLiveOutcome {
    pub output: Value,
    pub mutations: Vec<CapsuleLiveMutation>,
}

/// Pipeline-only live invocation that exposes already validated mutable-slice
/// copy-back values without committing them into an interpreter environment.
pub fn invoke_capsule_live_transactional_args(
    program: &Program,
    capsule_name: &str,
    inputs: Vec<Value>,
    allow: Option<Vec<String>>,
) -> (Result<CapsuleLiveOutcome, String>, Vec<CapsuleCall>) {
    invoke_capsule_live_args_inner(program, capsule_name, inputs, allow, true)
}

fn invoke_capsule_live_args_inner(
    program: &Program,
    capsule_name: &str,
    inputs: Vec<Value>,
    allow: Option<Vec<String>>,
    transactional: bool,
) -> (Result<CapsuleLiveOutcome, String>, Vec<CapsuleCall>) {
    let interpreter = Interpreter {
        program,
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: None,
        io: None,
        net: None,
        capsules: Some(match allow {
            Some(allow) => CapsuleHost::with_effects(allow),
            None => CapsuleHost::new(),
        }),
        concurrency: ConcurrencyTrace::default(),
        tracer: None,
    };
    let result = match program
        .capsules()
        .find(|capsule| capsule.name == capsule_name)
    {
        Some(capsule) => {
            let source_args = if transactional {
                (0..inputs.len())
                    .map(|index| Expr::Var(format!("__pipeline_arg_{index}")))
                    .collect::<Vec<_>>()
            } else {
                // Ordinary callers do not authorize mutable copy-back.
                vec![Expr::Int(0); inputs.len()]
            };
            let mut env = HashMap::new();
            if transactional {
                for (index, value) in inputs.iter().enumerate() {
                    env.insert(format!("__pipeline_arg_{index}"), value.clone());
                }
            }
            interpreter
                .call_capsule(capsule, &source_args, inputs, &mut env)
                .map(|output| {
                    let mutations = if transactional {
                        capsule
                            .inputs
                            .iter()
                            .enumerate()
                            .filter(|(_, binding)| binding.ty.is_mutable_slice())
                            .filter_map(|(input_index, _)| {
                                env.remove(&format!("__pipeline_arg_{input_index}"))
                                    .map(|value| CapsuleLiveMutation { input_index, value })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    CapsuleLiveOutcome { output, mutations }
                })
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
    /// One map per active call frame, giving the width each arithmetic node in
    /// that frame is checked at, keyed by node address.
    ///
    /// Integer arithmetic traps at the **declared** width rather than at 64
    /// bits, so `i32 + i32` overflowing an `i32` is an error even though the sum
    /// fits an `i64`. The evaluator has no type environment, so `int_width`
    /// supplies one per frame.
    ///
    /// A frame rather than the whole program, because the FIR path rebuilds a
    /// function's statements on every call: those nodes live only as long as the
    /// call, and a later call can reuse the same addresses. Evaluation is
    /// strictly nested — a callee's frame is pushed only after the caller has
    /// finished evaluating its arguments — so the top of the stack is always the
    /// frame the expression being evaluated belongs to.
    int_widths: std::cell::RefCell<Vec<HashMap<usize, String>>>,
    fir: Option<&'a BackendIr>,
    io: Option<IoContext>,
    net: Option<NetContext>,
    // The live capsule runtime (rt.1). `None` forbids live capsule calls, so
    // the default deterministic interpreter never spawns a host runtime.
    capsules: Option<CapsuleHost>,
    // The debugger capability: when present, every executed statement is
    // traced against its canonical node id (the same ids semantic patches
    // target). `None` on normal runs — zero tracing cost.
    concurrency: ConcurrencyTrace,
    tracer: Option<std::cell::RefCell<DebugTracer>>,
}

impl Interpreter<'_> {
    fn record_concurrency(
        &self,
        operation: &str,
        target: &str,
        decision: &str,
        count: usize,
        capacity: Option<usize>,
    ) {
        self.concurrency
            .record(operation, target, decision, count, capacity);
    }
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
    /// 1-based source line, resolved from the source map after the run. `None`
    /// when the node has no recorded span.
    pub line: Option<usize>,
    /// Watch expressions evaluated after this step, as `(source, value)`. A
    /// watch that cannot be evaluated here records its error instead.
    pub watches: Vec<(String, String)>,
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
    /// 1-based source line of the function header, resolved after the run.
    pub line: Option<usize>,
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
    /// Collection elements rendered per traced value; `usize::MAX` renders in
    /// full. See `DEBUG_VALUE_ELEMENTS`.
    value_elements: usize,
    /// Stop at this node only when the expression evaluates true.
    break_if: Option<(String, Expr)>,
    /// Expressions evaluated after every step, as `(source, parsed)`.
    watches: Vec<(String, Expr)>,
    /// Set while a condition or watch is being evaluated. Such an evaluation
    /// can call a function, which re-enters `exec_stmt`; without this the
    /// tracer would record the evaluation's own statements as program steps
    /// and re-enter its own `RefCell` borrow.
    evaluating: bool,
    /// Present only for an interactive session.
    session: Option<SessionChannels>,
    /// Current call depth, maintained alongside the enter/exit call records.
    /// `next` and `step-out` are depth comparisons against it.
    depth: usize,
    /// What to do at the next statement.
    mode: StepMode,
    /// Depth at which the pending `next` or `step-out` was issued.
    mode_depth: usize,
    /// Set when a session is asked to quit.
    cancelled: bool,
    /// Shared breakpoint node set, editable by a driver mid-run.
    break_nodes: Option<std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>>,
    /// Keep only the last N steps. `None` keeps them all.
    tail: Option<usize>,
    /// Steps dropped by `tail`, so the report can say what is missing rather
    /// than presenting a truncated trace as a complete one.
    dropped: usize,
}

impl DebugTracer {
    fn new(
        break_at: Option<String>,
        max_steps: usize,
        value_elements: usize,
        break_if: Option<(String, Expr)>,
        watches: Vec<(String, Expr)>,
    ) -> Self {
        Self {
            steps: Vec::new(),
            calls: Vec::new(),
            frames: Vec::new(),
            breakpoint: None,
            break_at,
            max_steps,
            seq: 0,
            value_elements,
            break_if,
            watches,
            evaluating: false,
            session: None,
            depth: 0,
            mode: StepMode::default(),
            mode_depth: 0,
            cancelled: false,
            break_nodes: None,
            tail: None,
            dropped: 0,
        }
    }

    /// Drop the oldest steps once the retained window is full, counting what
    /// was dropped so the report can say the trace is a tail rather than a
    /// complete run.
    fn trim_to_tail(&mut self) {
        let Some(tail) = self.tail else { return };
        if tail == 0 {
            self.dropped += self.steps.len();
            self.steps.clear();
            return;
        }
        while self.steps.len() > tail {
            self.steps.remove(0);
            self.dropped += 1;
        }
    }

    fn snapshot_with(env: &HashMap<String, Value>, elements: usize) -> Vec<(String, String)> {
        let mut locals: Vec<(String, String)> = env
            .iter()
            .map(|(name, value)| (name.clone(), summarize_value(value, elements)))
            .collect();
        locals.sort();
        locals
    }
}

/// Elements of a collection rendered before the tail is elided. A debug run
/// that reads a 2.6 MB file otherwise renders the whole buffer on every step:
/// 40 steps produced 11.3 MB of trace before this cap existed.
const DEBUG_VALUE_ELEMENTS: usize = 8;

/// Render a value for a trace, eliding the tail of long collections and
/// strings. `elements == usize::MAX` renders in full, which `--values full`
/// selects when the whole buffer really is what the reader needs.
pub fn summarize_value(value: &Value, elements: usize) -> String {
    if elements == usize::MAX {
        return value_to_text(value);
    }
    match value {
        Value::Array(values) if values.len() > elements => {
            let head = values
                .iter()
                .take(elements)
                .map(|item| summarize_value(item, elements))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{head}, … +{} more]", values.len() - elements)
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|item| summarize_value(item, elements))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // Cut on a character boundary so a multi-byte string cannot panic.
        Value::String(text) if text.chars().count() > DEBUG_STRING_CHARS => {
            let head: String = text.chars().take(DEBUG_STRING_CHARS).collect();
            format!("\"{head}…\" (len {})", text.chars().count())
        }
        Value::Struct(name, fields) => {
            let mut entries = fields
                .iter()
                .map(|(field, value)| format!("{field}: {}", summarize_value(value, elements)))
                .collect::<Vec<_>>();
            entries.sort();
            format!("{name} {{ {} }}", entries.join(", "))
        }
        other => value_to_text(other),
    }
}

const DEBUG_STRING_CHARS: usize = 64;

/// The bindings a leaf statement can write.
///
/// A statement writes only through its `let` name, its assignment root, or a
/// bare variable passed to a callee that copies back into it (a `&mut`
/// parameter, or a mutating builtin such as `push`). A projected argument such
/// as `f(buf[0 .. 8])` cannot be a copy-back target, and a call that only reads
/// its argument is over-reported rather than missed — the safe direction for a
/// debugger.
fn stmt_write_targets(stmt: &Stmt) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    match stmt {
        Stmt::Let { name, expr, .. } => {
            names.insert(name.clone());
            collect_call_receivers(expr, &mut names);
        }
        Stmt::Assign { target, expr } => {
            if let Some(root) = place_root(target) {
                names.insert(root);
            }
            collect_call_receivers(target, &mut names);
            collect_call_receivers(expr, &mut names);
        }
        Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
            collect_call_receivers(expr, &mut names)
        }
        _ => {}
    }
    names
}

/// The variable a place expression ultimately writes: `v`, `v[i]`, and
/// `v.field` all root at `v`.
fn place_root(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Var(name) => Some(name.clone()),
        Expr::Index { base, .. } => place_root(base),
        Expr::Field { base, .. } => place_root(base),
        _ => None,
    }
}

/// Bare-variable arguments of every call in an expression. These are the only
/// arguments a callee can copy back into. Lambda bodies are not descended into:
/// captures are by value, so a call inside one writes the closure's own
/// environment, never this statement's.
fn collect_call_receivers(expr: &Expr, out: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::Call { args, .. } => {
            for arg in args {
                if let Expr::Var(name) = arg {
                    out.insert(name.clone());
                }
                collect_call_receivers(arg, out);
            }
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => collect_call_receivers(expr, out),
        Expr::Binary { left, right, .. } => {
            collect_call_receivers(left, out);
            collect_call_receivers(right, out);
        }
        Expr::Field { base, .. } => collect_call_receivers(base, out),
        Expr::Index { base, index } => {
            collect_call_receivers(base, out);
            collect_call_receivers(index, out);
        }
        Expr::SliceRange { base, start, end } => {
            collect_call_receivers(base, out);
            collect_call_receivers(start, out);
            collect_call_receivers(end, out);
        }
        Expr::Var(_)
        | Expr::Lambda { .. }
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::TraitConst { .. } => {}
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
    /// Execute `foreign` capsules for real instead of trapping with
    /// `capsule_call_not_permitted`. Off by default so a plain debug run stays
    /// pure, cheap, and free of host side effects.
    pub live: bool,
    /// Effect grants handed to the capsule host when `live` is set. `None`
    /// means the host's default grant set, matching `run --live`.
    pub allow_effects: Option<Vec<String>>,
    /// Hosted filesystem and network access, matching `run --hosted`. Required
    /// to debug an application entry that reads or writes files.
    pub hosted: bool,
    pub fs_grants: Vec<String>,
    pub net_allow: Vec<String>,
    /// Collection elements rendered per traced value. `usize::MAX` renders in
    /// full, which `--values full` selects.
    pub value_elements: usize,
    /// Stop at this node only when the expression is true, as `(node, expr)`.
    pub break_if: Option<(String, Expr)>,
    /// Expressions evaluated after every step, as `(source, parsed)`.
    pub watches: Vec<(String, Expr)>,
    /// Set by `debug_session` so the run starts paused at the first statement.
    /// `None` for a one-shot run.
    pub session_mode: Option<StepMode>,
    /// Nodes to stop at, shared with a driver so breakpoints can be edited
    /// while the interpreter is paused. Independent of `break_at`, which is the
    /// one-shot CLI's single breakpoint.
    pub break_nodes: Option<std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>>,
    /// Retain only the last N steps. A long run is bounded by `max_steps`,
    /// which keeps the *first* N; `tail` keeps the last, which is usually where
    /// a failure is.
    pub tail: Option<usize>,
    /// A manifest-derived I/O policy, used in preference to `fs_grants`. This
    /// is what lets an application entry be debugged: its declared artifacts,
    /// scopes, and byte limits come along, so a publish inside the debugger
    /// behaves as it does under `fgx app-run`.
    pub io_policy: Option<IoPolicy>,
}

impl Default for DebugOptions {
    fn default() -> Self {
        Self {
            break_at: None,
            max_steps: 10_000,
            live: false,
            allow_effects: None,
            hosted: false,
            fs_grants: Vec::new(),
            net_allow: Vec::new(),
            value_elements: DEBUG_VALUE_ELEMENTS,
            break_if: None,
            watches: Vec::new(),
            session_mode: None,
            break_nodes: None,
            tail: None,
            io_policy: None,
        }
    }
}

// --- Interactive debug sessions (phase 4) ----------------------------------

/// What the session should do when it next reaches a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StepMode {
    /// Run until a breakpoint or the end of the program.
    #[default]
    Continue,
    /// Stop at the very next statement, including inside a call.
    StepIn,
    /// Stop at the next statement in this frame or a shallower one, stepping
    /// over calls made from here.
    Next,
    /// Stop only once this frame has returned.
    StepOut,
}

/// A command sent to a paused session.
#[derive(Clone, Debug, PartialEq)]
pub enum DebugCommand {
    StepIn,
    Next,
    StepOut,
    Continue,
    /// Evaluate an expression in the paused frame. The session stays paused.
    Evaluate(String),
    /// Abandon the run; the session finishes with `status: "cancelled"`.
    Quit,
}

/// Where a paused session is sitting.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugStop {
    pub node: String,
    pub line: Option<usize>,
    pub function: String,
    pub depth: usize,
    pub text: String,
    pub locals: Vec<(String, String)>,
}

/// What a session reports back.
#[derive(Clone, Debug)]
pub enum DebugEvent {
    /// Execution is paused before `stop.node`; the session awaits a command.
    Stopped(DebugStop),
    /// The result of an `Evaluate`, or the error explaining why it could not be
    /// evaluated here. The session is still paused.
    Evaluated {
        source: String,
        value: Result<String, String>,
    },
    /// The run ended. No further commands are read.
    Finished(Box<DebugReport>),
}

/// The paused-session plumbing handed to the tracer: an outbound event channel
/// and an inbound command channel. Blocking on the command channel is what
/// suspends the interpreter mid-run.
struct SessionChannels {
    events: std::sync::mpsc::Sender<DebugEvent>,
    commands: std::sync::mpsc::Receiver<DebugCommand>,
}

/// The result of a debug run (`forgentex.debug_trace.v1`): status, return
/// value, the step trace, the dynamic call tree, breakpoint state, and trap
/// post-mortem frames. Agents consume `to_json()`; humans read `to_text()`.
#[derive(Clone, Debug)]
pub struct DebugReport {
    pub entry: String,
    pub status: String,
    pub return_text: Option<String>,
    pub error: Option<String>,
    pub steps: Vec<DebugStep>,
    pub calls: Vec<DebugCall>,
    pub frames: Vec<DebugFrame>,
    pub breakpoint: Option<(String, Vec<(String, String)>)>,
    /// Audit transcript for every `foreign` capsule executed during the run.
    /// Empty unless the run was started with `live`.
    pub capsule_calls: Vec<CapsuleCall>,
    /// Steps executed but not retained, when `tail` bounded the trace. Zero
    /// means `steps` is the whole run.
    pub dropped_steps: usize,
    /// Task, channel, timer, and readiness transitions recorded during the run.
    ///
    /// A concurrent program's statements do not show why it is waiting: a park
    /// or a refused send is a transition, not a statement. Carrying the trace
    /// alongside the steps is what makes a stalled task diagnosable.
    pub concurrency: Vec<ConcurrencyEvent>,
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
    /// Attach source positions to every step and frame.
    ///
    /// Canonical node ids are the stable vocabulary the tracer records; line
    /// numbers are a projection resolved afterwards, so the interpreter stays
    /// free of span concerns. A node with no recorded span keeps `None` rather
    /// than being given a misleading position.
    pub fn resolve_positions(&mut self, map: &crate::span::SourceMap) {
        let line_of = |node: &str| {
            map.nodes
                .iter()
                .find(|entry| entry.node == node)
                .map(|entry| entry.span.start_line)
        };
        for step in &mut self.steps {
            step.line = line_of(&step.node);
        }
        for frame in &mut self.frames {
            frame.line = line_of(&format!("@function.{}", frame.function));
        }
    }

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
                "\n    {{\"seq\": {}, \"node\": {}, \"kind\": {}, \"text\": {}, \"changed\": {}, \"line\": {}, \"watches\": {}}}",
                step.seq,
                quoted_json(&step.node),
                quoted_json(&step.kind),
                quoted_json(&step.text),
                kv_object(&step.changed),
                step.line
                    .map(|line| line.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                kv_object(&step.watches)
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
                "\n    {{\"function\": {}, \"locals\": {}, \"line\": {}}}",
                quoted_json(&frame.function),
                kv_object(&frame.locals),
                frame
                    .line
                    .map(|line| line.to_string())
                    .unwrap_or_else(|| "null".to_string())
            ));
        }
        out.push_str("\n  ],\n");
        out.push_str(&format!("  \"dropped_steps\": {},\n", self.dropped_steps));
        // Every transition is kept in JSON: the text form groups them for
        // reading, but an agent needs the ordered sequence to follow causality.
        out.push_str("  \"concurrency\": [");
        for (idx, event) in self.concurrency.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"sequence\": {}, \"operation\": {}, \"target\": {}, \"decision\": {}, \"count\": {}, \"capacity\": {}}}",
                event.sequence,
                quoted_json(&event.operation),
                quoted_json(&event.target),
                quoted_json(&event.decision),
                event.count,
                event
                    .capacity
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string())
            ));
        }
        out.push_str(if self.concurrency.is_empty() {
            "],\n"
        } else {
            "\n  ],\n"
        });
        out.push_str(&format!(
            "  \"capsule_calls\": {}\n}}",
            capsule_calls_json(&self.capsule_calls)
        ));
        out
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("debug {} status={}\n", self.entry, self.status));
        if self.dropped_steps > 0 {
            // Say so plainly: a tail is not a complete trace, and reading one
            // as if it were makes the first line look like the start of the run.
            out.push_str(&format!(
                "showing the last {} steps; {} earlier steps dropped\n",
                self.steps.len(),
                self.dropped_steps
            ));
        }
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
            let watched = if step.watches.is_empty() {
                String::new()
            } else {
                format!(
                    "  {{{}}}",
                    step.watches
                        .iter()
                        .map(|(source, value)| format!("{source} = {value}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            out.push_str(&format!(
                "  {:>4}  {:>5}  {}  {}{changed}{watched}\n",
                step.seq,
                step.line.map(|line| format!("L{line}")).unwrap_or_default(),
                step.node,
                step.text
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
                match frame.line {
                    Some(line) => out.push_str(&format!("  fn {} (L{line})\n", frame.function)),
                    None => out.push_str(&format!("  fn {}\n", frame.function)),
                }
                for (name, value) in &frame.locals {
                    out.push_str(&format!("    {name} = {value}\n"));
                }
            }
        }
        if !self.concurrency.is_empty() {
            // Grouped by operation and decision: a raw transition list is long
            // and repetitive, while the shape of a stall — many parks, a
            // refused send — reads at a glance.
            let mut grouped: std::collections::BTreeMap<(&str, &str), usize> =
                std::collections::BTreeMap::new();
            for event in &self.concurrency {
                *grouped
                    .entry((event.operation.as_str(), event.decision.as_str()))
                    .or_default() += 1;
            }
            out.push_str(&format!(
                "concurrency: {} transitions\n",
                self.concurrency.len()
            ));
            for ((operation, decision), count) in grouped {
                out.push_str(&format!("  {operation} {decision} x{count}\n"));
            }
        }
        if !self.capsule_calls.is_empty() {
            out.push_str("capsule calls:\n");
            for call in &self.capsule_calls {
                out.push_str(&format!(
                    "  {} [{}] {}\n",
                    call.capsule, call.language, call.decision
                ));
            }
        }
        out
    }
}

/// Run an entry function as a paused, steppable session.
///
/// The interpreter runs on a scoped thread and blocks on the command channel
/// at each stop, which is what suspends it mid-run: the tree-walking evaluator
/// has no continuation to resume, so the pause has to be a real blocked thread.
/// `Program` is `Send + Sync` and the AST has no interior mutability, so only a
/// shared reference crosses the boundary.
///
/// `driver` receives every event and returns the next command. It is called on
/// the caller's thread; the run finishes when it sees `Finished`.
pub fn debug_session<F>(
    program: &Program,
    entry: &str,
    input: &str,
    options: DebugOptions,
    mut driver: F,
) -> Result<DebugReport, String>
where
    // The driver runs on the caller's thread, so it needs no `Send` bound;
    // only the interpreter closure crosses into the scoped thread.
    F: FnMut(&DebugEvent) -> DebugCommand,
{
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let (command_tx, command_rx) = std::sync::mpsc::channel();

    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let mut options = options;
            // A session starts paused at the first statement.
            options.session_mode = Some(StepMode::StepIn);
            debug_entry_with_session(
                program,
                entry,
                input,
                options,
                SessionChannels {
                    events: event_tx,
                    commands: command_rx,
                },
            )
        });

        // Pump events until the worker reports it has finished.
        while let Ok(event) = event_rx.recv() {
            let finished = matches!(event, DebugEvent::Finished(_));
            let command = driver(&event);
            if finished {
                break;
            }
            if command_tx.send(command).is_err() {
                break;
            }
        }
        drop(command_tx);

        worker
            .join()
            .map_err(|_| "debug session panicked".to_string())?
    })
}

/// Run an entry function under the tracing debugger (debugger v0). Executes
/// the deterministic AST path; see docs/debugger_plan.md.
pub fn debug_entry(
    program: &Program,
    entry: &str,
    input: &str,
    options: DebugOptions,
) -> Result<DebugReport, String> {
    debug_entry_impl(program, entry, input, options, None)
}

fn debug_entry_with_session(
    program: &Program,
    entry: &str,
    input: &str,
    options: DebugOptions,
    channels: SessionChannels,
) -> Result<DebugReport, String> {
    debug_entry_impl(program, entry, input, options, Some(channels))
}

fn debug_entry_impl(
    program: &Program,
    entry: &str,
    input: &str,
    options: DebugOptions,
    channels: Option<SessionChannels>,
) -> Result<DebugReport, String> {
    let input = RunInput::parse(input)?;
    // Runtime reach is opt-in. Without `live`/`hosted` the debugger keeps its
    // original pure behaviour, so an ordinary debug run still cannot reach the
    // filesystem, the network, or a foreign toolchain.
    let interpreter = Interpreter {
        program,
        int_widths: std::cell::RefCell::new(Vec::new()),
        fir: None,
        io: match (options.io_policy.clone(), options.hosted) {
            // A manifest policy carries artifact declarations and scopes that a
            // bare grant list cannot express, so it wins when present.
            (Some(policy), _) => Some(IoContext::from_policy(policy)),
            (None, true) => Some(IoContext::new(options.fs_grants.clone())),
            (None, false) => None,
        },
        net: if options.hosted {
            Some(NetContext::new(options.net_allow.clone()))
        } else {
            None
        },
        capsules: if options.live {
            Some(match options.allow_effects.clone() {
                Some(allow) => CapsuleHost::with_effects(allow),
                None => CapsuleHost::new(),
            })
        } else {
            None
        },
        concurrency: ConcurrencyTrace::default(),
        tracer: Some(std::cell::RefCell::new({
            let mut tracer = DebugTracer::new(
                options.break_at,
                options.max_steps,
                options.value_elements,
                options.break_if,
                options.watches,
            );
            tracer.session = channels;
            tracer.break_nodes = options.break_nodes.clone();
            tracer.tail = options.tail;
            if let Some(mode) = options.session_mode {
                tracer.mode = mode;
            }
            tracer
        })),
    };
    let function = program
        .find_function(entry)
        .ok_or_else(|| trap("unknown_entry", format!("unknown function '{entry}'")))?;
    let args = input.args_for(program, function)?;
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
        Err(err) if err.contains("debug_cancelled") => ("cancelled", None, None),
        Err(err) => ("trap", None, Some(err.clone())),
    };
    // A capsule call is one opaque step in the trace, so the transcript is
    // carried alongside it: without this the debugger shows that a foreign
    // boundary was crossed but no evidence of what happened across it.
    let capsule_calls = interpreter
        .capsules
        .as_ref()
        .map(CapsuleHost::calls)
        .unwrap_or_default();
    let report = DebugReport {
        entry: entry.to_string(),
        status: status.to_string(),
        return_text,
        error,
        steps: tracer.steps.clone(),
        calls: tracer.calls.clone(),
        frames: tracer.frames.clone(),
        breakpoint: tracer.breakpoint.clone(),
        capsule_calls,
        dropped_steps: tracer.dropped,
        concurrency: interpreter.concurrency.events(),
    };
    // Hand the finished report to a session driver before the channel closes,
    // so it sees a completion rather than inferring one from a hang-up.
    drop(tracer);
    if let Some(session) = interpreter
        .tracer
        .as_ref()
        .and_then(|tracer| tracer.borrow_mut().session.take())
    {
        let _ = session
            .events
            .send(DebugEvent::Finished(Box::new(report.clone())));
    }
    Ok(report)
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
    /// The operator's policy for managed capsules. Only the operator can grant
    /// the trusted profile; nothing a capsule declares can.
    managed: crate::managed_exec::ManagedPolicy,
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
    /// For managed capsules: which profile ran the call, which limits were
    /// enforced, and which declarations were waived under the trusted profile.
    pub managed: Option<ManagedReceipt>,
}

/// What was applied to one managed capsule call.
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedReceipt {
    /// `contained`, or `trusted` when a declared limit was waived.
    pub profile: String,
    /// `(key, value, source)` for each enforced limit.
    pub enforced: Vec<(String, String, String)>,
    /// Declared limits that were not applied. Empty unless the operator chose
    /// the trusted profile; never silently non-empty.
    pub waived: Vec<String>,
    /// Whether the call went through a persistent warm worker, which does not
    /// apply per-call limits and is therefore only used when none are declared.
    pub warm: bool,
    /// What the capsule itself printed, bounded. A capsule's output used to be
    /// taken as its result; now it is diagnostics, and diagnostics that vanish
    /// are their own debugging problem, so they are reported here.
    ///
    /// Warm calls only for now: the cold runners report a failing capsule's
    /// output in the error, but do not yet carry it back on success.
    pub diagnostics: String,
    /// Caller variables the capsule was granted by name. Everything else in the
    /// caller's environment is absent from the child.
    pub granted_env: Vec<String>,
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
            managed: crate::managed_exec::ManagedPolicy::default(),
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
            managed: crate::managed_exec::ManagedPolicy::default(),
        }
    }

    /// One warm call through this host's worker pool: spawn the capsule's
    /// worker on first use (sandbox-gated), then dispatch over the pipe. The
    /// enveloped response line is returned raw; the caller parses it.
    /// One warm call through this host's worker pool, with auto-respawn: if
    /// the worker died (a *transport* failure — broken pipe / EOF, not a
    /// capsule `{"err": …}`), replace it and retry the call once. A capsule's
    /// own error is deterministic and is never retried.
    /// What the capsule has printed since its worker last answered.
    pub(crate) fn warm_diagnostics(&self, capsule: &Capsule) -> String {
        self.workers
            .borrow()
            .get(&capsule.name)
            .map(|worker| worker.take_diagnostics())
            .unwrap_or_default()
    }

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

    /// Attach what was applied to the managed call recorded last.
    fn attach_managed(&self, receipt: ManagedReceipt) {
        if let Some(call) = self.calls.borrow_mut().last_mut() {
            call.managed = Some(receipt);
        }
    }

    /// Grant (or withhold) the trusted profile for managed capsules.
    pub fn with_managed_policy(mut self, policy: crate::managed_exec::ManagedPolicy) -> Self {
        self.managed = policy;
        self
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
            managed: None,
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
            managed: None,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IoUsage {
    pub read_bytes: usize,
    pub write_bytes: usize,
    pub directory_entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IoArtifactPolicy {
    pub name: String,
    pub root: PathBuf,
    pub media_type: String,
    pub max_bytes: usize,
    pub required: bool,
    pub publication: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IoArtifactReceipt {
    pub name: String,
    pub path: String,
    pub media_type: String,
    pub bytes: usize,
    pub sha256: String,
    pub publication: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IoPolicy {
    root: PathBuf,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    max_read_bytes: Option<usize>,
    max_write_bytes: Option<usize>,
    max_directory_entries: Option<usize>,
    artifacts: BTreeMap<String, IoArtifactPolicy>,
}

impl IoPolicy {
    pub fn scoped(
        root: &Path,
        read_scopes: &[PathBuf],
        write_scopes: &[PathBuf],
        max_read_bytes: Option<usize>,
        max_write_bytes: Option<usize>,
        max_directory_entries: Option<usize>,
        artifacts: Vec<IoArtifactPolicy>,
    ) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| {
            format!(
                "failed to resolve application root {}: {error}",
                root.display()
            )
        })?;
        let read_roots = canonical_scopes(&root, read_scopes, "fs.read")?;
        let write_roots = canonical_scopes(&root, write_scopes, "fs.write")?;
        let mut artifact_policies = BTreeMap::new();
        for mut artifact in artifacts {
            let artifact_root = root.join(&artifact.root).canonicalize().map_err(|error| {
                format!(
                    "failed to resolve application artifact '{}' root {}: {error}",
                    artifact.name,
                    artifact.root.display()
                )
            })?;
            if !artifact_root.is_dir() {
                return Err(format!(
                    "application artifact '{}' root {} is not a directory",
                    artifact.name,
                    artifact.root.display()
                ));
            }
            if !write_roots
                .iter()
                .any(|write_root| artifact_root.starts_with(write_root))
            {
                return Err(format!(
                    "application artifact '{}' root {} is outside the fs.write permission roots",
                    artifact.name,
                    artifact.root.display()
                ));
            }
            artifact.root = artifact_root;
            if artifact_policies
                .insert(artifact.name.clone(), artifact)
                .is_some()
            {
                return Err("duplicate application artifact policy".to_string());
            }
        }
        Ok(Self {
            root,
            read_roots,
            write_roots,
            max_read_bytes,
            max_write_bytes,
            max_directory_entries,
            artifacts: artifact_policies,
        })
    }
}

/// Runtime I/O host for `core_io_*` builtins: enforces granted capabilities
/// and records a deterministic transcript of every operation (v2.14).
pub struct IoContext {
    grants: std::collections::BTreeSet<String>,
    policy: Option<IoPolicy>,
    read_bytes: std::cell::Cell<usize>,
    write_bytes: std::cell::Cell<usize>,
    directory_entries: std::cell::Cell<usize>,
    ops: std::cell::RefCell<Vec<IoOp>>,
    artifacts: std::cell::RefCell<Vec<IoArtifactReceipt>>,
}

impl IoContext {
    pub fn new<I: IntoIterator<Item = String>>(grants: I) -> Self {
        Self {
            grants: grants.into_iter().collect(),
            policy: None,
            read_bytes: std::cell::Cell::new(0),
            write_bytes: std::cell::Cell::new(0),
            directory_entries: std::cell::Cell::new(0),
            ops: std::cell::RefCell::new(Vec::new()),
            artifacts: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn from_policy(policy: IoPolicy) -> Self {
        let mut grants = std::collections::BTreeSet::new();
        if !policy.read_roots.is_empty() {
            grants.insert("fs.read".to_string());
        }
        if !policy.write_roots.is_empty() {
            grants.insert("fs.write".to_string());
        }
        Self {
            grants,
            policy: Some(policy),
            read_bytes: std::cell::Cell::new(0),
            write_bytes: std::cell::Cell::new(0),
            directory_entries: std::cell::Cell::new(0),
            ops: std::cell::RefCell::new(Vec::new()),
            artifacts: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn ops(&self) -> Vec<IoOp> {
        self.ops.borrow().clone()
    }

    pub fn usage(&self) -> IoUsage {
        IoUsage {
            read_bytes: self.read_bytes.get(),
            write_bytes: self.write_bytes.get(),
            directory_entries: self.directory_entries.get(),
        }
    }

    pub fn artifacts(&self) -> Vec<IoArtifactReceipt> {
        self.artifacts.borrow().clone()
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
        let resolved = match self.resolve_path(path, false) {
            Ok(path) => path,
            Err(error) => {
                self.record("read", path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        if self.policy.is_some() {
            let bytes = self.read_within_budget("read", path, &resolved)?;
            let contents = String::from_utf8(bytes).map_err(|error| {
                self.record("read", path, 0, "error");
                trap("io_error", format!("read {path}: invalid UTF-8: {error}"))
            })?;
            self.add_budget(contents.len(), false);
            self.record("read", path, contents.len(), "allow");
            Ok(contents)
        } else {
            match std::fs::read_to_string(&resolved) {
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
    }

    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        if !self.grants.contains("fs.read") {
            self.record("read_bytes", path, 0, "deny");
            return Err(trap("io_denied", format!("fs.read not granted for {path}")));
        }
        let resolved = match self.resolve_path(path, false) {
            Ok(path) => path,
            Err(error) => {
                self.record("read_bytes", path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        if self.policy.is_some() {
            let bytes = self.read_within_budget("read_bytes", path, &resolved)?;
            self.add_budget(bytes.len(), false);
            self.record("read_bytes", path, bytes.len(), "allow");
            Ok(bytes)
        } else {
            match std::fs::read(&resolved) {
                Ok(bytes) => {
                    self.record("read_bytes", path, bytes.len(), "allow");
                    Ok(bytes)
                }
                Err(error) => {
                    self.record("read_bytes", path, 0, "error");
                    Err(trap("io_error", format!("read_bytes {path}: {error}")))
                }
            }
        }
    }

    fn list_dir(&self, path: &str) -> Result<Vec<Value>, String> {
        if !self.grants.contains("fs.read") {
            self.record("list_dir", path, 0, "deny");
            return Err(trap("io_denied", format!("fs.read not granted for {path}")));
        }
        let normalized = normalize_portable_path(path).map_err(|error| {
            self.record("list_dir", path, 0, "deny");
            trap("io_denied", error)
        })?;
        let resolved = match self.resolve_path(&normalized, false) {
            Ok(path) => path,
            Err(error) => {
                self.record("list_dir", path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        if !resolved.is_dir() {
            self.record("list_dir", path, 0, "error");
            return Err(trap(
                "io_error",
                format!("list_dir {path}: path is not a directory"),
            ));
        }

        const MAX_UNSCOPED_DIRECTORY_ENTRIES: usize = 10_000;
        const MAX_UNSCOPED_DIRECTORY_PATH_BYTES: usize = 16 * 1024 * 1024;
        let (remaining_entries, remaining_bytes) = if let Some(policy) = &self.policy {
            let Some(limit) = policy.max_directory_entries else {
                self.record("list_dir", path, 0, "deny");
                return Err(trap(
                    "io_denied",
                    "directory enumeration requires application.limits.max_directory_entries",
                ));
            };
            (
                limit.saturating_sub(self.directory_entries.get()),
                self.remaining_budget(false),
            )
        } else {
            (
                MAX_UNSCOPED_DIRECTORY_ENTRIES,
                MAX_UNSCOPED_DIRECTORY_PATH_BYTES,
            )
        };

        let read_dir = std::fs::read_dir(&resolved).map_err(|error| {
            self.record("list_dir", path, 0, "error");
            trap("io_error", format!("list_dir {path}: {error}"))
        })?;
        let mut entries = Vec::new();
        let mut path_bytes = 0usize;
        for entry in read_dir {
            if entries.len() >= remaining_entries {
                self.record("list_dir", path, 0, "budget_exhausted");
                return Err(trap(
                    "io_budget_exhausted",
                    format!(
                        "list_dir {path} exceeds application max_directory_entries (used {}, limit {})",
                        self.directory_entries.get(),
                        self.policy
                            .as_ref()
                            .and_then(|policy| policy.max_directory_entries)
                            .unwrap_or(MAX_UNSCOPED_DIRECTORY_ENTRIES)
                    ),
                ));
            }
            let entry = entry.map_err(|error| {
                self.record("list_dir", path, 0, "error");
                trap("io_error", format!("list_dir {path}: {error}"))
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                self.record("list_dir", path, 0, "error");
                trap(
                    "io_path_unportable",
                    format!("list_dir {path}: entry name is not valid UTF-8"),
                )
            })?;
            let name = normalize_portable_path(&name).map_err(|error| {
                self.record("list_dir", path, 0, "error");
                trap("io_path_unportable", format!("list_dir {path}: {error}"))
            })?;
            let logical_path =
                normalize_portable_path(&format!("{normalized}/{name}")).map_err(|error| {
                    self.record("list_dir", path, 0, "error");
                    trap("io_path_unportable", format!("list_dir {path}: {error}"))
                })?;
            let requested = logical_path.len();
            if path_bytes.saturating_add(requested) > remaining_bytes {
                self.record("list_dir", path, 0, "budget_exhausted");
                return Err(trap(
                    "io_budget_exhausted",
                    format!(
                        "list_dir {path} exceeds application max_read_bytes while returning normalized paths"
                    ),
                ));
            }
            path_bytes = path_bytes.saturating_add(requested);

            let file_type = entry.file_type().map_err(|error| {
                self.record("list_dir", path, 0, "error");
                trap("io_error", format!("list_dir {logical_path}: {error}"))
            })?;
            let (kind, size) = if file_type.is_file() {
                let size = entry
                    .metadata()
                    .map_err(|error| {
                        self.record("list_dir", path, 0, "error");
                        trap("io_error", format!("list_dir {logical_path}: {error}"))
                    })?
                    .len();
                ("file", usize::try_from(size).unwrap_or(usize::MAX))
            } else if file_type.is_dir() {
                ("directory", 0)
            } else if file_type.is_symlink() {
                ("symlink", 0)
            } else {
                ("other", 0)
            };
            let fields = HashMap::from([
                ("path".to_string(), Value::String(logical_path.clone())),
                ("kind".to_string(), Value::String(kind.to_string())),
                (
                    "size".to_string(),
                    Value::Int(i64::try_from(size).unwrap_or(i64::MAX)),
                ),
            ]);
            entries.push((logical_path, Value::Struct("DirEntry".to_string(), fields)));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let entry_count = entries.len();
        self.directory_entries
            .set(self.directory_entries.get().saturating_add(entry_count));
        self.add_budget(path_bytes, false);
        self.record("list_dir", path, path_bytes, "allow");
        Ok(entries.into_iter().map(|(_, value)| value).collect())
    }

    fn write_file(&self, path: &str, contents: &str) -> Result<usize, String> {
        if !self.grants.contains("fs.write") {
            self.record("write", path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("fs.write not granted for {path}"),
            ));
        }
        let resolved = match self.resolve_path(path, true) {
            Ok(path) => path,
            Err(error) => {
                self.record("write", path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        self.check_budget("write", path, contents.len(), true)?;
        match std::fs::write(&resolved, contents) {
            Ok(()) => {
                self.add_budget(contents.len(), true);
                self.record("write", path, contents.len(), "allow");
                Ok(contents.len())
            }
            Err(err) => {
                self.record("write", path, 0, "error");
                Err(trap("io_error", format!("write {path}: {err}")))
            }
        }
    }

    fn write_file_atomic(&self, path: &str, contents: &str) -> Result<usize, String> {
        self.publish_atomic("publish_text", path, contents.as_bytes())
    }

    fn write_bytes_atomic(&self, path: &str, bytes: &[u8]) -> Result<usize, String> {
        self.publish_atomic("publish_bytes", path, bytes)
    }

    pub(crate) fn publish_artifact(
        &self,
        operation: &str,
        name: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<usize, String> {
        let Some(declaration) = self
            .policy
            .as_ref()
            .and_then(|policy| policy.artifacts.get(name))
            .cloned()
        else {
            self.record(operation, path, 0, "deny");
            return Err(trap(
                "artifact_undeclared",
                format!("application artifact '{name}' is not declared"),
            ));
        };
        if self
            .artifacts
            .borrow()
            .iter()
            .any(|artifact| artifact.name == name)
        {
            self.record(operation, path, 0, "deny");
            return Err(trap(
                "artifact_already_published",
                format!("application artifact '{name}' may be published only once"),
            ));
        }
        if bytes.len() > declaration.max_bytes {
            self.record(operation, path, 0, "budget_exhausted");
            return Err(trap(
                "artifact_budget_exhausted",
                format!(
                    "application artifact '{name}' needs {} bytes, declared max_bytes is {}",
                    bytes.len(),
                    declaration.max_bytes
                ),
            ));
        }
        if !self.grants.contains("fs.write") {
            self.record(operation, path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("fs.write not granted for artifact '{name}'"),
            ));
        }
        let normalized = normalize_portable_path(path).map_err(|error| {
            self.record(operation, path, 0, "deny");
            trap("artifact_path_invalid", error)
        })?;
        let resolved = match self.resolve_path(&normalized, true) {
            Ok(path) => path,
            Err(error) => {
                self.record(operation, path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        if !resolved.starts_with(&declaration.root) {
            self.record(operation, path, 0, "deny");
            return Err(trap(
                "artifact_root_denied",
                format!(
                    "application artifact '{name}' path '{normalized}' is outside its declared root"
                ),
            ));
        }
        self.check_budget(operation, &normalized, bytes.len(), true)?;
        match atomic_publish_new(&resolved, bytes) {
            Ok(()) => {
                self.add_budget(bytes.len(), true);
                self.record(operation, &normalized, bytes.len(), "allow");
                self.artifacts.borrow_mut().push(IoArtifactReceipt {
                    name: name.to_string(),
                    path: normalized,
                    media_type: declaration.media_type,
                    bytes: bytes.len(),
                    sha256: crate::crypto::sha256_hex(bytes),
                    publication: declaration.publication,
                });
                Ok(bytes.len())
            }
            Err(error) => {
                self.record(operation, &normalized, 0, "error");
                Err(error)
            }
        }
    }

    fn publish_atomic(&self, operation: &str, path: &str, bytes: &[u8]) -> Result<usize, String> {
        if !self.grants.contains("fs.write") {
            self.record(operation, path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("fs.write not granted for {path}"),
            ));
        }
        let resolved = match self.resolve_path(path, true) {
            Ok(path) => path,
            Err(error) => {
                self.record(operation, path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        self.check_budget(operation, path, bytes.len(), true)?;
        match atomic_publish_new(&resolved, bytes) {
            Ok(()) => {
                self.add_budget(bytes.len(), true);
                self.record(operation, path, bytes.len(), "allow");
                Ok(bytes.len())
            }
            Err(error) => {
                self.record(operation, path, 0, "error");
                Err(error)
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
        let resolved_src = match self.resolve_path(src, false) {
            Ok(path) => path,
            Err(error) => {
                self.record("copy", &path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        let resolved_dst = match self.resolve_path(dst, true) {
            Ok(path) => path,
            Err(error) => {
                self.record("copy", &path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        if self.policy.is_some() {
            let bytes = self.read_within_budget("copy", &path, &resolved_src)?;
            self.check_budget("copy", &path, bytes.len(), true)?;
            return match std::fs::write(&resolved_dst, &bytes) {
                Ok(()) => {
                    self.add_budget(bytes.len(), false);
                    self.add_budget(bytes.len(), true);
                    self.record("copy", &path, bytes.len(), "allow");
                    Ok(bytes.len())
                }
                Err(error) => {
                    self.record("copy", &path, 0, "error");
                    Err(trap("io_error", format!("copy {path}: {error}")))
                }
            };
        }
        match std::fs::copy(&resolved_src, &resolved_dst) {
            Ok(bytes) => {
                self.add_budget(bytes as usize, false);
                self.add_budget(bytes as usize, true);
                self.record("copy", &path, bytes as usize, "allow");
                Ok(bytes as usize)
            }
            Err(err) => {
                self.record("copy", &path, 0, "error");
                Err(trap("io_error", format!("copy {path}: {err}")))
            }
        }
    }

    fn copy_file_atomic(&self, src: &str, dst: &str) -> Result<usize, String> {
        let path = format!("{src} -> {dst}");
        if !self.grants.contains("fs.read") || !self.grants.contains("fs.write") {
            self.record("copy_atomic", &path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("copy_atomic {path} requires fs.read and fs.write"),
            ));
        }
        let resolved_src = match self.resolve_path(src, false) {
            Ok(path) => path,
            Err(error) => {
                self.record("copy_atomic", &path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        let resolved_dst = match self.resolve_path(dst, true) {
            Ok(path) => path,
            Err(error) => {
                self.record("copy_atomic", &path, 0, "deny");
                return Err(trap("io_denied", error));
            }
        };
        let bytes = if self.policy.is_some() {
            self.read_within_budget("copy_atomic", &path, &resolved_src)?
        } else {
            std::fs::read(&resolved_src).map_err(|error| {
                self.record("copy_atomic", &path, 0, "error");
                trap("io_error", format!("copy_atomic {path}: {error}"))
            })?
        };
        self.check_budget("copy_atomic", &path, bytes.len(), true)?;
        match atomic_publish_new(&resolved_dst, &bytes) {
            Ok(()) => {
                self.add_budget(bytes.len(), false);
                self.add_budget(bytes.len(), true);
                self.record("copy_atomic", &path, bytes.len(), "allow");
                Ok(bytes.len())
            }
            Err(error) => {
                self.record("copy_atomic", &path, 0, "error");
                Err(error)
            }
        }
    }

    fn resolve_path(&self, path: &str, write: bool) -> Result<PathBuf, String> {
        self.resolve_path_allow_missing(path, write, write)
    }

    fn resolve_path_allow_missing(
        &self,
        path: &str,
        write: bool,
        allow_missing: bool,
    ) -> Result<PathBuf, String> {
        let Some(policy) = &self.policy else {
            return Ok(PathBuf::from(path));
        };
        let input = Path::new(path);
        if path.trim().is_empty()
            || input.is_absolute()
            || input.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir
                        | Component::CurDir
                        | Component::Prefix(_)
                        | Component::RootDir
                )
            })
        {
            return Err(format!(
                "{} path '{path}' must be normalized and contain no parent traversal",
                if write { "fs.write" } else { "fs.read" }
            ));
        }
        let candidate = policy.root.join(input);
        let resolved = if allow_missing && !candidate.exists() {
            let parent = candidate.parent().ok_or_else(|| {
                format!(
                    "{} path '{path}' has no resolvable parent directory",
                    if write { "fs.write" } else { "fs.read" }
                )
            })?;
            let parent = parent.canonicalize().map_err(|error| {
                format!(
                    "failed to resolve {} parent for '{path}': {error}",
                    if write { "fs.write" } else { "fs.read" }
                )
            })?;
            let file_name = candidate.file_name().ok_or_else(|| {
                format!(
                    "{} path '{path}' must identify a file",
                    if write { "fs.write" } else { "fs.read" }
                )
            })?;
            parent.join(file_name)
        } else {
            candidate.canonicalize().map_err(|error| {
                format!(
                    "failed to resolve {} path '{path}': {error}",
                    if write { "fs.write" } else { "fs.read" }
                )
            })?
        };
        let roots = if write {
            &policy.write_roots
        } else {
            &policy.read_roots
        };
        if roots.iter().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(format!(
                "{} path '{path}' is outside the application permission roots",
                if write { "fs.write" } else { "fs.read" }
            ))
        }
    }

    fn resolve_file_readiness(&self, path: &str) -> Result<PathBuf, String> {
        if !self.grants.contains("fs.read") {
            self.record("readiness_read", path, 0, "deny");
            return Err(trap("io_denied", format!("fs.read not granted for {path}")));
        }
        let resolved = self
            .resolve_path_allow_missing(path, false, true)
            .map_err(|error| {
                self.record("readiness_read", path, 0, "deny");
                trap("io_denied", error)
            })?;
        self.record(
            "readiness_read",
            path,
            0,
            if path_is_regular_file(&resolved) {
                "ready"
            } else {
                "pending"
            },
        );
        Ok(resolved)
    }

    fn resolve_file_create_readiness(&self, path: &str) -> Result<PathBuf, String> {
        if !self.grants.contains("fs.write") {
            self.record("readiness_create", path, 0, "deny");
            return Err(trap(
                "io_denied",
                format!("fs.write not granted for {path}"),
            ));
        }
        let resolved = self
            .resolve_path_allow_missing(path, true, true)
            .map_err(|error| {
                self.record("readiness_create", path, 0, "deny");
                trap("io_denied", error)
            })?;
        self.record(
            "readiness_create",
            path,
            0,
            if path_is_absent(&resolved) {
                "ready"
            } else {
                "pending"
            },
        );
        Ok(resolved)
    }

    fn read_within_budget(
        &self,
        operation: &str,
        display_path: &str,
        resolved: &Path,
    ) -> Result<Vec<u8>, String> {
        let remaining = self.remaining_budget(false);
        let file = std::fs::File::open(resolved).map_err(|error| {
            self.record(operation, display_path, 0, "error");
            trap("io_error", format!("{operation} {display_path}: {error}"))
        })?;
        let byte_limit = u64::try_from(remaining)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bytes = Vec::new();
        file.take(byte_limit)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                self.record(operation, display_path, 0, "error");
                trap("io_error", format!("{operation} {display_path}: {error}"))
            })?;
        if bytes.len() > remaining {
            self.record(operation, display_path, 0, "budget_exhausted");
            let limit = self
                .policy
                .as_ref()
                .and_then(|policy| policy.max_read_bytes)
                .unwrap_or_default();
            return Err(trap(
                "io_budget_exhausted",
                format!(
                    "{operation} {display_path} exceeds application max_read_bytes (used {}, limit {limit})",
                    self.read_bytes.get()
                ),
            ));
        }
        Ok(bytes)
    }

    fn check_budget(
        &self,
        operation: &str,
        display_path: &str,
        bytes: usize,
        write: bool,
    ) -> Result<(), String> {
        let Some(policy) = &self.policy else {
            return Ok(());
        };
        let (used, limit, label) = if write {
            (
                self.write_bytes.get(),
                policy.max_write_bytes,
                "max_write_bytes",
            )
        } else {
            (
                self.read_bytes.get(),
                policy.max_read_bytes,
                "max_read_bytes",
            )
        };
        if limit.is_some_and(|limit| used.saturating_add(bytes) > limit) {
            self.record(operation, display_path, 0, "budget_exhausted");
            return Err(trap(
                "io_budget_exhausted",
                format!(
                    "{operation} {display_path} exceeds application {label} (used {used}, requested {bytes}, limit {})",
                    limit.unwrap_or_default()
                ),
            ));
        }
        Ok(())
    }

    fn add_budget(&self, bytes: usize, write: bool) {
        let counter = if write {
            &self.write_bytes
        } else {
            &self.read_bytes
        };
        counter.set(counter.get().saturating_add(bytes));
    }

    fn remaining_budget(&self, write: bool) -> usize {
        let Some(policy) = &self.policy else {
            return usize::MAX;
        };
        let (used, limit) = if write {
            (self.write_bytes.get(), policy.max_write_bytes)
        } else {
            (self.read_bytes.get(), policy.max_read_bytes)
        };
        limit
            .map(|limit| limit.saturating_sub(used))
            .unwrap_or(usize::MAX)
    }
}

pub(crate) fn atomic_publish_new(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if target.exists() {
        return Err(trap(
            "io_target_exists",
            format!(
                "atomic publication refuses to replace existing destination {}",
                target.display()
            ),
        ));
    }
    let parent = target.parent().ok_or_else(|| {
        trap(
            "io_error",
            format!(
                "atomic publication destination {} has no parent directory",
                target.display()
            ),
        )
    })?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            trap(
                "io_error",
                format!(
                    "atomic publication destination {} has an invalid filename",
                    target.display()
                ),
            )
        })?;

    for attempt in 0..16u8 {
        let temporary = parent.join(format!(
            ".{file_name}.fgx-publish-{}-{attempt}",
            std::process::id()
        ));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(trap(
                    "io_error",
                    format!(
                        "failed to create atomic publication temporary file for {}: {error}",
                        target.display()
                    ),
                ));
            }
        };
        let publish_result = (|| {
            file.write_all(bytes).map_err(|error| {
                trap(
                    "io_error",
                    format!(
                        "failed to write atomic publication temporary file for {}: {error}",
                        target.display()
                    ),
                )
            })?;
            file.sync_all().map_err(|error| {
                trap(
                    "io_error",
                    format!(
                        "failed to sync atomic publication temporary file for {}: {error}",
                        target.display()
                    ),
                )
            })?;
            drop(file);
            std::fs::hard_link(&temporary, target).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists || target.exists() {
                    trap(
                        "io_target_exists",
                        format!(
                            "atomic publication refuses to replace existing destination {}",
                            target.display()
                        ),
                    )
                } else {
                    trap(
                        "io_error",
                        format!(
                            "failed to publish atomic destination {}: {error}",
                            target.display()
                        ),
                    )
                }
            })
        })();
        let _ = std::fs::remove_file(&temporary);
        return publish_result;
    }

    Err(trap(
        "io_error",
        format!(
            "could not reserve an atomic publication temporary file for {}",
            target.display()
        ),
    ))
}

const MAX_PORTABLE_PATH_BYTES: usize = 4096;
const MAX_PORTABLE_COMPONENT_BYTES: usize = 255;

pub(crate) fn normalize_portable_path(value: &str) -> Result<String, String> {
    if value.is_empty() {
        return Err("portable path must not be empty".to_string());
    }
    if value.len() > MAX_PORTABLE_PATH_BYTES {
        return Err(format!(
            "portable path exceeds {MAX_PORTABLE_PATH_BYTES} UTF-8 bytes"
        ));
    }
    if value.starts_with('/') || value.contains('\\') {
        return Err("portable path must be relative and use forward slashes only".to_string());
    }

    let mut components = Vec::new();
    for component in value.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(
                "portable path must be normalized and contain no empty, current, or parent component"
                    .to_string(),
            );
        }
        if component.len() > MAX_PORTABLE_COMPONENT_BYTES {
            return Err(format!(
                "portable path component exceeds {MAX_PORTABLE_COMPONENT_BYTES} UTF-8 bytes"
            ));
        }
        if component.ends_with([' ', '.'])
            || component
                .chars()
                .any(|character| character.is_control() || r#"<>:"|?*"#.contains(character))
            || is_reserved_portable_component(component)
        {
            return Err(format!(
                "portable path component '{component}' is not portable"
            ));
        }
        components.push(component);
    }
    Ok(components.join("/"))
}

fn is_reserved_portable_component(component: &str) -> bool {
    let base = component
        .split_once('.')
        .map(|(base, _)| base)
        .unwrap_or(component)
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                suffix.len() == 1
                    && suffix
                        .as_bytes()
                        .first()
                        .is_some_and(|digit| (b'1'..=b'9').contains(digit))
            })
}

fn portable_path_starts_with(path: &str, base: &str) -> bool {
    path == base
        || path
            .strip_prefix(base)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn canonical_scopes(
    root: &Path,
    scopes: &[PathBuf],
    capability: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut canonical = Vec::new();
    for scope in scopes {
        if scope.as_os_str().is_empty()
            || scope.is_absolute()
            || scope.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir
                        | Component::CurDir
                        | Component::Prefix(_)
                        | Component::RootDir
                )
            })
        {
            return Err(format!(
                "{capability} application scope '{}' must be a non-empty relative path without parent traversal",
                scope.display()
            ));
        }
        let resolved = root.join(scope).canonicalize().map_err(|error| {
            format!(
                "failed to resolve {capability} application scope '{}': {error}",
                scope.display()
            )
        })?;
        if !resolved.starts_with(root) {
            return Err(format!(
                "{capability} application scope '{}' escapes the package root",
                scope.display()
            ));
        }
        canonical.push(resolved);
    }
    canonical.sort();
    canonical.dedup();
    Ok(canonical)
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
        // Plan before dispatch so a capsule declaring a limit that cannot be
        // enforced is refused before any process starts — warm or cold.
        let managed_plan = if is_managed_language(&capsule.language) {
            match crate::managed_exec::plan(capsule, host.managed) {
                Ok(plan) => Some(plan),
                Err(err) => {
                    host.record(&capsule.name, &capsule.language, &audit, &err, "denied");
                    return Err(trap("capsule_limit_unenforceable", err));
                }
            }
        } else {
            None
        };
        // A warm worker is one long-lived process shared by every call, so it
        // cannot apply per-call limits. A capsule that declares any runs on the
        // contained cold path instead, where they are enforced.
        let declares_limits = !crate::sandbox::analyze_capsule(capsule).limits.is_empty();
        if host.warm
            && !declares_limits
            && crate::capsule_worker::supports_language(&capsule.language)
        {
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
                    host.attach_managed(ManagedReceipt {
                        profile: "warm_worker".to_string(),
                        enforced: Vec::new(),
                        waived: Vec::new(),
                        warm: true,
                        diagnostics: host.warm_diagnostics(capsule),
                        granted_env: Vec::new(),
                    });
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
            "c" | "cpp" | "c++" | "rust" | "zig" | "cuda"
        ) {
            return self.call_native_capsule(host, capsule, source_args, args, env, &audit);
        }
        let policy = host.managed;
        let dispatch = match capsule.language.as_str() {
            "python" => crate::python_run::invoke_live_with(capsule, &inputs, policy),
            "node" | "typescript" => crate::node_run::invoke_live_with(capsule, &inputs, policy),
            "go" => crate::go_run::invoke_live_with(capsule, &inputs, policy),
            "julia" => crate::julia_run::invoke_live_with(capsule, &inputs, policy),
            "moonbit" => crate::moonbit_run::invoke_live_with(capsule, &inputs, policy),
            "java" => {
                let plain = args.iter().map(value_to_arg).collect::<Vec<_>>();
                crate::jvm_run::invoke_live_with(capsule, &plain, policy)
            }
            // Same type-directed boundary as the JVM. Wired but unverified on a
            // box without a .NET SDK (see dotnet_run::invoke_live).
            "csharp" => {
                let plain = args.iter().map(value_to_arg).collect::<Vec<_>>();
                crate::dotnet_run::invoke_live_with(capsule, &plain, policy)
            }
            other => Err(format!(
                "live invocation of '{other}' capsules is not wired yet"
            )),
        };
        let receipt = managed_plan.map(|plan| ManagedReceipt {
            profile: plan.profile.to_string(),
            diagnostics: String::new(),
            enforced: plan
                .enforced
                .iter()
                .map(|(key, value, source)| (key.clone(), value.clone(), source.to_string()))
                .collect(),
            waived: plan.waived.clone(),
            warm: false,
            granted_env: plan.granted_env.clone(),
        });
        match dispatch {
            Ok(output) => {
                host.record(&capsule.name, &capsule.language, &audit, &output, "allow");
                if let Some(receipt) = receipt.clone() {
                    host.attach_managed(receipt);
                }
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
                if let Some(receipt) = receipt {
                    host.attach_managed(receipt);
                }
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
                let value =
                    retag_native_struct_output(self.program, capsule, value_from_json(&json)?);
                // The complete scalar result is decoded before mutable state
                // is replaced. Together with FFI envelope validation, a bad
                // native response is atomic from the interpreter's view.
                apply_native_mutations(
                    self.program,
                    capsule,
                    source_args,
                    &invocation.mutations,
                    env,
                )?;
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

    /// The declared integer width of an arithmetic node, if it has one.
    ///
    /// Absent for float and non-numeric operations, and for expressions built
    /// only from literals: those have no width until a context supplies one,
    /// and are checked at the boundary they flow into instead.
    fn integer_width(&self, expr: &Expr) -> Option<String> {
        if !matches!(expr, Expr::Binary { op, .. } if matches!(
            op.as_str(),
            "+" | "-" | "*" | "/" | "%" | "<<" | ">>"
        )) {
            return None;
        }
        self.int_widths
            .borrow()
            .last()?
            .get(&(expr as *const Expr as usize))
            .cloned()
    }

    /// Run `body` with the widths of `frame` in scope.
    ///
    /// The guard shape matters: the frame has to be popped even when the call
    /// traps, or an error unwinding out of a callee would leave its widths
    /// applying to the caller's nodes.
    fn with_int_widths<T>(&self, frame: HashMap<usize, String>, body: impl FnOnce() -> T) -> T {
        self.int_widths.borrow_mut().push(frame);
        let result = body();
        self.int_widths.borrow_mut().pop();
        result
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
            if !t.evaluating {
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
                t.depth += 1;
            }
        }
        let widths = crate::int_width::annotate(&function.body, &function.params, &function.ret);
        let outcome = self.with_int_widths(widths, || {
            self.run_body(&function.body, &mut env, &format!("{}.stmt", function.id))
        });
        match outcome {
            Ok(control) => {
                let value = match control {
                    Control::Return(value) => value,
                    _ => Value::Void,
                };
                if let Some(tracer) = &self.tracer {
                    let mut t = tracer.borrow_mut();
                    if !t.evaluating {
                        t.seq += 1;
                        let seq = t.seq;
                        t.calls.push(DebugCall {
                            seq,
                            event: "exit".to_string(),
                            function: function.name.clone(),
                            detail: value_to_text(&value),
                        });
                        t.depth = t.depth.saturating_sub(1);
                    }
                }
                Ok((value, env))
            }
            Err(err) => {
                // Trap post-mortem: each unwinding frame snapshots its locals,
                // innermost first — a full stack picture at the failure.
                if let Some(tracer) = &self.tracer {
                    let mut tracer = tracer.borrow_mut();
                    if !tracer.evaluating {
                        let elements = tracer.value_elements;
                        tracer.frames.push(DebugFrame {
                            function: function.name.clone(),
                            locals: DebugTracer::snapshot_with(&env, elements),
                            line: None,
                        });
                    }
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
        // The FIR path rebuilds `body` on every call, so its widths are computed
        // for these nodes and discarded with them.
        let widths = crate::int_width::annotate(&body, &function.params, &function.ret);
        let control = self.with_int_widths(widths, || {
            self.run_body(&body, &mut env, &format!("{}.stmt", function.id))
        })?;
        if let Control::Return(value) = control {
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
        // A condition or watch expression may call a function, which re-enters
        // here. Those statements are not program steps and must not be traced,
        // nor may they re-enter the tracer's borrow.
        if tracer.borrow().evaluating {
            return self.exec_stmt_inner(stmt, env, node);
        }
        // An interactive session pauses before the node executes, exactly where
        // an unconditional breakpoint would.
        let interactive = tracer.borrow().session.is_some();
        if interactive {
            if let Some(control) = self.session_pause(tracer, stmt, env, node)? {
                return control;
            }
        }
        {
            let mut t = tracer.borrow_mut();
            // A breakpoint stops BEFORE the node executes, with full locals.
            if !interactive && t.break_at.as_deref() == Some(node) {
                let elements = t.value_elements;
                t.breakpoint = Some((node.to_string(), DebugTracer::snapshot_with(env, elements)));
                return Err(trap("breakpoint_hit", format!("breakpoint at {node}")));
            }
            // With `tail` the trace is a sliding window, so the step cap counts
            // executed steps rather than retained ones — otherwise the run would
            // stop as soon as the window filled.
            let executed = t.steps.len() + t.dropped;
            if executed >= t.max_steps {
                return Err(trap(
                    "step_limit_exceeded",
                    format!("debug trace exceeded {} steps", t.max_steps),
                ));
            }
        }
        // A conditional breakpoint stops before the node too, but only when its
        // expression holds in the environment as it stands there.
        let conditional = if interactive {
            None
        } else {
            let t = tracer.borrow();
            match &t.break_if {
                Some((target, expr)) if target == node => Some(expr.clone()),
                _ => None,
            }
        };
        if let Some(expr) = conditional {
            // A condition that is not a boolean, or that fails to evaluate,
            // does not stop: a broken condition must not masquerade as a hit.
            let hit = self
                .eval_traced(tracer, &expr, env)
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if hit {
                let mut t = tracer.borrow_mut();
                let elements = t.value_elements;
                t.breakpoint = Some((node.to_string(), DebugTracer::snapshot_with(env, elements)));
                return Err(trap(
                    "breakpoint_hit",
                    format!("conditional breakpoint at {node}"),
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
                line: None,
                watches: Vec::new(),
            });
            t.trim_to_tail();
            drop(t);
            return self.exec_stmt_inner(stmt, env, node);
        }
        // Report the bindings this statement writes, rather than diffing the
        // whole environment against a clone of it. The clone was O(heap) on
        // every step: tracing a program holding a 2.6 MB buffer copied that
        // buffer once per step, which is quadratic over a run.
        //
        // This is sound because a leaf statement can only write a binding it
        // names. Closures capture by value, so there is no aliasing path from a
        // statement to a binding it does not mention, and a `&mut` parameter
        // requires a direct variable rather than a projection.
        let targets = stmt_write_targets(stmt);
        let result = self.exec_stmt_inner(stmt, env, node);
        let mut t = tracer.borrow_mut();
        t.seq += 1;
        let seq = t.seq;
        let elements = t.value_elements;
        let mut changed: Vec<(String, String)> = targets
            .iter()
            .filter_map(|name| {
                env.get(name)
                    .map(|value| (name.clone(), summarize_value(value, elements)))
            })
            .collect();
        changed.sort();
        let watch_exprs = t.watches.clone();
        drop(t);
        let watches = self.evaluate_watches(tracer, &watch_exprs, env, elements);
        let mut t = tracer.borrow_mut();
        t.steps.push(DebugStep {
            seq,
            node: node.to_string(),
            kind: stmt_kind(stmt).to_string(),
            text: stmt_summary(stmt),
            changed,
            line: None,
            watches,
        });
        t.trim_to_tail();
        result
    }

    /// Decide whether an interactive session should pause before this node, and
    /// if so publish a stop and block until a command arrives.
    ///
    /// Returns `Ok(Some(control))` when the session asked to quit, so the caller
    /// unwinds; `Ok(None)` means carry on executing this statement.
    #[allow(clippy::type_complexity)]
    fn session_pause(
        &self,
        tracer: &std::cell::RefCell<DebugTracer>,
        stmt: &Stmt,
        env: &mut HashMap<String, Value>,
        node: &str,
    ) -> Result<Option<Result<Control, String>>, String> {
        let (step_stop, unconditional, conditional, depth, elements) = {
            let t = tracer.borrow();
            if t.cancelled {
                return Ok(Some(Err(trap(
                    "debug_cancelled",
                    "debug session cancelled",
                ))));
            }
            // `next` and `step-out` are depth comparisons: a call made from the
            // stopped frame runs to completion because its statements are
            // deeper than the depth the command was issued at.
            let step_stop = match t.mode {
                StepMode::StepIn => true,
                StepMode::Next => t.depth <= t.mode_depth,
                StepMode::StepOut => t.depth < t.mode_depth,
                StepMode::Continue => false,
            };
            // `break_at` is the one-shot CLI's single breakpoint; `break_nodes`
            // is the session's editable set, which a DAP client rewrites while
            // the interpreter is paused.
            let unconditional = t.break_at.as_deref() == Some(node)
                || t.break_nodes
                    .as_ref()
                    .is_some_and(|nodes| nodes.lock().is_ok_and(|set| set.contains(node)));
            let conditional = match &t.break_if {
                Some((target, expr)) if target == node => Some(expr.clone()),
                _ => None,
            };
            (
                step_stop,
                unconditional,
                conditional,
                t.depth,
                t.value_elements,
            )
        };
        let conditional_hit = if step_stop || unconditional {
            false
        } else if let Some(expr) = conditional {
            self.eval_traced(tracer, &expr, env)
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        } else {
            false
        };
        let should_stop = step_stop || unconditional || conditional_hit;
        if !should_stop {
            return Ok(None);
        }

        let function = {
            let t = tracer.borrow();
            let mut stack = Vec::new();
            for call in &t.calls {
                if call.event == "enter" {
                    stack.push(call.function.clone());
                } else if call.event == "exit" {
                    stack.pop();
                }
            }
            stack.pop().unwrap_or_default()
        };
        let stop = DebugStop {
            node: node.to_string(),
            line: None,
            function,
            depth,
            text: stmt_summary(stmt),
            locals: DebugTracer::snapshot_with(env, elements),
        };
        {
            let t = tracer.borrow();
            let session = t.session.as_ref().expect("session present");
            if session.events.send(DebugEvent::Stopped(stop)).is_err() {
                // The driver hung up; finish the run rather than block forever.
                drop(t);
                tracer.borrow_mut().mode = StepMode::Continue;
                return Ok(None);
            }
        }

        // Block until the driver decides what happens next. `Evaluate` answers
        // and keeps waiting, so a paused frame can be inspected repeatedly.
        loop {
            let command = {
                let t = tracer.borrow();
                let session = t.session.as_ref().expect("session present");
                session.commands.recv()
            };
            let Ok(command) = command else {
                tracer.borrow_mut().mode = StepMode::Continue;
                return Ok(None);
            };
            match command {
                DebugCommand::Evaluate(source) => {
                    let value = crate::parser::parse_expr(&source)
                        .and_then(|expr| self.eval_traced(tracer, &expr, env))
                        .map(|value| summarize_value(&value, elements));
                    let t = tracer.borrow();
                    let session = t.session.as_ref().expect("session present");
                    let _ = session.events.send(DebugEvent::Evaluated { source, value });
                }
                DebugCommand::Quit => {
                    let mut t = tracer.borrow_mut();
                    t.cancelled = true;
                    return Ok(Some(Err(trap(
                        "debug_cancelled",
                        "debug session cancelled",
                    ))));
                }
                other => {
                    let mut t = tracer.borrow_mut();
                    t.mode = match other {
                        DebugCommand::StepIn => StepMode::StepIn,
                        DebugCommand::Next => StepMode::Next,
                        DebugCommand::StepOut => StepMode::StepOut,
                        _ => StepMode::Continue,
                    };
                    t.mode_depth = depth;
                    return Ok(None);
                }
            }
        }
    }

    /// Evaluate an expression with tracing suspended, so a call inside it does
    /// not appear in the trace or re-enter the tracer's borrow.
    fn eval_traced(
        &self,
        tracer: &std::cell::RefCell<DebugTracer>,
        expr: &Expr,
        env: &mut HashMap<String, Value>,
    ) -> Result<Value, String> {
        tracer.borrow_mut().evaluating = true;
        let result = self.eval_expr(expr, env);
        tracer.borrow_mut().evaluating = false;
        result
    }

    /// Evaluate every watch against the environment as it stands after a step.
    /// A watch that cannot be evaluated here — a name not yet in scope, say —
    /// records its error rather than aborting the run: a debugger must not
    /// change whether the program completes.
    fn evaluate_watches(
        &self,
        tracer: &std::cell::RefCell<DebugTracer>,
        watches: &[(String, Expr)],
        env: &mut HashMap<String, Value>,
        elements: usize,
    ) -> Vec<(String, String)> {
        watches
            .iter()
            .map(|(source, expr)| {
                let rendered = match self.eval_traced(tracer, expr, env) {
                    Ok(value) => summarize_value(&value, elements),
                    Err(err) => format!("<{}>", err.trim_start_matches("trap: ")),
                };
                (source.clone(), rendered)
            })
            .collect()
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
                let width = self.integer_width(expr);
                let left = self.eval_expr(left, env)?;
                let right = self.eval_expr(right, env)?;
                // The shift amount is checked against the shifted type's width
                // before the shift happens: `eval_binary` only knows about 64,
                // so `1 << 40` on an `i32` would otherwise succeed.
                if let Some(width) = &width {
                    match op.as_str() {
                        "<<" | ">>" => check_shift_amount(&right, width)?,
                        "/" | "%" => check_division(op, &left, &right, width)?,
                        _ => {}
                    }
                }
                let value = eval_binary(op, left, right)?;
                // Integer arithmetic is checked at the declared width, so a sum
                // that fits an `i64` but not the `i32` it was written at is an
                // error rather than a value no `i32` could hold.
                match width {
                    Some(width) => narrow_to_width(op, value, &width),
                    None => Ok(value),
                }
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
                // Fast path: read one element of a named array in place.
                // Evaluating the base through `eval_expr` clones the whole
                // array out of the environment, which makes a single index
                // O(len) and turns any linear scan into O(n^2). A bare
                // variable lookup has no side effects, so evaluating the
                // index first leaves observable order unchanged, and the
                // array is re-resolved afterwards in case evaluating the
                // index rebound the name.
                if let Expr::Var(name) = base.as_ref() {
                    if matches!(env.get(name), Some(Value::Array(_))) {
                        let index = checked_index(self.eval_expr(index, env)?.as_i64()?)?;
                        let Some(Value::Array(values)) = env.get(name) else {
                            return Err(trap("type_error", "indexed expression is not an array"));
                        };
                        return values.get(index).cloned().ok_or_else(|| {
                            trap(
                                "index_out_of_bounds",
                                format!("index {index} out of bounds for length {}", values.len()),
                            )
                        });
                    }
                }
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
            Expr::SliceRange { base, start, end } => {
                // Fast path: project a window of a named array without first
                // cloning the whole array out of the environment. Taking N
                // windows of one buffer is otherwise O(N * len) even though
                // the windows themselves are small. A bare variable lookup has
                // no side effects, so evaluating the bounds first leaves
                // observable order unchanged, and the array is re-resolved
                // afterwards in case evaluating a bound rebound the name.
                if let Expr::Var(name) = base.as_ref() {
                    if matches!(env.get(name), Some(Value::Array(_))) {
                        let start = checked_index(self.eval_expr(start, env)?.as_i64()?)?;
                        let end = checked_index(self.eval_expr(end, env)?.as_i64()?)?;
                        let Some(Value::Array(values)) = env.get(name) else {
                            return Err(trap("type_error", "sliced expression is not an array"));
                        };
                        if start <= end && end <= values.len() {
                            return Ok(Value::Array(values[start..end].to_vec()));
                        }
                        return Err(trap(
                            "slice_range_out_of_bounds",
                            format!(
                                "slice range {start}..{end} is invalid for length {}",
                                values.len()
                            ),
                        ));
                    }
                }
                let value = self.eval_expr(base, env)?;
                let start = checked_index(self.eval_expr(start, env)?.as_i64()?)?;
                let end = checked_index(self.eval_expr(end, env)?.as_i64()?)?;
                match value {
                    Value::Array(values) if start <= end && end <= values.len() => {
                        Ok(Value::Array(values[start..end].to_vec()))
                    }
                    Value::Array(values) => Err(trap(
                        "slice_range_out_of_bounds",
                        format!(
                            "slice range {start}..{end} is invalid for length {}",
                            values.len()
                        ),
                    )),
                    _ => Err(trap("type_error", "sliced expression is not an array")),
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
            "clock_new" => {
                expect_arity(name, args, 0)?;
                let id = self.concurrency.allocate_clock();
                self.record_concurrency(
                    "clock.new",
                    &id.to_string(),
                    "open",
                    0,
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::AsyncClock {
                    id,
                    now: std::rc::Rc::new(std::cell::Cell::new(0)),
                    timers: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                }))
            }
            "clock_now" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "clock_now expects an async-clock variable",
                    ));
                };
                let now = match env.get(var) {
                    Some(Value::AsyncClock { now, .. }) => now.get(),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("clock_now target '{var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{var}'"),
                        ));
                    }
                };
                self.record_concurrency("clock.now", var, "observed", now, None);
                Ok(Some(Value::Int(now as i64)))
            }
            "timer_after" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "timer_after expects an async-clock variable",
                    ));
                };
                match env.get(var) {
                    Some(Value::AsyncClock { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("timer_after target '{var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{var}'"),
                        ));
                    }
                }
                let delay = self.eval_expr(&args[1], env)?.as_i64()?;
                let delay = usize::try_from(delay)
                    .ok()
                    .filter(|delay| *delay <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "timer_range",
                            format!("timer delay must be between 0 and {MAX_CLOCK_TICK}"),
                        )
                    })?;
                let (now, timers) = match env.get(var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.get(), timers.clone()),
                    _ => unreachable!("async-clock binding changed type while evaluating delay"),
                };
                let deadline = now
                    .checked_add(delay)
                    .filter(|tick| *tick <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "clock_range",
                            format!("timer deadline cannot exceed logical tick {MAX_CLOCK_TICK}"),
                        )
                    })?;
                let scheduled = timers.borrow().len();
                if delay > 0 && scheduled >= MAX_CLOCK_TIMERS {
                    self.record_concurrency(
                        "timer.after",
                        var,
                        "capacity_exhausted",
                        scheduled,
                        Some(MAX_CLOCK_TIMERS),
                    );
                    return Err(trap(
                        "timer_capacity",
                        format!(
                            "async clock '{var}' cannot hold more than {MAX_CLOCK_TIMERS} timers"
                        ),
                    ));
                }
                let token_id = self.concurrency.allocate_wake_token();
                let signaled = std::rc::Rc::new(std::cell::Cell::new(delay == 0));
                let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
                let waiters = std::rc::Rc::new(std::cell::Cell::new(0));
                let (decision, count) = if delay == 0 {
                    ("ready", scheduled)
                } else {
                    let count = scheduled + 1;
                    timers.borrow_mut().push(TimerRegistration {
                        deadline,
                        sequence: self.concurrency.allocate_timer_sequence(),
                        token_id,
                        signaled: signaled.clone(),
                        cancelled: cancelled.clone(),
                        waiters: waiters.clone(),
                    });
                    ("scheduled", count)
                };
                self.record_concurrency(
                    "timer.after",
                    var,
                    decision,
                    count,
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::WakeToken {
                    id: token_id,
                    signaled,
                    cancelled,
                    waiters,
                    timer_queue: Some(timers),
                }))
            }
            "timer_interval" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "timer_interval expects an async-clock variable",
                    ));
                };
                match env.get(var) {
                    Some(Value::AsyncClock { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("timer_interval target '{var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{var}'"),
                        ));
                    }
                }
                let period = self.eval_expr(&args[1], env)?.as_i64()?;
                let period = usize::try_from(period)
                    .ok()
                    .filter(|period| (1..=MAX_CLOCK_TICK).contains(period))
                    .ok_or_else(|| {
                        trap(
                            "timer_interval_range",
                            format!("timer interval period must be between 1 and {MAX_CLOCK_TICK}"),
                        )
                    })?;
                let (now, timers) = match env.get(var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    _ => unreachable!("async-clock binding changed type while evaluating period"),
                };
                let first_deadline = now
                    .get()
                    .checked_add(period)
                    .filter(|tick| *tick <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "clock_range",
                            format!(
                                "first interval deadline cannot exceed logical tick {MAX_CLOCK_TICK}"
                            ),
                        )
                    })?;
                let id = self.concurrency.allocate_interval();
                self.record_concurrency(
                    "timer.interval.open",
                    &id.to_string(),
                    "open",
                    period,
                    Some(MAX_CLOCK_TICK),
                );
                Ok(Some(Value::IntervalTimer {
                    id,
                    now,
                    timers,
                    period,
                    next_deadline: std::rc::Rc::new(std::cell::Cell::new(Some(first_deadline))),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                }))
            }
            "timer_interval_next" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "timer_interval_target",
                        "timer_interval_next expects an interval-timer variable",
                    ));
                };
                let (id, now, timers, period, next_deadline, open) = match env.get(var) {
                    Some(Value::IntervalTimer {
                        id,
                        now,
                        timers,
                        period,
                        next_deadline,
                        open,
                    }) => (
                        *id,
                        now.clone(),
                        timers.clone(),
                        *period,
                        next_deadline.clone(),
                        open.clone(),
                    ),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("timer_interval_next target '{var}' is not an interval timer"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown interval timer '{var}'"),
                        ));
                    }
                };
                if !open.get() {
                    return Err(trap(
                        "timer_interval_closed",
                        format!("interval timer '{var}' is closed"),
                    ));
                }
                let deadline = next_deadline.get().ok_or_else(|| {
                    trap(
                        "timer_interval_exhausted",
                        format!(
                            "interval timer '{var}' has no deadline at or below logical tick {MAX_CLOCK_TICK}"
                        ),
                    )
                })?;
                let scheduled = timers.borrow().len();
                let ready = deadline <= now.get();
                if !ready && scheduled >= MAX_CLOCK_TIMERS {
                    self.record_concurrency(
                        "timer.interval.next",
                        &id.to_string(),
                        "capacity_exhausted",
                        scheduled,
                        Some(MAX_CLOCK_TIMERS),
                    );
                    return Err(trap(
                        "timer_capacity",
                        format!(
                            "interval timer '{var}' cannot reserve more than {MAX_CLOCK_TIMERS} clock timers"
                        ),
                    ));
                }
                let token_id = self.concurrency.allocate_wake_token();
                let signaled = std::rc::Rc::new(std::cell::Cell::new(ready));
                let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
                let waiters = std::rc::Rc::new(std::cell::Cell::new(0));
                if !ready {
                    timers.borrow_mut().push(TimerRegistration {
                        deadline,
                        sequence: self.concurrency.allocate_timer_sequence(),
                        token_id,
                        signaled: signaled.clone(),
                        cancelled: cancelled.clone(),
                        waiters: waiters.clone(),
                    });
                }
                next_deadline.set(
                    deadline
                        .checked_add(period)
                        .filter(|tick| *tick <= MAX_CLOCK_TICK),
                );
                let count = timers.borrow().len();
                self.record_concurrency(
                    "timer.interval.next",
                    &id.to_string(),
                    if ready { "ready" } else { "scheduled" },
                    count,
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::WakeToken {
                    id: token_id,
                    signaled,
                    cancelled,
                    waiters,
                    timer_queue: Some(timers),
                }))
            }
            "timer_interval_cancel" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "timer_interval_target",
                        "timer_interval_cancel expects an interval-timer variable",
                    ));
                };
                let (id, timers, open) = match env.get(var) {
                    Some(Value::IntervalTimer {
                        id, timers, open, ..
                    }) => (*id, timers.clone(), open.clone()),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!(
                                "timer_interval_cancel target '{var}' is not an interval timer"
                            ),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown interval timer '{var}'"),
                        ));
                    }
                };
                let did_cancel = open.replace(false);
                self.record_concurrency(
                    "timer.interval.cancel",
                    &id.to_string(),
                    if did_cancel {
                        "closed"
                    } else {
                        "already_closed"
                    },
                    timers.borrow().len(),
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::Bool(did_cancel)))
            }
            "timer_cancel" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "timer_target",
                        "timer_cancel expects a timer wake-token variable",
                    ));
                };
                let (id, signaled, cancelled, waiters, timers) = match env.get(var) {
                    Some(Value::WakeToken {
                        id,
                        signaled,
                        cancelled,
                        waiters,
                        timer_queue: Some(timers),
                    }) => (
                        *id,
                        signaled.clone(),
                        cancelled.clone(),
                        waiters.clone(),
                        timers.clone(),
                    ),
                    Some(Value::WakeToken { .. }) => {
                        return Err(trap(
                            "timer_source",
                            format!("timer_cancel target '{var}' is not a logical timer token"),
                        ));
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("timer_cancel target '{var}' is not a wake token"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown timer token '{var}'"),
                        ));
                    }
                };
                let active = timers.borrow().iter().any(|timer| timer.token_id == id);
                let (decision, did_cancel) = if signaled.get() {
                    timers.borrow_mut().retain(|timer| timer.token_id != id);
                    ("already_fired", false)
                } else if cancelled.get() {
                    timers.borrow_mut().retain(|timer| timer.token_id != id);
                    ("already_cancelled", false)
                } else if !active {
                    ("already_closed", false)
                } else {
                    if waiters.get() != 0 {
                        return Err(trap(
                            "timer_cancel_waiters",
                            format!(
                                "timer token '{var}' has {} parked waiter(s); cancel the tasks first",
                                waiters.get()
                            ),
                        ));
                    }
                    cancelled.set(true);
                    timers.borrow_mut().retain(|timer| timer.token_id != id);
                    ("cancelled", true)
                };
                let count = timers.borrow().len();
                self.record_concurrency(
                    "timer.cancel",
                    &id.to_string(),
                    decision,
                    count,
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::Bool(did_cancel)))
            }
            "timer_reschedule" => {
                expect_arity(name, args, 3)?;
                let Expr::Var(clock_var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "timer_reschedule expects an async-clock variable",
                    ));
                };
                let Expr::Var(token_var) = &args[1] else {
                    return Err(trap(
                        "timer_target",
                        "timer_reschedule expects a timer wake-token variable",
                    ));
                };
                match env.get(clock_var) {
                    Some(Value::AsyncClock { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("timer_reschedule target '{clock_var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{clock_var}'"),
                        ));
                    }
                }
                match env.get(token_var) {
                    Some(Value::WakeToken { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("timer_reschedule target '{token_var}' is not a wake token"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown timer token '{token_var}'"),
                        ));
                    }
                }
                let delay = self.eval_expr(&args[2], env)?.as_i64()?;
                let delay = usize::try_from(delay)
                    .ok()
                    .filter(|delay| *delay <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "timer_range",
                            format!("timer delay must be between 0 and {MAX_CLOCK_TICK}"),
                        )
                    })?;
                let (now, clock_timers) = match env.get(clock_var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.get(), timers.clone()),
                    _ => unreachable!("async-clock binding changed type while evaluating delay"),
                };
                let (id, signaled, cancelled, timers) = match env.get(token_var) {
                    Some(Value::WakeToken {
                        id,
                        signaled,
                        cancelled,
                        timer_queue: Some(timers),
                        ..
                    }) => (*id, signaled.clone(), cancelled.clone(), timers.clone()),
                    Some(Value::WakeToken { .. }) => {
                        return Err(trap(
                            "timer_source",
                            format!(
                                "timer_reschedule target '{token_var}' is not a logical timer token"
                            ),
                        ));
                    }
                    _ => unreachable!("timer-token binding changed type while evaluating delay"),
                };
                if !std::rc::Rc::ptr_eq(&timers, &clock_timers) {
                    return Err(trap(
                        "timer_reschedule_clock",
                        format!("timer token '{token_var}' belongs to a different async clock"),
                    ));
                }
                let deadline = now
                    .checked_add(delay)
                    .filter(|tick| *tick <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "clock_range",
                            format!("timer deadline cannot exceed logical tick {MAX_CLOCK_TICK}"),
                        )
                    })?;
                let active = timers.borrow().iter().any(|timer| timer.token_id == id);
                if signaled.get() || cancelled.get() || !active {
                    timers.borrow_mut().retain(|timer| timer.token_id != id);
                    let decision = if signaled.get() {
                        "already_fired"
                    } else if cancelled.get() {
                        "already_cancelled"
                    } else {
                        "already_closed"
                    };
                    let count = timers.borrow().len();
                    self.record_concurrency(
                        "timer.reschedule",
                        &id.to_string(),
                        decision,
                        count,
                        Some(MAX_CLOCK_TIMERS),
                    );
                    return Ok(Some(Value::Bool(false)));
                }
                let registration = {
                    let mut scheduled = timers.borrow_mut();
                    let index = scheduled
                        .iter()
                        .position(|timer| timer.token_id == id)
                        .expect("active timer registration was observed above");
                    scheduled.remove(index)
                };
                let decision = if delay == 0 {
                    signaled.set(true);
                    "ready"
                } else {
                    timers.borrow_mut().push(TimerRegistration {
                        deadline,
                        sequence: self.concurrency.allocate_timer_sequence(),
                        token_id: id,
                        signaled: registration.signaled,
                        cancelled: registration.cancelled,
                        waiters: registration.waiters,
                    });
                    "rescheduled"
                };
                let count = timers.borrow().len();
                self.record_concurrency(
                    "timer.reschedule",
                    &id.to_string(),
                    decision,
                    count,
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::Bool(true)))
            }
            "clock_advance" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "clock_advance expects an async-clock variable",
                    ));
                };
                match env.get(var) {
                    Some(Value::AsyncClock { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("clock_advance target '{var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{var}'"),
                        ));
                    }
                }
                let ticks = self.eval_expr(&args[1], env)?.as_i64()?;
                let ticks = usize::try_from(ticks)
                    .ok()
                    .filter(|ticks| *ticks <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "clock_range",
                            format!("clock advance must be between 0 and {MAX_CLOCK_TICK} ticks"),
                        )
                    })?;
                let (now, timers) = match env.get(var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    _ => unreachable!("async-clock binding changed type while evaluating ticks"),
                };
                let new_now = now
                    .get()
                    .checked_add(ticks)
                    .filter(|tick| *tick <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "clock_range",
                            format!("logical clock cannot exceed tick {MAX_CLOCK_TICK}"),
                        )
                    })?;
                now.set(new_now);
                let mut due = Vec::new();
                {
                    let mut scheduled = timers.borrow_mut();
                    let mut pending = Vec::with_capacity(scheduled.len());
                    for timer in scheduled.drain(..) {
                        if timer.deadline <= new_now {
                            due.push(timer);
                        } else {
                            pending.push(timer);
                        }
                    }
                    *scheduled = pending;
                }
                due.sort_by_key(|timer| (timer.deadline, timer.sequence));
                for timer in &due {
                    if timer.cancelled.get() {
                        continue;
                    }
                    let decision = if timer.signaled.replace(true) {
                        "already_signaled"
                    } else {
                        "signaled"
                    };
                    self.record_concurrency(
                        "timer.fire",
                        &timer.token_id.to_string(),
                        decision,
                        timer.waiters.get(),
                        Some(MAX_WAKE_TOKEN_WAITERS),
                    );
                }
                self.record_concurrency(
                    "clock.advance",
                    var,
                    "advanced",
                    due.len(),
                    Some(MAX_CLOCK_TIMERS),
                );
                Ok(Some(Value::Int(due.len() as i64)))
            }
            "wake_token_new" => {
                expect_arity(name, args, 0)?;
                let id = self.concurrency.allocate_wake_token();
                self.record_concurrency(
                    "wake_token.new",
                    &id.to_string(),
                    "open",
                    0,
                    Some(MAX_WAKE_TOKEN_WAITERS),
                );
                Ok(Some(Value::WakeToken {
                    id,
                    signaled: std::rc::Rc::new(std::cell::Cell::new(false)),
                    cancelled: std::rc::Rc::new(std::cell::Cell::new(false)),
                    waiters: std::rc::Rc::new(std::cell::Cell::new(0)),
                    timer_queue: None,
                }))
            }
            "wake_token_signal" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "wake_token_target",
                        "wake_token_signal expects a wake-token variable",
                    ));
                };
                let (decision, awakened, waiters) = match env.get(var) {
                    Some(Value::WakeToken {
                        signaled,
                        cancelled,
                        waiters,
                        ..
                    }) => {
                        let waiters = waiters.get();
                        if cancelled.get() {
                            ("cancelled", 0, waiters)
                        } else if signaled.replace(true) {
                            ("already_signaled", 0, waiters)
                        } else {
                            ("signaled", waiters, waiters)
                        }
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("wake_token_signal target '{var}' is not a wake token"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown wake token '{var}'"),
                        ));
                    }
                };
                self.record_concurrency(
                    "wake_token.signal",
                    var,
                    decision,
                    waiters,
                    Some(MAX_WAKE_TOKEN_WAITERS),
                );
                Ok(Some(Value::Int(awakened as i64)))
            }
            "wake_token_select" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?;
                let Value::Array(arms) = value else {
                    return Err(trap(
                        "wake_token_select_type",
                        "wake_token_select expects a Vec[WakeToken]",
                    ));
                };
                if arms.len() > MAX_WAKE_TOKEN_SELECT_ARMS {
                    self.record_concurrency(
                        "wake_token.select",
                        "set",
                        "capacity_exhausted",
                        arms.len(),
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "wake_token_select_capacity",
                        format!(
                            "wake_token_select cannot inspect more than {MAX_WAKE_TOKEN_SELECT_ARMS} arms"
                        ),
                    ));
                }
                let mut tokens = Vec::with_capacity(arms.len());
                let mut ids = BTreeSet::new();
                for arm in arms {
                    let Value::WakeToken {
                        id,
                        signaled,
                        cancelled,
                        waiters,
                        timer_queue,
                    } = arm
                    else {
                        return Err(trap(
                            "wake_token_select_type",
                            "wake_token_select expects every arm to be a WakeToken",
                        ));
                    };
                    if !ids.insert(id) {
                        return Err(trap(
                            "wake_token_select_duplicate",
                            format!("wake_token_select contains duplicate token {id}"),
                        ));
                    }
                    if cancelled.get() {
                        return Err(trap(
                            "wake_token_select_cancelled",
                            format!("wake_token_select arm {id} is already cancelled"),
                        ));
                    }
                    if waiters.get() != 0 {
                        return Err(trap(
                            "wake_token_select_waiters",
                            format!(
                                "wake_token_select arm {id} already has {} parked waiter(s)",
                                waiters.get()
                            ),
                        ));
                    }
                    tokens.push((id, signaled, cancelled, timer_queue));
                }
                let winner = tokens.iter().position(|(_, signaled, _, _)| signaled.get());
                let Some(winner) = winner else {
                    self.record_concurrency(
                        "wake_token.select",
                        "set",
                        "pending",
                        tokens.len(),
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Ok(Some(Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "None".to_string(),
                        value: None,
                    }));
                };
                let winner_id = tokens[winner].0;
                if let Some(queue) = &tokens[winner].3 {
                    queue
                        .borrow_mut()
                        .retain(|timer| timer.token_id != winner_id);
                }
                for (index, (id, _, cancelled, timer_queue)) in tokens.iter().enumerate() {
                    if index == winner {
                        continue;
                    }
                    cancelled.set(true);
                    if let Some(queue) = timer_queue {
                        queue.borrow_mut().retain(|timer| timer.token_id != *id);
                    }
                    self.record_concurrency(
                        "wake_token.cancel",
                        &id.to_string(),
                        "select_loser",
                        index,
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                }
                self.record_concurrency(
                    "wake_token.select",
                    &winner_id.to_string(),
                    "selected",
                    winner,
                    Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                );
                Ok(Some(Value::Variant {
                    enum_name: "Option".to_string(),
                    variant: "Some".to_string(),
                    value: Some(Box::new(Value::Int(winner as i64))),
                }))
            }
            "readiness_arm_token"
            | "readiness_arm_task"
            | "readiness_arm_channel"
            | "readiness_arm_channel_send"
            | "readiness_arm_file_read"
            | "readiness_arm_file_create" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "readiness_target",
                        format!("{name} expects a handle variable"),
                    ));
                };
                let source = match (name, env.get(var)) {
                    (
                        "readiness_arm_token",
                        Some(Value::WakeToken {
                            id,
                            signaled,
                            cancelled,
                            waiters,
                            timer_queue,
                        }),
                    ) => {
                        if cancelled.get() {
                            return Err(trap(
                                "readiness_arm_cancelled",
                                format!("wake token '{var}' is already cancelled"),
                            ));
                        }
                        if waiters.get() != 0 {
                            return Err(trap(
                                "readiness_arm_waiters",
                                format!(
                                    "wake token '{var}' already has {} parked waiter(s); adapt the task instead",
                                    waiters.get()
                                ),
                            ));
                        }
                        ReadinessSource::Token {
                            target: format!("token:{var}"),
                            id: *id,
                            signaled: signaled.clone(),
                            cancelled: cancelled.clone(),
                            waiters: waiters.clone(),
                            timer_queue: timer_queue.clone(),
                        }
                    }
                    (
                        "readiness_arm_task",
                        Some(Value::Task {
                            state, ready, open, ..
                        }),
                    ) => {
                        if !open.get() {
                            return Err(trap(
                                "readiness_arm_closed",
                                format!("task '{var}' is already closed"),
                            ));
                        }
                        ReadinessSource::Task {
                            target: format!("task:{var}"),
                            ready: ready.clone(),
                            open: open.clone(),
                            waiting: match state {
                                TaskState::Waiting(registration) => Some(registration.clone()),
                                _ => None,
                            },
                        }
                    }
                    ("readiness_arm_channel", Some(Value::Channel { readable, .. })) => {
                        ReadinessSource::Channel {
                            target: format!("channel:{var}"),
                            readable: readable.clone(),
                        }
                    }
                    ("readiness_arm_channel_send", Some(Value::Channel { writable, .. })) => {
                        ReadinessSource::ChannelSend {
                            target: format!("channel_send:{var}"),
                            writable: writable.clone(),
                        }
                    }
                    ("readiness_arm_file_read", Some(Value::String(path))) => {
                        let resolved = self.require_io()?.resolve_file_readiness(path)?;
                        ReadinessSource::FileRead {
                            target: format!("file_read:{var}"),
                            path: resolved,
                        }
                    }
                    ("readiness_arm_file_create", Some(Value::String(path))) => {
                        let resolved = self.require_io()?.resolve_file_create_readiness(path)?;
                        ReadinessSource::FileCreate {
                            target: format!("file_create:{var}"),
                            path: resolved,
                        }
                    }
                    ("readiness_arm_token", Some(_)) => {
                        return Err(trap(
                            "type_error",
                            format!("readiness_arm_token target '{var}' is not a wake token"),
                        ));
                    }
                    ("readiness_arm_task", Some(_)) => {
                        return Err(trap(
                            "type_error",
                            format!("readiness_arm_task target '{var}' is not a task"),
                        ));
                    }
                    ("readiness_arm_channel", Some(_)) => {
                        return Err(trap(
                            "type_error",
                            format!("readiness_arm_channel target '{var}' is not a channel"),
                        ));
                    }
                    ("readiness_arm_channel_send", Some(_)) => {
                        return Err(trap(
                            "type_error",
                            format!("readiness_arm_channel_send target '{var}' is not a channel"),
                        ));
                    }
                    ("readiness_arm_file_read", Some(_)) => {
                        return Err(trap(
                            "type_error",
                            format!("readiness_arm_file_read target '{var}' is not a Path"),
                        ));
                    }
                    ("readiness_arm_file_create", Some(_)) => {
                        return Err(trap(
                            "type_error",
                            format!("readiness_arm_file_create target '{var}' is not a Path"),
                        ));
                    }
                    (_, None) => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown readiness source '{var}'"),
                        ));
                    }
                    _ => unreachable!("readiness adapter name was matched above"),
                };
                self.record_concurrency(
                    "readiness.arm",
                    readiness_source_target(&source),
                    if readiness_source_is_ready(&source) {
                        "ready"
                    } else {
                        "pending"
                    },
                    usize::from(readiness_source_is_ready(&source)),
                    None,
                );
                Ok(Some(Value::ReadinessArm { source }))
            }
            "readiness_wait" | "readiness_wait_timeout" => {
                let has_timeout = name == "readiness_wait_timeout";
                expect_arity(name, args, if has_timeout { 3 } else { 2 })?;
                let Expr::Var(clock_var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        format!("{name} expects an async-clock variable"),
                    ));
                };
                let (now, clock_timers) = match env.get(clock_var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("{name} target '{clock_var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{clock_var}'"),
                        ));
                    }
                };
                let value = self.eval_expr(&args[1], env)?;
                let Value::Array(arms) = value else {
                    return Err(trap(
                        "readiness_wait_type",
                        format!("{name} expects a Vec[ReadinessArm]"),
                    ));
                };
                let timeout = if has_timeout {
                    let delay = self.eval_expr(&args[2], env)?.as_i64()?;
                    let delay = usize::try_from(delay)
                        .ok()
                        .filter(|delay| *delay <= MAX_CLOCK_TICK)
                        .ok_or_else(|| {
                            trap(
                                "readiness_timeout_range",
                                format!(
                                    "readiness timeout must be between 0 and {MAX_CLOCK_TICK} ticks"
                                ),
                            )
                        })?;
                    let deadline = now
                        .get()
                        .checked_add(delay)
                        .filter(|tick| *tick <= MAX_CLOCK_TICK)
                        .ok_or_else(|| {
                            trap(
                                "clock_range",
                                format!(
                                    "readiness timeout cannot exceed logical tick {MAX_CLOCK_TICK}"
                                ),
                            )
                        })?;
                    Some((delay, deadline))
                } else {
                    None
                };
                if arms.is_empty() && timeout.is_none() {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "stalled",
                        0,
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "reactor_stalled",
                        "readiness_wait has no readiness arms",
                    ));
                }
                if arms.len() > MAX_WAKE_TOKEN_SELECT_ARMS {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "capacity_exhausted",
                        arms.len(),
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "readiness_wait_capacity",
                        format!(
                            "{name} cannot inspect more than {MAX_WAKE_TOKEN_SELECT_ARMS} arms"
                        ),
                    ));
                }
                let mut sources = Vec::with_capacity(arms.len());
                for arm in arms {
                    let Value::ReadinessArm { source } = arm else {
                        return Err(trap(
                            "readiness_wait_type",
                            format!("{name} expects every arm to be a ReadinessArm"),
                        ));
                    };
                    if sources
                        .iter()
                        .any(|existing| readiness_sources_same(existing, &source))
                    {
                        return Err(trap(
                            "readiness_wait_duplicate",
                            format!(
                                "{name} contains duplicate source '{}'",
                                readiness_source_target(&source)
                            ),
                        ));
                    }
                    match &source {
                        ReadinessSource::Token {
                            id,
                            cancelled,
                            waiters,
                            timer_queue,
                            ..
                        } => {
                            if cancelled.get() {
                                return Err(trap(
                                    "readiness_arm_cancelled",
                                    format!("readiness token arm {id} is already cancelled"),
                                ));
                            }
                            if waiters.get() != 0 {
                                return Err(trap(
                                    "readiness_arm_waiters",
                                    format!(
                                        "readiness token arm {id} already has {} parked waiter(s)",
                                        waiters.get()
                                    ),
                                ));
                            }
                            if let Some(queue) = timer_queue {
                                if !std::rc::Rc::ptr_eq(queue, &clock_timers) {
                                    return Err(trap(
                                        "readiness_wait_clock",
                                        format!(
                                            "readiness timer arm {id} belongs to a different async clock"
                                        ),
                                    ));
                                }
                            }
                        }
                        ReadinessSource::Task { open, waiting, .. } => {
                            if !open.get() {
                                return Err(trap(
                                    "readiness_arm_closed",
                                    format!(
                                        "readiness task arm '{}' is already closed",
                                        readiness_source_target(&source)
                                    ),
                                ));
                            }
                            if let Some(WakeRegistration {
                                token_id,
                                timer_queue: Some(queue),
                                ..
                            }) = waiting
                            {
                                if !std::rc::Rc::ptr_eq(queue, &clock_timers) {
                                    return Err(trap(
                                        "readiness_wait_clock",
                                        format!(
                                            "readiness task timer source {token_id} belongs to a different async clock"
                                        ),
                                    ));
                                }
                            }
                        }
                        ReadinessSource::Channel { .. }
                        | ReadinessSource::ChannelSend { .. }
                        | ReadinessSource::FileRead { .. }
                        | ReadinessSource::FileCreate { .. } => {}
                    }
                    sources.push(source);
                }

                let mut winner = sources.iter().position(readiness_source_is_ready);
                if winner.is_some() {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "ready",
                        0,
                        Some(MAX_CLOCK_TICK),
                    );
                } else {
                    let timer_ids = sources
                        .iter()
                        .filter_map(readiness_source_timer_id)
                        .collect::<BTreeSet<_>>();
                    let next_deadline = clock_timers
                        .borrow()
                        .iter()
                        .filter(|timer| {
                            timer_ids.contains(&timer.token_id) && !timer.cancelled.get()
                        })
                        .map(|timer| timer.deadline)
                        .min();
                    let (wait_deadline, arm_deadline_selected) = match (next_deadline, timeout) {
                        (Some(deadline), Some((_, timeout_deadline)))
                            if deadline <= timeout_deadline =>
                        {
                            (deadline, true)
                        }
                        (_, Some((_, timeout_deadline))) => (timeout_deadline, false),
                        (Some(deadline), None) => (deadline, true),
                        (None, None) => {
                            self.record_concurrency(
                                "reactor.wait",
                                clock_var,
                                "stalled",
                                0,
                                Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                            );
                            return Err(trap(
                                "reactor_stalled",
                                "readiness_wait has no ready arm or scheduled timer that can make progress",
                            ));
                        }
                    };
                    let previous_now = now.get();
                    now.set(wait_deadline);
                    let mut due = Vec::new();
                    {
                        let mut scheduled = clock_timers.borrow_mut();
                        let mut pending = Vec::with_capacity(scheduled.len());
                        for timer in scheduled.drain(..) {
                            if timer.deadline <= wait_deadline {
                                due.push(timer);
                            } else {
                                pending.push(timer);
                            }
                        }
                        *scheduled = pending;
                    }
                    due.sort_by_key(|timer| (timer.deadline, timer.sequence));
                    for timer in &due {
                        if timer.cancelled.get() {
                            continue;
                        }
                        let decision = if timer.signaled.replace(true) {
                            "already_signaled"
                        } else {
                            "signaled"
                        };
                        self.record_concurrency(
                            "timer.fire",
                            &timer.token_id.to_string(),
                            decision,
                            timer.waiters.get(),
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                    }
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "advanced",
                        wait_deadline - previous_now,
                        Some(MAX_CLOCK_TICK),
                    );
                    winner = sources.iter().position(readiness_source_is_ready);
                    if arm_deadline_selected && winner.is_none() {
                        return Err(trap(
                            "reactor_stalled",
                            format!("{name} advanced without making an arm ready"),
                        ));
                    }
                }

                let Some(winner) = winner else {
                    let Some((delay, _)) = timeout else {
                        return Err(trap(
                            "reactor_stalled",
                            "readiness_wait advanced without making an arm ready",
                        ));
                    };
                    self.record_concurrency(
                        "readiness.wait_timeout",
                        clock_var,
                        "timed_out",
                        delay,
                        Some(MAX_CLOCK_TICK),
                    );
                    return Ok(Some(option_none()));
                };
                match &sources[winner] {
                    ReadinessSource::Token {
                        id, timer_queue, ..
                    } => {
                        if let Some(queue) = timer_queue {
                            queue.borrow_mut().retain(|timer| timer.token_id != *id);
                        }
                    }
                    ReadinessSource::Task { ready, waiting, .. } => {
                        ready.set(true);
                        if let Some(WakeRegistration {
                            token_id,
                            timer_queue: Some(queue),
                            ..
                        }) = waiting
                        {
                            queue
                                .borrow_mut()
                                .retain(|timer| timer.token_id != *token_id);
                        }
                    }
                    ReadinessSource::Channel { .. }
                    | ReadinessSource::ChannelSend { .. }
                    | ReadinessSource::FileRead { .. }
                    | ReadinessSource::FileCreate { .. } => {}
                }
                for (index, source) in sources.iter().enumerate() {
                    if index == winner {
                        continue;
                    }
                    if let ReadinessSource::Token {
                        id,
                        cancelled,
                        timer_queue,
                        ..
                    } = source
                    {
                        cancelled.set(true);
                        if let Some(queue) = timer_queue {
                            queue.borrow_mut().retain(|timer| timer.token_id != *id);
                        }
                        self.record_concurrency(
                            "wake_token.cancel",
                            &id.to_string(),
                            "readiness_loser",
                            index,
                            Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                        );
                    }
                }
                self.record_concurrency(
                    if has_timeout {
                        "readiness.wait_timeout"
                    } else {
                        "readiness.wait"
                    },
                    readiness_source_target(&sources[winner]),
                    "selected",
                    winner,
                    Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                );
                if has_timeout {
                    Ok(Some(Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "Some".to_string(),
                        value: Some(Box::new(Value::Int(winner as i64))),
                    }))
                } else {
                    Ok(Some(Value::Int(winner as i64)))
                }
            }
            "wake_token_wait" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(clock_var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "wake_token_wait expects an async-clock variable",
                    ));
                };
                let (now, clock_timers) = match env.get(clock_var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("wake_token_wait target '{clock_var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{clock_var}'"),
                        ));
                    }
                };
                let value = self.eval_expr(&args[1], env)?;
                let Value::Array(arms) = value else {
                    return Err(trap(
                        "wake_token_wait_type",
                        "wake_token_wait expects a Vec[WakeToken]",
                    ));
                };
                if arms.is_empty() {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "stalled",
                        0,
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "reactor_stalled",
                        "wake_token_wait has no readiness arms",
                    ));
                }
                if arms.len() > MAX_WAKE_TOKEN_SELECT_ARMS {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "capacity_exhausted",
                        arms.len(),
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "wake_token_wait_capacity",
                        format!(
                            "wake_token_wait cannot inspect more than {MAX_WAKE_TOKEN_SELECT_ARMS} arms"
                        ),
                    ));
                }
                let mut tokens = Vec::with_capacity(arms.len());
                let mut ids = BTreeSet::new();
                for arm in arms {
                    let Value::WakeToken {
                        id,
                        signaled,
                        cancelled,
                        waiters,
                        timer_queue,
                    } = arm
                    else {
                        return Err(trap(
                            "wake_token_wait_type",
                            "wake_token_wait expects every arm to be a WakeToken",
                        ));
                    };
                    if !ids.insert(id) {
                        return Err(trap(
                            "wake_token_wait_duplicate",
                            format!("wake_token_wait contains duplicate token {id}"),
                        ));
                    }
                    if cancelled.get() {
                        return Err(trap(
                            "wake_token_wait_cancelled",
                            format!("wake_token_wait arm {id} is already cancelled"),
                        ));
                    }
                    if waiters.get() != 0 {
                        return Err(trap(
                            "wake_token_wait_waiters",
                            format!(
                                "wake_token_wait arm {id} already has {} parked waiter(s)",
                                waiters.get()
                            ),
                        ));
                    }
                    if let Some(queue) = &timer_queue {
                        if !std::rc::Rc::ptr_eq(queue, &clock_timers) {
                            return Err(trap(
                                "wake_token_wait_clock",
                                format!(
                                    "wake_token_wait timer arm {id} belongs to a different async clock"
                                ),
                            ));
                        }
                    }
                    tokens.push((id, signaled, cancelled, timer_queue));
                }

                let mut winner = tokens.iter().position(|(_, signaled, _, _)| signaled.get());
                if winner.is_some() {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "ready",
                        0,
                        Some(MAX_CLOCK_TICK),
                    );
                } else {
                    let next_deadline = clock_timers
                        .borrow()
                        .iter()
                        .filter(|timer| ids.contains(&timer.token_id) && !timer.cancelled.get())
                        .map(|timer| timer.deadline)
                        .min();
                    let Some(next_deadline) = next_deadline else {
                        self.record_concurrency(
                            "reactor.wait",
                            clock_var,
                            "stalled",
                            0,
                            Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                        );
                        return Err(trap(
                            "reactor_stalled",
                            "wake_token_wait has no ready arm or scheduled timer that can make progress",
                        ));
                    };
                    let previous_now = now.get();
                    now.set(next_deadline);
                    let mut due = Vec::new();
                    {
                        let mut scheduled = clock_timers.borrow_mut();
                        let mut pending = Vec::with_capacity(scheduled.len());
                        for timer in scheduled.drain(..) {
                            if timer.deadline <= next_deadline {
                                due.push(timer);
                            } else {
                                pending.push(timer);
                            }
                        }
                        *scheduled = pending;
                    }
                    due.sort_by_key(|timer| (timer.deadline, timer.sequence));
                    for timer in &due {
                        if timer.cancelled.get() {
                            continue;
                        }
                        let decision = if timer.signaled.replace(true) {
                            "already_signaled"
                        } else {
                            "signaled"
                        };
                        self.record_concurrency(
                            "timer.fire",
                            &timer.token_id.to_string(),
                            decision,
                            timer.waiters.get(),
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                    }
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "advanced",
                        next_deadline - previous_now,
                        Some(MAX_CLOCK_TICK),
                    );
                    winner = tokens.iter().position(|(_, signaled, _, _)| signaled.get());
                }

                let Some(winner) = winner else {
                    return Err(trap(
                        "reactor_stalled",
                        "wake_token_wait advanced without making an arm ready",
                    ));
                };
                let winner_id = tokens[winner].0;
                if let Some(queue) = &tokens[winner].3 {
                    queue
                        .borrow_mut()
                        .retain(|timer| timer.token_id != winner_id);
                }
                for (index, (id, _, cancelled, timer_queue)) in tokens.iter().enumerate() {
                    if index == winner {
                        continue;
                    }
                    cancelled.set(true);
                    if let Some(queue) = timer_queue {
                        queue.borrow_mut().retain(|timer| timer.token_id != *id);
                    }
                    self.record_concurrency(
                        "wake_token.cancel",
                        &id.to_string(),
                        "wait_loser",
                        index,
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                }
                self.record_concurrency(
                    "wake_token.wait",
                    &winner_id.to_string(),
                    "selected",
                    winner,
                    Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                );
                Ok(Some(Value::Int(winner as i64)))
            }
            "task_spawn" | "task_spawn_parked" => {
                expect_arity(name, args, 2)?;
                let callable = self.eval_expr(&args[0], env)?;
                if !matches!(callable, Value::Function(_) | Value::Closure { .. }) {
                    return Err(trap(
                        "task_not_callable",
                        format!("{name} expects a one-argument function value"),
                    ));
                }
                let input = self.eval_expr(&args[1], env)?;
                let target = match &callable {
                    Value::Function(name) => name.as_str(),
                    Value::Closure { .. } => "closure",
                    _ => unreachable!("task callable was validated"),
                };
                let (operation, decision, state) = if name == "task_spawn" {
                    ("task.spawn", "pending", TaskState::Pending)
                } else {
                    ("task.spawn_parked", "parked", TaskState::Parked)
                };
                self.record_concurrency(operation, target, decision, 1, None);
                Ok(Some(Value::Task {
                    ready: std::rc::Rc::new(std::cell::Cell::new(matches!(
                        state,
                        TaskState::Pending
                    ))),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                    state,
                    callable: Box::new(callable),
                    input: Box::new(input),
                    continuations: Vec::new(),
                }))
            }
            "task_spawn_on" => {
                expect_arity(name, args, 3)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "wake_token_target",
                        "task_spawn_on expects a wake-token variable",
                    ));
                };
                match env.get(var) {
                    Some(Value::WakeToken { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_spawn_on target '{var}' is not a wake token"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown wake token '{var}'"),
                        ));
                    }
                }
                let callable = self.eval_expr(&args[1], env)?;
                if !matches!(callable, Value::Function(_) | Value::Closure { .. }) {
                    return Err(trap(
                        "task_not_callable",
                        "task_spawn_on expects a one-argument function value",
                    ));
                }
                let input = self.eval_expr(&args[2], env)?;
                let (id, signaled, cancelled, waiters, timer_queue) = match env.get(var) {
                    Some(Value::WakeToken {
                        id,
                        signaled,
                        cancelled,
                        waiters,
                        timer_queue,
                    }) => (
                        *id,
                        signaled.clone(),
                        cancelled.clone(),
                        waiters.clone(),
                        timer_queue.clone(),
                    ),
                    _ => {
                        unreachable!("wake-token binding changed type while evaluating task values")
                    }
                };
                if cancelled.get() {
                    return Err(trap(
                        "wake_token_cancelled",
                        format!("task_spawn_on target '{var}' is cancelled"),
                    ));
                }
                let (state, decision, waiter_count) = if signaled.get() {
                    (TaskState::Pending, "ready", waiters.get())
                } else {
                    let waiter_count = waiters.get();
                    if waiter_count >= MAX_WAKE_TOKEN_WAITERS {
                        self.record_concurrency(
                            "task.spawn_on",
                            var,
                            "capacity_exhausted",
                            waiter_count,
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                        return Err(trap(
                            "wake_token_capacity",
                            format!(
                                "wake token '{var}' cannot register more than {MAX_WAKE_TOKEN_WAITERS} waiters"
                            ),
                        ));
                    }
                    let waiter_count = waiter_count + 1;
                    waiters.set(waiter_count);
                    (
                        TaskState::Waiting(WakeRegistration {
                            token_id: id,
                            signaled,
                            waiters,
                            timer_queue,
                        }),
                        "parked",
                        waiter_count,
                    )
                };
                self.record_concurrency(
                    "task.spawn_on",
                    var,
                    decision,
                    waiter_count,
                    Some(MAX_WAKE_TOKEN_WAITERS),
                );
                Ok(Some(Value::Task {
                    ready: std::rc::Rc::new(std::cell::Cell::new(matches!(
                        state,
                        TaskState::Pending
                    ))),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                    state,
                    callable: Box::new(callable),
                    input: Box::new(input),
                    continuations: Vec::new(),
                }))
            }
            "task_then" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_then expects a task-handle variable",
                    ));
                };
                match env.get(var) {
                    Some(Value::Task {
                        state: TaskState::Parked | TaskState::Waiting(_) | TaskState::Pending,
                        ..
                    }) => {}
                    Some(Value::Task { .. }) => {
                        return Err(trap(
                            "task_already_consumed",
                            format!("task '{var}' was already awaited, cancelled, or chained"),
                        ));
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_then target '{var}' is not a task"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task handle '{var}'"),
                        ));
                    }
                }
                let continuation = self.eval_expr(&args[1], env)?;
                if !matches!(continuation, Value::Function(_) | Value::Closure { .. }) {
                    return Err(trap(
                        "task_not_callable",
                        "task_then expects a one-argument continuation",
                    ));
                }
                let task = env.get(var).cloned().ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown task handle '{var}'"))
                })?;
                let Value::Task {
                    state,
                    ready,
                    open,
                    callable,
                    input,
                    mut continuations,
                } = task
                else {
                    return Err(trap(
                        "type_error",
                        format!("task_then target '{var}' is not a task"),
                    ));
                };
                if !matches!(
                    state,
                    TaskState::Parked | TaskState::Waiting(_) | TaskState::Pending
                ) {
                    return Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was closed while evaluating its continuation"),
                    ));
                }
                if continuations.len() >= MAX_TASK_CONTINUATIONS {
                    self.record_concurrency(
                        "task.then",
                        var,
                        "capacity_exhausted",
                        continuations.len(),
                        Some(MAX_TASK_CONTINUATIONS),
                    );
                    return Err(trap(
                        "task_continuation_capacity",
                        format!(
                            "task '{var}' cannot contain more than {MAX_TASK_CONTINUATIONS} continuations"
                        ),
                    ));
                }
                open.set(false);
                env.insert(
                    var.clone(),
                    Value::Task {
                        state: TaskState::Consumed,
                        ready: ready.clone(),
                        open,
                        callable: callable.clone(),
                        input: input.clone(),
                        continuations: continuations.clone(),
                    },
                );
                continuations.push(continuation);
                let depth = continuations.len();
                self.record_concurrency(
                    "task.then",
                    var,
                    "chained",
                    depth,
                    Some(MAX_TASK_CONTINUATIONS),
                );
                Ok(Some(Value::Task {
                    state,
                    ready: std::rc::Rc::new(std::cell::Cell::new(ready.get())),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                    callable,
                    input,
                    continuations,
                }))
            }
            "task_wait_on" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_wait_on expects a task-handle variable",
                    ));
                };
                let Expr::Var(token_var) = &args[1] else {
                    return Err(trap(
                        "wake_token_target",
                        "task_wait_on expects a wake-token variable",
                    ));
                };
                let (token_id, signaled, cancelled, waiters, timer_queue) = match env.get(token_var)
                {
                    Some(Value::WakeToken {
                        id,
                        signaled,
                        cancelled,
                        waiters,
                        timer_queue,
                    }) => (
                        *id,
                        signaled.clone(),
                        cancelled.clone(),
                        waiters.clone(),
                        timer_queue.clone(),
                    ),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_wait_on target '{token_var}' is not a wake token"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown wake token '{token_var}'"),
                        ));
                    }
                };
                if cancelled.get() {
                    return Err(trap(
                        "wake_token_cancelled",
                        format!("task_wait_on target '{token_var}' is cancelled"),
                    ));
                }
                let task = env.get(var).cloned().ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown task handle '{var}'"))
                })?;
                let Value::Task {
                    state,
                    ready,
                    open,
                    callable,
                    input,
                    continuations,
                } = task
                else {
                    return Err(trap(
                        "type_error",
                        format!("task_wait_on target '{var}' is not a task"),
                    ));
                };
                if !matches!(
                    state,
                    TaskState::Parked | TaskState::Waiting(_) | TaskState::Pending
                ) {
                    return Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was already awaited, cancelled, or rebound"),
                    ));
                }
                let reuses_registration = matches!(
                    &state,
                    TaskState::Waiting(registration) if registration.token_id == token_id
                );
                let available_waiters = waiters
                    .get()
                    .saturating_sub(usize::from(reuses_registration));
                if !signaled.get() && available_waiters >= MAX_WAKE_TOKEN_WAITERS {
                    self.record_concurrency(
                        "task.wait_on",
                        &format!("{var}@{token_var}"),
                        "capacity_exhausted",
                        waiters.get(),
                        Some(MAX_WAKE_TOKEN_WAITERS),
                    );
                    return Err(trap(
                        "wake_token_capacity",
                        format!(
                            "wake token '{token_var}' cannot register more than {MAX_WAKE_TOKEN_WAITERS} waiters"
                        ),
                    ));
                }
                if let TaskState::Waiting(registration) = &state {
                    release_wake_registration(registration);
                }
                ready.set(false);
                open.set(false);
                env.insert(
                    var.clone(),
                    Value::Task {
                        state: TaskState::Consumed,
                        ready,
                        open,
                        callable: callable.clone(),
                        input: input.clone(),
                        continuations: continuations.clone(),
                    },
                );
                let (state, decision, waiter_count) = if signaled.get() {
                    (TaskState::Pending, "ready", waiters.get())
                } else {
                    let waiter_count = waiters.get() + 1;
                    waiters.set(waiter_count);
                    (
                        TaskState::Waiting(WakeRegistration {
                            token_id,
                            signaled,
                            waiters,
                            timer_queue,
                        }),
                        "parked",
                        waiter_count,
                    )
                };
                self.record_concurrency(
                    "task.wait_on",
                    &format!("{var}@{token_var}"),
                    decision,
                    waiter_count,
                    Some(MAX_WAKE_TOKEN_WAITERS),
                );
                Ok(Some(Value::Task {
                    ready: std::rc::Rc::new(std::cell::Cell::new(matches!(
                        state,
                        TaskState::Pending
                    ))),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                    state,
                    callable,
                    input,
                    continuations,
                }))
            }
            "task_wake" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_wake expects a task-handle variable",
                    ));
                };
                let (decision, woke) = match env.get_mut(var) {
                    Some(Value::Task { state, ready, .. })
                        if matches!(state, TaskState::Parked | TaskState::Waiting(_)) =>
                    {
                        if let TaskState::Waiting(registration) = state {
                            release_wake_registration(registration);
                        }
                        *state = TaskState::Pending;
                        ready.set(true);
                        ("woken", true)
                    }
                    Some(Value::Task {
                        state: TaskState::Pending,
                        ..
                    }) => ("already_ready", false),
                    Some(Value::Task { .. }) => ("already_closed", false),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_wake target '{var}' is not a task"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task handle '{var}'"),
                        ));
                    }
                };
                self.record_concurrency("task.wake", var, decision, usize::from(woke), None);
                Ok(Some(Value::Bool(woke)))
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
                    mut state,
                    ready,
                    open,
                    callable,
                    input,
                    continuations,
                } = task
                else {
                    return Err(trap(
                        "type_error",
                        format!("task_await target '{var}' is not a task"),
                    ));
                };
                if let TaskState::Waiting(registration) = &state {
                    if !registration.signaled.get() {
                        return Err(trap(
                            "task_waiting",
                            format!(
                                "task '{var}' is waiting for wake token {} to be signaled",
                                registration.token_id
                            ),
                        ));
                    }
                    release_wake_registration(registration);
                    self.record_concurrency(
                        "task.wake_event",
                        var,
                        "woken",
                        registration.waiters.get(),
                        Some(MAX_WAKE_TOKEN_WAITERS),
                    );
                    state = TaskState::Pending;
                    ready.set(true);
                }
                match state {
                    TaskState::Parked => Err(trap(
                        "task_parked",
                        format!("task '{var}' must be woken before it can run"),
                    )),
                    TaskState::Cancelled => Err(trap(
                        "task_cancelled",
                        format!("task '{var}' was cancelled before it ran"),
                    )),
                    TaskState::Consumed => Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was already awaited or cancelled"),
                    )),
                    TaskState::Pending => {
                        ready.set(false);
                        open.set(false);
                        env.insert(
                            var.clone(),
                            Value::Task {
                                state: TaskState::Consumed,
                                ready: ready.clone(),
                                open: open.clone(),
                                callable: callable.clone(),
                                input: input.clone(),
                                continuations: continuations.clone(),
                            },
                        );
                        let result = self.execute_task_chain(*callable, *input, continuations, var);
                        self.record_concurrency(
                            "task.await",
                            var,
                            if result.is_ok() { "completed" } else { "error" },
                            usize::from(result.is_ok()),
                            None,
                        );
                        Ok(Some(result?))
                    }
                    TaskState::Waiting(_) => {
                        unreachable!("waiting task state was resolved before execution")
                    }
                }
            }
            "task_poll" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_poll expects a task-handle variable",
                    ));
                };
                let task = env.get(var).cloned().ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown task handle '{var}'"))
                })?;
                let Value::Task {
                    mut state,
                    ready,
                    open,
                    callable,
                    input,
                    continuations,
                } = task
                else {
                    return Err(trap(
                        "type_error",
                        format!("task_poll target '{var}' is not a task"),
                    ));
                };
                if let TaskState::Waiting(registration) = &state {
                    if !registration.signaled.get() {
                        self.record_concurrency(
                            "task.poll",
                            var,
                            "waiting",
                            registration.waiters.get(),
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                        return Ok(Some(option_none()));
                    }
                    release_wake_registration(registration);
                    self.record_concurrency(
                        "task.wake_event",
                        var,
                        "woken",
                        registration.waiters.get(),
                        Some(MAX_WAKE_TOKEN_WAITERS),
                    );
                    state = TaskState::Pending;
                    ready.set(true);
                }
                match state {
                    TaskState::Parked => {
                        self.record_concurrency("task.poll", var, "parked", 0, None);
                        Ok(Some(option_none()))
                    }
                    TaskState::Cancelled => Err(trap(
                        "task_cancelled",
                        format!("task '{var}' was cancelled before it ran"),
                    )),
                    TaskState::Consumed => Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was already completed or cancelled"),
                    )),
                    TaskState::Pending => {
                        ready.set(false);
                        open.set(false);
                        env.insert(
                            var.clone(),
                            Value::Task {
                                state: TaskState::Consumed,
                                ready: ready.clone(),
                                open: open.clone(),
                                callable: callable.clone(),
                                input: input.clone(),
                                continuations: continuations.clone(),
                            },
                        );
                        let result = self.execute_task_chain(*callable, *input, continuations, var);
                        self.record_concurrency(
                            "task.poll",
                            var,
                            if result.is_ok() { "completed" } else { "error" },
                            usize::from(result.is_ok()),
                            None,
                        );
                        Ok(Some(Value::Variant {
                            enum_name: "Option".to_string(),
                            variant: "Some".to_string(),
                            value: Some(Box::new(result?)),
                        }))
                    }
                    TaskState::Waiting(_) => {
                        unreachable!("waiting task state was resolved before polling")
                    }
                }
            }
            "task_wait" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(clock_var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "task_wait expects an async-clock variable",
                    ));
                };
                let Expr::Var(var) = &args[1] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_wait expects a task-handle variable",
                    ));
                };
                let (now, clock_timers) = match env.get(clock_var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_wait target '{clock_var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{clock_var}'"),
                        ));
                    }
                };
                let task = env.get(var).cloned().ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown task handle '{var}'"))
                })?;
                let Value::Task {
                    mut state,
                    ready,
                    open,
                    callable,
                    input,
                    continuations,
                } = task
                else {
                    return Err(trap(
                        "type_error",
                        format!("task_wait target '{var}' is not a task"),
                    ));
                };
                let mut reactor_wait_recorded = false;
                if let TaskState::Waiting(registration) = &state {
                    if let Some(queue) = &registration.timer_queue {
                        if !std::rc::Rc::ptr_eq(queue, &clock_timers) {
                            return Err(trap(
                                "task_wait_clock",
                                format!(
                                    "task '{var}' timer source {} belongs to a different async clock",
                                    registration.token_id
                                ),
                            ));
                        }
                    }
                    if !registration.signaled.get() {
                        let next_deadline = registration.timer_queue.as_ref().and_then(|queue| {
                            queue
                                .borrow()
                                .iter()
                                .filter(|timer| {
                                    timer.token_id == registration.token_id
                                        && !timer.cancelled.get()
                                })
                                .map(|timer| timer.deadline)
                                .min()
                        });
                        let Some(next_deadline) = next_deadline else {
                            self.record_concurrency(
                                "reactor.wait",
                                clock_var,
                                "stalled",
                                0,
                                Some(MAX_CLOCK_TICK),
                            );
                            self.record_concurrency("task.wait", var, "stalled", 0, None);
                            return Err(trap(
                                "reactor_stalled",
                                format!(
                                    "task '{var}' has no ready or scheduled timer source that can make progress"
                                ),
                            ));
                        };
                        let previous_now = now.get();
                        now.set(next_deadline);
                        let mut due = Vec::new();
                        {
                            let mut scheduled = clock_timers.borrow_mut();
                            let mut pending = Vec::with_capacity(scheduled.len());
                            for timer in scheduled.drain(..) {
                                if timer.deadline <= next_deadline {
                                    due.push(timer);
                                } else {
                                    pending.push(timer);
                                }
                            }
                            *scheduled = pending;
                        }
                        due.sort_by_key(|timer| (timer.deadline, timer.sequence));
                        for timer in &due {
                            if timer.cancelled.get() {
                                continue;
                            }
                            let decision = if timer.signaled.replace(true) {
                                "already_signaled"
                            } else {
                                "signaled"
                            };
                            self.record_concurrency(
                                "timer.fire",
                                &timer.token_id.to_string(),
                                decision,
                                timer.waiters.get(),
                                Some(MAX_WAKE_TOKEN_WAITERS),
                            );
                        }
                        self.record_concurrency(
                            "reactor.wait",
                            clock_var,
                            "advanced",
                            next_deadline - previous_now,
                            Some(MAX_CLOCK_TICK),
                        );
                        reactor_wait_recorded = true;
                    } else {
                        self.record_concurrency(
                            "reactor.wait",
                            clock_var,
                            "ready",
                            0,
                            Some(MAX_CLOCK_TICK),
                        );
                        reactor_wait_recorded = true;
                    }
                    if !registration.signaled.get() {
                        return Err(trap(
                            "reactor_stalled",
                            format!("task_wait advanced without making task '{var}' ready"),
                        ));
                    }
                    if let Some(queue) = &registration.timer_queue {
                        queue
                            .borrow_mut()
                            .retain(|timer| timer.token_id != registration.token_id);
                    }
                    release_wake_registration(registration);
                    self.record_concurrency(
                        "task.wake_event",
                        var,
                        "woken",
                        registration.waiters.get(),
                        Some(MAX_WAKE_TOKEN_WAITERS),
                    );
                    state = TaskState::Pending;
                    ready.set(true);
                }
                match state {
                    TaskState::Parked => {
                        self.record_concurrency(
                            "reactor.wait",
                            clock_var,
                            "stalled",
                            0,
                            Some(MAX_CLOCK_TICK),
                        );
                        self.record_concurrency("task.wait", var, "stalled", 0, None);
                        Err(trap(
                            "reactor_stalled",
                            format!("task '{var}' is parked without a represented wake source"),
                        ))
                    }
                    TaskState::Cancelled => Err(trap(
                        "task_cancelled",
                        format!("task '{var}' was cancelled before it ran"),
                    )),
                    TaskState::Consumed => Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was already completed or cancelled"),
                    )),
                    TaskState::Pending => {
                        if !reactor_wait_recorded {
                            self.record_concurrency(
                                "reactor.wait",
                                clock_var,
                                "ready",
                                0,
                                Some(MAX_CLOCK_TICK),
                            );
                        }
                        ready.set(false);
                        open.set(false);
                        env.insert(
                            var.clone(),
                            Value::Task {
                                state: TaskState::Consumed,
                                ready: ready.clone(),
                                open: open.clone(),
                                callable: callable.clone(),
                                input: input.clone(),
                                continuations: continuations.clone(),
                            },
                        );
                        let result = self.execute_task_chain(*callable, *input, continuations, var);
                        self.record_concurrency(
                            "task.wait",
                            var,
                            if result.is_ok() { "completed" } else { "error" },
                            usize::from(result.is_ok()),
                            None,
                        );
                        Ok(Some(result?))
                    }
                    TaskState::Waiting(_) => {
                        unreachable!("waiting task state was resolved before blocking wait")
                    }
                }
            }
            "task_timeout" => {
                expect_arity(name, args, 3)?;
                let Expr::Var(clock_var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "task_timeout expects an async-clock variable",
                    ));
                };
                let Expr::Var(var) = &args[1] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_timeout expects a task-handle variable",
                    ));
                };
                match env.get(clock_var) {
                    Some(Value::AsyncClock { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_timeout target '{clock_var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{clock_var}'"),
                        ));
                    }
                }
                match env.get(var) {
                    Some(Value::Task { .. }) => {}
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_timeout target '{var}' is not a task"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task handle '{var}'"),
                        ));
                    }
                }
                let delay = self.eval_expr(&args[2], env)?.as_i64()?;
                let delay = usize::try_from(delay)
                    .ok()
                    .filter(|delay| *delay <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "task_timeout_range",
                            format!("task timeout must be between 0 and {MAX_CLOCK_TICK} ticks"),
                        )
                    })?;
                let (now, clock_timers) = match env.get(clock_var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    _ => unreachable!("async-clock binding changed type while evaluating timeout"),
                };
                let timeout_deadline = now
                    .get()
                    .checked_add(delay)
                    .filter(|tick| *tick <= MAX_CLOCK_TICK)
                    .ok_or_else(|| {
                        trap(
                            "clock_range",
                            format!("task timeout cannot exceed logical tick {MAX_CLOCK_TICK}"),
                        )
                    })?;
                let task = env.get(var).cloned().ok_or_else(|| {
                    trap("unknown_symbol", format!("unknown task handle '{var}'"))
                })?;
                let Value::Task {
                    mut state,
                    ready,
                    open,
                    callable,
                    input,
                    continuations,
                } = task
                else {
                    unreachable!("task binding changed type while evaluating timeout")
                };
                if !open.get() || matches!(state, TaskState::Cancelled | TaskState::Consumed) {
                    return Err(trap(
                        "task_already_consumed",
                        format!("task '{var}' was already completed or cancelled"),
                    ));
                }
                if let TaskState::Waiting(registration) = &state {
                    if let Some(queue) = &registration.timer_queue {
                        if !std::rc::Rc::ptr_eq(queue, &clock_timers) {
                            return Err(trap(
                                "task_timeout_clock",
                                format!(
                                    "task '{var}' timer source {} belongs to a different async clock",
                                    registration.token_id
                                ),
                            ));
                        }
                    }
                }

                let mut task_won = matches!(state, TaskState::Pending)
                    || matches!(
                        &state,
                        TaskState::Waiting(registration) if registration.signaled.get()
                    );
                if task_won {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "ready",
                        0,
                        Some(MAX_CLOCK_TICK),
                    );
                } else {
                    let task_deadline = match &state {
                        TaskState::Waiting(registration) if registration.timer_queue.is_some() => {
                            clock_timers
                                .borrow()
                                .iter()
                                .filter(|timer| {
                                    timer.token_id == registration.token_id
                                        && !timer.cancelled.get()
                                })
                                .map(|timer| timer.deadline)
                                .min()
                        }
                        _ => None,
                    };
                    let task_deadline =
                        task_deadline.filter(|deadline| *deadline <= timeout_deadline);
                    let wait_deadline = task_deadline.unwrap_or(timeout_deadline);
                    let previous_now = now.get();
                    now.set(wait_deadline);
                    let mut due = Vec::new();
                    {
                        let mut scheduled = clock_timers.borrow_mut();
                        let mut pending = Vec::with_capacity(scheduled.len());
                        for timer in scheduled.drain(..) {
                            if timer.deadline <= wait_deadline {
                                due.push(timer);
                            } else {
                                pending.push(timer);
                            }
                        }
                        *scheduled = pending;
                    }
                    due.sort_by_key(|timer| (timer.deadline, timer.sequence));
                    for timer in &due {
                        if timer.cancelled.get() {
                            continue;
                        }
                        let decision = if timer.signaled.replace(true) {
                            "already_signaled"
                        } else {
                            "signaled"
                        };
                        self.record_concurrency(
                            "timer.fire",
                            &timer.token_id.to_string(),
                            decision,
                            timer.waiters.get(),
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                    }
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "advanced",
                        wait_deadline - previous_now,
                        Some(MAX_CLOCK_TICK),
                    );
                    task_won = task_deadline.is_some()
                        && matches!(
                            &state,
                            TaskState::Waiting(registration) if registration.signaled.get()
                        );
                    if task_deadline.is_some() && !task_won {
                        return Err(trap(
                            "reactor_stalled",
                            format!("task_timeout advanced without making task '{var}' ready"),
                        ));
                    }
                }

                if task_won {
                    if let TaskState::Waiting(registration) = &state {
                        if let Some(queue) = &registration.timer_queue {
                            queue
                                .borrow_mut()
                                .retain(|timer| timer.token_id != registration.token_id);
                        }
                        release_wake_registration(registration);
                        self.record_concurrency(
                            "task.wake_event",
                            var,
                            "woken",
                            registration.waiters.get(),
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                        state = TaskState::Pending;
                        ready.set(true);
                    }
                    debug_assert_eq!(state, TaskState::Pending);
                    ready.set(false);
                    open.set(false);
                    env.insert(
                        var.clone(),
                        Value::Task {
                            state: TaskState::Consumed,
                            ready: ready.clone(),
                            open: open.clone(),
                            callable: callable.clone(),
                            input: input.clone(),
                            continuations: continuations.clone(),
                        },
                    );
                    let result = self.execute_task_chain(*callable, *input, continuations, var);
                    self.record_concurrency(
                        "task.timeout",
                        var,
                        if result.is_ok() { "completed" } else { "error" },
                        delay,
                        Some(MAX_CLOCK_TICK),
                    );
                    Ok(Some(Value::Variant {
                        enum_name: "Option".to_string(),
                        variant: "Some".to_string(),
                        value: Some(Box::new(result?)),
                    }))
                } else {
                    if let TaskState::Waiting(registration) = &state {
                        release_wake_registration(registration);
                    }
                    ready.set(false);
                    open.set(false);
                    env.insert(
                        var.clone(),
                        Value::Task {
                            state: TaskState::Cancelled,
                            ready,
                            open,
                            callable,
                            input,
                            continuations,
                        },
                    );
                    self.record_concurrency("task.cancel", var, "timeout", 0, None);
                    self.record_concurrency(
                        "task.timeout",
                        var,
                        "timed_out",
                        delay,
                        Some(MAX_CLOCK_TICK),
                    );
                    Ok(Some(option_none()))
                }
            }
            "task_race" => {
                expect_arity(name, args, 2)?;
                let Expr::Var(clock_var) = &args[0] else {
                    return Err(trap(
                        "clock_target",
                        "task_race expects an async-clock variable",
                    ));
                };
                let Expr::Var(tasks_var) = &args[1] else {
                    return Err(trap(
                        "task_race_target",
                        "task_race expects a task-vector variable",
                    ));
                };
                let (now, clock_timers) = match env.get(clock_var) {
                    Some(Value::AsyncClock { now, timers, .. }) => (now.clone(), timers.clone()),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_race target '{clock_var}' is not an async clock"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown async clock '{clock_var}'"),
                        ));
                    }
                };
                let tasks = env.get(tasks_var).cloned().ok_or_else(|| {
                    trap(
                        "unknown_symbol",
                        format!("unknown task vector '{tasks_var}'"),
                    )
                })?;
                let Value::Array(tasks) = tasks else {
                    return Err(trap(
                        "task_race_type",
                        format!("task_race target '{tasks_var}' is not a task vector"),
                    ));
                };
                if tasks.is_empty() {
                    self.record_concurrency(
                        "task.race",
                        tasks_var,
                        "empty",
                        0,
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "task_race_empty",
                        "task_race requires at least one task",
                    ));
                }
                if tasks.len() > MAX_WAKE_TOKEN_SELECT_ARMS {
                    self.record_concurrency(
                        "task.race",
                        tasks_var,
                        "capacity_exhausted",
                        tasks.len(),
                        Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                    );
                    return Err(trap(
                        "task_race_capacity",
                        format!(
                            "task_race cannot inspect more than {MAX_WAKE_TOKEN_SELECT_ARMS} tasks"
                        ),
                    ));
                }

                let mut identities = Vec::with_capacity(tasks.len());
                for (index, task) in tasks.iter().enumerate() {
                    let Value::Task { state, open, .. } = task else {
                        return Err(trap(
                            "task_race_type",
                            format!("task_race input {index} is not a task"),
                        ));
                    };
                    if !open.get() || matches!(state, TaskState::Cancelled | TaskState::Consumed) {
                        return Err(trap(
                            "task_race_closed",
                            format!("task_race input {index} is already closed"),
                        ));
                    }
                    if identities
                        .iter()
                        .any(|identity| std::rc::Rc::ptr_eq(identity, open))
                    {
                        return Err(trap(
                            "task_race_duplicate",
                            format!("task_race input {index} duplicates an earlier task"),
                        ));
                    }
                    identities.push(open.clone());
                    if let TaskState::Waiting(registration) = state {
                        if let Some(queue) = &registration.timer_queue {
                            if !std::rc::Rc::ptr_eq(queue, &clock_timers) {
                                return Err(trap(
                                    "task_race_clock",
                                    format!(
                                        "task_race input {index} timer source {} belongs to a different async clock",
                                        registration.token_id
                                    ),
                                ));
                            }
                        }
                    }
                }

                let task_is_ready = |task: &Value| match task {
                    Value::Task {
                        state: TaskState::Pending,
                        ..
                    } => true,
                    Value::Task {
                        state: TaskState::Waiting(registration),
                        ..
                    } => registration.signaled.get(),
                    _ => false,
                };
                let mut winner = tasks.iter().position(task_is_ready);
                if winner.is_some() {
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "ready",
                        0,
                        Some(MAX_CLOCK_TICK),
                    );
                } else {
                    let timer_ids = tasks
                        .iter()
                        .filter_map(|task| match task {
                            Value::Task {
                                state: TaskState::Waiting(registration),
                                ..
                            } if !registration.signaled.get()
                                && registration.timer_queue.is_some() =>
                            {
                                Some(registration.token_id)
                            }
                            _ => None,
                        })
                        .collect::<BTreeSet<_>>();
                    let next_deadline = clock_timers
                        .borrow()
                        .iter()
                        .filter(|timer| {
                            timer_ids.contains(&timer.token_id) && !timer.cancelled.get()
                        })
                        .map(|timer| timer.deadline)
                        .min();
                    let Some(next_deadline) = next_deadline else {
                        self.record_concurrency(
                            "reactor.wait",
                            clock_var,
                            "stalled",
                            0,
                            Some(MAX_CLOCK_TICK),
                        );
                        self.record_concurrency(
                            "task.race",
                            tasks_var,
                            "stalled",
                            0,
                            Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                        );
                        return Err(trap(
                            "reactor_stalled",
                            "task_race has no ready task or scheduled timer that can make progress",
                        ));
                    };
                    let previous_now = now.get();
                    now.set(next_deadline);
                    let mut due = Vec::new();
                    {
                        let mut scheduled = clock_timers.borrow_mut();
                        let mut pending = Vec::with_capacity(scheduled.len());
                        for timer in scheduled.drain(..) {
                            if timer.deadline <= next_deadline {
                                due.push(timer);
                            } else {
                                pending.push(timer);
                            }
                        }
                        *scheduled = pending;
                    }
                    due.sort_by_key(|timer| (timer.deadline, timer.sequence));
                    for timer in &due {
                        if timer.cancelled.get() {
                            continue;
                        }
                        let decision = if timer.signaled.replace(true) {
                            "already_signaled"
                        } else {
                            "signaled"
                        };
                        self.record_concurrency(
                            "timer.fire",
                            &timer.token_id.to_string(),
                            decision,
                            timer.waiters.get(),
                            Some(MAX_WAKE_TOKEN_WAITERS),
                        );
                    }
                    self.record_concurrency(
                        "reactor.wait",
                        clock_var,
                        "advanced",
                        next_deadline - previous_now,
                        Some(MAX_CLOCK_TICK),
                    );
                    winner = tasks.iter().position(task_is_ready);
                }

                let Some(winner) = winner else {
                    return Err(trap(
                        "reactor_stalled",
                        "task_race advanced without making a task ready",
                    ));
                };
                let task_count = tasks.len();
                let mut closed_tasks = Vec::with_capacity(task_count);
                let mut winner_work = None;
                for (index, task) in tasks.into_iter().enumerate() {
                    let Value::Task {
                        state,
                        ready,
                        open,
                        callable,
                        input,
                        continuations,
                    } = task
                    else {
                        unreachable!("task_race inputs were validated before selection")
                    };
                    let target = format!("{tasks_var}[{index}]");
                    if index == winner {
                        if let TaskState::Waiting(registration) = &state {
                            if let Some(queue) = &registration.timer_queue {
                                queue
                                    .borrow_mut()
                                    .retain(|timer| timer.token_id != registration.token_id);
                            }
                            release_wake_registration(registration);
                            self.record_concurrency(
                                "task.wake_event",
                                &target,
                                "woken",
                                registration.waiters.get(),
                                Some(MAX_WAKE_TOKEN_WAITERS),
                            );
                        }
                        ready.set(false);
                        open.set(false);
                        closed_tasks.push(Value::Task {
                            state: TaskState::Consumed,
                            ready,
                            open,
                            callable: callable.clone(),
                            input: input.clone(),
                            continuations: continuations.clone(),
                        });
                        winner_work = Some((callable, input, continuations, target));
                    } else {
                        if let TaskState::Waiting(registration) = &state {
                            release_wake_registration(registration);
                        }
                        ready.set(false);
                        open.set(false);
                        closed_tasks.push(Value::Task {
                            state: TaskState::Cancelled,
                            ready,
                            open,
                            callable,
                            input,
                            continuations,
                        });
                        self.record_concurrency(
                            "task.cancel",
                            &target,
                            "race_loser",
                            index,
                            Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                        );
                    }
                }
                env.insert(tasks_var.clone(), Value::Array(closed_tasks));
                let (callable, input, continuations, target) =
                    winner_work.expect("task_race selected one validated task");
                let result = self.execute_task_chain(*callable, *input, continuations, &target);
                self.record_concurrency(
                    "task.race",
                    tasks_var,
                    if result.is_ok() { "completed" } else { "error" },
                    winner,
                    Some(MAX_WAKE_TOKEN_SELECT_ARMS),
                );
                Ok(Some(result?))
            }
            "task_cancel" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_handle_target",
                        "task_cancel expects a task-handle variable",
                    ));
                };
                let (decision, cancelled) = match env.get_mut(var) {
                    Some(Value::Task {
                        state, ready, open, ..
                    }) if matches!(
                        state,
                        TaskState::Parked | TaskState::Waiting(_) | TaskState::Pending
                    ) =>
                    {
                        if let TaskState::Waiting(registration) = state {
                            release_wake_registration(registration);
                        }
                        *state = TaskState::Cancelled;
                        ready.set(false);
                        open.set(false);
                        ("cancelled", true)
                    }
                    Some(Value::Task { .. }) => ("already_closed", false),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_cancel target '{var}' is not a task"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task handle '{var}'"),
                        ));
                    }
                };
                self.record_concurrency("task.cancel", var, decision, usize::from(cancelled), None);
                Ok(Some(Value::Bool(cancelled)))
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
                self.record_concurrency("task_group.new", "", "active", 0, Some(capacity));
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
                let capacity = match env.get(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        capacity,
                        ..
                    }) => *capacity,
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
                let (full, child_count) = match env.get(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        capacity,
                        tasks,
                    }) => (tasks.len() >= *capacity, tasks.len()),
                    Some(Value::TaskGroup { .. }) => {
                        return Err(trap(
                            "task_group_closed",
                            format!(
                                "task group '{var}' was awaited or cancelled while evaluating child values"
                            ),
                        ));
                    }
                    _ => unreachable!(
                        "task group binding changed type while evaluating child values"
                    ),
                };
                if full {
                    self.record_concurrency(
                        "task_group.spawn",
                        var,
                        "capacity_exhausted",
                        child_count,
                        Some(capacity),
                    );
                    return Ok(Some(Value::Bool(false)));
                }
                let Some(Value::TaskGroup { tasks, .. }) = env.get_mut(var) else {
                    unreachable!("task group was validated before evaluating child values")
                };
                tasks.push(Value::Task {
                    state: TaskState::Pending,
                    ready: std::rc::Rc::new(std::cell::Cell::new(true)),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                    callable: Box::new(callable),
                    input: Box::new(input),
                    continuations: Vec::new(),
                });
                let child_count = tasks.len();
                self.record_concurrency(
                    "task_group.spawn",
                    var,
                    "accepted",
                    child_count,
                    Some(capacity),
                );
                Ok(Some(Value::Bool(true)))
            }
            "task_group_spawn_wait" => {
                expect_arity(name, args, 3)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_spawn_wait expects a task-group variable",
                    ));
                };
                let capacity = match env.get(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        capacity,
                        ..
                    }) => *capacity,
                    Some(Value::TaskGroup { .. }) => {
                        return Err(trap(
                            "task_group_closed",
                            format!("task group '{var}' was already awaited or cancelled"),
                        ));
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_group_spawn_wait target '{var}' is not a task group"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task group '{var}'"),
                        ));
                    }
                };
                let callable = self.eval_expr(&args[1], env)?;
                if !matches!(callable, Value::Function(_) | Value::Closure { .. }) {
                    return Err(trap(
                        "task_not_callable",
                        "task_group_spawn_wait expects a one-argument function value",
                    ));
                }
                let input = self.eval_expr(&args[2], env)?;
                let full = match env.get(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        capacity,
                        tasks,
                    }) => tasks.len() >= *capacity,
                    Some(Value::TaskGroup { .. }) => {
                        return Err(trap(
                            "task_group_closed",
                            format!(
                                "task group '{var}' was awaited or cancelled while evaluating child values"
                            ),
                        ));
                    }
                    _ => unreachable!(
                        "task group binding changed type while evaluating child values"
                    ),
                };
                let completed = if full {
                    let oldest = match env.get_mut(var) {
                        Some(Value::TaskGroup { tasks, .. }) => tasks.remove(0),
                        _ => {
                            unreachable!("task group was validated before evaluating child values")
                        }
                    };
                    let Value::Task {
                        state: TaskState::Pending,
                        callable,
                        input,
                        ..
                    } = oldest
                    else {
                        return Err(trap(
                            "task_group_child_state",
                            "task group contained a non-pending child",
                        ));
                    };
                    let result = self.execute_task_callable(*callable, *input);
                    if result.is_err() {
                        let remaining = match env.get(var) {
                            Some(Value::TaskGroup { tasks, .. }) => tasks.len(),
                            _ => unreachable!("task group remained valid while advancing a child"),
                        };
                        self.record_concurrency(
                            "task_group.spawn_wait",
                            var,
                            "error",
                            remaining,
                            Some(capacity),
                        );
                    }
                    Some(result?)
                } else {
                    None
                };
                let Some(Value::TaskGroup { tasks, .. }) = env.get_mut(var) else {
                    unreachable!("task group remained valid while applying backpressure")
                };
                tasks.push(Value::Task {
                    state: TaskState::Pending,
                    ready: std::rc::Rc::new(std::cell::Cell::new(true)),
                    open: std::rc::Rc::new(std::cell::Cell::new(true)),
                    callable: Box::new(callable),
                    input: Box::new(input),
                    continuations: Vec::new(),
                });
                let child_count = tasks.len();
                self.record_concurrency(
                    "task_group.spawn_wait",
                    var,
                    if completed.is_some() {
                        "capacity_released"
                    } else {
                        "accepted"
                    },
                    child_count,
                    Some(capacity),
                );
                Ok(Some(Value::Variant {
                    enum_name: "Option".to_string(),
                    variant: if completed.is_some() {
                        "Some".to_string()
                    } else {
                        "None".to_string()
                    },
                    value: completed.map(Box::new),
                }))
            }
            "task_group_await_all" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_await_all expects a task-group variable",
                    ));
                };
                let (capacity, tasks) = match env.get_mut(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        capacity,
                        tasks,
                    }) => {
                        let tasks = std::mem::take(tasks);
                        let capacity = *capacity;
                        if let Some(Value::TaskGroup { state, .. }) = env.get_mut(var) {
                            *state = TaskGroupState::Consumed;
                        }
                        (capacity, tasks)
                    }
                    Some(Value::TaskGroup { .. }) => {
                        return Err(trap(
                            "task_group_closed",
                            format!("task group '{var}' was already awaited or cancelled"),
                        ));
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_group_await_all target '{var}' is not a task group"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task group '{var}'"),
                        ));
                    }
                };
                let mut results = Vec::with_capacity(tasks.len());
                for task in tasks {
                    let Value::Task {
                        state: TaskState::Pending,
                        callable,
                        input,
                        ..
                    } = task
                    else {
                        return Err(trap(
                            "task_group_child_state",
                            "task group contained a non-pending child",
                        ));
                    };
                    results.push(self.execute_task_callable(*callable, *input)?);
                }
                self.record_concurrency(
                    "task_group.await_all",
                    var,
                    "completed",
                    results.len(),
                    Some(capacity),
                );
                Ok(Some(Value::Array(results)))
            }
            "task_group_await_next" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_await_next expects a task-group variable",
                    ));
                };
                let (task, remaining, capacity) = match env.get_mut(var) {
                    Some(Value::TaskGroup {
                        state: TaskGroupState::Active,
                        tasks,
                        capacity,
                    }) => {
                        if tasks.is_empty() {
                            self.record_concurrency(
                                "task_group.await_next",
                                var,
                                "empty",
                                0,
                                Some(*capacity),
                            );
                            return Ok(Some(Value::Variant {
                                enum_name: "Option".to_string(),
                                variant: "None".to_string(),
                                value: None,
                            }));
                        }
                        let task = tasks.remove(0);
                        (task, tasks.len(), *capacity)
                    }
                    Some(Value::TaskGroup { .. }) => {
                        return Err(trap(
                            "task_group_closed",
                            format!("task group '{var}' was already awaited or cancelled"),
                        ));
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_group_await_next target '{var}' is not a task group"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task group '{var}'"),
                        ));
                    }
                };
                let Value::Task {
                    state: TaskState::Pending,
                    callable,
                    input,
                    ..
                } = task
                else {
                    return Err(trap(
                        "task_group_child_state",
                        "task group contained a non-pending child",
                    ));
                };
                let result = self.execute_task_callable(*callable, *input);
                self.record_concurrency(
                    "task_group.await_next",
                    var,
                    if result.is_ok() { "completed" } else { "error" },
                    remaining,
                    Some(capacity),
                );
                Ok(Some(Value::Variant {
                    enum_name: "Option".to_string(),
                    variant: "Some".to_string(),
                    value: Some(Box::new(result?)),
                }))
            }
            "task_group_cancel" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "task_group_target",
                        "task_group_cancel expects a task-group variable",
                    ));
                };
                let (decision, cancelled, capacity) = match env.get_mut(var) {
                    Some(Value::TaskGroup {
                        state,
                        tasks,
                        capacity,
                    }) if *state == TaskGroupState::Active => {
                        let mut cancelled = 0usize;
                        for task in tasks {
                            if let Value::Task {
                                state, ready, open, ..
                            } = task
                            {
                                if *state == TaskState::Pending {
                                    *state = TaskState::Cancelled;
                                    ready.set(false);
                                    open.set(false);
                                    cancelled += 1;
                                }
                            }
                        }
                        *state = TaskGroupState::Cancelled;
                        ("cancelled", cancelled, *capacity)
                    }
                    Some(Value::TaskGroup { capacity, .. }) => ("already_closed", 0, *capacity),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("task_group_cancel target '{var}' is not a task group"),
                        ));
                    }
                    None => {
                        return Err(trap(
                            "unknown_symbol",
                            format!("unknown task group '{var}'"),
                        ));
                    }
                };
                self.record_concurrency(
                    "task_group.cancel",
                    var,
                    decision,
                    cancelled,
                    Some(capacity),
                );
                Ok(Some(Value::Int(cancelled as i64)))
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
                self.record_concurrency("channel.new", "", "open", 0, Some(capacity));
                Ok(Some(Value::Channel {
                    capacity,
                    values: Vec::new(),
                    closed: false,
                    readable: std::rc::Rc::new(std::cell::Cell::new(false)),
                    writable: std::rc::Rc::new(std::cell::Cell::new(true)),
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
                let (decision, count, capacity) = match env.get_mut(var) {
                    Some(Value::Channel {
                        capacity,
                        values,
                        closed,
                        readable,
                        writable,
                    }) => {
                        if *closed {
                            writable.set(false);
                            ("closed", values.len(), *capacity)
                        } else if values.len() >= *capacity {
                            writable.set(false);
                            ("capacity_exhausted", values.len(), *capacity)
                        } else {
                            values.push(value);
                            readable.set(true);
                            writable.set(values.len() < *capacity);
                            ("accepted", values.len(), *capacity)
                        }
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("channel_send target '{var}' is not a channel"),
                        ));
                    }
                    None => {
                        return Err(trap("unknown_symbol", format!("unknown channel '{var}'")));
                    }
                };
                self.record_concurrency("channel.send", var, decision, count, Some(capacity));
                Ok(Some(Value::Bool(decision == "accepted")))
            }
            "channel_recv" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "channel_target",
                        "channel_recv expects a channel variable",
                    ));
                };
                let (value, decision, count, capacity) = match env.get_mut(var) {
                    Some(Value::Channel {
                        values,
                        capacity,
                        closed,
                        readable,
                        writable,
                    }) if !values.is_empty() => {
                        let value = Value::Variant {
                            enum_name: "Option".to_string(),
                            variant: "Some".to_string(),
                            value: Some(Box::new(values.remove(0))),
                        };
                        readable.set(!values.is_empty() || *closed);
                        writable.set(!*closed && values.len() < *capacity);
                        (value, "received", values.len(), *capacity)
                    }
                    Some(Value::Channel {
                        values, capacity, ..
                    }) => (
                        Value::Variant {
                            enum_name: "Option".to_string(),
                            variant: "None".to_string(),
                            value: None,
                        },
                        "empty",
                        values.len(),
                        *capacity,
                    ),
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("channel_recv target '{var}' is not a channel"),
                        ));
                    }
                    None => {
                        return Err(trap("unknown_symbol", format!("unknown channel '{var}'")));
                    }
                };
                self.record_concurrency("channel.recv", var, decision, count, Some(capacity));
                Ok(Some(value))
            }
            "channel_close" => {
                expect_arity(name, args, 1)?;
                let Expr::Var(var) = &args[0] else {
                    return Err(trap(
                        "channel_target",
                        "channel_close expects a channel variable",
                    ));
                };
                let (decision, count, capacity) = match env.get_mut(var) {
                    Some(Value::Channel {
                        closed,
                        values,
                        capacity,
                        readable,
                        writable,
                    }) => {
                        let decision = if *closed { "already_closed" } else { "closed" };
                        *closed = true;
                        readable.set(true);
                        writable.set(false);
                        (decision, values.len(), *capacity)
                    }
                    Some(_) => {
                        return Err(trap(
                            "type_error",
                            format!("channel_close target '{var}' is not a channel"),
                        ));
                    }
                    None => {
                        return Err(trap("unknown_symbol", format!("unknown channel '{var}'")));
                    }
                };
                self.record_concurrency("channel.close", var, decision, count, Some(capacity));
                Ok(Some(Value::Void))
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
            // Payloads the file itself carries (docs/project_filesystem.md).
            // Reading one is not I/O: the bytes are in the program, so this
            // needs no effect and grants no authority. An out-of-line payload
            // *would* be a file read, and is refused rather than quietly
            // reaching outside the program.
            "section_text" | "section_bytes" => {
                expect_arity(name, args, 1)?;
                let id = match &args[0] {
                    Expr::String(id) => id.clone(),
                    _ => {
                        return Err(trap(
                            "section_ref_not_literal",
                            format!("'{name}' needs a literal node id"),
                        ))
                    }
                };
                let item = self
                    .program
                    .items
                    .iter()
                    .find(|item| crate::ast::item_node_id(item) == id)
                    .ok_or_else(|| {
                        trap(
                            "section_ref_unknown",
                            format!("'{id}' is not a section carried in this file"),
                        )
                    })?;
                let bytes = match item {
                    crate::ast::Item::Data(section) => {
                        let mut text = section.content.join("
");
                        if name == "section_text" {
                            return Ok(Some(Value::String(text)));
                        }
                        text.push('\n');
                        text.into_bytes()
                    }
                    crate::ast::Item::Blob(section) => match &section.payload {
                        crate::ast::BlobPayload::Inline(lines) => {
                            let bytes = crate::section::decode_base64(&lines.join(""))
                                .map_err(|err| trap("blob_payload_invalid", err))?;
                            // The declared size and hash are checked here as
                            // well as by `fgx check`: a program must not read
                            // bytes that disagree with what the file says they
                            // are, whether or not the checker ran.
                            if bytes.len() as u64 != section.bytes {
                                return Err(trap(
                                    "blob_size_mismatch",
                                    format!(
                                        "blob '{id}' declares {} bytes but its payload decodes to {}",
                                        section.bytes,
                                        bytes.len()
                                    ),
                                ));
                            }
                            if let Some(declared) = &section.sha256 {
                                let actual = crate::crypto::sha256_hex(&bytes);
                                if &actual != declared {
                                    return Err(trap(
                                        "blob_hash_mismatch",
                                        format!("blob '{id}' does not match its declared sha256"),
                                    ));
                                }
                            }
                            bytes
                        }
                        crate::ast::BlobPayload::Store(path) => {
                            return Err(trap(
                                "section_payload_out_of_line",
                                format!(
                                    "blob '{id}' keeps its bytes in the store at '{path}', which a run cannot read yet; carry it inline, or extract it with `fgx fs extract` and read the file through a granted effect"
                                ),
                            ))
                        }
                    },
                    other => {
                        return Err(trap(
                            "section_ref_not_a_payload",
                            format!("'{id}' is a {} and has no payload to read", crate::ast::item_kind_name(other)),
                        ))
                    }
                };
                if name == "section_text" {
                    let text = String::from_utf8(bytes).map_err(|_| {
                        trap(
                            "section_payload_not_utf8",
                            format!("'{id}' is not valid UTF-8, so it has no text to read"),
                        )
                    })?;
                    return Ok(Some(Value::String(text)));
                }
                Ok(Some(Value::Array(
                    bytes
                        .into_iter()
                        .map(|byte| Value::Int(byte as i64))
                        .collect(),
                )))
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
            "core_path_parse" | "core_runtime_path_parse" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(
                    normalize_portable_path(&value).map_err(|error| trap("path_invalid", error))?,
                )))
            }
            "core_path_join" | "core_runtime_path_join" => {
                expect_arity(name, args, 2)?;
                let base = self.eval_expr(&args[0], env)?.as_str_value()?;
                let child = self.eval_expr(&args[1], env)?.as_str_value()?;
                let base =
                    normalize_portable_path(&base).map_err(|error| trap("path_invalid", error))?;
                let child =
                    normalize_portable_path(&child).map_err(|error| trap("path_invalid", error))?;
                Ok(Some(Value::String(
                    normalize_portable_path(&format!("{base}/{child}"))
                        .map_err(|error| trap("path_invalid", error))?,
                )))
            }
            "core_path_parent" | "core_runtime_path_parent" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let path =
                    normalize_portable_path(&path).map_err(|error| trap("path_invalid", error))?;
                let value = path
                    .rsplit_once('/')
                    .map(|(parent, _)| Box::new(Value::String(parent.to_string())));
                Ok(Some(Value::Variant {
                    enum_name: "Option".to_string(),
                    variant: if value.is_some() { "Some" } else { "None" }.to_string(),
                    value,
                }))
            }
            "core_path_file_name" | "core_runtime_path_file_name" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let path =
                    normalize_portable_path(&path).map_err(|error| trap("path_invalid", error))?;
                Ok(Some(Value::String(
                    path.rsplit('/').next().unwrap_or(&path).to_string(),
                )))
            }
            "core_path_extension" | "core_runtime_path_extension" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let path =
                    normalize_portable_path(&path).map_err(|error| trap("path_invalid", error))?;
                let file_name = path.rsplit('/').next().unwrap_or(&path);
                let extension = file_name
                    .rsplit_once('.')
                    .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
                    .map(|(_, extension)| extension)
                    .unwrap_or("");
                Ok(Some(Value::String(extension.to_string())))
            }
            "core_path_to_str" | "core_runtime_path_to_str" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(
                    normalize_portable_path(&path).map_err(|error| trap("path_invalid", error))?,
                )))
            }
            "core_path_starts_with" | "core_runtime_path_starts_with" => {
                expect_arity(name, args, 2)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let base = self.eval_expr(&args[1], env)?.as_str_value()?;
                let path =
                    normalize_portable_path(&path).map_err(|error| trap("path_invalid", error))?;
                let base =
                    normalize_portable_path(&base).map_err(|error| trap("path_invalid", error))?;
                Ok(Some(Value::Bool(portable_path_starts_with(&path, &base))))
            }
            "core_io_read_file" | "core_runtime_io_read_file" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::String(io.read_file(&path)?)))
            }
            "core_io_read_bytes" | "core_runtime_io_read_bytes" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Array(
                    io.read_bytes(&path)?
                        .into_iter()
                        .map(|byte| Value::Int(i64::from(byte)))
                        .collect(),
                )))
            }
            "core_io_list_dir" | "core_runtime_io_list_dir" => {
                expect_arity(name, args, 1)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Array(io.list_dir(&path)?)))
            }
            "core_io_write_file" | "core_runtime_io_write_file" => {
                expect_arity(name, args, 2)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let contents = self.eval_expr(&args[1], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.write_file(&path, &contents)? as i64)))
            }
            "core_io_write_file_atomic" | "core_runtime_io_write_file_atomic" => {
                expect_arity(name, args, 2)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let contents = self.eval_expr(&args[1], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(
                    io.write_file_atomic(&path, &contents)? as i64
                )))
            }
            "core_io_write_bytes_atomic" | "core_runtime_io_write_bytes_atomic" => {
                expect_arity(name, args, 2)?;
                let path = self.eval_expr(&args[0], env)?.as_str_value()?;
                let value = self.eval_expr(&args[1], env)?;
                let bytes = value_to_bytes(value, name)?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(
                    io.write_bytes_atomic(&path, &bytes)? as i64
                )))
            }
            "core_io_publish_text_artifact" | "core_runtime_io_publish_text_artifact" => {
                expect_arity(name, args, 3)?;
                let artifact = self.eval_expr(&args[0], env)?.as_str_value()?;
                let path = self.eval_expr(&args[1], env)?.as_str_value()?;
                let contents = self.eval_expr(&args[2], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.publish_artifact(
                    "publish_artifact_text",
                    &artifact,
                    &path,
                    contents.as_bytes(),
                )? as i64)))
            }
            "core_io_publish_bytes_artifact" | "core_runtime_io_publish_bytes_artifact" => {
                expect_arity(name, args, 3)?;
                let artifact = self.eval_expr(&args[0], env)?.as_str_value()?;
                let path = self.eval_expr(&args[1], env)?.as_str_value()?;
                let value = self.eval_expr(&args[2], env)?;
                let bytes = value_to_bytes(value, name)?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.publish_artifact(
                    "publish_artifact_bytes",
                    &artifact,
                    &path,
                    &bytes,
                )? as i64)))
            }
            "core_io_copy_file" | "core_runtime_io_copy_file" => {
                expect_arity(name, args, 2)?;
                let src = self.eval_expr(&args[0], env)?.as_str_value()?;
                let dst = self.eval_expr(&args[1], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.copy_file(&src, &dst)? as i64)))
            }
            "core_io_copy_file_atomic" | "core_runtime_io_copy_file_atomic" => {
                expect_arity(name, args, 2)?;
                let src = self.eval_expr(&args[0], env)?.as_str_value()?;
                let dst = self.eval_expr(&args[1], env)?.as_str_value()?;
                let io = self.require_io()?;
                Ok(Some(Value::Int(io.copy_file_atomic(&src, &dst)? as i64)))
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
            "core_serialize_json_canonical" | "core_runtime_serialize_json_canonical" => {
                expect_arity(name, args, 1)?;
                let source = self.eval_expr(&args[0], env)?.as_str_value()?;
                Ok(Some(Value::String(canonical_json(&source)?)))
            }
            "core_serialize_json_is_canonical" | "core_runtime_serialize_json_is_canonical" => {
                expect_arity(name, args, 1)?;
                let source = self.eval_expr(&args[0], env)?.as_str_value()?;
                let canonical = canonical_json(&source);
                Ok(Some(Value::Bool(matches!(
                    canonical,
                    Ok(ref value) if value == &source
                ))))
            }
            "core_serialize_utf8_encode" | "core_runtime_serialize_utf8_encode" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_str_value()?;
                require_serialized_size(value.len(), name)?;
                Ok(Some(bytes_to_value(value.into_bytes())))
            }
            "core_serialize_utf8_decode" | "core_runtime_serialize_utf8_decode" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?;
                let bytes = value_to_serialized_bytes(value, name)?;
                require_serialized_size(bytes.len(), name)?;
                let value = String::from_utf8(bytes)
                    .map_err(|_| trap("serialize_invalid_utf8", "bytes are not valid UTF-8"))?;
                Ok(Some(Value::String(value)))
            }
            "core_serialize_u16_le" | "core_runtime_serialize_u16_le" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()?;
                let value = u16::try_from(value).map_err(|_| {
                    trap(
                        "serialize_integer_range",
                        format!("{name} value {value} is outside u16"),
                    )
                })?;
                Ok(Some(bytes_to_value(value.to_le_bytes().to_vec())))
            }
            "core_serialize_u32_le" | "core_runtime_serialize_u32_le" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()?;
                let value = u32::try_from(value).map_err(|_| {
                    trap(
                        "serialize_integer_range",
                        format!("{name} value {value} is outside u32"),
                    )
                })?;
                Ok(Some(bytes_to_value(value.to_le_bytes().to_vec())))
            }
            "core_serialize_i32_le" | "core_runtime_serialize_i32_le" => {
                expect_arity(name, args, 1)?;
                let value = self.eval_expr(&args[0], env)?.as_i64()?;
                let value = i32::try_from(value).map_err(|_| {
                    trap(
                        "serialize_integer_range",
                        format!("{name} value {value} is outside i32"),
                    )
                })?;
                Ok(Some(bytes_to_value(value.to_le_bytes().to_vec())))
            }
            "core_serialize_read_u16_le"
            | "core_runtime_serialize_read_u16_le"
            | "core_serialize_read_u32_le"
            | "core_runtime_serialize_read_u32_le"
            | "core_serialize_read_i32_le"
            | "core_runtime_serialize_read_i32_le" => {
                expect_arity(name, args, 2)?;
                let value = self.eval_expr(&args[0], env)?;
                let bytes = value_to_serialized_bytes(value, name)?;
                require_serialized_size(bytes.len(), name)?;
                let offset = self.eval_expr(&args[1], env)?.as_i64()?;
                let offset = usize::try_from(offset).map_err(|_| {
                    trap(
                        "serialize_bounds",
                        format!("{name} offset {offset} is outside usize"),
                    )
                })?;
                let width = if name.contains("u16") { 2 } else { 4 };
                let end = offset
                    .checked_add(width)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| {
                        trap(
                            "serialize_bounds",
                            format!(
                                "{name} needs bytes [{offset}..{}) but input length is {}",
                                offset.saturating_add(width),
                                bytes.len()
                            ),
                        )
                    })?;
                let slice = &bytes[offset..end];
                let value = if name.contains("u16") {
                    i64::from(u16::from_le_bytes([slice[0], slice[1]]))
                } else if name.contains("i32") {
                    i64::from(i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
                } else {
                    i64::from(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
                };
                Ok(Some(Value::Int(value)))
            }
            "core_serialize_concat" | "core_runtime_serialize_concat" => {
                expect_arity(name, args, 2)?;
                let left = value_to_serialized_bytes(self.eval_expr(&args[0], env)?, name)?;
                let right = value_to_serialized_bytes(self.eval_expr(&args[1], env)?, name)?;
                let total = left.len().checked_add(right.len()).ok_or_else(|| {
                    trap("serialize_size_limit", format!("{name} length overflow"))
                })?;
                require_serialized_size(total, name)?;
                let mut bytes = Vec::with_capacity(total);
                bytes.extend_from_slice(&left);
                bytes.extend_from_slice(&right);
                Ok(Some(bytes_to_value(bytes)))
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

    fn execute_task_chain(
        &self,
        callable: Value,
        input: Value,
        continuations: Vec<Value>,
        target: &str,
    ) -> Result<Value, String> {
        let mut value = self.execute_task_callable(callable, input)?;
        let total = continuations.len();
        for (index, continuation) in continuations.into_iter().enumerate() {
            let result = self.execute_task_callable(continuation, value);
            self.record_concurrency(
                "task.resume",
                target,
                if result.is_ok() { "completed" } else { "error" },
                index + 1,
                Some(total),
            );
            value = result?;
        }
        Ok(value)
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
        Expr::SliceRange { base, start, end } => {
            collect_var_names(base, out);
            collect_var_names(start, out);
            collect_var_names(end, out);
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

fn retag_native_struct_output(program: &Program, capsule: &Capsule, value: Value) -> Value {
    let [output] = capsule.outputs.as_slice() else {
        return value;
    };
    retag_native_value_for_type(program, &output.ty, value)
}

fn retag_native_value_for_type(program: &Program, ty: &Type, value: Value) -> Value {
    match (ty, value) {
        (Type::Array { elem, .. } | Type::Slice { elem, .. }, Value::Array(values)) => {
            Value::Array(
                values
                    .into_iter()
                    .map(|value| retag_native_value_for_type(program, elem, value))
                    .collect(),
            )
        }
        (Type::Generic { name, args }, Value::Array(values))
            if name == "Vec" && args.len() == 1 =>
        {
            Value::Array(
                values
                    .into_iter()
                    .map(|value| retag_native_value_for_type(program, &args[0], value))
                    .collect(),
            )
        }
        (Type::Named(name), Value::Struct(_, mut values)) => {
            if let Some(decl) = program.structs().find(|decl| decl.name == *name) {
                for field in &decl.fields {
                    if let Some(value) = values.remove(&field.name) {
                        values.insert(
                            field.name.clone(),
                            retag_native_value_for_type(program, &field.ty, value),
                        );
                    }
                }
            }
            Value::Struct(name.clone(), values)
        }
        (_, value) => value,
    }
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

/// The inclusive range of an integer type, as `(min, max)` over `i128` so every
/// supported width — signed and unsigned, up to 64 bits — is expressible.
pub(crate) fn integer_range(width: &str) -> Option<(i128, i128)> {
    Some(match width {
        "i8" => (i8::MIN as i128, i8::MAX as i128),
        "i16" => (i16::MIN as i128, i16::MAX as i128),
        "i32" => (i32::MIN as i128, i32::MAX as i128),
        // `isize` is the host pointer width; the interpreter runs on 64-bit
        // hosts and the C backend guards against `PTRDIFF_MAX` for the same
        // reason, so both agree wherever the program is actually run.
        "i64" | "isize" => (i64::MIN as i128, i64::MAX as i128),
        "u8" => (0, u8::MAX as i128),
        "u16" => (0, u16::MAX as i128),
        "u32" => (0, u32::MAX as i128),
        "u64" | "usize" => (0, u64::MAX as i128),
        _ => return None,
    })
}

/// Reject `MIN / -1` and `MIN % -1`.
///
/// The quotient is one past the type's maximum, so division is already caught
/// by the range check — but the remainder is zero and would slip through, while
/// the operation is still an overflow. Both are rejected here so the two forms
/// answer alike, which is also what the C backend and Rust do.
fn check_division(op: &str, left: &Value, right: &Value, width: &str) -> Result<(), String> {
    let Some((min, _)) = integer_range(width) else {
        return Ok(());
    };
    if min == 0 {
        return Ok(());
    }
    let (Value::Int(numerator), Value::Int(denominator)) = (left, right) else {
        return Ok(());
    };
    if *numerator as i128 == min && *denominator == -1 {
        return Err(trap(
            "integer_overflow",
            format!("{op} of the smallest {width} by -1 overflows"),
        ));
    }
    Ok(())
}

/// Reject a shift amount outside `[0, width)`.
///
/// Shifting by at least the width is undefined in C and rejected by every other
/// language with defined integer semantics, so it traps here rather than
/// producing a value that depends on the backend.
fn check_shift_amount(amount: &Value, width: &str) -> Result<(), String> {
    let bits = i128::from(width_bits(width));
    let raw = match amount {
        Value::Int(raw) => *raw as i128,
        Value::UInt(raw) => *raw as i128,
        _ => return Ok(()),
    };
    if raw < 0 || raw >= bits {
        return Err(trap(
            "invalid_shift",
            format!("shift amount {raw} is outside 0..{bits} for {width}"),
        ));
    }
    Ok(())
}

fn width_bits(width: &str) -> u32 {
    match width {
        "i8" | "u8" => 8,
        "i16" | "u16" => 16,
        "i32" | "u32" => 32,
        _ => 64,
    }
}

/// Apply the declared width to a value the 64-bit evaluator just produced.
///
/// Arithmetic traps when the result leaves the width. Shifts are the exception:
/// a shift moves a bit pattern, so bits leaving the top are discarded rather
/// than an error — but the shift *amount* must be inside the width, which is
/// checked here because the evaluator only knows about 64.
fn narrow_to_width(op: &str, value: Value, width: &str) -> Result<Value, String> {
    let Some((min, max)) = integer_range(width) else {
        return Ok(value);
    };
    let raw = match &value {
        Value::Int(raw) => *raw as i128,
        Value::UInt(raw) => *raw as i128,
        // Float and non-numeric results are not width-checked.
        _ => return Ok(value),
    };
    if matches!(op, "<<" | ">>") {
        let bits = width_bits(width);
        let truncated = if bits >= 64 {
            raw
        } else {
            let mask = (1i128 << bits) - 1;
            let kept = raw & mask;
            // Re-sign the retained bits for a signed width, so shifting off the
            // top of an `i32` yields an `i32` rather than a large positive
            // number, matching the two's-complement result the C backend
            // produces for the same expression.
            if min < 0 && kept > max {
                kept - (1i128 << bits)
            } else {
                kept
            }
        };
        return Ok(rewrap(&value, truncated));
    }
    if raw < min || raw > max {
        return Err(trap(
            "integer_overflow",
            format!("{op} result {raw} does not fit {width}"),
        ));
    }
    Ok(value)
}

fn rewrap(original: &Value, raw: i128) -> Value {
    match original {
        Value::UInt(_) if raw >= 0 => Value::UInt(raw as u64),
        _ => Value::Int(raw as i64),
    }
}

fn eval_binary(op: &str, left: Value, right: Value) -> Result<Value, String> {
    match op {
        // Enum variants (and bools) compare by structural equality; everything
        // else falls back to numeric comparison.
        "==" if !left.is_number() || !right.is_number() => Ok(Value::Bool(left == right)),
        "!=" if !left.is_number() || !right.is_number() => Ok(Value::Bool(left != right)),
        "==" | "!=" | "<" | ">" | "<=" | ">=" => {
            Ok(Value::Bool(compare_numbers(&left, &right, op)?))
        }
        "&&" => Ok(Value::Bool(left.as_bool()? && right.as_bool()?)),
        "||" => Ok(Value::Bool(left.as_bool()? || right.as_bool()?)),
        "+" => numeric_op(left, right, i64::checked_add, u64::checked_add, |a, b| {
            Ok(a + b)
        }),
        "-" => numeric_op(left, right, i64::checked_sub, u64::checked_sub, |a, b| {
            Ok(a - b)
        }),
        "*" => numeric_op(left, right, i64::checked_mul, u64::checked_mul, |a, b| {
            Ok(a * b)
        }),
        "/" => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(trap("division_by_zero", "integer division by zero"))
            }
            (Value::Int(a), Value::Int(b)) => a
                .checked_div(b)
                .map(Value::Int)
                .ok_or_else(|| trap("integer_overflow", "checked integer division failed")),
            (Value::UInt(_), Value::UInt(0)) => {
                Err(trap("division_by_zero", "unsigned division by zero"))
            }
            (Value::UInt(_), Value::Int(0)) | (Value::Int(_), Value::UInt(0)) => {
                Err(trap("division_by_zero", "unsigned division by zero"))
            }
            (Value::UInt(a), Value::UInt(b)) => Ok(Value::UInt(a / b)),
            (Value::UInt(a), Value::Int(b)) if b > 0 => Ok(Value::UInt(a / b as u64)),
            (Value::Int(a), Value::UInt(b)) if a >= 0 && b != 0 => Ok(Value::UInt(a as u64 / b)),
            (Value::UInt(_), Value::Int(_)) | (Value::Int(_), Value::UInt(_)) => Err(trap(
                "type_error",
                "unsigned division requires a nonnegative integer operand",
            )),
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
            (Value::UInt(_), Value::UInt(0)) => {
                Err(trap("division_by_zero", "unsigned modulo by zero"))
            }
            (Value::UInt(a), Value::UInt(b)) => Ok(Value::UInt(a % b)),
            (Value::UInt(a), Value::Int(b)) if b > 0 => Ok(Value::UInt(a % b as u64)),
            (Value::Int(a), Value::UInt(b)) if a >= 0 && b != 0 => Ok(Value::UInt(a as u64 % b)),
            _ => Err(trap("type_error", "modulo requires integer values")),
        },
        "<<" => match (left, right) {
            (Value::Int(a), Value::Int(b)) if (0..64).contains(&b) => Ok(Value::Int(a << b)),
            (Value::Int(_), Value::Int(b)) => {
                Err(trap("invalid_shift", format!("invalid shift amount {b}")))
            }
            (Value::UInt(a), Value::Int(b)) if (0..64).contains(&b) => Ok(Value::UInt(a << b)),
            _ => Err(trap("type_error", "shift requires integer values")),
        },
        ">>" => match (left, right) {
            (Value::Int(a), Value::Int(b)) if (0..64).contains(&b) => Ok(Value::Int(a >> b)),
            (Value::Int(_), Value::Int(b)) => {
                Err(trap("invalid_shift", format!("invalid shift amount {b}")))
            }
            (Value::UInt(a), Value::Int(b)) if (0..64).contains(&b) => Ok(Value::UInt(a >> b)),
            _ => Err(trap("type_error", "shift requires integer values")),
        },
        "&" => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
            (Value::UInt(a), Value::UInt(b)) => Ok(Value::UInt(a & b)),
            (Value::UInt(a), Value::Int(b)) if b >= 0 => Ok(Value::UInt(a & b as u64)),
            (Value::Int(a), Value::UInt(b)) if a >= 0 => Ok(Value::UInt(a as u64 & b)),
            _ => Err(trap("type_error", "bitwise and requires integer values")),
        },
        "|" => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            (Value::UInt(a), Value::UInt(b)) => Ok(Value::UInt(a | b)),
            (Value::UInt(a), Value::Int(b)) if b >= 0 => Ok(Value::UInt(a | b as u64)),
            (Value::Int(a), Value::UInt(b)) if a >= 0 => Ok(Value::UInt(a as u64 | b)),
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
    uint_op: impl FnOnce(u64, u64) -> Option<u64>,
    float_op: impl FnOnce(f64, f64) -> Result<f64, String>,
) -> Result<Value, String> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => int_op(a, b)
            .map(Value::Int)
            .ok_or_else(|| trap("integer_overflow", "checked integer arithmetic failed")),
        (Value::UInt(a), Value::UInt(b)) => uint_op(a, b)
            .map(Value::UInt)
            .ok_or_else(|| trap("integer_overflow", "checked unsigned arithmetic failed")),
        (Value::UInt(a), Value::Int(b)) if b >= 0 => uint_op(a, b as u64)
            .map(Value::UInt)
            .ok_or_else(|| trap("integer_overflow", "checked unsigned arithmetic failed")),
        (Value::Int(a), Value::UInt(b)) if a >= 0 => uint_op(a as u64, b)
            .map(Value::UInt)
            .ok_or_else(|| trap("integer_overflow", "checked unsigned arithmetic failed")),
        (Value::UInt(_), Value::Int(_)) | (Value::Int(_), Value::UInt(_)) => Err(trap(
            "type_error",
            "unsigned arithmetic requires a nonnegative integer operand",
        )),
        (a, b) => Ok(Value::Float(float_op(a.as_f64()?, b.as_f64()?)?)),
    }
}

fn compare_numbers(left: &Value, right: &Value, op: &str) -> Result<bool, String> {
    let ordering = match (left, right) {
        (Value::Int(left), Value::Int(right)) => left.cmp(right),
        (Value::UInt(left), Value::UInt(right)) => left.cmp(right),
        (Value::Int(left), Value::UInt(right)) => match u64::try_from(*left) {
            Ok(left) => left.cmp(right),
            Err(_) => std::cmp::Ordering::Less,
        },
        (Value::UInt(left), Value::Int(right)) => match u64::try_from(*right) {
            Ok(right) => left.cmp(&right),
            Err(_) => std::cmp::Ordering::Greater,
        },
        _ => left
            .as_f64()?
            .partial_cmp(&right.as_f64()?)
            .ok_or_else(|| trap("type_error", "numeric comparison is unordered"))?,
    };
    Ok(match op {
        "==" => ordering == std::cmp::Ordering::Equal,
        "!=" => ordering != std::cmp::Ordering::Equal,
        "<" => ordering == std::cmp::Ordering::Less,
        ">" => ordering == std::cmp::Ordering::Greater,
        "<=" => ordering != std::cmp::Ordering::Greater,
        ">=" => ordering != std::cmp::Ordering::Less,
        _ => {
            return Err(trap(
                "unknown_operator",
                format!("unknown comparison '{op}'"),
            ))
        }
    })
}

fn cast_value(ty: &Type, value: Value) -> Result<Value, String> {
    match ty {
        Type::Bool => Ok(Value::Bool(value.as_bool()?)),
        Type::Primitive(name) if name == "f32" || name == "f64" => {
            Ok(Value::Float(value.as_f64()?))
        }
        Type::Primitive(name) if name == "str" => Ok(Value::String(value.as_str_value()?)),
        Type::Primitive(name) if name == "u64" || name == "usize" => match value {
            Value::UInt(value) => Ok(Value::UInt(value)),
            other => Ok(Value::Int(other.as_i64()?)),
        },
        Type::Primitive(_) => Ok(Value::Int(value.as_i64()?)),
        _ => Ok(value),
    }
}

impl Value {
    fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::UInt(_) | Value::Float(_))
    }

    fn as_i64(&self) -> Result<i64, String> {
        match self {
            Value::Int(value) => Ok(*value),
            Value::UInt(value) => i64::try_from(*value).map_err(|_| {
                trap(
                    "integer_overflow",
                    format!("unsigned value {value} exceeds signed integer range"),
                )
            }),
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
            Value::UInt(value) => Ok(*value as f64),
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

fn value_to_bytes(value: Value, intrinsic: &str) -> Result<Vec<u8>, String> {
    let Value::Array(values) = value else {
        return Err(trap(
            "type_error",
            format!("{intrinsic} expects a Vec[u8] value"),
        ));
    };
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| match value {
            Value::Int(value) => u8::try_from(value).map_err(|_| {
                trap(
                    "type_error",
                    format!("{intrinsic} byte at index {index} is outside u8 range"),
                )
            }),
            other => Err(trap(
                "type_error",
                format!("{intrinsic} byte at index {index} is not an integer: {other:?}"),
            )),
        })
        .collect()
}

pub(crate) const MAX_SERIALIZED_BYTES: usize = 16 * 1024 * 1024;

fn require_serialized_size(bytes: usize, operation: &str) -> Result<(), String> {
    if bytes > MAX_SERIALIZED_BYTES {
        return Err(trap(
            "serialize_size_limit",
            format!("{operation} requires {bytes} bytes, limit is {MAX_SERIALIZED_BYTES}"),
        ));
    }
    Ok(())
}

fn value_to_serialized_bytes(value: Value, operation: &str) -> Result<Vec<u8>, String> {
    if let Value::Array(values) = &value {
        require_serialized_size(values.len(), operation)?;
    }
    value_to_bytes(value, operation)
}

fn bytes_to_value(bytes: Vec<u8>) -> Value {
    Value::Array(
        bytes
            .into_iter()
            .map(|byte| Value::Int(i64::from(byte)))
            .collect(),
    )
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
        Value::UInt(value) => value.to_string(),
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
        Value::WakeToken {
            id,
            signaled,
            cancelled,
            waiters,
            ..
        } => format!(
            "wake_token(id={id}, signaled={}, cancelled={}, waiters={})",
            signaled.get(),
            cancelled.get(),
            waiters.get()
        ),
        Value::AsyncClock { id, now, timers } => format!(
            "async_clock(id={id}, now={}, timers={})",
            now.get(),
            timers.borrow().len()
        ),
        Value::IntervalTimer {
            id,
            period,
            next_deadline,
            open,
            ..
        } => format!(
            "interval_timer(id={id}, period={period}, next={:?}, open={})",
            next_deadline.get(),
            open.get()
        ),
        Value::ReadinessArm { source } => {
            format!("readiness_arm({})", readiness_source_target(source))
        }
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
            ..
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
        Type::Primitive(name) if ty.is_integer() => {
            matches!(value, Value::Int(_))
                || (matches!(value, Value::UInt(_)) && (name == "u64" || name == "usize"))
        }
        Type::Primitive(_) if ty.is_float() => matches!(value, Value::Float(_)),
        Type::Primitive(_) => false,
        Type::Named(name) => match value {
            Value::String(_) => name == "Path",
            Value::Struct(type_name, _) => type_name == name,
            Value::Variant { enum_name, .. } => enum_name == name,
            Value::WakeToken { .. } => name == "WakeToken",
            Value::AsyncClock { .. } => name == "AsyncClock",
            Value::IntervalTimer { .. } => name == "IntervalTimer",
            Value::ReadinessArm { .. } => name == "ReadinessArm",
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
            ("Fn" | "FnOnce", Value::Function(_) | Value::Closure { .. }) => true,
            ("Task", Value::Task { .. }) | ("Channel", Value::Channel { .. }) => true,
            _ => false,
        },
        Type::Array { elem, len } => match value {
            Value::Array(values) => {
                values.len() == *len && values.iter().all(|value| value_matches_type(value, elem))
            }
            _ => false,
        },
        ty if is_native_mut_string_type(ty) => {
            matches!(value, Value::String(_))
        }
        Type::Slice { elem, .. } => match value {
            Value::Array(values) => values.iter().all(|value| value_matches_type(value, elem)),
            _ => false,
        },
        Type::Pointer { .. } => false,
        Type::Void => matches!(value, Value::Void),
        Type::Unknown => true,
    }
}

fn is_native_mut_string_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Slice { mutable: true, elem }
            if matches!(elem.as_ref(), Type::Primitive(name) if name == "str")
                || matches!(elem.as_ref(), Type::Named(name) if name == "$String")
    )
}

fn is_native_resizable_string_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Slice { mutable: true, elem }
            if matches!(elem.as_ref(), Type::Named(name) if name == "$String")
    )
}

pub(crate) fn validate_native_mutable_targets(
    capsule: &Capsule,
    source_args: &[Expr],
) -> Result<(), String> {
    let mut targets = BTreeSet::new();
    for (index, binding) in capsule.inputs.iter().enumerate() {
        if !matches!(binding.ty, Type::Slice { mutable: true, .. }) {
            continue;
        }
        match source_args.get(index) {
            Some(Expr::Var(name)) => {
                if !targets.insert(name) {
                    return Err(trap(
                        "native_capsule_mut_alias",
                        format!("mutable native inputs cannot alias direct variable '{name}'"),
                    ));
                }
            }
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

pub(crate) fn apply_native_mutations(
    program: &Program,
    capsule: &Capsule,
    source_args: &[Expr],
    mutations: &[crate::ffi::NativeMutation],
    env: &mut HashMap<String, Value>,
) -> Result<(), String> {
    let expected = capsule
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(index, binding)| {
            matches!(binding.ty, Type::Slice { mutable: true, .. }).then_some(index)
        })
        .collect::<BTreeSet<_>>();
    if mutations.len() != expected.len() {
        return Err(trap(
            "native_capsule_bad_output",
            format!(
                "native response carried {} mutable copy-backs; expected {}",
                mutations.len(),
                expected.len()
            ),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut staged = Vec::with_capacity(mutations.len());
    for mutation in mutations {
        if !seen.insert(mutation.input_index) {
            return Err(trap(
                "native_capsule_bad_output",
                format!(
                    "native response repeated mutable input {}",
                    mutation.input_index
                ),
            ));
        }
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
        if !targets.insert(name) {
            return Err(trap(
                "native_capsule_mut_alias",
                format!("mutable native inputs cannot alias direct variable '{name}'"),
            ));
        }
        let current = env.get(name).ok_or_else(|| {
            trap(
                "native_capsule_mut_target",
                format!("mutable native target '{name}' is missing from interpreter state"),
            )
        })?;
        if !value_matches_type(current, &binding.ty) {
            return Err(trap(
                "native_capsule_mut_target",
                format!(
                    "mutable native target '{name}' no longer matches {}",
                    binding.ty.to_agent()
                ),
            ));
        }
        let replacement = if is_native_mut_string_type(&binding.ty) {
            let value = mutation.string.as_ref().ok_or_else(|| {
                trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable string '{}' omitted validated copy-back",
                        binding.name
                    ),
                )
            })?;
            if !mutation.values.is_empty() || mutation.records.is_some() {
                return Err(trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable string '{}' carried an unexpected non-string copy-back",
                        binding.name
                    ),
                ));
            }
            Value::String(value.clone())
        } else if let Some(struct_name) = native_mut_struct_slice_name(&binding.ty) {
            if mutation.string.is_some() || !mutation.values.is_empty() {
                return Err(trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable aggregate slice '{}' carried a non-record copy-back",
                        binding.name
                    ),
                ));
            }
            let records = mutation.records.as_ref().ok_or_else(|| {
                trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable aggregate slice '{}' omitted validated copy-back",
                        binding.name
                    ),
                )
            })?;
            let values = records
                .iter()
                .map(|record| {
                    let mut index = 0;
                    let value =
                        native_struct_value_from_leaves(program, struct_name, record, &mut index)?;
                    if index != record.len() {
                        return Err(trap(
                            "native_capsule_bad_output",
                            format!(
                                "native mutable aggregate '{}' carried extra scalar leaves",
                                binding.name
                            ),
                        ));
                    }
                    Ok(value)
                })
                .collect::<Result<Vec<_>, String>>()?;
            Value::Array(values)
        } else {
            if mutation.string.is_some() || mutation.records.is_some() {
                return Err(trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable scalar slice '{}' carried a non-scalar copy-back",
                        binding.name
                    ),
                ));
            }
            let values = mutation
                .values
                .iter()
                .map(capsule_scalar_to_value)
                .collect::<Result<Vec<_>, _>>()?;
            Value::Array(values)
        };
        if !value_matches_type(&replacement, &binding.ty) {
            return Err(trap(
                "native_capsule_bad_output",
                format!(
                    "native mutable input '{}' carried a mismatched copy-back value",
                    binding.name
                ),
            ));
        }
        match (current, &replacement) {
            (Value::String(before), Value::String(after))
                if !is_native_resizable_string_type(&binding.ty) && before.len() != after.len() =>
            {
                return Err(trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable string '{}' changed byte length from {} to {}",
                        binding.name,
                        before.len(),
                        after.len()
                    ),
                ));
            }
            (Value::Array(before), Value::Array(after)) if before.len() != after.len() => {
                return Err(trap(
                    "native_capsule_bad_output",
                    format!(
                        "native mutable slice '{}' changed length from {} to {}",
                        binding.name,
                        before.len(),
                        after.len()
                    ),
                ));
            }
            _ => {}
        }
        staged.push((name.clone(), replacement));
    }
    if seen != expected {
        return Err(trap(
            "native_capsule_bad_output",
            "native response did not cover every mutable input exactly once",
        ));
    }
    for (name, replacement) in staged {
        env.insert(name, replacement);
    }
    Ok(())
}

fn native_mut_struct_slice_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Slice {
            mutable: true,
            elem,
        } => match elem.as_ref() {
            Type::Named(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

fn native_struct_value_from_leaves(
    program: &Program,
    name: &str,
    leaves: &[crate::ast::CapsuleValue],
    index: &mut usize,
) -> Result<Value, String> {
    let decl = program
        .structs()
        .find(|decl| decl.name == name)
        .ok_or_else(|| {
            trap(
                "native_capsule_bad_output",
                format!("native mutable copy-back references undeclared struct '{name}'"),
            )
        })?;
    let mut fields = HashMap::new();
    for field in &decl.fields {
        let value = native_struct_field_from_leaves(program, &field.ty, leaves, index).map_err(
            |error| {
                format!(
                    "{error} (while rebuilding native field '{name}.{}')",
                    field.name
                )
            },
        )?;
        fields.insert(field.name.clone(), value);
    }
    Ok(Value::Struct(name.to_string(), fields))
}

fn native_struct_field_from_leaves(
    program: &Program,
    ty: &Type,
    leaves: &[crate::ast::CapsuleValue],
    index: &mut usize,
) -> Result<Value, String> {
    match ty {
        Type::Named(name) => native_struct_value_from_leaves(program, name, leaves, index),
        Type::Array { elem, len } => (0..*len)
            .map(|_| native_struct_field_from_leaves(program, elem, leaves, index))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        _ => {
            let value = leaves.get(*index).ok_or_else(|| {
                trap(
                    "native_capsule_bad_output",
                    "native mutable aggregate ended before all scalar leaves",
                )
            })?;
            *index += 1;
            capsule_scalar_to_value(value)
        }
    }
}

fn capsule_scalar_to_value(value: &crate::ast::CapsuleValue) -> Result<Value, String> {
    match value {
        crate::ast::CapsuleValue::Int(value) => Ok(Value::Int(*value)),
        crate::ast::CapsuleValue::UInt(value) => Ok(Value::UInt(*value)),
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
            "simd_add" => numeric_op(left, right, i64::checked_add, u64::checked_add, |a, b| {
                Ok(a + b)
            }),
            "simd_mul" => numeric_op(left, right, i64::checked_mul, u64::checked_mul, |a, b| {
                Ok(a * b)
            }),
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
        "simd_reduce_add" => numeric_op(acc, value, i64::checked_add, u64::checked_add, |a, b| {
            Ok(a + b)
        }),
        "simd_reduce_mul" => numeric_op(acc, value, i64::checked_mul, u64::checked_mul, |a, b| {
            Ok(a * b)
        }),
        _ => Err(trap(
            "unknown_function",
            format!("unknown SIMD intrinsic '{name}'"),
        )),
    })
}

fn trap(code: &str, message: impl Into<String>) -> String {
    format!("trap: {code}: {}", message.into())
}

fn option_none() -> Value {
    Value::Variant {
        enum_name: "Option".to_string(),
        variant: "None".to_string(),
        value: None,
    }
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

    fn args_for(&self, program: &Program, function: &FunctionDecl) -> Result<Vec<Value>, String> {
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
                values
                    .iter()
                    .zip(&function.params)
                    .map(|(value, param)| {
                        value_from_json(value)
                            .map(|value| retag_native_value_for_type(program, &param.ty, value))
                    })
                    .collect()
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
                        .map(|value| retag_native_value_for_type(program, &param.ty, value))
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
        JsonValue::UInt(value) => Ok(Value::UInt(*value)),
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

/// Strict bounded JSON-to-runtime-value decoding for authenticated internal
/// cache payloads. Callers must still validate the resulting value against the
/// current static type before use.
pub(crate) fn value_from_json_text(source: &str) -> Result<Value, String> {
    let value = parse_json_value(source)?;
    value_from_json(&value)
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
    out.push_str(&format!(
        "  \"schema\": {},\n",
        quoted_json(crate::public_compat::RUN_RESULT_SCHEMA)
    ));
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

fn append_concurrency_receipt(base: String, events: &[ConcurrencyEvent]) -> String {
    if events.is_empty() {
        return base;
    }
    let event_count = events.len();
    let events = events
        .iter()
        .map(|event| {
            let capacity = event
                .capacity
                .map_or_else(|| "null".to_string(), |value| value.to_string());
            format!(
                "{{\"sequence\":{},\"operation\":{},\"target\":{},\"decision\":{},\"count\":{},\"capacity\":{capacity}}}",
                event.sequence,
                quoted_json(&event.operation),
                quoted_json(&event.target),
                quoted_json(&event.decision),
                event.count,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let trimmed = base.trim_end();
    let body = trimmed.strip_suffix('}').unwrap_or(trimmed).trim_end();
    format!(
        "{body},\n  \"concurrency\": {{\"schema\": \"forgentex.concurrency_receipt.v1\", \"mode\": \"deterministic_cooperative\", \"event_count\": {}, \"events\": [{events}]}}\n}}",
        event_count
    )
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

/// Marshal an interpreter value into the live native ABI representation.
/// Scalars, bounded borrowed UTF-8 strings, scalar slices, and restricted
/// structs are copied into runner-owned storage; no interpreter pointer crosses
/// the process boundary.
fn value_to_native_arg(
    program: &Program,
    value: &Value,
    ty: &Type,
) -> Result<crate::ffi::NativeValue, String> {
    match (value, ty) {
        (Value::String(value), ty) if is_native_mut_string_type(ty) => {
            Ok(crate::ffi::NativeValue::MutString(value.clone()))
        }
        (
            Value::Array(values),
            Type::Slice {
                mutable: false,
                elem,
            },
        ) if matches!(elem.as_ref(), Type::Named(_)) => {
            let Type::Named(expected) = elem.as_ref() else {
                unreachable!();
            };
            values
                .iter()
                .map(|value| {
                    let mut leaves = Vec::new();
                    flatten_native_struct_value(program, value, expected, &mut leaves)?;
                    Ok(leaves)
                })
                .collect::<Result<Vec<_>, String>>()
                .map(crate::ffi::NativeValue::ReadStructSlice)
        }
        (
            Value::Array(values),
            Type::Slice {
                mutable: true,
                elem,
            },
        ) if matches!(elem.as_ref(), Type::Named(_)) => {
            let Type::Named(expected) = elem.as_ref() else {
                unreachable!();
            };
            values
                .iter()
                .map(|value| {
                    let mut leaves = Vec::new();
                    flatten_native_struct_value(program, value, expected, &mut leaves)?;
                    Ok(leaves)
                })
                .collect::<Result<Vec<_>, String>>()
                .map(crate::ffi::NativeValue::MutStructSlice)
        }
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
            let mut leaves = Vec::new();
            flatten_native_struct_value(
                program,
                &Value::Struct(actual.clone(), values.clone()),
                expected,
                &mut leaves,
            )?;
            Ok(crate::ffi::NativeValue::Struct(leaves))
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

fn flatten_native_struct_value(
    program: &Program,
    value: &Value,
    expected: &str,
    leaves: &mut Vec<crate::ast::CapsuleValue>,
) -> Result<(), String> {
    let Value::Struct(actual, values) = value else {
        return Err(trap(
            "native_capsule_argument_type",
            format!("native nested struct input expects {expected}"),
        ));
    };
    if actual != expected {
        return Err(trap(
            "native_capsule_argument_type",
            format!("native struct input expects {expected}, got {actual}"),
        ));
    }
    let decl = program
        .structs()
        .find(|decl| decl.name == expected)
        .ok_or_else(|| {
            trap(
                "native_capsule_boundary_unsupported",
                format!("native struct input '{expected}' is not declared"),
            )
        })?;
    if values.len() != decl.fields.len()
        || values
            .keys()
            .any(|name| !decl.fields.iter().any(|field| field.name == *name))
    {
        return Err(trap(
            "native_capsule_argument_type",
            format!("native struct input '{expected}' has unexpected fields"),
        ));
    }
    for field in &decl.fields {
        let field_value = values.get(&field.name).ok_or_else(|| {
            trap(
                "native_capsule_argument_type",
                format!(
                    "native struct input '{expected}' is missing field '{}'",
                    field.name
                ),
            )
        })?;
        flatten_native_struct_field_value(
            program,
            field_value,
            &field.ty,
            &format!("{expected}.{}", field.name),
            leaves,
        )?;
    }
    Ok(())
}

fn flatten_native_struct_field_value(
    program: &Program,
    value: &Value,
    ty: &Type,
    context: &str,
    leaves: &mut Vec<crate::ast::CapsuleValue>,
) -> Result<(), String> {
    match ty {
        Type::Named(nested) => flatten_native_struct_value(program, value, nested, leaves),
        Type::Array { elem, len } => {
            let Value::Array(values) = value else {
                return Err(trap(
                    "native_capsule_argument_type",
                    format!("native struct array field '{context}' expects {len} values"),
                ));
            };
            if values.len() != *len {
                return Err(trap(
                    "native_capsule_argument_type",
                    format!(
                        "native struct array field '{context}' expects {} values, got {}",
                        len,
                        values.len()
                    ),
                ));
            }
            for (index, value) in values.iter().enumerate() {
                flatten_native_struct_field_value(
                    program,
                    value,
                    elem,
                    &format!("{context}[{index}]"),
                    leaves,
                )?;
            }
            Ok(())
        }
        _ => {
            if !value_matches_type(value, ty) {
                return Err(trap(
                    "native_capsule_argument_type",
                    format!(
                        "native struct field '{context}' contains a value incompatible with {}",
                        ty.to_agent()
                    ),
                ));
            }
            leaves.push(value_to_capsule_scalar(value)?);
            Ok(())
        }
    }
}

fn value_to_capsule_scalar(value: &Value) -> Result<crate::ast::CapsuleValue, String> {
    match value {
        Value::Int(value) => Ok(crate::ast::CapsuleValue::Int(*value)),
        Value::UInt(value) => Ok(crate::ast::CapsuleValue::UInt(*value)),
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
        Value::UInt(value) => value.to_string(),
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
        Value::WakeToken {
            id,
            signaled,
            cancelled,
            waiters,
            ..
        } => format!(
            "{{\"$wake_token\":{{\"id\":{id},\"signaled\":{},\"cancelled\":{},\"waiters\":{}}}}}",
            signaled.get(),
            cancelled.get(),
            waiters.get()
        ),
        Value::AsyncClock { id, now, timers } => format!(
            "{{\"$async_clock\":{{\"id\":{id},\"now\":{},\"timers\":{}}}}}",
            now.get(),
            timers.borrow().len()
        ),
        Value::IntervalTimer {
            id,
            period,
            next_deadline,
            open,
            ..
        } => format!(
            "{{\"$interval_timer\":{{\"id\":{id},\"period\":{period},\"next\":{},\"open\":{}}}}}",
            next_deadline
                .get()
                .map_or_else(|| "null".to_string(), |tick| tick.to_string()),
            open.get()
        ),
        Value::ReadinessArm { source } => format!(
            "{{\"$readiness_arm\":{}}}",
            quoted_json(readiness_source_target(source))
        ),
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
            ..
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
        TaskState::Parked => "parked",
        TaskState::Waiting(_) => "waiting",
        TaskState::Pending => "pending",
        TaskState::Cancelled => "cancelled",
        TaskState::Consumed => "consumed",
    }
}

fn readiness_source_target(source: &ReadinessSource) -> &str {
    match source {
        ReadinessSource::Token { target, .. }
        | ReadinessSource::Task { target, .. }
        | ReadinessSource::Channel { target, .. }
        | ReadinessSource::ChannelSend { target, .. }
        | ReadinessSource::FileRead { target, .. }
        | ReadinessSource::FileCreate { target, .. } => target,
    }
}

fn readiness_source_is_ready(source: &ReadinessSource) -> bool {
    match source {
        ReadinessSource::Token {
            signaled,
            cancelled,
            ..
        } => signaled.get() && !cancelled.get(),
        ReadinessSource::Task {
            ready,
            open,
            waiting,
            ..
        } => {
            open.get()
                && (ready.get()
                    || waiting
                        .as_ref()
                        .is_some_and(|registration| registration.signaled.get()))
        }
        ReadinessSource::Channel { readable, .. } => readable.get(),
        ReadinessSource::ChannelSend { writable, .. } => writable.get(),
        ReadinessSource::FileRead { path, .. } => path_is_regular_file(path),
        ReadinessSource::FileCreate { path, .. } => path_is_absent(path),
    }
}

fn readiness_source_timer_id(source: &ReadinessSource) -> Option<usize> {
    match source {
        ReadinessSource::Token {
            id,
            timer_queue: Some(_),
            ..
        } => Some(*id),
        ReadinessSource::Task {
            waiting:
                Some(WakeRegistration {
                    token_id,
                    timer_queue: Some(_),
                    ..
                }),
            ..
        } => Some(*token_id),
        _ => None,
    }
}

fn readiness_sources_same(left: &ReadinessSource, right: &ReadinessSource) -> bool {
    match (left, right) {
        (ReadinessSource::Token { id: left, .. }, ReadinessSource::Token { id: right, .. }) => {
            left == right
        }
        (
            ReadinessSource::Task {
                ready: left,
                open: left_open,
                ..
            },
            ReadinessSource::Task {
                ready: right,
                open: right_open,
                ..
            },
        ) => std::rc::Rc::ptr_eq(left, right) && std::rc::Rc::ptr_eq(left_open, right_open),
        (
            ReadinessSource::Channel { readable: left, .. },
            ReadinessSource::Channel {
                readable: right, ..
            },
        ) => std::rc::Rc::ptr_eq(left, right),
        (
            ReadinessSource::ChannelSend { writable: left, .. },
            ReadinessSource::ChannelSend {
                writable: right, ..
            },
        ) => std::rc::Rc::ptr_eq(left, right),
        (
            ReadinessSource::FileRead { path: left, .. },
            ReadinessSource::FileRead { path: right, .. },
        ) => left == right,
        (
            ReadinessSource::FileCreate { path: left, .. },
            ReadinessSource::FileCreate { path: right, .. },
        ) => left == right,
        _ => false,
    }
}

fn path_is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn path_is_absent(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

fn release_wake_registration(registration: &WakeRegistration) {
    let waiters = registration.waiters.get();
    debug_assert!(waiters > 0, "wake registration released exactly once");
    registration.waiters.set(waiters.saturating_sub(1));
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

const MAX_JSON_DEPTH: usize = 128;
const MAX_JSON_VALUES: usize = 1_000_000;

pub(crate) fn canonical_json(source: &str) -> Result<String, String> {
    require_serialized_size(source.len(), "core_serialize_json_canonical")?;
    let value = JsonParser::new(source).parse()?;
    let mut writer = CanonicalJsonWriter::new();
    writer.write_value(&value)?;
    Ok(writer.finish())
}

pub(crate) fn parse_json_value(source: &str) -> Result<JsonValue, String> {
    require_serialized_size(source.len(), "json parser")?;
    JsonParser::new(source).parse()
}

struct CanonicalJsonWriter {
    out: String,
}

impl CanonicalJsonWriter {
    fn new() -> Self {
        Self { out: String::new() }
    }

    fn finish(self) -> String {
        self.out
    }

    fn push(&mut self, value: &str) -> Result<(), String> {
        let next = self
            .out
            .len()
            .checked_add(value.len())
            .ok_or_else(|| trap("serialize_size_limit", "canonical JSON length overflow"))?;
        require_serialized_size(next, "core_serialize_json_canonical")?;
        self.out.push_str(value);
        Ok(())
    }

    fn push_char(&mut self, value: char) -> Result<(), String> {
        let mut encoded = [0; 4];
        self.push(value.encode_utf8(&mut encoded))
    }

    fn write_string(&mut self, value: &str) -> Result<(), String> {
        self.push("\"")?;
        for ch in value.chars() {
            match ch {
                '"' => self.push("\\\"")?,
                '\\' => self.push("\\\\")?,
                '\n' => self.push("\\n")?,
                '\r' => self.push("\\r")?,
                '\t' => self.push("\\t")?,
                '\u{08}' => self.push("\\b")?,
                '\u{0c}' => self.push("\\f")?,
                ch if (ch as u32) < 0x20 => self.push(&format!("\\u{:04x}", ch as u32))?,
                ch => self.push_char(ch)?,
            }
        }
        self.push("\"")
    }

    fn write_value(&mut self, value: &JsonValue) -> Result<(), String> {
        match value {
            JsonValue::Null => self.push("null"),
            JsonValue::Bool(value) => self.push(if *value { "true" } else { "false" }),
            JsonValue::Int(value) => self.push(&value.to_string()),
            JsonValue::UInt(value) => self.push(&value.to_string()),
            JsonValue::Float(value) => {
                if !value.is_finite() {
                    return Err(trap(
                        "invalid_json",
                        "canonical JSON does not permit non-finite numbers",
                    ));
                }
                let text = if *value == 0.0 {
                    "0".to_string()
                } else {
                    let decimal = value.to_string();
                    if !decimal.contains(['.', 'e', 'E']) && decimal.parse::<i64>().is_err() {
                        format!("{value:e}")
                    } else {
                        decimal
                    }
                };
                self.push(&text)
            }
            JsonValue::String(value) => self.write_string(value),
            JsonValue::Array(values) => {
                self.push("[")?;
                for (idx, value) in values.iter().enumerate() {
                    if idx > 0 {
                        self.push(",")?;
                    }
                    self.write_value(value)?;
                }
                self.push("]")
            }
            JsonValue::Object(values) => {
                self.push("{")?;
                for (idx, (key, value)) in values.iter().enumerate() {
                    if idx > 0 {
                        self.push(",")?;
                    }
                    self.write_string(key)?;
                    self.push(":")?;
                    self.write_value(value)?;
                }
                self.push("}")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

struct JsonParser<'a> {
    source: &'a [u8],
    pos: usize,
    depth: usize,
    values: usize,
}

impl<'a> JsonParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
            depth: 0,
            values: 0,
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
        self.values = self
            .values
            .checked_add(1)
            .ok_or_else(|| trap("invalid_json", "JSON value count overflow"))?;
        if self.values > MAX_JSON_VALUES {
            return Err(trap(
                "invalid_json",
                format!("JSON contains more than {MAX_JSON_VALUES} values"),
            ));
        }
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
                        b'u' => {
                            let first = self.parse_hex_quad()?;
                            let code = if (0xd800..=0xdbff).contains(&first) {
                                if self.next() != Some(b'\\') || self.next() != Some(b'u') {
                                    return Err(trap(
                                        "invalid_json",
                                        "high surrogate is not followed by a low surrogate",
                                    ));
                                }
                                let second = self.parse_hex_quad()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err(trap(
                                        "invalid_json",
                                        "high surrogate is not followed by a low surrogate",
                                    ));
                                }
                                0x10000
                                    + (((u32::from(first) - 0xd800) << 10)
                                        | (u32::from(second) - 0xdc00))
                            } else if (0xdc00..=0xdfff).contains(&first) {
                                return Err(trap(
                                    "invalid_json",
                                    "low surrogate has no preceding high surrogate",
                                ));
                            } else {
                                u32::from(first)
                            };
                            out.push(char::from_u32(code).ok_or_else(|| {
                                trap("invalid_json", "unicode escape is not a scalar value")
                            })?);
                        }
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
        self.enter_container()?;
        let mut values = Vec::new();
        self.skip_ws();
        if self.consume(b']') {
            self.depth -= 1;
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.parse_value()?);
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
            self.skip_ws();
            if self.peek() == Some(b']') {
                return Err(trap("invalid_json", "trailing comma in array"));
            }
        }
        self.depth -= 1;
        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect(b'{')?;
        self.enter_container()?;
        let mut values = BTreeMap::new();
        self.skip_ws();
        if self.consume(b'}') {
            self.depth -= 1;
            return Ok(JsonValue::Object(values));
        }
        loop {
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            if values.insert(key.clone(), value).is_some() {
                return Err(trap(
                    "invalid_json",
                    format!("duplicate object key {}", crate::json::string(&key)),
                ));
            }
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
            self.skip_ws();
            if self.peek() == Some(b'}') {
                return Err(trap("invalid_json", "trailing comma in object"));
            }
        }
        self.depth -= 1;
        Ok(JsonValue::Object(values))
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        self.consume(b'-');
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(trap("invalid_json", "leading zero in number"));
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(trap("invalid_json", "number requires an integer part")),
        }
        let mut is_float = false;
        if self.consume(b'.') {
            is_float = true;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(trap("invalid_json", "fraction requires a digit"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            let _ = self.consume(b'+') || self.consume(b'-');
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(trap("invalid_json", "exponent requires a digit"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.source[start..self.pos])
            .map_err(|_| trap("invalid_json", "number is not utf8"))?;
        if is_float {
            let value = text
                .parse::<f64>()
                .map_err(|_| trap("invalid_json", format!("invalid float '{text}'")))?;
            if !value.is_finite() {
                return Err(trap("invalid_json", format!("non-finite float '{text}'")));
            }
            Ok(JsonValue::Float(value))
        } else {
            match text.parse::<i64>() {
                Ok(value) => Ok(JsonValue::Int(value)),
                Err(_) if !text.starts_with('-') => text
                    .parse::<u64>()
                    .map(JsonValue::UInt)
                    .map_err(|_| trap("invalid_json", format!("invalid integer '{text}'"))),
                Err(_) => Err(trap("invalid_json", format!("invalid integer '{text}'"))),
            }
        }
    }

    fn enter_container(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_JSON_DEPTH {
            return Err(trap(
                "invalid_json",
                format!("JSON nesting exceeds {MAX_JSON_DEPTH} containers"),
            ));
        }
        Ok(())
    }

    fn parse_hex_quad(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .next()
                .and_then(|byte| (byte as char).to_digit(16))
                .ok_or_else(|| trap("invalid_json", "invalid unicode escape"))?;
            value = (value << 4) | digit as u16;
        }
        Ok(value)
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
