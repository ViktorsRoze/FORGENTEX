use std::collections::{BTreeSet, HashSet};

use crate::ast::{AsmBlock, Expr, FunctionDecl, Program, Stmt};
use crate::diag::{diagnostics_to_json, Diagnostic};
use crate::targets::{normalize_family, target_by_triple, TargetSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssemblySummary {
    pub node: String,
    pub function: String,
    pub target: String,
    pub changed_registers: Vec<String>,
    pub clobbers: Vec<String>,
    pub features: Vec<String>,
    pub memory_effect: bool,
    pub abi_risk: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyCheckResult {
    pub target: String,
    pub diagnostics: Vec<Diagnostic>,
    pub summaries: Vec<AssemblySummary>,
}

pub fn check_assembly(program: &Program, triple: &str) -> Vec<Diagnostic> {
    analyze_assembly(program, triple).diagnostics
}

pub fn analyze_assembly(program: &Program, triple: &str) -> AssemblyCheckResult {
    let Some(target) = target_by_triple(triple) else {
        return AssemblyCheckResult {
            target: triple.to_string(),
            diagnostics: vec![Diagnostic::new(
                "unknown_target",
                "@program",
                format!("unknown target triple '{triple}'"),
            )
            .suggest("run_fgx_targets")],
            summaries: Vec::new(),
        };
    };

    let mut diagnostics = Vec::new();
    let mut summaries = Vec::new();
    for function in program.functions() {
        let mut env = function
            .params
            .iter()
            .map(|param| param.name.clone())
            .collect::<HashSet<_>>();
        analyze_body(
            &function.body,
            &function.id,
            function,
            target,
            &mut env,
            &mut diagnostics,
            &mut summaries,
        );
    }

    AssemblyCheckResult {
        target: triple.to_string(),
        diagnostics,
        summaries,
    }
}

pub fn review_text(result: &AssemblyCheckResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("Assembly review for {}\n", result.target));
    if result.summaries.is_empty() {
        out.push_str("  - No assembly blocks found.\n");
        return out;
    }
    for summary in &result.summaries {
        out.push_str(&format!(
            "  - {} target={} changed=[{}] clobbers=[{}] features=[{}] memory={} abi_risk={}\n",
            summary.node,
            summary.target,
            summary.changed_registers.join(","),
            summary.clobbers.join(","),
            summary.features.join(","),
            yes_no(summary.memory_effect),
            summary.abi_risk
        ));
    }
    out
}

pub fn asm_check_json(result: &AssemblyCheckResult) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.asm_check.v1\",\n");
    out.push_str(&format!("  \"target\": {},\n", quoted(&result.target)));
    out.push_str("  \"review\": [");
    for (idx, summary) in result.summaries.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"node\": {},\n      \"function\": {},\n      \"target\": {},",
            quoted(&summary.node),
            quoted(&summary.function),
            quoted(&summary.target)
        ));
        push_string_array(
            &mut out,
            6,
            "changed_registers",
            &summary.changed_registers,
            true,
        );
        push_string_array(&mut out, 6, "clobbers", &summary.clobbers, true);
        push_string_array(&mut out, 6, "features", &summary.features, true);
        out.push_str(&format!(
            "\n      \"memory_effect\": {},\n      \"abi_risk\": {}\n    }}",
            bool_json(summary.memory_effect),
            quoted(&summary.abi_risk)
        ));
    }
    if !result.summaries.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("],\n");
    let diagnostics_json = diagnostics_to_json(&result.diagnostics);
    let diagnostics = diagnostics_json
        .trim()
        .strip_prefix("{\n  \"diagnostics\": ")
        .and_then(|value| value.strip_suffix("\n}"))
        .unwrap_or("[]");
    out.push_str(&format!("  \"diagnostics\": {diagnostics}\n"));
    out.push('}');
    out
}

