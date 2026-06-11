use std::collections::{BTreeSet, HashMap};

use crate::ast::{Expr, FunctionDecl, Program, Stmt, StructDecl, Type};
use crate::simd::{check_intrinsic_call, is_simd_intrinsic};
use crate::targets::is_cpu_feature_probe;

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionEffects {
    pub id: String,
    pub name: String,
    pub unsafe_fn: bool,
    pub declared: Vec<String>,
    pub inferred: Vec<String>,
    pub missing: Vec<String>,
    pub invalid: Vec<String>,
    pub capabilities: Vec<String>,
}

pub fn analyze(program: &Program) -> Vec<FunctionEffects> {
    let functions = program
        .functions()
        .map(|function| (function.name.clone(), function))
        .collect::<HashMap<_, _>>();
    let structs = program
        .structs()
        .map(|decl| (decl.name.clone(), decl))
        .collect::<HashMap<_, _>>();

    program
        .functions()
        .map(|function| analyze_function(function, &functions, &structs))
        .collect()
}

pub fn effects_to_json(functions: &[FunctionEffects]) -> String {
    let mut out = String::new();
    out.push_str("{\n  \"schema\": \"forgentex.effects.v1\",\n  \"functions\": [");
    for (idx, function) in functions.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"id\": \"{}\",\n      \"name\": \"{}\",\n      \"unsafe\": {},",
            escape(&function.id),
            escape(&function.name),
            function.unsafe_fn
        ));
        push_string_array(&mut out, 6, "declared", &function.declared, true);
        push_string_array(&mut out, 6, "inferred", &function.inferred, true);
        push_string_array(&mut out, 6, "missing", &function.missing, true);
        push_string_array(&mut out, 6, "invalid", &function.invalid, true);
        push_string_array(&mut out, 6, "capabilities", &function.capabilities, false);
        out.push_str("\n    }");
    }
    out.push_str("\n  ]\n}");
    out
}

pub fn is_known_effect(effect: &str) -> bool {
    matches!(
        effect,
        "alloc"
            | "io.read"
            | "io.write"
            | "fs.read"
            | "fs.write"
            | "network.read"
            | "network.write"
            | "network.connect"
            | "network.udp"
            | "network.http"
            | "network.websocket"
            | "serialize"
            | "stream"
            | "crypto.sign"
            | "crypto.verify"
            | "crypto.hash"
            | "crypto.secret"
            | "crypto.derive"
            | "crypto.nonce"
            | "ledger.mock"
            | "agent.payment"
            | "agent.patch"
            | "memory.raw"
            | "cpu.simd"
            | "cpu.asm"
    ) || effect.starts_with("cpu.")
        || effect.starts_with("target.")
}

pub fn effect_requires_unsafe(effect: &str) -> bool {
    matches!(effect, "memory.raw" | "cpu.asm")
}

fn analyze_function(
    function: &FunctionDecl,
    functions: &HashMap<String, &FunctionDecl>,
    structs: &HashMap<String, &StructDecl>,
) -> FunctionEffects {
    let declared = sorted_unique(function.effects.iter().cloned());
    let declared_set = declared.iter().cloned().collect::<BTreeSet<_>>();
    let mut inferred = BTreeSet::new();
    let mut env = function
        .params
        .iter()
        .map(|param| (param.name.clone(), param.ty.clone()))
        .collect::<HashMap<_, _>>();

    for param in &function.params {
        collect_type_effects(&param.ty, &mut inferred);
    }
    collect_type_effects(&function.ret, &mut inferred);
    collect_body_effects(&function.body, &mut env, functions, structs, &mut inferred);

    let inferred = inferred.into_iter().collect::<Vec<_>>();
    let missing = inferred
        .iter()
        .filter(|effect| effect_requires_declaration(effect, function.unsafe_fn))
        .filter(|effect| !declared_set.contains(*effect))
        .cloned()
        .collect::<Vec<_>>();
    let invalid = declared
        .iter()
        .filter(|effect| !is_known_effect(effect))
        .cloned()
        .collect::<Vec<_>>();
    let mut capabilities = declared_set;
    for effect in &inferred {
        if !effect_requires_unsafe(effect) || !function.unsafe_fn {
            capabilities.insert(effect.clone());
        }
    }

    FunctionEffects {
        id: function.id.clone(),
        name: function.name.clone(),
        unsafe_fn: function.unsafe_fn,
        declared,
        inferred,
        missing,
        invalid,
        capabilities: capabilities.into_iter().collect(),
    }
}

