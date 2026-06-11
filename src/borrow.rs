use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{Expr, FunctionDecl, Item, Program, Stmt, Type};
use crate::diag::Diagnostic;
use crate::lifetimes::{analyze_contracts, LifetimeContract, LifetimeParam};
use crate::parser::parse_file;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq)]
pub struct BorrowReport {
    pub schema: String,
    pub file: String,
    pub status: String,
    pub diagnostics: Vec<Diagnostic>,
    pub rules: Vec<BorrowRule>,
    pub lifetime_contracts: Vec<LifetimeContract>,
    pub regions: Vec<BorrowRegion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowRule {
    pub id: String,
    pub status: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowRegion {
    pub id: String,
    pub function: String,
    pub root: String,
    pub mutable: bool,
    pub loans: Vec<BorrowLoan>,
    pub uses: Vec<BorrowUse>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowLoan {
    pub id: String,
    pub binding: String,
    pub source: String,
    pub origin: String,
    pub mutable: bool,
    pub ty: String,
    pub node: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowUse {
    pub node: String,
    pub kind: String,
    pub place: String,
    pub root: String,
}

pub fn analyze_borrows(path: &Path) -> Result<BorrowReport, String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let program = parse_file(path, &source)?;
    let diagnostics = check(&program)
        .into_iter()
        .filter(|diagnostic| is_borrow_diagnostic(&diagnostic.code))
        .collect::<Vec<_>>();
    let status = if diagnostics.is_empty() {
        "pass"
    } else {
        "fail"
    };
    Ok(BorrowReport {
        schema: "forgentex.borrow_report.v1".to_string(),
        file: display_path(path),
        status: status.to_string(),
        diagnostics,
        rules: borrow_rules(),
        lifetime_contracts: analyze_contracts(&program),
        regions: analyze_regions(&program),
    })
}

impl BorrowReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex borrow report\n");
        out.push_str(&format!(
            "schema={} file={} status={} diagnostics={}\n",
            self.schema,
            self.file,
            self.status,
            self.diagnostics.len()
        ));
        out.push_str("Rules:\n");
        for rule in &self.rules {
            out.push_str(&format!(
                "  - {} status={} {}\n",
                rule.id, rule.status, rule.description
            ));
        }
        out.push_str("Lifetime contracts:\n");
        for contract in &self.lifetime_contracts {
            out.push_str(&format!(
                "  - {} returns=[{}] mutates=[{}]\n",
                contract.function,
                lifetime_param_labels(&contract.returns).join(","),
                lifetime_param_labels(&contract.mutates).join(",")
            ));
        }
        out.push_str("Regions:\n");
        for region in &self.regions {
            out.push_str(&format!(
                "  - {} function={} root={} mutable={} loans={} uses={}\n",
                region.id,
                region.function,
                region.root,
                region.mutable,
                region.loans.len(),
                region.uses.len()
            ));
            for loan in &region.loans {
                out.push_str(&format!(
                    "    loan {} binding={} source={} origin={} mutable={} type={} node={}\n",
                    loan.id,
                    loan.binding,
                    loan.source,
                    loan.origin,
                    loan.mutable,
                    loan.ty,
                    loan.node
                ));
            }
            for borrow_use in &region.uses {
                out.push_str(&format!(
                    "    use kind={} place={} root={} node={}\n",
                    borrow_use.kind, borrow_use.place, borrow_use.root, borrow_use.node
                ));
            }
        }
        if !self.diagnostics.is_empty() {
            out.push_str("Diagnostics:\n");
            for diagnostic in &self.diagnostics {
                out.push_str(&format!(
                    "  - {} node={} message=\"{}\" suggestions=[{}]\n",
                    diagnostic.code,
                    diagnostic.node,
                    diagnostic.message,
                    diagnostic.suggested_patches.join(",")
                ));
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "file", &quoted(&self.file), true);
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_line(&mut out, 1, "\"rules\": [");
        for (idx, rule) in self.rules.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "id", &quoted(&rule.id), true);
            push_prop(&mut out, 3, "status", &quoted(&rule.status), true);
            push_prop(
                &mut out,
                3,
                "description",
                &quoted(&rule.description),
                false,
            );
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.rules.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"lifetime_contracts\": [");
        for (idx, contract) in self.lifetime_contracts.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_lifetime_contract(&mut out, contract);
        }
        if !self.lifetime_contracts.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"regions\": [");
        for (idx, region) in self.regions.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_region(&mut out, region);
        }
        if !self.regions.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "],");
        push_line(&mut out, 1, "\"diagnostics\": [");
        for (idx, diagnostic) in self.diagnostics.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_line(&mut out, 2, "{");
            push_prop(&mut out, 3, "error", &quoted(&diagnostic.code), true);
            push_prop(&mut out, 3, "node", &quoted(&diagnostic.node), true);
            push_prop(&mut out, 3, "message", &quoted(&diagnostic.message), true);
            if let Some(expected) = &diagnostic.expected {
                push_prop(&mut out, 3, "expected", &quoted(expected), true);
            }
            if let Some(found) = &diagnostic.found {
                push_prop(&mut out, 3, "found", &quoted(found), true);
            }
            push_string_array(
                &mut out,
                3,
                "suggested_patches",
                &diagnostic.suggested_patches,
                false,
            );
            push_indent(&mut out, 2);
            out.push('}');
        }
        if !self.diagnostics.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "]");
        out.push('}');
        out
    }
}

