use std::collections::{BTreeMap, HashMap};

use crate::ast::{Expr, FunctionDecl, Program, Stmt, Type};
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
    Void,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    pub entry: String,
    pub return_value: Value,
    pub bindings: HashMap<String, Value>,
    pub status: String,
}

pub fn run_demo(program: &Program) -> Result<String, String> {
    let interpreter = Interpreter {
        program,
        io: None,
        net: None,
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
    let input = RunInput::parse(input)?;
    let interpreter = Interpreter {
        program,
        io: None,
        net: None,
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
    let interpreter = Interpreter {
        program,
        io: Some(IoContext::new(grants)),
        net: None,
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
    let interpreter = Interpreter {
        program,
        io: None,
        net: Some(NetContext::new(allow)),
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
        io: None,
        net: None,
    };
    let mut env = HashMap::new();
    interpreter.eval_expr(expr, &mut env)
}

struct Interpreter<'a> {
    program: &'a Program,
    io: Option<IoContext>,
    net: Option<NetContext>,
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

    fn execute_function(
        &self,
        function: &FunctionDecl,
        args: Vec<Value>,
    ) -> Result<(Value, HashMap<String, Value>), String> {
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
        for stmt in &function.body {
            if let Control::Return(value) = self.exec_stmt(stmt, &mut env)? {
                return Ok((value, env));
            }
        }
        Ok((Value::Void, env))
    }

    fn exec_stmt(&self, stmt: &Stmt, env: &mut HashMap<String, Value>) -> Result<Control, String> {
        match stmt {
            Stmt::Let { name, expr, .. } => {
                let value = self.eval_expr(expr, env)?;
                env.insert(name.clone(), value);
                Ok(Control::Continue)
            }
            Stmt::Return(Some(expr)) => Ok(Control::Return(self.eval_expr(expr, env)?)),
            Stmt::Return(None) => Ok(Control::Return(Value::Void)),
            Stmt::Require(expr) => {
                if !self.eval_expr(expr, env)?.as_bool()? {
                    return Err(trap("require_failed", format!("require failed: {expr:?}")));
                }
                Ok(Control::Continue)
            }
            Stmt::If { cond, then_body } => {
                if self.eval_expr(cond, env)?.as_bool()? {
                    for stmt in then_body {
                        if let Control::Return(value) = self.exec_stmt(stmt, env)? {
                            return Ok(Control::Return(value));
                        }
                    }
                }
                Ok(Control::Continue)
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
                    for stmt in body {
                        if let Control::Return(value) = self.exec_stmt(stmt, env)? {
                            return Ok(Control::Return(value));
                        }
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
            Expr::Var(name) => env
                .get(name)
                .cloned()
                .ok_or_else(|| trap("unknown_symbol", format!("unknown symbol '{name}'"))),
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
                let values = args
                    .iter()
                    .map(|arg| self.eval_expr(arg, env))
                    .collect::<Result<Vec<_>, _>>()?;
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

    fn eval_runtime_intrinsic(
        &self,
        name: &str,
        args: &[Expr],
        env: &mut HashMap<String, Value>,
    ) -> Result<Option<Value>, String> {
        match name {
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

enum Control {
    Continue,
    Return(Value),
}

fn eval_binary(op: &str, left: Value, right: Value) -> Result<Value, String> {
    match op {
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
        while let Some(ch) = self.next() {
            match ch {
                b'"' => return Ok(out),
                b'\\' => {
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
                _ => out.push(char::from(ch)),
            }
        }
        Err(trap("invalid_json", "unterminated string"))
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
