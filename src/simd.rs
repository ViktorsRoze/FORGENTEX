use std::collections::BTreeSet;

use crate::ast::{Expr, FunctionDecl, Program, Stmt, Type};
use crate::diag::{diagnostics_array_to_json, Diagnostic};
use crate::stdlib::canonical_simd_intrinsic;
use crate::targets::{target_by_triple, TargetSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorShape {
    pub elem: Type,
    pub lanes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimdSummary {
    pub id: String,
    pub name: String,
    pub intrinsics: Vec<String>,
    pub vector_types: Vec<String>,
    pub scalar_fallback: bool,
    pub target_only: bool,
    pub equivalence_test: bool,
    pub lowering: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimdReview {
    pub target: String,
    pub diagnostics: Vec<Diagnostic>,
    pub functions: Vec<SimdSummary>,
}

pub fn is_simd_intrinsic(name: &str) -> bool {
    canonical_simd_intrinsic(name).is_some()
}

pub fn vector_shape(ty: &Type) -> Option<VectorShape> {
    let Type::Generic { name, args } = ty else {
        return None;
    };
    if name != "vec" || args.len() != 2 {
        return None;
    }
    let lanes = lane_count_type(&args[1])?;
    Some(VectorShape {
        elem: args[0].clone(),
        lanes,
    })
}

pub fn mask_lanes(ty: &Type) -> Option<usize> {
    let Type::Generic { name, args } = ty else {
        return None;
    };
    if name != "mask" || args.len() != 1 {
        return None;
    }
    lane_count_type(&args[0])
}

pub fn check_intrinsic_call(name: &str, args: &[Type], node: &str) -> (Type, Vec<Diagnostic>) {
    let canonical = canonical_simd_intrinsic(name).unwrap_or_else(|| name.to_string());
    let name = canonical.as_str();
    if let Some(shape) = parse_typed_vector_suffix(name.strip_prefix("simd_load_").unwrap_or("")) {
        return check_load(name, &shape, args, node);
    }
    if let Some(shape) = parse_typed_vector_suffix(name.strip_prefix("simd_store_").unwrap_or("")) {
        return check_store(name, &shape, args, node);
    }
    if let Some(shape) = parse_typed_vector_suffix(name.strip_prefix("simd_vec_").unwrap_or("")) {
        return check_vec_constructor(name, &shape, args, node);
    }
    if let Some(shape) = parse_typed_vector_suffix(name.strip_prefix("simd_splat_").unwrap_or("")) {
        return check_splat(name, &shape, args, node);
    }
    if let Some((source, target)) =
        parse_transform_suffix(name.strip_prefix("simd_widen_").unwrap_or(""))
    {
        return check_transform(name, &source, &target, args, node);
    }
    if let Some((source, target)) =
        parse_transform_suffix(name.strip_prefix("simd_narrow_").unwrap_or(""))
    {
        return check_transform(name, &source, &target, args, node);
    }

    match name {
        "simd_add" | "simd_mul" | "simd_saturating_add" => check_binary_vector(name, args, node),
        "simd_eq" => check_compare(name, args, node),
        "simd_select" => check_select(name, args, node),
        "simd_reduce_add" | "simd_reduce_mul" => check_reduce(name, args, node),
        "simd_shuffle" => check_shuffle(name, args, node),
        _ => (
            Type::Unknown,
            vec![Diagnostic::new(
                "simd_unknown_intrinsic",
                node,
                format!("unknown SIMD intrinsic '{name}'"),
            )
            .suggest("replace_simd_intrinsic")],
        ),
    }
}

pub fn check_simd_fallbacks(program: &Program, triple: &str) -> Vec<Diagnostic> {
    if target_by_triple(triple).is_none() {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    for function in program.functions() {
        if !body_has_simd(&function.body) || is_target_only(function) {
            continue;
        }
        if !has_scalar_fallback(program, function) {
            diagnostics.push(
                Diagnostic::new(
                    "simd_fallback_required",
                    &function.id,
                    format!(
                        "SIMD function '{}' requires a scalar fallback or explicit target-only marker",
                        function.name
                    ),
                )
                .suggest("add_scalar_fallback")
                .suggest("mark_target_only"),
            );
        }
    }
    diagnostics
}

pub fn analyze_simd(program: &Program, triple: &str) -> SimdReview {
    let target = target_by_triple(triple);
    let mut diagnostics = if target.is_some() {
        check_simd_fallbacks(program, triple)
    } else {
        vec![Diagnostic::new(
            "unknown_target",
            "@program",
            format!("unknown target triple '{triple}'"),
        )
        .suggest("run_fgx_targets")]
    };
    let mut functions = Vec::new();
    for function in program.functions() {
        let mut intrinsics = BTreeSet::new();
        let mut vector_types = BTreeSet::new();
        collect_simd_summary(&function.body, &mut intrinsics, &mut vector_types);
        if intrinsics.is_empty() {
            continue;
        }
        let scalar_fallback = has_scalar_fallback(program, function);
        let target_only = is_target_only(function);
        let equivalence_test = has_equivalence_test(program, function);
        if !equivalence_test {
            diagnostics.push(
                Diagnostic::new(
                    "simd_equivalence_test_missing",
                    &function.id,
                    format!(
                        "SIMD function '{}' has no scalar/vector equivalence test marker",
                        function.name
                    ),
                )
                .suggest("add_simd_equivalence_test"),
            );
        }
        functions.push(SimdSummary {
            id: function.id.clone(),
            name: function.name.clone(),
            intrinsics: intrinsics.into_iter().collect(),
            vector_types: vector_types.into_iter().collect(),
            scalar_fallback,
            target_only,
            equivalence_test,
            lowering: target
                .map(|target| lowering_plan(target, function))
                .unwrap_or_default(),
        });
    }
    SimdReview {
        target: triple.to_string(),
        diagnostics,
        functions,
    }
}

pub fn review_text(review: &SimdReview) -> String {
    let mut out = String::new();
    out.push_str(&format!("SIMD review for {}\n", review.target));
    if review.functions.is_empty() {
        out.push_str("  - No SIMD intrinsics found.\n");
        return out;
    }
    for function in &review.functions {
        out.push_str(&format!(
            "  - {} intrinsics=[{}] vectors=[{}] scalar_fallback={} target_only={} equivalence_tests={} lowering=[{}]\n",
            function.id,
            function.intrinsics.join(","),
            function.vector_types.join(","),
            yes_no(function.scalar_fallback),
            yes_no(function.target_only),
            yes_no(function.equivalence_test),
            function.lowering.join(",")
        ));
    }
    out
}

pub fn review_json(review: &SimdReview) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.simd_review.v1\",\n");
    out.push_str(&format!("  \"target\": {},\n", quoted(&review.target)));
    out.push_str("  \"functions\": [");
    for (idx, function) in review.functions.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"id\": {},\n      \"name\": {},",
            quoted(&function.id),
            quoted(&function.name)
        ));
        push_string_array(&mut out, 6, "intrinsics", &function.intrinsics, true);
        push_string_array(&mut out, 6, "vector_types", &function.vector_types, true);
        out.push_str(&format!(
            "\n      \"scalar_fallback\": {},\n      \"target_only\": {},\n      \"equivalence_tests\": {},",
            bool_json(function.scalar_fallback),
            bool_json(function.target_only),
            bool_json(function.equivalence_test)
        ));
        push_string_array(&mut out, 6, "lowering", &function.lowering, false);
        out.push_str("\n    }");
    }
    if !review.functions.is_empty() {
        out.push('\n');
        out.push_str("  ");
    }
    out.push_str("],\n");
    let diagnostics = diagnostics_array_to_json(&review.diagnostics);
    out.push_str(&format!("  \"diagnostics\": {diagnostics}\n"));
    out.push('}');
    out
}

