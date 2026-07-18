use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ast::{Expr, FunctionDecl, Program, Stmt};
use crate::diag::Diagnostic;
use crate::stdlib::{standard_target_family_probe, standard_target_feature_probe};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetSpec {
    pub triple: &'static str,
    pub family: &'static str,
    pub default_features: &'static [&'static str],
    pub known_features: &'static [&'static str],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetMetadata {
    pub features: Vec<String>,
    pub families: Vec<String>,
    pub labels: Vec<String>,
}

const X86_FEATURES: &[&str] = &["sse2", "avx2", "avx512f", "bmi2", "popcnt"];
const ARM_FEATURES: &[&str] = &["neon", "vfpv3", "thumb2"];
const AARCH64_FEATURES: &[&str] = &["neon", "sve", "sve2", "crc", "crypto"];
const RISCV_FEATURES: &[&str] = &[
    "i", "m", "a", "f", "d", "c", "v", "zbb", "zba", "zbs", "crypto",
];

const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        triple: "x86_64-pc-windows-msvc",
        family: "x86_64",
        default_features: &["sse2"],
        known_features: X86_FEATURES,
    },
    TargetSpec {
        triple: "x86_64-unknown-linux-gnu",
        family: "x86_64",
        default_features: &["sse2"],
        known_features: X86_FEATURES,
    },
    TargetSpec {
        triple: "aarch64-apple-darwin",
        family: "aarch64",
        default_features: &["neon"],
        known_features: AARCH64_FEATURES,
    },
    TargetSpec {
        triple: "aarch64-unknown-linux-gnu",
        family: "aarch64",
        default_features: &["neon"],
        known_features: AARCH64_FEATURES,
    },
    TargetSpec {
        triple: "aarch64-pc-windows-msvc",
        family: "aarch64",
        default_features: &["neon"],
        known_features: AARCH64_FEATURES,
    },
    TargetSpec {
        triple: "armv7-unknown-linux-gnueabihf",
        family: "arm",
        default_features: &["vfpv3", "thumb2"],
        known_features: ARM_FEATURES,
    },
    TargetSpec {
        triple: "riscv64gc-unknown-linux-gnu",
        family: "riscv64",
        default_features: &["i", "m", "a", "f", "d", "c"],
        known_features: RISCV_FEATURES,
    },
    TargetSpec {
        triple: "riscv32imac-unknown-none-elf",
        family: "riscv32",
        default_features: &["i", "m", "a", "c"],
        known_features: RISCV_FEATURES,
    },
];

pub fn all_targets() -> &'static [TargetSpec] {
    TARGETS
}

pub fn target_by_triple(triple: &str) -> Option<&'static TargetSpec> {
    TARGETS.iter().find(|target| target.triple == triple)
}

pub fn targets_text() -> String {
    let mut out = String::new();
    out.push_str("Forgentex target architecture model\n");
    out.push_str("Families: x86_64, arm, aarch64, riscv32, riscv64\n");
    out.push_str(
        "Feature probes use core_target_has_feature_<feature>() guards; cpu_has_<feature>() remains a compatibility alias.\n\n",
    );
    for target in TARGETS {
        out.push_str(&format!(
            "{} family={} defaults=[{}] features=[{}]\n",
            target.triple,
            target.family,
            target.default_features.join(","),
            target.known_features.join(",")
        ));
    }
    out
}

pub fn targets_json() -> String {
    let mut out = String::new();
    out.push_str("{\n  \"schema\": \"forgentex.targets.v1\",\n  \"families\": [");
    for (idx, family) in ["x86_64", "arm", "aarch64", "riscv32", "riscv64"]
        .iter()
        .enumerate()
    {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(family));
    }
    out.push_str("],\n  \"targets\": [");
    for (idx, target) in TARGETS.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"triple\": {},\n      \"family\": {},",
            quoted(target.triple),
            quoted(target.family)
        ));
        push_string_array(
            &mut out,
            6,
            "default_features",
            target.default_features,
            true,
        );
        push_string_array(&mut out, 6, "known_features", target.known_features, false);
        out.push_str("\n    }");
    }
    out.push_str("\n  ]\n}");
    out
}