fn collect_body_effects(
    body: &[Stmt],
    env: &mut HashMap<String, Type>,
    functions: &HashMap<String, &FunctionDecl>,
    structs: &HashMap<String, &StructDecl>,
    effects: &mut BTreeSet<String>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                collect_expr_effects(expr, env, functions, structs, effects);
                let expr_ty = expr_type(expr, env, functions, structs);
                env.insert(name.clone(), ty.clone().unwrap_or(expr_ty));
            }
            Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
                collect_expr_effects(expr, env, functions, structs, effects);
            }
            Stmt::Return(None) => {}
            Stmt::If { cond, then_body } => {
                collect_expr_effects(cond, env, functions, structs, effects);
                let mut branch_env = env.clone();
                collect_body_effects(then_body, &mut branch_env, functions, structs, effects);
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                collect_expr_effects(start, env, functions, structs, effects);
                collect_expr_effects(end, env, functions, structs, effects);
                let mut loop_env = env.clone();
                loop_env.insert(var.clone(), Type::Primitive("usize".to_string()));
                collect_body_effects(body, &mut loop_env, functions, structs, effects);
            }
            Stmt::Assign { target, expr } => {
                collect_expr_effects(target, env, functions, structs, effects);
                collect_expr_effects(expr, env, functions, structs, effects);
            }
            Stmt::Asm(asm) => {
                effects.insert("cpu.asm".to_string());
                effects.insert("memory.raw".to_string());
                effects.insert(format!("target.{}", asm.target));
                for output in &asm.outputs {
                    env.insert(output.value.clone(), Type::Unknown);
                }
            }
        }
    }
}

fn collect_expr_effects(
    expr: &Expr,
    env: &HashMap<String, Type>,
    functions: &HashMap<String, &FunctionDecl>,
    structs: &HashMap<String, &StructDecl>,
    effects: &mut BTreeSet<String>,
) {
    match expr {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) | Expr::Var(_) => {}
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            collect_expr_effects(expr, env, functions, structs, effects);
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_effects(left, env, functions, structs, effects);
            collect_expr_effects(right, env, functions, structs, effects);
        }
        Expr::Call { name, args } => {
            for effect in builtin_effects(name) {
                effects.insert(effect.to_string());
            }
            if let Some(function) = functions.get(name) {
                for effect in &function.effects {
                    effects.insert(effect.clone());
                }
            }
            for arg in args {
                collect_expr_effects(arg, env, functions, structs, effects);
            }
        }
        Expr::Index { base, index } => {
            let base_ty = expr_type(base, env, functions, structs);
            if matches!(base_ty, Type::Pointer { .. }) {
                effects.insert("memory.raw".to_string());
            }
            collect_expr_effects(base, env, functions, structs, effects);
            collect_expr_effects(index, env, functions, structs, effects);
        }
    }
}

fn expr_type(
    expr: &Expr,
    env: &HashMap<String, Type>,
    functions: &HashMap<String, &FunctionDecl>,
    structs: &HashMap<String, &StructDecl>,
) -> Type {
    match expr {
        Expr::Int(_) => Type::Primitive("$int".to_string()),
        Expr::Float(_) => Type::Primitive("$float".to_string()),
        Expr::Bool(_) => Type::Bool,
        Expr::String(_) => Type::Primitive("str".to_string()),
        Expr::Var(name) => env.get(name).cloned().unwrap_or(Type::Unknown),
        Expr::Unary { op, expr } => {
            if op == "!" {
                Type::Bool
            } else {
                expr_type(expr, env, functions, structs)
            }
        }
        Expr::Binary { op, left, right } => match op.as_str() {
            "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" => Type::Bool,
            _ => {
                let left_ty = expr_type(left, env, functions, structs);
                if !left_ty.is_literal() {
                    left_ty
                } else {
                    expr_type(right, env, functions, structs)
                }
            }
        },
        Expr::Call { name, args } => {
            if is_cpu_feature_probe(name) || name == "aligned" {
                Type::Bool
            } else if is_simd_intrinsic(name) {
                let arg_types = args
                    .iter()
                    .map(|arg| expr_type(arg, env, functions, structs))
                    .collect::<Vec<_>>();
                check_intrinsic_call(name, &arg_types, "@effects").0
            } else if structs.contains_key(name) {
                Type::Named(name.clone())
            } else {
                functions
                    .get(name)
                    .map(|function| function.ret.clone())
                    .unwrap_or(Type::Unknown)
            }
        }
        Expr::Cast { ty, .. } => ty.clone(),
        Expr::Field { base, field } => {
            let base_ty = expr_type(base, env, functions, structs);
            if field == "len" && matches!(base_ty, Type::Slice { .. } | Type::Array { .. }) {
                return Type::Primitive("usize".to_string());
            }
            match base_ty {
                Type::Named(name) => structs
                    .get(&name)
                    .and_then(|decl| {
                        decl.fields
                            .iter()
                            .find(|candidate| candidate.name == *field)
                    })
                    .map(|field| field.ty.clone())
                    .unwrap_or(Type::Unknown),
                _ => Type::Unknown,
            }
        }
        Expr::Index { base, .. } => expr_type(base, env, functions, structs)
            .element_type()
            .unwrap_or(Type::Unknown),
    }
}

fn collect_type_effects(ty: &Type, effects: &mut BTreeSet<String>) {
    match ty {
        Type::Pointer { elem, .. } => {
            effects.insert("memory.raw".to_string());
            collect_type_effects(elem, effects);
        }
        Type::Slice { elem, .. } | Type::Array { elem, .. } => collect_type_effects(elem, effects),
        Type::Generic { name, args } => {
            if name == "SecretKey" {
                effects.insert("crypto.secret".to_string());
            }
            for arg in args {
                collect_type_effects(arg, effects);
            }
        }
        Type::Bool | Type::Primitive(_) | Type::Named(_) | Type::Void | Type::Unknown => {}
    }
}