fn analyze_body(
    body: &[Stmt],
    id_prefix: &str,
    function: &FunctionDecl,
    target: &TargetSpec,
    env: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    summaries: &mut Vec<AssemblySummary>,
) {
    for (idx, stmt) in body.iter().enumerate() {
        let stmt_id = format!("{id_prefix}.stmt.{idx}");
        match stmt {
            Stmt::Let { name, .. } => {
                env.insert(name.clone());
            }
            Stmt::Return(_)
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Break
            | Stmt::Continue => {}
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for (label, branch) in [("then", then_body), ("else", else_body)] {
                    let mut branch_env = env.clone();
                    analyze_body(
                        branch,
                        &format!("{stmt_id}.{label}"),
                        function,
                        target,
                        &mut branch_env,
                        diagnostics,
                        summaries,
                    );
                }
            }
            Stmt::For { var, body, .. } => {
                let mut loop_env = env.clone();
                loop_env.insert(var.clone());
                analyze_body(
                    body,
                    &format!("{stmt_id}.body"),
                    function,
                    target,
                    &mut loop_env,
                    diagnostics,
                    summaries,
                );
            }
            Stmt::While { body, .. } => {
                let mut loop_env = env.clone();
                analyze_body(
                    body,
                    &format!("{stmt_id}.body"),
                    function,
                    target,
                    &mut loop_env,
                    diagnostics,
                    summaries,
                );
            }
            Stmt::Match { arms, .. } => {
                for (arm_idx, arm) in arms.iter().enumerate() {
                    let mut arm_env = env.clone();
                    analyze_body(
                        &arm.body,
                        &format!("{stmt_id}.arm{arm_idx}"),
                        function,
                        target,
                        &mut arm_env,
                        diagnostics,
                        summaries,
                    );
                }
            }
            Stmt::Asm(asm) => {
                analyze_asm_block(
                    asm,
                    &format!("{stmt_id}.asm"),
                    function,
                    target,
                    env,
                    diagnostics,
                    summaries,
                );
            }
        }
    }
}

fn analyze_asm_block(
    asm: &AsmBlock,
    node: &str,
    function: &FunctionDecl,
    target: &TargetSpec,
    env: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
    summaries: &mut Vec<AssemblySummary>,
) {
    if !function.unsafe_fn {
        diagnostics.push(
            Diagnostic::new(
                "unsafe_required",
                node,
                "assembly blocks are only allowed inside unsafe functions",
            )
            .suggest("mark_function_unsafe"),
        );
    }

    let Some(dialect) = AsmDialect::from_target(&asm.target) else {
        diagnostics.push(
            Diagnostic::new(
                "asm_unknown_dialect",
                node,
                format!("unknown assembly dialect '{}'", asm.target),
            )
            .suggest("choose_known_asm_dialect"),
        );
        return;
    };

    if normalize_family(&asm.target) != target.family {
        diagnostics.push(
            Diagnostic::new(
                "asm_target_mismatch",
                node,
                format!(
                    "assembly dialect '{}' is not compatible with selected target '{}'",
                    asm.target, target.triple
                ),
            )
            .suggest("choose_matching_target")
            .suggest("move_assembly_to_matching_arch_path"),
        );
    }

    let enabled_features = enabled_features(function, target);
    let mut required_features = BTreeSet::new();
    let mut changed_registers = BTreeSet::new();
    let mut clobbers = BTreeSet::new();

    for binding in &asm.inputs {
        validate_register(
            &binding.register,
            RegisterRole::Input,
            dialect,
            &enabled_features,
            node,
            diagnostics,
        );
        if !env.contains(&binding.value) {
            diagnostics.push(
                Diagnostic::new(
                    "asm_unknown_input",
                    node,
                    format!(
                        "assembly input '{}' is not a visible local or parameter",
                        binding.value
                    ),
                )
                .suggest("bind_input_before_asm"),
            );
        }
    }

    for binding in &asm.outputs {
        validate_register(
            &binding.register,
            RegisterRole::Output,
            dialect,
            &enabled_features,
            node,
            diagnostics,
        );
        changed_registers.insert(canonical_register(&binding.register));
        env.insert(binding.value.clone());
    }

    for clobber in expanded_clobbers(&asm.clobbers) {
        validate_clobber(&clobber, dialect, &enabled_features, node, diagnostics);
        if is_memory_clobber(&clobber) || is_flag_clobber(&clobber) {
            clobbers.insert(clobber);
        } else {
            let canonical = canonical_register(&clobber);
            changed_registers.insert(canonical.clone());
            clobbers.insert(canonical);
        }
    }

    for op in &asm.ops {
        validate_op(
            op,
            dialect,
            target,
            &enabled_features,
            node,
            diagnostics,
            &mut required_features,
        );
    }

    let summary = AssemblySummary {
        node: node.to_string(),
        function: function.name.clone(),
        target: asm.target.clone(),
        changed_registers: changed_registers.into_iter().collect(),
        clobbers: clobbers.into_iter().collect(),
        features: required_features.into_iter().collect(),
        memory_effect: true,
        abi_risk: abi_risk(dialect, asm),
    };
    summaries.push(summary);
}