fn check_binary_vector(name: &str, args: &[Type], node: &str) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 2, node);
    let Some(left) = args.first().and_then(vector_shape) else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    let Some(right) = args.get(1).and_then(vector_shape) else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    if left != right {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                format!("{name} requires matching vector element types and lane counts"),
            )
            .with_types(&vec_type(&left), &vec_type(&right))
            .suggest("replace_vector_argument"),
        );
    }
    if name == "simd_saturating_add" && !left.elem.is_integer() {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                "simd_saturating_add requires integer vector elements",
            )
            .with_types(&Type::Named("integer vector".to_string()), &vec_type(&left))
            .suggest("replace_vector_element_type"),
        );
    }
    (vec_type(&left), diagnostics)
}

fn check_compare(name: &str, args: &[Type], node: &str) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 2, node);
    let Some(left) = args.first().and_then(vector_shape) else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    let Some(right) = args.get(1).and_then(vector_shape) else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    if left != right {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                "simd_eq requires matching vector types",
            )
            .with_types(&vec_type(&left), &vec_type(&right))
            .suggest("replace_vector_argument"),
        );
    }
    (
        Type::Generic {
            name: "mask".to_string(),
            args: vec![Type::Named(left.lanes.to_string())],
        },
        diagnostics,
    )
}