fn borrow_rules() -> Vec<BorrowRule> {
    vec![
        rule(
            "lifetime_region_graph",
            "enabled",
            "borrow-report records function-local borrow roots, loans, and use sites",
        ),
        rule(
            "non_lexical_borrow_endings",
            "enabled",
            "simple local borrow aliases stop blocking writes after their last use",
        ),
        rule(
            "loop_carried_borrow_liveness",
            "enabled",
            "borrow aliases from outside a loop remain live across loop back-edges when the loop body reads them",
        ),
        rule(
            "conditional_borrow_state_join",
            "enabled",
            "borrow aliases reassigned in conditional branches keep all possible roots after the branch",
        ),
        rule(
            "reborrows_and_two_phase_access",
            "enabled",
            "local mutable reborrows form parent-child chains and mutable call arguments activate after reservation",
        ),
        rule(
            "place_projection_alias_analysis",
            "enabled",
            "field and constant-index borrow places prove obvious disjointness and keep overlapping projections rejected",
        ),
        rule(
            "interprocedural_lifetime_contracts",
            "enabled",
            "function contracts record returned borrow parameters and mutating borrow parameters across helper calls",
        ),
        rule(
            "moves",
            "enabled",
            "owned values move into owned destinations and cannot be reused",
        ),
        rule(
            "conditional_move_merge",
            "enabled",
            "moves inside if bodies and loop bodies propagate to later reads",
        ),
        rule(
            "field_move_paths",
            "enabled",
            "owned struct field moves are tracked separately and can be reinitialized",
        ),
        rule(
            "borrow_return_provenance",
            "enabled",
            "borrow-return calls preserve parameter-origin lifetime evidence when provable",
        ),
        rule(
            "conditional_borrow_return_join",
            "enabled",
            "conditional borrow returns can join multiple proven parameter origins",
        ),
        rule(
            "lexical_borrow_alias_locks",
            "enabled",
            "live local borrow aliases block writes through other handles to the same root",
        ),
        rule(
            "borrow_alias_roots",
            "enabled",
            "local borrow aliases resolve to their original root for call alias checks",
        ),
        rule(
            "reference_escape",
            "enabled",
            "returned borrows must be tied to parameter-origin references",
        ),
        rule(
            "raw_pointer_unsafe_gate",
            "enabled",
            "raw pointer arguments, returns, and dereferences require unsafe functions",
        ),
        rule(
            "cfg_dataflow_borrowck",
            "enabled",
            "a CFG dataflow pass computes moved paths, alias places, and liveness uniformly over every control-flow shape behind the staged rules",
        ),
    ]
}

fn rule(id: &str, status: &str, description: &str) -> BorrowRule {
    BorrowRule {
        id: id.to_string(),
        status: status.to_string(),
        description: description.to_string(),
    }
}

fn is_borrow_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "use_after_move"
            | "mutable_borrow_alias"
            | "escaping_reference"
            | "raw_pointer_requires_unsafe"
            | "raw_pointer_dereference_requires_unsafe"
            | "immutable_write"
    )
}

fn display_path(path: &Path) -> String {
    PathBuf::from(path).display().to_string().replace('\\', "/")
}