fn validate_op(
    op: &[String],
    dialect: AsmDialect,
    target: &TargetSpec,
    enabled_features: &BTreeSet<String>,
    node: &str,
    diagnostics: &mut Vec<Diagnostic>,
    required_features: &mut BTreeSet<String>,
) {
    let Some(mnemonic) = op.first() else {
        diagnostics.push(
            Diagnostic::new("asm_empty_op", node, "assembly op cannot be empty")
                .suggest("remove_empty_op"),
        );
        return;
    };

    if let Some(feature) = instruction_required_feature(dialect, mnemonic) {
        required_features.insert(feature.to_string());
        require_feature(feature, target, enabled_features, node, diagnostics);
    }

    for operand in op.iter().skip(1) {
        let token = clean_operand(operand);
        if token.is_empty() || token.parse::<i64>().is_ok() {
            continue;
        }
        if looks_register_like(token) {
            validate_register(
                token,
                RegisterRole::Operand,
                dialect,
                enabled_features,
                node,
                diagnostics,
            );
        }
    }
}

fn validate_register(
    register: &str,
    role: RegisterRole,
    dialect: AsmDialect,
    enabled_features: &BTreeSet<String>,
    node: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let register = clean_operand(register);
    if !dialect.accepts_register(register, enabled_features) {
        diagnostics.push(
            Diagnostic::new(
                "asm_invalid_register",
                node,
                format!("register '{register}' is not valid for {}", dialect.name()),
            )
            .suggest("replace_register_for_target"),
        );
        return;
    }
    if matches!(role, RegisterRole::Output | RegisterRole::Clobber)
        && dialect.is_reserved_write_register(register)
    {
        diagnostics.push(
            Diagnostic::new(
                "asm_reserved_register_write",
                node,
                format!("register '{register}' cannot be written or clobbered directly"),
            )
            .suggest("use_abi_safe_register"),
        );
    }
}

fn validate_clobber(
    clobber: &str,
    dialect: AsmDialect,
    enabled_features: &BTreeSet<String>,
    node: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if is_memory_clobber(clobber) {
        return;
    }
    if is_flag_clobber(clobber) {
        if dialect.has_flags_clobber() {
            return;
        }
        diagnostics.push(
            Diagnostic::new(
                "asm_invalid_clobber",
                node,
                format!("flags clobber is not valid for {}", dialect.name()),
            )
            .suggest("remove_invalid_clobber"),
        );
        return;
    }
    validate_register(
        clobber,
        RegisterRole::Clobber,
        dialect,
        enabled_features,
        node,
        diagnostics,
    );
}

fn require_feature(
    feature: &str,
    target: &TargetSpec,
    enabled_features: &BTreeSet<String>,
    node: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !target.known_features.contains(&feature) {
        diagnostics.push(
            Diagnostic::new(
                "asm_feature_mismatch",
                node,
                format!(
                    "feature '{feature}' is not available for target family '{}'",
                    target.family
                ),
            )
            .suggest("choose_matching_target")
            .suggest("replace_feature_specific_instruction"),
        );
        return;
    }
    if !enabled_features.contains(feature) {
        diagnostics.push(
            Diagnostic::new(
                "asm_feature_required",
                node,
                format!("instruction requires explicit target feature 'cpu.{feature}'"),
            )
            .suggest("add_cpu_feature_effect")
            .suggest("replace_feature_specific_instruction"),
        );
    }
}