fn check_select(name: &str, args: &[Type], node: &str) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 3, node);
    let mask = args.first().and_then(mask_lanes);
    let left = args.get(1).and_then(vector_shape);
    let right = args.get(2).and_then(vector_shape);
    let Some(left) = left else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    let Some(right) = right else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    if left != right || mask != Some(left.lanes) {
        diagnostics.push(
            Diagnostic::new(
                "simd_lane_mismatch",
                node,
                "simd_select requires mask and vector lane counts to match",
            )
            .suggest("replace_mask_or_vector_argument"),
        );
    }
    (vec_type(&left), diagnostics)
}

fn check_reduce(name: &str, args: &[Type], node: &str) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 1, node);
    let Some(vector) = args.first().and_then(vector_shape) else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    (vector.elem, diagnostics)
}

fn check_shuffle(name: &str, args: &[Type], node: &str) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if args.len() < 2 {
        diagnostics.push(
            Diagnostic::new(
                "simd_intrinsic_arity",
                node,
                "simd_shuffle expects a vector and at least one lane index",
            )
            .suggest("fix_simd_arguments"),
        );
    }
    let Some(vector) = args.first().and_then(vector_shape) else {
        diagnostics.push(simd_requires_vector(name, node));
        return (Type::Unknown, diagnostics);
    };
    (vec_type(&vector), diagnostics)
}

fn check_load(
    name: &str,
    shape: &VectorShape,
    args: &[Type],
    node: &str,
) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 2, node);
    let source_elem = args.first().and_then(indexable_element_type);
    if source_elem.as_ref() != Some(&shape.elem) {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                format!("{name} requires a slice, array, or pointer with matching element type"),
            )
            .with_types(&shape.elem, source_elem.as_ref().unwrap_or(&Type::Unknown))
            .suggest("replace_load_source"),
        );
    }
    check_index_arg(name, args.get(1), node, &mut diagnostics);
    (vec_type(shape), diagnostics)
}

fn check_store(
    name: &str,
    shape: &VectorShape,
    args: &[Type],
    node: &str,
) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 3, node);
    let destination = args.first();
    let destination_elem = destination.and_then(indexable_element_type);
    if destination_elem.as_ref() != Some(&shape.elem) {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                format!("{name} requires a destination with matching element type"),
            )
            .with_types(
                &shape.elem,
                destination_elem.as_ref().unwrap_or(&Type::Unknown),
            )
            .suggest("replace_store_destination"),
        );
    }
    if !destination.is_some_and(is_mutable_index_target_type) {
        diagnostics.push(
            Diagnostic::new(
                "simd_requires_mutable_slice",
                node,
                format!("{name} requires a mutable slice or mutable pointer destination"),
            )
            .suggest("change_destination_to_mutable_slice"),
        );
    }
    check_index_arg(name, args.get(1), node, &mut diagnostics);
    if args.get(2).and_then(vector_shape).as_ref() != Some(shape) {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                format!("{name} requires a matching vector value"),
            )
            .with_types(&vec_type(shape), args.get(2).unwrap_or(&Type::Unknown))
            .suggest("replace_vector_argument"),
        );
    }
    (Type::Void, diagnostics)
}

fn check_vec_constructor(
    name: &str,
    shape: &VectorShape,
    args: &[Type],
    node: &str,
) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, shape.lanes, node);
    for arg in args {
        if !is_scalar_assignable(&shape.elem, arg) {
            diagnostics.push(
                Diagnostic::new(
                    "simd_type_mismatch",
                    node,
                    format!("{name} lane value does not match vector element type"),
                )
                .with_types(&shape.elem, arg)
                .suggest("replace_lane_value"),
            );
        }
    }
    (vec_type(shape), diagnostics)
}