pub fn check_program_target(program: &Program, triple: &str) -> Vec<Diagnostic> {
    let Some(target) = target_by_triple(triple) else {
        return vec![Diagnostic::new(
            "unknown_target",
            "@program",
            format!("unknown target triple '{triple}'"),
        )
        .suggest("run_fgx_targets")];
    };

    let requirements = program
        .functions()
        .map(|function| (function.name.clone(), requirements_for_function(function)))
        .collect::<HashMap<_, _>>();
    let mut diagnostics = Vec::new();

    for function in program.functions() {
        let metadata = requirements
            .get(&function.name)
            .cloned()
            .unwrap_or_default();
        validate_declared_metadata(function, &metadata, &mut diagnostics);
        validate_asm_targets(function, target, &mut diagnostics);
        check_targeted_calls(
            &function.body,
            function,
            target,
            &requirements,
            &GuardContext::default(),
            &mut diagnostics,
        );
    }

    diagnostics
}

pub fn target_metadata_for_function(function: &FunctionDecl) -> TargetMetadata {
    let requirements = requirements_for_function(function);
    let mut labels = BTreeSet::new();
    for feature in &requirements.features {
        labels.insert(format!("cpu.{feature}"));
    }
    for family in &requirements.families {
        labels.insert(format!("target.{family}"));
    }
    TargetMetadata {
        features: requirements.features.into_iter().collect(),
        families: requirements.families.into_iter().collect(),
        labels: labels.into_iter().collect(),
    }
}

pub fn is_cpu_feature_probe(name: &str) -> bool {
    standard_target_feature_probe(name).is_some() || standard_target_family_probe(name).is_some()
}

fn validate_declared_metadata(
    function: &FunctionDecl,
    metadata: &TargetRequirements,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for feature in &metadata.features {
        if feature == "simd" {
            continue;
        }
        if feature_families(feature).is_empty() {
            diagnostics.push(
                Diagnostic::new(
                    "unknown_target_feature",
                    &function.id,
                    format!("unknown CPU target feature '{feature}'"),
                )
                .suggest("choose_known_target_feature")
                .suggest("run_fgx_targets"),
            );
        }
    }
    for family in &metadata.families {
        if family == "only" {
            continue;
        }
        if !is_known_family(family) {
            diagnostics.push(
                Diagnostic::new(
                    "unknown_target_family",
                    &function.id,
                    format!("unknown target architecture family '{family}'"),
                )
                .suggest("choose_known_target_family")
                .suggest("run_fgx_targets"),
            );
        }
    }
}

fn validate_asm_targets(
    function: &FunctionDecl,
    target: &TargetSpec,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for asm_target in asm_targets(&function.body) {
        let family = normalize_family(&asm_target);
        if !is_known_family(&family) {
            diagnostics.push(
                Diagnostic::new(
                    "unknown_target_family",
                    &function.id,
                    format!("assembly block uses unknown target family '{asm_target}'"),
                )
                .suggest("choose_known_target_family")
                .suggest("run_fgx_targets"),
            );
            continue;
        }
        if family != target.family {
            diagnostics.push(
                Diagnostic::new(
                    "target_arch_mismatch",
                    &function.id,
                    format!(
                        "assembly target '{family}' is not compatible with selected target '{}'",
                        target.triple
                    ),
                )
                .suggest("choose_matching_target")
                .suggest("move_target_specific_code_behind_arch_guard"),
            );
        }
    }
}

fn check_targeted_calls(
    body: &[Stmt],
    caller: &FunctionDecl,
    target: &TargetSpec,
    requirements: &HashMap<String, TargetRequirements>,
    guards: &GuardContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let { expr, .. }
            | Stmt::Return(Some(expr))
            | Stmt::Require(expr)
            | Stmt::Expr(expr) => {
                check_expr_calls(expr, caller, target, requirements, guards, diagnostics);
            }
            Stmt::Return(None) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                check_expr_calls(cond, caller, target, requirements, guards, diagnostics);
                let mut branch_guards = guards.clone();
                collect_feature_guards(cond, &mut branch_guards);
                check_targeted_calls(
                    then_body,
                    caller,
                    target,
                    requirements,
                    &branch_guards,
                    diagnostics,
                );
                // The else branch is the un-guarded path: the feature tested in
                // `cond` is NOT guaranteed there, so use the original guards.
                check_targeted_calls(else_body, caller, target, requirements, guards, diagnostics);
            }
            Stmt::For {
                start, end, body, ..
            } => {
                check_expr_calls(start, caller, target, requirements, guards, diagnostics);
                check_expr_calls(end, caller, target, requirements, guards, diagnostics);
                check_targeted_calls(body, caller, target, requirements, guards, diagnostics);
            }
            Stmt::While { cond, body } => {
                check_expr_calls(cond, caller, target, requirements, guards, diagnostics);
                check_targeted_calls(body, caller, target, requirements, guards, diagnostics);
            }
            Stmt::Match { scrutinee, arms } => {
                check_expr_calls(scrutinee, caller, target, requirements, guards, diagnostics);
                for arm in arms {
                    check_targeted_calls(
                        &arm.body,
                        caller,
                        target,
                        requirements,
                        guards,
                        diagnostics,
                    );
                }
            }
            Stmt::Assign { target: lhs, expr } => {
                check_expr_calls(lhs, caller, target, requirements, guards, diagnostics);
                check_expr_calls(expr, caller, target, requirements, guards, diagnostics);
            }
        }
    }
}

