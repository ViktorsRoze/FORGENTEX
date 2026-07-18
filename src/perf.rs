use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Expr, FunctionDecl, Stmt};
use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::simd::is_simd_intrinsic;
use crate::targets::target_by_triple;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchOptions {
    pub source: PathBuf,
    pub entry: String,
    pub input: PathBuf,
    pub target: String,
    pub iterations: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchReport {
    pub schema: String,
    pub source: String,
    pub entry: String,
    pub target: String,
    pub iterations: usize,
    pub correctness: String,
    pub sample_count: usize,
    pub architecture_path: String,
    pub estimated_cycles: u64,
    pub scalar_reference_cycles: u64,
    pub speedup_vs_scalar: f64,
    pub operations: OperationCounts,
    pub allocation_report: AllocationReport,
    pub bounds_report: BoundsCheckReport,
    pub evidence: Vec<String>,
    pub suggestions: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperationCounts {
    pub loops: usize,
    pub scalar_ops: usize,
    pub calls: usize,
    pub simd_calls: usize,
    pub index_exprs: usize,
    pub stores: usize,
    pub max_vector_lanes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationReport {
    pub allocations: usize,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundsCheckReport {
    pub checks: usize,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PerfComparison {
    pub target: String,
    pub scalar_cycles: u64,
    pub optimized_cycles: u64,
    pub speedup: f64,
    pub architecture_path: String,
    pub correctness: String,
    pub evidence: Vec<String>,
    pub allocation_report: AllocationReport,
    pub bounds_report: BoundsCheckReport,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PerfReport {
    pub schema: String,
    pub scalar_source: String,
    pub optimized_source: String,
    pub entry: String,
    pub optimized_entry: String,
    pub input: String,
    pub patch_gate: String,
    pub comparisons: Vec<PerfComparison>,
    pub suggestions: Vec<String>,
}

const DEFAULT_PERF_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "riscv64gc-unknown-linux-gnu",
];

pub fn run_bench(options: &BenchOptions) -> Result<BenchReport, String> {
    if target_by_triple(&options.target).is_none() {
        return Err(format!("unknown target triple '{}'", options.target));
    }
    let source = fs::read_to_string(&options.source)
        .map_err(|err| format!("failed to read {}: {err}", options.source.display()))?;
    let input = fs::read_to_string(&options.input)
        .map_err(|err| format!("failed to read {}: {err}", options.input.display()))?;
    let program = parse_file(&options.source, &source)?;
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err("benchmark source has type diagnostics".to_string());
    }
    let function = program
        .find_function(&options.entry)
        .ok_or_else(|| format!("benchmark entry '{}' not found", options.entry))?;
    let run_output = run_entry_json(&program, &options.entry, &input)?;
    let correctness = run_status(&run_output);
    let sample_count = sample_count(&input).unwrap_or(1).max(1);
    let operations = analyze_function(function);
    let allocation_report = AllocationReport {
        allocations: estimate_allocations(&operations),
        status: if estimate_allocations(&operations) == 0 {
            "none".to_string()
        } else {
            "allocating".to_string()
        },
    };
    let bounds_report = BoundsCheckReport {
        checks: operations.index_exprs.saturating_mul(sample_count.max(1)),
        status: if operations.index_exprs == 0 {
            "none".to_string()
        } else {
            "guarded".to_string()
        },
    };
    let scalar_reference_cycles = estimate_scalar_cycles(&operations, sample_count);
    let architecture_path = architecture_path(&options.target, operations.simd_calls > 0);
    let estimated_cycles = estimate_cycles(&operations, sample_count, &options.target);
    let speedup_vs_scalar = ratio(scalar_reference_cycles, estimated_cycles);
    let evidence = vec![
        format!("correctness={correctness}"),
        format!("target={}", options.target),
        format!("iterations={}", options.iterations),
        "benchmark=deterministic-estimate".to_string(),
        format!("bounds_checks={}", bounds_report.checks),
        format!("allocations={}", allocation_report.allocations),
    ];
    Ok(BenchReport {
        schema: "forgentex.bench.v1".to_string(),
        source: display_path(&options.source),
        entry: options.entry.clone(),
        target: options.target.clone(),
        iterations: options.iterations,
        correctness,
        sample_count,
        architecture_path,
        estimated_cycles,
        scalar_reference_cycles,
        speedup_vs_scalar,
        suggestions: suggestions(&operations, &allocation_report, &bounds_report),
        operations,
        allocation_report,
        bounds_report,
        evidence,
    })
}

pub fn run_perf_report(
    scalar_source: &Path,
    optimized_source: &Path,
    entry: &str,
    optimized_entry: &str,
    input: &Path,
    iterations: usize,
) -> Result<PerfReport, String> {
    let mut comparisons = Vec::new();
    for target in DEFAULT_PERF_TARGETS {
        let scalar = run_bench(&BenchOptions {
            source: scalar_source.to_path_buf(),
            entry: entry.to_string(),
            input: input.to_path_buf(),
            target: (*target).to_string(),
            iterations,
        })?;
        let optimized = run_bench(&BenchOptions {
            source: optimized_source.to_path_buf(),
            entry: optimized_entry.to_string(),
            input: input.to_path_buf(),
            target: (*target).to_string(),
            iterations,
        })?;
        comparisons.push(PerfComparison {
            target: (*target).to_string(),
            scalar_cycles: scalar.estimated_cycles,
            optimized_cycles: optimized.estimated_cycles,
            speedup: ratio(scalar.estimated_cycles, optimized.estimated_cycles),
            architecture_path: optimized.architecture_path,
            correctness: if scalar.correctness == "pass" && optimized.correctness == "pass" {
                "pass".to_string()
            } else {
                "fail".to_string()
            },
            evidence: vec![
                format!("scalar_correctness={}", scalar.correctness),
                format!("optimized_correctness={}", optimized.correctness),
                format!("target_benchmark={target}"),
                "target_specific_evidence=yes".to_string(),
            ],
            allocation_report: optimized.allocation_report,
            bounds_report: optimized.bounds_report,
        });
    }
    let patch_gate = if comparisons
        .iter()
        .all(|comparison| comparison.correctness == "pass")
        && comparisons.len() == DEFAULT_PERF_TARGETS.len()
    {
        "pass"
    } else {
        "fail"
    };
    Ok(PerfReport {
        schema: "forgentex.perf_report.v1".to_string(),
        scalar_source: display_path(scalar_source),
        optimized_source: display_path(optimized_source),
        entry: entry.to_string(),
        optimized_entry: optimized_entry.to_string(),
        input: display_path(input),
        patch_gate: patch_gate.to_string(),
        comparisons,
        suggestions: vec![
            "performance patches must keep correctness tests".to_string(),
            "attach target-specific benchmark evidence for x86_64, AArch64, and RISC-V".to_string(),
        ],
    })
}

impl BenchReport {
    pub fn is_success(&self) -> bool {
        self.correctness == "pass" || self.correctness == "ok"
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex benchmark\nschema={} source={} entry={} target={} correctness={} iterations={}\n  path={} samples={} estimated_cycles={} scalar_reference_cycles={} speedup_vs_scalar={:.2}\n  ops loops={} scalar_ops={} calls={} simd_calls={} indexes={} stores={} lanes={}\n  allocations status={} count={}\n  bounds status={} checks={}\n  evidence=[{}]\n  suggestions=[{}]\n",
            self.schema,
            self.source,
            self.entry,
            self.target,
            self.correctness,
            self.iterations,
            self.architecture_path,
            self.sample_count,
            self.estimated_cycles,
            self.scalar_reference_cycles,
            self.speedup_vs_scalar,
            self.operations.loops,
            self.operations.scalar_ops,
            self.operations.calls,
            self.operations.simd_calls,
            self.operations.index_exprs,
            self.operations.stores,
            self.operations.max_vector_lanes,
            self.allocation_report.status,
            self.allocation_report.allocations,
            self.bounds_report.status,
            self.bounds_report.checks,
            self.evidence.join(","),
            self.suggestions.join(",")
        )
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "source", &quoted(&self.source), true);
        push_prop(&mut out, 1, "entry", &quoted(&self.entry), true);
        push_prop(&mut out, 1, "target", &quoted(&self.target), true);
        push_prop(
            &mut out,
            1,
            "iterations",
            &self.iterations.to_string(),
            true,
        );
        push_prop(&mut out, 1, "correctness", &quoted(&self.correctness), true);
        push_prop(
            &mut out,
            1,
            "sample_count",
            &self.sample_count.to_string(),
            true,
        );
        push_prop(
            &mut out,
            1,
            "architecture_path",
            &quoted(&self.architecture_path),
            true,
        );
        push_prop(
            &mut out,
            1,
            "estimated_cycles",
            &self.estimated_cycles.to_string(),
            true,
        );
        push_prop(
            &mut out,
            1,
            "scalar_reference_cycles",
            &self.scalar_reference_cycles.to_string(),
            true,
        );
        push_prop(
            &mut out,
            1,
            "speedup_vs_scalar",
            &format_float(self.speedup_vs_scalar),
            true,
        );
        push_operations(&mut out, &self.operations, true);
        push_allocation_report(&mut out, &self.allocation_report, true);
        push_bounds_report(&mut out, &self.bounds_report, true);
        push_string_array(&mut out, 1, "evidence", &self.evidence, true);
        push_string_array(&mut out, 1, "suggestions", &self.suggestions, false);
        out.push_str("\n}");
        out
    }
}

impl PerfReport {
    pub fn is_success(&self) -> bool {
        self.patch_gate == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex performance report\n");
        out.push_str(&format!(
            "schema={} scalar={} optimized={} gate={}\n",
            self.schema, self.scalar_source, self.optimized_source, self.patch_gate
        ));
        for comparison in &self.comparisons {
            out.push_str(&format!(
                "  - target={} path={} correctness={} scalar_cycles={} optimized_cycles={} speedup={:.2} bounds={} allocations={} evidence=[{}]\n",
                comparison.target,
                comparison.architecture_path,
                comparison.correctness,
                comparison.scalar_cycles,
                comparison.optimized_cycles,
                comparison.speedup,
                comparison.bounds_report.checks,
                comparison.allocation_report.allocations,
                comparison.evidence.join(",")
            ));
        }
        out.push_str(&format!("suggestions=[{}]\n", self.suggestions.join(",")));
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(
            &mut out,
            1,
            "scalar_source",
            &quoted(&self.scalar_source),
            true,
        );
        push_prop(
            &mut out,
            1,
            "optimized_source",
            &quoted(&self.optimized_source),
            true,
        );
        push_prop(&mut out, 1, "entry", &quoted(&self.entry), true);
        push_prop(
            &mut out,
            1,
            "optimized_entry",
            &quoted(&self.optimized_entry),
            true,
        );
        push_prop(&mut out, 1, "input", &quoted(&self.input), true);
        push_prop(&mut out, 1, "patch_gate", &quoted(&self.patch_gate), true);
        push_line(&mut out, 1, "\"comparisons\": [");
        for (idx, comparison) in self.comparisons.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "target", &quoted(&comparison.target), true);
            push_prop(
                &mut out,
                3,
                "architecture_path",
                &quoted(&comparison.architecture_path),
                true,
            );
            push_prop(
                &mut out,
                3,
                "correctness",
                &quoted(&comparison.correctness),
                true,
            );
            push_prop(
                &mut out,
                3,
                "scalar_cycles",
                &comparison.scalar_cycles.to_string(),
                true,
            );
            push_prop(
                &mut out,
                3,
                "optimized_cycles",
                &comparison.optimized_cycles.to_string(),
                true,
            );
            push_prop(
                &mut out,
                3,
                "speedup",
                &format_float(comparison.speedup),
                true,
            );
            push_allocation_report_at(&mut out, 3, &comparison.allocation_report, true);
            push_bounds_report_at(&mut out, 3, &comparison.bounds_report, true);
            push_string_array(&mut out, 3, "evidence", &comparison.evidence, false);
            out.push('\n');
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.comparisons.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_string_array(&mut out, 1, "suggestions", &self.suggestions, false);
        out.push_str("\n}");
        out
    }
}

fn analyze_function(function: &FunctionDecl) -> OperationCounts {
    let mut counts = OperationCounts::default();
    analyze_body(&function.body, &mut counts);
    counts.max_vector_lanes = counts.max_vector_lanes.max(1);
    counts
}

fn analyze_body(body: &[Stmt], counts: &mut OperationCounts) {
    for stmt in body {
        match stmt {
            Stmt::Let { expr, .. } => analyze_expr(expr, counts),
            Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
                analyze_expr(expr, counts);
            }
            Stmt::Return(None) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                analyze_expr(cond, counts);
                analyze_body(then_body, counts);
                analyze_body(else_body, counts);
            }
            Stmt::For {
                start, end, body, ..
            } => {
                counts.loops += 1;
                analyze_expr(start, counts);
                analyze_expr(end, counts);
                analyze_body(body, counts);
            }
            Stmt::While { cond, body } => {
                counts.loops += 1;
                analyze_expr(cond, counts);
                analyze_body(body, counts);
            }
            Stmt::Match { scrutinee, arms } => {
                analyze_expr(scrutinee, counts);
                for arm in arms {
                    analyze_body(&arm.body, counts);
                }
            }
            Stmt::Assign { target, expr } => {
                counts.stores += 1;
                analyze_expr(target, counts);
                analyze_expr(expr, counts);
            }
        }
    }
}