fn check_splat(
    name: &str,
    shape: &VectorShape,
    args: &[Type],
    node: &str,
) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 1, node);
    if let Some(arg) = args.first() {
        if !is_scalar_assignable(&shape.elem, arg) {
            diagnostics.push(
                Diagnostic::new(
                    "simd_type_mismatch",
                    node,
                    format!("{name} scalar does not match vector element type"),
                )
                .with_types(&shape.elem, arg)
                .suggest("replace_splat_value"),
            );
        }
    }
    (vec_type(shape), diagnostics)
}

fn check_transform(
    name: &str,
    source: &VectorShape,
    target: &VectorShape,
    args: &[Type],
    node: &str,
) -> (Type, Vec<Diagnostic>) {
    let mut diagnostics = arity(name, args, 1, node);
    if args.first().and_then(vector_shape).as_ref() != Some(source) {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                format!("{name} source vector type does not match intrinsic suffix"),
            )
            .with_types(&vec_type(source), args.first().unwrap_or(&Type::Unknown))
            .suggest("replace_vector_argument"),
        );
    }
    (vec_type(target), diagnostics)
}

fn arity(name: &str, args: &[Type], expected: usize, node: &str) -> Vec<Diagnostic> {
    if args.len() == expected {
        Vec::new()
    } else {
        vec![Diagnostic::new(
            "simd_intrinsic_arity",
            node,
            format!("{name} expects {expected} argument(s), got {}", args.len()),
        )
        .suggest("fix_simd_arguments")]
    }
}

fn simd_requires_vector(name: &str, node: &str) -> Diagnostic {
    Diagnostic::new(
        "simd_requires_vector",
        node,
        format!("{name} requires vector arguments"),
    )
    .suggest("replace_with_vector_argument")
}

fn check_index_arg(name: &str, arg: Option<&Type>, node: &str, diagnostics: &mut Vec<Diagnostic>) {
    let usize_ty = Type::Primitive("usize".to_string());
    let Some(arg) = arg else {
        return;
    };
    if !is_scalar_assignable(&usize_ty, arg) {
        diagnostics.push(
            Diagnostic::new(
                "simd_type_mismatch",
                node,
                format!("{name} index must be usize-compatible"),
            )
            .with_types(&usize_ty, arg)
            .suggest("replace_index_expression"),
        );
    }
}

fn parse_typed_vector_suffix(suffix: &str) -> Option<VectorShape> {
    let (elem, lanes) = suffix.rsplit_once('x')?;
    if elem.is_empty() {
        return None;
    }
    let lanes = lanes.parse::<usize>().ok()?;
    if lanes == 0 {
        return None;
    }
    Some(VectorShape {
        elem: Type::parse(elem),
        lanes,
    })
}

fn parse_transform_suffix(suffix: &str) -> Option<(VectorShape, VectorShape)> {
    let (source, target) = suffix.split_once('_')?;
    Some((
        parse_typed_vector_suffix(source)?,
        parse_typed_vector_suffix(target)?,
    ))
}

fn lane_count_type(ty: &Type) -> Option<usize> {
    match ty {
        Type::Named(value) | Type::Primitive(value) => value.parse::<usize>().ok(),
        _ => None,
    }
}

fn vec_type(shape: &VectorShape) -> Type {
    Type::Generic {
        name: "vec".to_string(),
        args: vec![shape.elem.clone(), Type::Named(shape.lanes.to_string())],
    }
}

fn is_scalar_assignable(expected: &Type, found: &Type) -> bool {
    expected == found
        || matches!(found, Type::Primitive(name) if name == "$int")
            && (expected.is_integer() || expected.is_float())
        || matches!(found, Type::Primitive(name) if name == "$float") && expected.is_float()
        || *found == Type::Unknown
}

fn indexable_element_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array { elem, .. } | Type::Slice { elem, .. } | Type::Pointer { elem, .. } => {
            Some((**elem).clone())
        }
        _ => None,
    }
}

fn is_mutable_index_target_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Slice { mutable: true, .. } | Type::Pointer { mutable: true, .. }
    )
}

fn body_has_simd(body: &[Stmt]) -> bool {
    body.iter().any(stmt_has_simd)
}