fn check_expr_calls(
    expr: &Expr,
    caller: &FunctionDecl,
    target: &TargetSpec,
    requirements: &HashMap<String, TargetRequirements>,
    guards: &GuardContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        Expr::Lambda { body, .. } => {
            check_expr_calls(body, caller, target, requirements, guards, diagnostics)
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            check_expr_calls(expr, caller, target, requirements, guards, diagnostics);
        }
        Expr::Binary { left, right, .. } => {
            check_expr_calls(left, caller, target, requirements, guards, diagnostics);
            check_expr_calls(right, caller, target, requirements, guards, diagnostics);
        }
        Expr::Call { name, args } => {
            for arg in args {
                check_expr_calls(arg, caller, target, requirements, guards, diagnostics);
            }
            if is_cpu_feature_probe(name) {
                return;
            }
            let Some(callee_requirements) = requirements.get(name) else {
                return;
            };
            validate_call_requirements(
                name,
                caller,
                callee_requirements,
                target,
                guards,
                diagnostics,
            );
        }
        Expr::Index { base, index } => {
            check_expr_calls(base, caller, target, requirements, guards, diagnostics);
            check_expr_calls(index, caller, target, requirements, guards, diagnostics);
        }
    }
}

fn validate_call_requirements(
    callee_name: &str,
    caller: &FunctionDecl,
    requirements: &TargetRequirements,
    target: &TargetSpec,
    guards: &GuardContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for family in &requirements.families {
        if !is_known_family(family) || family == target.family {
            continue;
        }
        diagnostics.push(
            Diagnostic::new(
                "target_arch_mismatch",
                &caller.id,
                format!(
                    "call to '{callee_name}' requires target family '{family}', but selected target '{}' is '{}'",
                    target.triple, target.family
                ),
            )
            .suggest("choose_matching_target")
            .suggest("guard_target_specific_path"),
        );
        return;
    }

    let defaults = target
        .default_features
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut missing_guards = Vec::new();
    for feature in &requirements.features {
        if feature == "simd" || feature_families(feature).is_empty() {
            continue;
        }
        if !feature_families(feature).contains(&target.family) {
            diagnostics.push(
                Diagnostic::new(
                    "target_feature_mismatch",
                    &caller.id,
                    format!(
                        "call to '{callee_name}' requires CPU feature '{feature}', which is not available on target family '{}'",
                        target.family
                    ),
                )
                .suggest("choose_matching_target")
                .suggest("replace_arch_specific_call"),
            );
            return;
        }
        if !defaults.contains(feature.as_str()) && !guards.features.contains(feature) {
            missing_guards.push(feature.clone());
        }
    }

    if !missing_guards.is_empty() {
        diagnostics.push(
            Diagnostic::new(
                "target_guard_required",
                &caller.id,
                format!(
                    "call to '{callee_name}' requires target feature guard(s): {}",
                    missing_guards.join(",")
                ),
            )
            .suggest("wrap_call_in_cpu_feature_guard")
            .suggest("add_scalar_fallback"),
        );
    }
}

#[derive(Clone, Debug, Default)]
struct GuardContext {
    features: BTreeSet<String>,
}

fn collect_feature_guards(expr: &Expr, guards: &mut GuardContext) {
    match expr {
        Expr::Lambda { body, .. } => collect_feature_guards(body, guards),
        Expr::Call { name, .. } => {
            if let Some(feature) = standard_target_feature_probe(name) {
                guards.features.insert(feature);
            }
        }
        Expr::Binary { op, left, right } if op == "&&" => {
            collect_feature_guards(left, guards);
            collect_feature_guards(right, guards);
        }
        Expr::Unary { .. }
        | Expr::Binary { .. }
        | Expr::Cast { .. }
        | Expr::Field { .. }
        | Expr::Index { .. }
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
    }
}