fn analyze_expr(expr: &Expr, counts: &mut OperationCounts) {
    match expr {
        Expr::Lambda { body, .. } => analyze_expr(body, counts),
        Expr::Binary { left, right, .. } => {
            counts.scalar_ops += 1;
            analyze_expr(left, counts);
            analyze_expr(right, counts);
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            counts.scalar_ops += 1;
            analyze_expr(expr, counts);
        }
        Expr::Call { name, args } => {
            counts.calls += 1;
            if is_simd_intrinsic(name) {
                counts.simd_calls += 1;
                counts.max_vector_lanes = counts.max_vector_lanes.max(simd_lanes(name));
                if is_simd_memory_intrinsic(name) {
                    counts.index_exprs += 1;
                }
                if name.contains("_store_") || name.contains("simd_store_") {
                    counts.stores += 1;
                }
            }
            for arg in args {
                analyze_expr(arg, counts);
            }
        }
        Expr::Index { base, index } => {
            counts.index_exprs += 1;
            analyze_expr(base, counts);
            analyze_expr(index, counts);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
    }
}

fn estimate_scalar_cycles(operations: &OperationCounts, sample_count: usize) -> u64 {
    let per_sample = operations.scalar_ops.max(1) as u64 * 3
        + operations.index_exprs as u64 * 2
        + operations.stores as u64 * 2
        + operations.calls as u64 * 4;
    let loop_cost = operations.loops.max(1) as u64 * sample_count as u64;
    per_sample
        .saturating_mul(sample_count.max(1) as u64)
        .saturating_add(loop_cost)
        .max(1)
}

fn estimate_cycles(operations: &OperationCounts, sample_count: usize, target: &str) -> u64 {
    if operations.simd_calls == 0 {
        return estimate_scalar_cycles(operations, sample_count);
    }
    let lanes = operations.max_vector_lanes.max(1);
    let batches = sample_count.div_ceil(lanes).max(1) as u64;
    let simd_weight = match target_by_triple(target).map(|target| target.family) {
        Some("x86_64") => 5,
        Some("aarch64") => 5,
        Some("riscv64") | Some("riscv32") => 6,
        Some("arm") => 7,
        _ => 8,
    };
    let vector_cost = operations.simd_calls.max(1) as u64 * simd_weight * batches;
    let scalar_tail = operations
        .scalar_ops
        .saturating_sub(operations.simd_calls)
        .max(1) as u64
        * batches;
    vector_cost
        .saturating_add(scalar_tail)
        .saturating_add(operations.index_exprs as u64 * batches)
        .saturating_add(operations.stores as u64 * batches)
        .max(1)
}

fn architecture_path(target: &str, simd: bool) -> String {
    if !simd {
        return "scalar.reference".to_string();
    }
    match target_by_triple(target).map(|target| target.family) {
        Some("x86_64") => "x86_64.avx2-review".to_string(),
        Some("aarch64") => "aarch64.neon-review".to_string(),
        Some("riscv64") | Some("riscv32") => "riscv.vector-review".to_string(),
        Some("arm") => "arm.neon-review".to_string(),
        _ => "scalar.reference".to_string(),
    }
}

fn suggestions(
    operations: &OperationCounts,
    allocation_report: &AllocationReport,
    bounds_report: &BoundsCheckReport,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    if bounds_report.checks > 0 {
        suggestions.push("hoist length checks before hot loops".to_string());
    }
    if allocation_report.allocations > 0 {
        suggestions.push("reuse caller-provided buffers to avoid allocations".to_string());
    }
    if operations.simd_calls > 0 {
        suggestions.push("keep scalar fallback and attach target benchmark evidence".to_string());
    } else if operations.loops > 0 {
        suggestions.push("consider portable SIMD patch with equivalence test".to_string());
    }
    suggestions
}

fn estimate_allocations(operations: &OperationCounts) -> usize {
    operations.calls.saturating_sub(operations.simd_calls + 8)
}

fn is_simd_memory_intrinsic(name: &str) -> bool {
    name.contains("_load_")
        || name.contains("simd_load_")
        || name.contains("_store_")
        || name.contains("simd_store_")
}

fn run_status(run_output: &str) -> String {
    for status in ["pass", "ok", "fail"] {
        if run_output.contains(&format!("\"status\": \"{status}\"")) {
            return status.to_string();
        }
    }
    "unknown".to_string()
}

fn sample_count(input: &str) -> Option<usize> {
    ["dst", "values", "samples", "a"]
        .iter()
        .find_map(|name| array_len_for_key(input, name))
}

fn array_len_for_key(input: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let start = input.find(&needle)?;
    let after_key = &input[start + needle.len()..];
    let bracket = after_key.find('[')?;
    let mut depth = 0usize;
    let mut items = 0usize;
    let mut in_value = false;
    for ch in after_key[bracket..].chars() {
        match ch {
            '[' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            ']' => {
                if depth == 1 {
                    return Some(if in_value { items + 1 } else { 0 });
                }
                depth = depth.saturating_sub(1);
            }
            ',' if depth == 1 => {
                items += 1;
                in_value = false;
            }
            ch if depth == 1 && !ch.is_whitespace() => in_value = true,
            _ => {}
        }
    }
    None
}

fn simd_lanes(name: &str) -> usize {
    name.rsplit_once('x')
        .and_then(|(_, lanes)| lanes.parse::<usize>().ok())
        .filter(|lanes| *lanes > 0)
        .unwrap_or(8)
}

fn ratio(left: u64, right: u64) -> f64 {
    if right == 0 {
        0.0
    } else {
        left as f64 / right as f64
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn push_operations(out: &mut String, operations: &OperationCounts, trailing: bool) {
    push_line(out, 1, "\"operations\": {");
    push_prop(out, 2, "loops", &operations.loops.to_string(), true);
    push_prop(
        out,
        2,
        "scalar_ops",
        &operations.scalar_ops.to_string(),
        true,
    );
    push_prop(out, 2, "calls", &operations.calls.to_string(), true);
    push_prop(
        out,
        2,
        "simd_calls",
        &operations.simd_calls.to_string(),
        true,
    );
    push_prop(
        out,
        2,
        "index_exprs",
        &operations.index_exprs.to_string(),
        true,
    );
    push_prop(out, 2, "stores", &operations.stores.to_string(), true);
    push_prop(
        out,
        2,
        "max_vector_lanes",
        &operations.max_vector_lanes.to_string(),
        false,
    );
    push_indent(out, 1);
    out.push('}');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_allocation_report(out: &mut String, report: &AllocationReport, trailing: bool) {
    push_allocation_report_at(out, 1, report, trailing);
}

fn push_allocation_report_at(
    out: &mut String,
    indent: usize,
    report: &AllocationReport,
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str(&format!(
        "\"allocation_report\": {{\"allocations\": {}, \"status\": {}}}",
        report.allocations,
        quoted(&report.status)
    ));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_bounds_report(out: &mut String, report: &BoundsCheckReport, trailing: bool) {
    push_bounds_report_at(out, 1, report, trailing);
}

fn push_bounds_report_at(
    out: &mut String,
    indent: usize,
    report: &BoundsCheckReport,
    trailing: bool,
) {
    push_indent(out, indent);
    out.push_str(&format!(
        "\"bounds_report\": {{\"checks\": {}, \"status\": {}}}",
        report.checks,
        quoted(&report.status)
    ));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_string_array(
    out: &mut String,
    indent: usize,
    key: &str,
    values: &[String],
    trailing: bool,
) {
    push_indent(out, indent);
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
    out.push('\n');
}

fn push_prop(out: &mut String, indent: usize, key: &str, value: &str, trailing: bool) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": {value}"));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_line(out: &mut String, indent: usize, line: &str) {
    push_indent(out, indent);
    out.push_str(line);
    out.push('\n');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
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

fn format_float(value: f64) -> String {
    format!("{value:.2}")
}