fn stmt_has_simd(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { expr, .. }
        | Stmt::Return(Some(expr))
        | Stmt::Require(expr)
        | Stmt::Expr(expr) => expr_has_simd(expr),
        Stmt::Return(None) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => false,
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => expr_has_simd(cond) || body_has_simd(then_body) || body_has_simd(else_body),
        Stmt::For {
            start, end, body, ..
        } => expr_has_simd(start) || expr_has_simd(end) || body_has_simd(body),
        Stmt::While { cond, body } => expr_has_simd(cond) || body_has_simd(body),
        Stmt::Match { scrutinee, arms } => {
            expr_has_simd(scrutinee) || arms.iter().any(|arm| body_has_simd(&arm.body))
        }
        Stmt::Assign { target, expr } => expr_has_simd(target) || expr_has_simd(expr),
    }
}

fn expr_has_simd(expr: &Expr) -> bool {
    match expr {
        Expr::Lambda { body, .. } => expr_has_simd(body),
        Expr::Call { name, args } => is_simd_intrinsic(name) || args.iter().any(expr_has_simd),
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            expr_has_simd(expr)
        }
        Expr::Binary { left, right, .. } => expr_has_simd(left) || expr_has_simd(right),
        Expr::Index { base, index } => expr_has_simd(base) || expr_has_simd(index),
        Expr::SliceRange { base, start, end } => {
            expr_has_simd(base) || expr_has_simd(start) || expr_has_simd(end)
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => false,
    }
}

fn collect_simd_summary(
    body: &[Stmt],
    intrinsics: &mut BTreeSet<String>,
    vector_types: &mut BTreeSet<String>,
) {
    for stmt in body {
        match stmt {
            Stmt::Let { ty, expr, .. } => {
                if let Some(ty) = ty {
                    if vector_shape(ty).is_some() || mask_lanes(ty).is_some() {
                        vector_types.insert(ty.to_agent());
                    }
                }
                collect_expr_simd(expr, intrinsics, vector_types);
            }
            Stmt::Return(Some(expr)) | Stmt::Require(expr) | Stmt::Expr(expr) => {
                collect_expr_simd(expr, intrinsics, vector_types);
            }
            Stmt::Return(None) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => {}
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                collect_expr_simd(cond, intrinsics, vector_types);
                collect_simd_summary(then_body, intrinsics, vector_types);
                collect_simd_summary(else_body, intrinsics, vector_types);
            }
            Stmt::For {
                start, end, body, ..
            } => {
                collect_expr_simd(start, intrinsics, vector_types);
                collect_expr_simd(end, intrinsics, vector_types);
                collect_simd_summary(body, intrinsics, vector_types);
            }
            Stmt::While { cond, body } => {
                collect_expr_simd(cond, intrinsics, vector_types);
                collect_simd_summary(body, intrinsics, vector_types);
            }
            Stmt::Match { scrutinee, arms } => {
                collect_expr_simd(scrutinee, intrinsics, vector_types);
                for arm in arms {
                    collect_simd_summary(&arm.body, intrinsics, vector_types);
                }
            }
            Stmt::Assign { target, expr } => {
                collect_expr_simd(target, intrinsics, vector_types);
                collect_expr_simd(expr, intrinsics, vector_types);
            }
        }
    }
}