#[derive(Clone, Debug)]
struct BorrowBinding {
    root: String,
    mutable: bool,
    ty: String,
}

struct RegionBuilder {
    function_id: String,
    stmt_counter: usize,
    bindings: HashMap<String, BorrowBinding>,
    regions: Vec<BorrowRegion>,
}

impl RegionBuilder {
    fn new(function: &FunctionDecl) -> Self {
        let mut builder = Self {
            function_id: function.id.clone(),
            stmt_counter: 0,
            bindings: HashMap::new(),
            regions: Vec::new(),
        };
        for param in &function.params {
            if is_borrow_type(&param.ty) {
                let mutable = is_mutable_borrow_type(&param.ty);
                builder.add_loan(
                    &param.name,
                    &param.name,
                    "parameter",
                    mutable,
                    &param.ty.to_human(),
                    format!("{}.param.{}", function.id, param.name),
                );
            }
        }
        builder
    }

    fn analyze(mut self, body: &[Stmt]) -> Vec<BorrowRegion> {
        self.walk_statements(body);
        self.regions
    }

    fn walk_statements(&mut self, body: &[Stmt]) {
        for stmt in body {
            let node = self.next_stmt_node();
            self.walk_stmt(stmt, &node);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt, node: &str) {
        match stmt {
            Stmt::Let { name, ty, expr } => {
                self.collect_expr_uses(expr, node);
                if let Some(ty) = ty.as_ref().filter(|ty| is_borrow_type(ty)) {
                    let mutable = is_mutable_borrow_type(ty);
                    let root = self.borrow_root(expr).unwrap_or_else(|| name.clone());
                    let origin = if expr_place(expr) != root {
                        "reborrow"
                    } else {
                        "let"
                    };
                    self.add_use(&root, "loan_source", &expr_place(expr), node);
                    self.add_loan(
                        name,
                        &root,
                        origin,
                        mutable,
                        &ty.to_human(),
                        node.to_string(),
                    );
                }
            }
            Stmt::Return(Some(expr)) => {
                if let Some(root) = self.borrow_root(expr) {
                    self.add_use(&root, "return", &expr_place(expr), node);
                }
                self.collect_expr_uses(expr, node);
            }
            Stmt::Return(None) => {}
            Stmt::Require(expr) | Stmt::Expr(expr) => self.collect_expr_uses(expr, node),
            Stmt::If { cond, then_body } => {
                self.collect_expr_uses(cond, node);
                self.with_child_scope(|builder| builder.walk_statements(then_body));
            }
            Stmt::For {
                start, end, body, ..
            } => {
                self.collect_expr_uses(start, node);
                self.collect_expr_uses(end, node);
                self.with_child_scope(|builder| builder.walk_statements(body));
            }
            Stmt::Assign { target, expr } => {
                self.collect_assign_target(target, node);
                self.collect_expr_uses(expr, node);
                if let Some(name) = target_var(target) {
                    if let (Some(existing), Some(root)) =
                        (self.bindings.get(name).cloned(), self.borrow_root(expr))
                    {
                        let origin = if expr_place(expr) != root {
                            "reborrow_assignment"
                        } else {
                            "assignment"
                        };
                        self.add_use(&root, "assignment_source", &expr_place(expr), node);
                        self.add_loan(
                            name,
                            &root,
                            origin,
                            existing.mutable,
                            &existing.ty,
                            node.to_string(),
                        );
                    }
                }
            }
            Stmt::Asm(_) => {}
        }
    }

    fn collect_assign_target(&mut self, target: &Expr, node: &str) {
        match target {
            Expr::Index { base, index } => {
                if let Some(root) = self.borrow_root(base) {
                    self.add_use(&root, "index_write", &expr_place(base), node);
                }
                self.collect_expr_uses(index, node);
            }
            Expr::Field { base, .. } => self.collect_assign_target(base, node),
            Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => {
                self.collect_assign_target(expr, node)
            }
            Expr::Var(name) => {
                if let Some(binding) = self.bindings.get(name).cloned() {
                    self.add_use(&binding.root, "assignment_target", name, node);
                }
            }
            _ => self.collect_expr_uses(target, node),
        }
    }

    fn collect_expr_uses(&mut self, expr: &Expr, node: &str) {
        match expr {
            Expr::Var(name) => {
                if let Some(binding) = self.bindings.get(name).cloned() {
                    self.add_use(&binding.root, "read", name, node);
                }
            }
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => {
                self.collect_expr_uses(expr, node)
            }
            Expr::Binary { left, right, .. } => {
                self.collect_expr_uses(left, node);
                self.collect_expr_uses(right, node);
            }
            Expr::Call { name, args } => {
                for (idx, arg) in args.iter().enumerate() {
                    if let Some(root) = self.borrow_root(arg) {
                        self.add_use(
                            &root,
                            &format!("call_arg:{name}:{idx}"),
                            &expr_place(arg),
                            node,
                        );
                    }
                    self.collect_expr_uses(arg, node);
                }
            }
            Expr::Field { base, .. } => self.collect_expr_uses(base, node),
            Expr::Index { base, index } => {
                if let Some(root) = self.borrow_root(base) {
                    self.add_use(&root, "index_read", &expr_place(base), node);
                }
                self.collect_expr_uses(base, node);
                self.collect_expr_uses(index, node);
            }
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => {}
        }
    }

    fn with_child_scope(&mut self, f: impl FnOnce(&mut Self)) {
        let saved = self.bindings.clone();
        f(self);
        self.bindings = saved;
    }

    fn borrow_root(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Var(name) => self.bindings.get(name).map(|binding| binding.root.clone()),
            Expr::Field { base, .. } | Expr::Index { base, .. } => self.borrow_root(base),
            Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => self.borrow_root(expr),
            Expr::Call { .. }
            | Expr::Binary { .. }
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_) => None,
        }
    }

    fn add_loan(
        &mut self,
        binding: &str,
        root: &str,
        origin: &str,
        mutable: bool,
        ty: &str,
        node: String,
    ) {
        let region_idx = self.region_index(root, mutable);
        let loan_id = format!(
            "{}.loan.{}.{}",
            self.function_id,
            self.regions[region_idx].loans.len() + 1,
            sanitize_id(binding)
        );
        self.regions[region_idx].loans.push(BorrowLoan {
            id: loan_id,
            binding: binding.to_string(),
            source: root.to_string(),
            origin: origin.to_string(),
            mutable,
            ty: ty.to_string(),
            node,
        });
        if mutable {
            self.regions[region_idx].mutable = true;
        }
        self.bindings.insert(
            binding.to_string(),
            BorrowBinding {
                root: root.to_string(),
                mutable,
                ty: ty.to_string(),
            },
        );
    }

    fn add_use(&mut self, root: &str, kind: &str, place: &str, node: &str) {
        let mutable = self
            .bindings
            .values()
            .any(|binding| binding.root == root && binding.mutable);
        let region_idx = self.region_index(root, mutable);
        self.regions[region_idx].uses.push(BorrowUse {
            node: node.to_string(),
            kind: kind.to_string(),
            place: place.to_string(),
            root: root.to_string(),
        });
    }

    fn region_index(&mut self, root: &str, mutable: bool) -> usize {
        if let Some(idx) = self.regions.iter().position(|region| region.root == root) {
            if mutable {
                self.regions[idx].mutable = true;
            }
            return idx;
        }
        let id = format!("{}.region.{}", self.function_id, sanitize_id(root));
        self.regions.push(BorrowRegion {
            id,
            function: self.function_id.clone(),
            root: root.to_string(),
            mutable,
            loans: Vec::new(),
            uses: Vec::new(),
        });
        self.regions.len() - 1
    }

    fn next_stmt_node(&mut self) -> String {
        self.stmt_counter += 1;
        format!("{}.stmt{}", self.function_id, self.stmt_counter)
    }
}