#[derive(Clone, Debug, Default)]
struct TargetRequirements {
    features: BTreeSet<String>,
    families: BTreeSet<String>,
}

fn requirements_for_function(function: &FunctionDecl) -> TargetRequirements {
    let mut requirements = TargetRequirements::default();
    for effect in &function.effects {
        if let Some(feature) = effect.strip_prefix("cpu.") {
            requirements.features.insert(feature.to_string());
        } else if let Some(family) = effect.strip_prefix("target.") {
            requirements.families.insert(normalize_family(family));
        }
    }
    collect_body_requirements(&function.body, &mut requirements);
    requirements
}

fn collect_body_requirements(body: &[Stmt], requirements: &mut TargetRequirements) {
    for stmt in body {
        match stmt {
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_body_requirements(then_body, requirements);
                collect_body_requirements(else_body, requirements);
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                collect_body_requirements(body, requirements)
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    collect_body_requirements(&arm.body, requirements);
                }
            }
            Stmt::Asm(asm) => {
                requirements.families.insert(normalize_family(&asm.target));
            }
            Stmt::Let { .. }
            | Stmt::Return(_)
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Break
            | Stmt::Continue => {}
        }
    }
}

fn asm_targets(body: &[Stmt]) -> Vec<String> {
    let mut targets = Vec::new();
    for stmt in body {
        match stmt {
            Stmt::Asm(asm) => targets.push(asm.target.clone()),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                targets.extend(asm_targets(then_body));
                targets.extend(asm_targets(else_body));
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => targets.extend(asm_targets(body)),
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    targets.extend(asm_targets(&arm.body));
                }
            }
            Stmt::Let { .. }
            | Stmt::Return(_)
            | Stmt::Require(_)
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Break
            | Stmt::Continue => {}
        }
    }
    targets
}

pub(crate) fn normalize_family(value: &str) -> String {
    match value {
        "x86" | "x64" | "amd64" => "x86_64".to_string(),
        "armv7" | "arm32" => "arm".to_string(),
        "riscv64gc" => "riscv64".to_string(),
        "riscv32imac" => "riscv32".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn is_known_family(family: &str) -> bool {
    matches!(family, "x86_64" | "arm" | "aarch64" | "riscv32" | "riscv64")
}

pub(crate) fn feature_families(feature: &str) -> &'static [&'static str] {
    match feature {
        "sse2" | "avx2" | "avx512f" | "bmi2" | "popcnt" => &["x86_64"],
        "neon" => &["arm", "aarch64"],
        "vfpv3" | "thumb2" => &["arm"],
        "sve" | "sve2" | "crc" => &["aarch64"],
        "crypto" => &["aarch64", "riscv32", "riscv64"],
        "i" | "m" | "a" | "f" | "d" | "c" | "v" | "zbb" | "zba" | "zbs" => &["riscv32", "riscv64"],
        _ => &[],
    }
}

fn push_string_array(out: &mut String, indent: usize, key: &str, values: &[&str], trailing: bool) {
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
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[allow(dead_code)]
fn _families_by_feature() -> BTreeMap<&'static str, &'static [&'static str]> {
    [
        ("sse2", &["x86_64"][..]),
        ("avx2", &["x86_64"][..]),
        ("avx512f", &["x86_64"][..]),
        ("bmi2", &["x86_64"][..]),
        ("popcnt", &["x86_64"][..]),
        ("neon", &["arm", "aarch64"][..]),
        ("vfpv3", &["arm"][..]),
        ("thumb2", &["arm"][..]),
        ("sve", &["aarch64"][..]),
        ("sve2", &["aarch64"][..]),
        ("crc", &["aarch64"][..]),
        ("crypto", &["aarch64", "riscv32", "riscv64"][..]),
        ("i", &["riscv32", "riscv64"][..]),
        ("m", &["riscv32", "riscv64"][..]),
        ("a", &["riscv32", "riscv64"][..]),
        ("f", &["riscv32", "riscv64"][..]),
        ("d", &["riscv32", "riscv64"][..]),
        ("c", &["riscv32", "riscv64"][..]),
        ("v", &["riscv32", "riscv64"][..]),
        ("zbb", &["riscv32", "riscv64"][..]),
        ("zba", &["riscv32", "riscv64"][..]),
        ("zbs", &["riscv32", "riscv64"][..]),
    ]
    .into_iter()
    .collect()
}