fn enabled_features(function: &FunctionDecl, target: &TargetSpec) -> BTreeSet<String> {
    let mut features = target
        .default_features
        .iter()
        .map(|feature| (*feature).to_string())
        .collect::<BTreeSet<_>>();
    for effect in &function.effects {
        if let Some(feature) = effect.strip_prefix("cpu.") {
            features.insert(feature.to_string());
        }
    }
    features
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegisterRole {
    Input,
    Output,
    Clobber,
    Operand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsmDialect {
    X86_64,
    Armv7,
    Aarch64,
    Riscv32,
    Riscv64,
}

impl AsmDialect {
    fn from_target(target: &str) -> Option<Self> {
        match target {
            "x86" | "x64" | "amd64" | "x86_64" => Some(Self::X86_64),
            "arm" | "armv7" | "arm32" => Some(Self::Armv7),
            "aarch64" => Some(Self::Aarch64),
            "riscv32" | "riscv32imac" => Some(Self::Riscv32),
            "riscv64" | "riscv64gc" => Some(Self::Riscv64),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "asm x86_64",
            Self::Armv7 => "asm armv7",
            Self::Aarch64 => "asm aarch64",
            Self::Riscv32 => "asm riscv32",
            Self::Riscv64 => "asm riscv64",
        }
    }

    fn accepts_register(self, register: &str, features: &BTreeSet<String>) -> bool {
        match self {
            Self::X86_64 => is_x86_register(register),
            Self::Armv7 => is_armv7_register(register),
            Self::Aarch64 => is_aarch64_register(register, features),
            Self::Riscv32 | Self::Riscv64 => is_riscv_register(register, features),
        }
    }

    fn is_reserved_write_register(self, register: &str) -> bool {
        match self {
            Self::X86_64 => register == "rsp",
            Self::Armv7 => matches!(register, "r13" | "r15" | "sp" | "pc"),
            Self::Aarch64 => register == "sp",
            Self::Riscv32 | Self::Riscv64 => {
                matches!(
                    register,
                    "x0" | "zero" | "sp" | "x2" | "gp" | "x3" | "tp" | "x4"
                )
            }
        }
    }

    fn has_flags_clobber(self) -> bool {
        !matches!(self, Self::Riscv32 | Self::Riscv64)
    }

    fn callee_saved_registers(self) -> &'static [&'static str] {
        match self {
            Self::X86_64 => &["rbx", "rbp", "r12", "r13", "r14", "r15"],
            Self::Armv7 => &["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11"],
            Self::Aarch64 => &[
                "x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28",
            ],
            Self::Riscv32 | Self::Riscv64 => &[
                "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "x8",
                "x9", "x18", "x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27",
            ],
        }
    }
}

fn is_x86_register(register: &str) -> bool {
    matches!(
        register,
        "rax" | "rbx" | "rcx" | "rdx" | "rsi" | "rdi" | "rsp" | "rbp"
    ) || numbered_register(register, "r", 8, 15)
        || numbered_register(register, "xmm", 0, 31)
}

fn is_armv7_register(register: &str) -> bool {
    numbered_register(register, "r", 0, 15)
        || matches!(register, "sp" | "lr" | "pc" | "flags")
        || numbered_register(register, "s", 0, 31)
        || numbered_register(register, "d", 0, 31)
        || numbered_register(register, "q", 0, 15)
}

fn is_aarch64_register(register: &str, features: &BTreeSet<String>) -> bool {
    numbered_register(register, "x", 0, 30)
        || numbered_register(register, "w", 0, 30)
        || register == "sp"
        || numbered_register(register, "v", 0, 31)
        || ((features.contains("sve") || features.contains("sve2"))
            && numbered_register(register, "p", 0, 15))
}

fn is_riscv_register(register: &str, features: &BTreeSet<String>) -> bool {
    numbered_register(register, "x", 0, 31)
        || numbered_register(register, "f", 0, 31)
        || (features.contains("v") && numbered_register(register, "v", 0, 31))
        || matches!(
            register,
            "zero"
                | "ra"
                | "sp"
                | "gp"
                | "tp"
                | "fp"
                | "t0"
                | "t1"
                | "t2"
                | "t3"
                | "t4"
                | "t5"
                | "t6"
                | "s0"
                | "s1"
                | "s2"
                | "s3"
                | "s4"
                | "s5"
                | "s6"
                | "s7"
                | "s8"
                | "s9"
                | "s10"
                | "s11"
                | "a0"
                | "a1"
                | "a2"
                | "a3"
                | "a4"
                | "a5"
                | "a6"
                | "a7"
        )
}

fn numbered_register(register: &str, prefix: &str, min: usize, max: usize) -> bool {
    let Some(number) = register.strip_prefix(prefix) else {
        return false;
    };
    let Ok(number) = number.parse::<usize>() else {
        return false;
    };
    (min..=max).contains(&number)
}

fn instruction_required_feature(dialect: AsmDialect, mnemonic: &str) -> Option<&'static str> {
    let mnemonic = mnemonic.to_ascii_lowercase();
    match dialect {
        AsmDialect::X86_64 => match mnemonic.as_str() {
            "popcnt" => Some("popcnt"),
            "pdep" | "pext" => Some("bmi2"),
            "vaddps" | "vmulps" | "vaddpd" | "vmulpd" | "vperm2i128" => Some("avx2"),
            _ if mnemonic.starts_with("v") && mnemonic.contains("512") => Some("avx512f"),
            _ => None,
        },
        AsmDialect::Armv7 => match mnemonic.as_str() {
            "vcnt" | "vadd" | "vmul" | "vld1" | "vst1" => Some("neon"),
            _ => None,
        },
        AsmDialect::Aarch64 => match mnemonic.as_str() {
            "cnt" | "addv" | "ld1" | "st1" | "mla" => Some("neon"),
            "ptrue" | "whilelt" | "ld1w" | "st1w" => Some("sve"),
            _ => None,
        },
        AsmDialect::Riscv32 | AsmDialect::Riscv64 => match mnemonic.as_str() {
            "cpop" | "clz" | "ctz" => Some("zbb"),
            "sh1add" | "sh2add" | "sh3add" => Some("zba"),
            "bset" | "bclr" | "binv" | "bext" => Some("zbs"),
            _ if mnemonic.starts_with('v') => Some("v"),
            _ => None,
        },
    }
}