fn effect_requires_declaration(effect: &str, unsafe_fn: bool) -> bool {
    !(unsafe_fn && (matches!(effect, "cpu.asm" | "memory.raw") || effect.starts_with("target.")))
}

fn builtin_effects(name: &str) -> Vec<&'static str> {
    if is_simd_intrinsic(name) {
        return vec!["cpu.simd"];
    }
    match name {
        "alloc" | "alloc_buffer" | "realloc" | "free" => vec!["alloc"],
        "read_file" | "file_read" | "io_read" => vec!["io.read"],
        "write_file" | "file_write" | "io_write" => vec!["io.write"],
        "core_fs_scope_read"
        | "core_runtime_fs_scope_read"
        | "core_fs_read_text"
        | "core_runtime_fs_read_text" => vec!["fs.read"],
        "core_fs_scope_write"
        | "core_runtime_fs_scope_write"
        | "core_fs_write_text"
        | "core_runtime_fs_write_text" => vec!["fs.write"],
        "core_fs_copy_text" | "core_runtime_fs_copy_text" => vec!["fs.read", "fs.write"],
        "http_get" | "net_recv" | "network_read" | "tcp_read" => vec!["network.read"],
        "http_post" | "net_send" | "network_write" | "tcp_write" => vec!["network.write"],
        "core_net_http_get" | "core_runtime_net_http_get" => {
            vec!["network.connect", "network.read", "network.http"]
        }
        "core_net_http_post" | "core_runtime_net_http_post" => {
            vec![
                "network.connect",
                "network.write",
                "network.read",
                "network.http",
            ]
        }
        "core_net_tcp_send" | "core_runtime_net_tcp_send" => {
            vec!["network.connect", "network.write"]
        }
        "core_net_udp_send" | "core_runtime_net_udp_send" => {
            vec!["network.udp", "network.write"]
        }
        "core_net_websocket_send" | "core_runtime_net_websocket_send" => {
            vec!["network.connect", "network.write", "network.websocket"]
        }
        "core_serialize_text_len" | "core_runtime_serialize_text_len" => vec!["serialize"],
        "core_stream_count_bytes" | "core_runtime_stream_count_bytes" => vec!["stream"],
        "core_crypto_mock_secret_key" | "core_runtime_crypto_mock_secret_key" => {
            vec!["crypto.secret"]
        }
        "core_crypto_public_key_ed25519" | "core_runtime_crypto_public_key_ed25519" => {
            vec!["crypto.derive", "crypto.secret"]
        }
        "core_crypto_hash_blake3" | "core_runtime_crypto_hash_blake3" => vec!["crypto.hash"],
        "core_crypto_sign_ed25519" | "core_runtime_crypto_sign_ed25519" => {
            vec!["crypto.sign", "crypto.secret"]
        }
        "core_crypto_verify_ed25519" | "core_runtime_crypto_verify_ed25519" => {
            vec!["crypto.verify"]
        }
        "core_crypto_nonce" | "core_runtime_crypto_nonce" => vec!["crypto.nonce"],
        "core_crypto_export_secret_key" | "core_runtime_crypto_export_secret_key" => {
            vec!["crypto.secret", "serialize"]
        }
        "core_crypto_clone_secret_key" | "core_runtime_crypto_clone_secret_key" => {
            vec!["crypto.secret"]
        }
        "core_agent_log_secret" | "core_runtime_agent_log_secret" => {
            vec!["crypto.secret", "io.write"]
        }
        "core_agent_token_amount" | "core_runtime_agent_token_amount" => vec!["ledger.mock"],
        "core_agent_payment_receipt" | "core_runtime_agent_payment_receipt" => {
            vec!["agent.payment", "ledger.mock"]
        }
        "core_agent_receipt_ok" | "core_runtime_agent_receipt_ok" => Vec::new(),
        "core_agent_signed_patch_hash" | "core_runtime_agent_signed_patch_hash" => {
            vec!["agent.patch", "crypto.hash"]
        }
        "crypto_sign" => vec!["crypto.sign"],
        "crypto_verify" => vec!["crypto.verify"],
        "crypto_hash" => vec!["crypto.hash"],
        _ if name.starts_with("alloc_") => vec!["alloc"],
        _ if name.starts_with("network_write_") || name.starts_with("net_send_") => {
            vec!["network.write"]
        }
        _ if name.starts_with("network_") || name.starts_with("net_") => vec!["network.read"],
        _ if name.starts_with("crypto_") => vec!["crypto.hash"],
        _ => Vec::new(),
    }
}

fn sorted_unique(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn push_string_array(
    out: &mut String,
    indent: usize,
    key: &str,
    values: &[String],
    trailing: bool,
) {
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("\"{}\"", escape(value)));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
}

use crate::json::escape;