fn analyze_regions(program: &Program) -> Vec<BorrowRegion> {
    let mut regions = Vec::new();
    for item in &program.items {
        if let Item::Function(function) = item {
            regions.extend(RegionBuilder::new(function).analyze(&function.body));
        }
    }
    regions
}

fn is_borrow_type(ty: &Type) -> bool {
    matches!(ty, Type::Slice { .. })
}

fn is_mutable_borrow_type(ty: &Type) -> bool {
    matches!(ty, Type::Slice { mutable: true, .. })
}

fn target_var(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var(name) => Some(name),
        Expr::Cast { expr, .. } | Expr::Unary { expr, .. } => target_var(expr),
        _ => None,
    }
}

fn expr_place(expr: &Expr) -> String {
    match expr {
        Expr::Int(value) => value.to_string(),
        Expr::Float(value) => value.to_string(),
        Expr::Bool(value) => value.to_string(),
        Expr::String(value) => format!("\"{}\"", value.replace('"', "\\\"")),
        Expr::Var(name) => name.clone(),
        Expr::Unary { op, expr } => format!("{op}{}", expr_place(expr)),
        Expr::Binary { op, left, right } => {
            format!("({} {op} {})", expr_place(left), expr_place(right))
        }
        Expr::Call { name, args } => {
            let args = args.iter().map(expr_place).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        Expr::Cast { ty, expr } => format!("{} as {}", expr_place(expr), ty.to_human()),
        Expr::Field { base, field } => format!("{}.{}", expr_place(base), field),
        Expr::Index { base, index } => format!("{}[{}]", expr_place(base), expr_place(index)),
    }
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn lifetime_param_labels(params: &[LifetimeParam]) -> Vec<String> {
    params
        .iter()
        .map(|param| format!("{}:{}", param.index, param.name))
        .collect()
}

fn push_lifetime_contract(out: &mut String, contract: &LifetimeContract) {
    push_line(out, 2, "{");
    push_prop(out, 3, "function", &quoted(&contract.function), true);
    push_line(out, 3, "\"returns\": [");
    for (idx, param) in contract.returns.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_lifetime_param(out, param);
    }
    if !contract.returns.is_empty() {
        out.push('\n');
    }
    push_line(out, 3, "],");
    push_line(out, 3, "\"mutates\": [");
    for (idx, param) in contract.mutates.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_lifetime_param(out, param);
    }
    if !contract.mutates.is_empty() {
        out.push('\n');
    }
    push_line(out, 3, "]");
    push_indent(out, 2);
    out.push('}');
}