fn abi_risk(dialect: AsmDialect, asm: &AsmBlock) -> String {
    let clobbers = expanded_clobbers(&asm.clobbers)
        .into_iter()
        .map(|clobber| canonical_register(&clobber))
        .collect::<BTreeSet<_>>();
    if clobbers.iter().any(|clobber| {
        dialect
            .callee_saved_registers()
            .iter()
            .any(|register| register == clobber)
    }) {
        return "high".to_string();
    }
    if clobbers.iter().any(|clobber| clobber == "memory") || !asm.outputs.is_empty() {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn expanded_clobbers(clobbers: &[String]) -> Vec<String> {
    clobbers
        .iter()
        .flat_map(|clobber| clobber.split([',', ' ', '\t']))
        .map(str::trim)
        .filter(|clobber| !clobber.is_empty())
        .map(canonical_register)
        .collect()
}

fn looks_register_like(token: &str) -> bool {
    is_x86_register(token)
        || is_armv7_register(token)
        || is_aarch64_register(
            token,
            &BTreeSet::from(["sve".to_string(), "sve2".to_string()]),
        )
        || is_riscv_register(token, &BTreeSet::from(["v".to_string()]))
        || token
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, 'r' | 'x' | 'w' | 'v' | 'p' | 's' | 'd' | 'q' | 'f'))
}

fn clean_operand(operand: &str) -> &str {
    operand.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | '[' | ']' | '(' | ')' | '{' | '}' | '+' | '*' | '#' | '!'
        )
    })
}

fn canonical_register(register: &str) -> String {
    clean_operand(register).to_ascii_lowercase()
}

fn is_memory_clobber(clobber: &str) -> bool {
    clobber == "memory" || clobber == "mem"
}

fn is_flag_clobber(clobber: &str) -> bool {
    matches!(clobber, "flags" | "flag" | "cc" | "nzcv")
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
        out.push_str(&quoted(value));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[allow(dead_code)]
fn _expr_uses(expr: &Expr) -> bool {
    matches!(expr, Expr::Var(_))
}