fn collect_expr_simd(
    expr: &Expr,
    intrinsics: &mut BTreeSet<String>,
    vector_types: &mut BTreeSet<String>,
) {
    match expr {
        Expr::Lambda { body, .. } => collect_expr_simd(body, intrinsics, vector_types),
        Expr::Call { name, args } => {
            if is_simd_intrinsic(name) {
                intrinsics.insert(name.clone());
                if let Some(shape) = intrinsic_declared_shape(name) {
                    vector_types.insert(vec_type(&shape).to_agent());
                }
            }
            for arg in args {
                collect_expr_simd(arg, intrinsics, vector_types);
            }
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { base: expr, .. } => {
            collect_expr_simd(expr, intrinsics, vector_types);
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_simd(left, intrinsics, vector_types);
            collect_expr_simd(right, intrinsics, vector_types);
        }
        Expr::Index { base, index } => {
            collect_expr_simd(base, intrinsics, vector_types);
            collect_expr_simd(index, intrinsics, vector_types);
        }
        Expr::SliceRange { base, start, end } => {
            collect_expr_simd(base, intrinsics, vector_types);
            collect_expr_simd(start, intrinsics, vector_types);
            collect_expr_simd(end, intrinsics, vector_types);
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::TraitConst { .. } => {}
    }
}

fn intrinsic_declared_shape(name: &str) -> Option<VectorShape> {
    let canonical = canonical_simd_intrinsic(name)?;
    parse_typed_vector_suffix(canonical.strip_prefix("simd_load_").unwrap_or(""))
        .or_else(|| parse_typed_vector_suffix(canonical.strip_prefix("simd_store_").unwrap_or("")))
        .or_else(|| parse_typed_vector_suffix(canonical.strip_prefix("simd_vec_").unwrap_or("")))
        .or_else(|| parse_typed_vector_suffix(canonical.strip_prefix("simd_splat_").unwrap_or("")))
}

fn has_scalar_fallback(program: &Program, function: &FunctionDecl) -> bool {
    if has_in_body_scalar_fallback(&function.body) {
        return true;
    }
    fallback_names(&function.name)
        .iter()
        .any(|candidate| program.find_function(candidate).is_some())
}

fn has_in_body_scalar_fallback(body: &[Stmt]) -> bool {
    for (idx, stmt) in body.iter().enumerate() {
        if matches!(stmt, Stmt::If { then_body, .. } if body_has_simd(then_body))
            && body[idx + 1..].iter().any(stmt_is_scalar_work)
        {
            return true;
        }
        match stmt {
            Stmt::If {
                then_body,
                else_body,
                ..
            } if has_in_body_scalar_fallback(then_body)
                || has_in_body_scalar_fallback(else_body) =>
            {
                return true
            }
            Stmt::For { body, .. } if has_in_body_scalar_fallback(body) => return true,
            _ => {}
        }
    }
    false
}

fn stmt_is_scalar_work(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(expr) => !expr_has_simd(expr),
        Stmt::For { .. } | Stmt::While { .. } | Stmt::Assign { .. } | Stmt::Return(_) => true,
        Stmt::Let { expr, .. } => !expr_has_simd(expr),
        Stmt::If { then_body, .. } => then_body.iter().any(stmt_is_scalar_work),
        Stmt::Match { arms, .. } => arms
            .iter()
            .any(|arm| arm.body.iter().any(stmt_is_scalar_work)),
        Stmt::Require(_) | Stmt::Asm(_) | Stmt::Break | Stmt::Continue => false,
    }
}

fn fallback_names(name: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(base) = name.strip_suffix("_simd") {
        names.push(format!("{base}_scalar"));
        names.push(base.to_string());
    }
    names.push(format!("{name}_scalar"));
    names
}

fn is_target_only(function: &FunctionDecl) -> bool {
    function
        .effects
        .iter()
        .any(|effect| effect == "target.only")
}

fn has_equivalence_test(program: &Program, function: &FunctionDecl) -> bool {
    let candidates = [
        format!("test_{}_equivalence", function.name),
        format!("{}_equivalence_test", function.name),
        format!("{}_equivalence", function.name),
    ];
    program.functions().any(|candidate| {
        let name = candidate.name.as_str();
        candidates.iter().any(|expected| expected == name)
            || (name.contains("equivalence") && name.contains(&function.name))
    })
}

fn lowering_plan(target: &TargetSpec, function: &FunctionDecl) -> Vec<String> {
    let mut plan = match target.family {
        "x86_64" => vec![
            "x86_64.sse2".to_string(),
            "x86_64.avx2-if-available".to_string(),
        ],
        "arm" => vec!["armv7.neon-if-available".to_string()],
        "aarch64" => vec![
            "aarch64.neon".to_string(),
            "aarch64.sve-if-available".to_string(),
        ],
        "riscv32" => vec![
            "riscv32.scalar-default".to_string(),
            "riscv32.v-if-cpu.v".to_string(),
        ],
        "riscv64" => vec![
            "riscv64.scalar-default".to_string(),
            "riscv64.v-if-cpu.v".to_string(),
        ],
        _ => Vec::new(),
    };
    if !is_target_only(function) {
        plan.push("scalar-fallback".to_string());
    }
    plan
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