fn push_lifetime_param(out: &mut String, param: &LifetimeParam) {
    push_line(out, 4, "{");
    push_prop(out, 5, "index", &param.index.to_string(), true);
    push_prop(out, 5, "name", &quoted(&param.name), true);
    push_prop(out, 5, "mutable", bool_json(param.mutable), true);
    push_prop(out, 5, "type", &quoted(&param.ty), false);
    push_indent(out, 4);
    out.push('}');
}

fn push_region(out: &mut String, region: &BorrowRegion) {
    push_line(out, 2, "{");
    push_prop(out, 3, "id", &quoted(&region.id), true);
    push_prop(out, 3, "function", &quoted(&region.function), true);
    push_prop(out, 3, "root", &quoted(&region.root), true);
    push_prop(out, 3, "mutable", bool_json(region.mutable), true);
    push_line(out, 3, "\"loans\": [");
    for (idx, loan) in region.loans.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_loan(out, loan);
    }
    if !region.loans.is_empty() {
        out.push('\n');
    }
    push_line(out, 3, "],");
    push_line(out, 3, "\"uses\": [");
    for (idx, borrow_use) in region.uses.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_use(out, borrow_use);
    }
    if !region.uses.is_empty() {
        out.push('\n');
    }
    push_line(out, 3, "]");
    push_indent(out, 2);
    out.push('}');
}

fn push_loan(out: &mut String, loan: &BorrowLoan) {
    push_line(out, 4, "{");
    push_prop(out, 5, "id", &quoted(&loan.id), true);
    push_prop(out, 5, "binding", &quoted(&loan.binding), true);
    push_prop(out, 5, "source", &quoted(&loan.source), true);
    push_prop(out, 5, "origin", &quoted(&loan.origin), true);
    push_prop(out, 5, "mutable", bool_json(loan.mutable), true);
    push_prop(out, 5, "type", &quoted(&loan.ty), true);
    push_prop(out, 5, "node", &quoted(&loan.node), false);
    push_indent(out, 4);
    out.push('}');
}

fn push_use(out: &mut String, borrow_use: &BorrowUse) {
    push_line(out, 4, "{");
    push_prop(out, 5, "node", &quoted(&borrow_use.node), true);
    push_prop(out, 5, "kind", &quoted(&borrow_use.kind), true);
    push_prop(out, 5, "place", &quoted(&borrow_use.place), true);
    push_prop(out, 5, "root", &quoted(&borrow_use.root), false);
    push_indent(out, 4);
    out.push('}');
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
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
