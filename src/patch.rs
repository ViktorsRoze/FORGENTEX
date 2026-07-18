use crate::ast::{AsmBlock, Expr, Field, FunctionDecl, Item, Param, Program, Stmt, Type};
use crate::diag::Diagnostic;
use crate::parser::parse_expr;
use crate::span::{ParseDiagnostic, SourceSpan};

#[derive(Clone, Debug)]
pub struct PatchResult {
    pub program: Program,
    pub review: ReviewReport,
}

#[derive(Clone, Debug, Default)]
pub struct PatchOptions {
    pub check_only: bool,
}

#[derive(Clone, Debug)]
pub struct ReviewReport {
    pub lines: Vec<String>,
    pub operation_log: Vec<String>,
    pub public_api_changed: bool,
    pub unsafe_changed: bool,
    pub abi_changed: bool,
    pub allocation_changed: bool,
    pub filesystem_changed: bool,
    pub network_changed: bool,
    pub effect_changed: bool,
    pub architecture_changed: bool,
    pub check_only: bool,
    pub patch_version: String,
    pub author: Option<String>,
    pub mock_signature: Option<String>,
    pub signed_by: Option<String>,
    pub signature_algorithm: Option<String>,
    pub signature_value: Option<String>,
    pub signature_status: String,
    pub expected_diagnostics: Vec<String>,
    pub review_metadata: Vec<(String, String)>,
    pub target_notes: Vec<String>,
}

impl ReviewReport {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            operation_log: Vec::new(),
            public_api_changed: false,
            unsafe_changed: false,
            abi_changed: false,
            allocation_changed: false,
            filesystem_changed: false,
            network_changed: false,
            effect_changed: false,
            architecture_changed: false,
            check_only: false,
            patch_version: "0".to_string(),
            author: None,
            mock_signature: None,
            signed_by: None,
            signature_algorithm: None,
            signature_value: None,
            signature_status: "v0-compat".to_string(),
            expected_diagnostics: Vec::new(),
            review_metadata: Vec::new(),
            target_notes: Vec::new(),
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Patch summary:\n");
        if self.lines.is_empty() {
            out.push_str("  - No semantic changes recorded.\n");
        } else {
            for line in &self.lines {
                out.push_str(&format!("  - {line}\n"));
            }
        }
        out.push_str(&format!(
            "  - Public API changed: {}\n",
            yes_no(self.public_api_changed)
        ));
        out.push_str(&format!(
            "  - Unsafe or assembly changed: {}\n",
            yes_no(self.unsafe_changed)
        ));
        out.push_str(&format!(
            "  - ABI or layout changed: {}\n",
            yes_no(self.abi_changed)
        ));
        out.push_str(&format!(
            "  - Allocation behavior changed: {}\n",
            yes_no(self.allocation_changed)
        ));
        out.push_str(&format!(
            "  - Filesystem access changed: {}\n",
            yes_no(self.filesystem_changed)
        ));
        out.push_str(&format!(
            "  - Network access changed: {}\n",
            yes_no(self.network_changed)
        ));
        out.push_str(&format!(
            "  - Effect or capability changed: {}\n",
            yes_no(self.effect_changed)
        ));
        out.push_str(&format!(
            "  - Architecture-specific behavior changed: {}\n",
            yes_no(self.architecture_changed)
        ));
        if !self.target_notes.is_empty() {
            out.push_str(&format!(
                "  - Architecture paths: {}\n",
                self.target_notes.join("; ")
            ));
        }
        if self.check_only {
            out.push_str("  - Check-only: yes\n");
        }
        out.push_str(&format!(
            "  - Signature status: {}\n",
            self.signature_status
        ));
        if let Some(signed_by) = &self.signed_by {
            out.push_str(&format!(
                "  - Signed by: {} ({})\n",
                signed_by,
                self.signature_algorithm.as_deref().unwrap_or("unknown")
            ));
        }
        out
    }

    pub fn render_json(&self, diagnostics: &[Diagnostic]) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_json_prop(&mut out, 1, "schema", "\"forgentex.patch_review.v1\"", true);
        push_json_prop(&mut out, 1, "version", &quoted(&self.patch_version), true);
        push_json_prop(
            &mut out,
            1,
            "author",
            &optional_quoted(self.author.as_deref()),
            true,
        );
        push_json_prop(
            &mut out,
            1,
            "mock_signature",
            &optional_quoted(self.mock_signature.as_deref()),
            true,
        );
        push_json_prop(
            &mut out,
            1,
            "signed_by",
            &optional_quoted(self.signed_by.as_deref()),
            true,
        );
        push_json_prop(
            &mut out,
            1,
            "signature_algorithm",
            &optional_quoted(self.signature_algorithm.as_deref()),
            true,
        );
        push_json_prop(
            &mut out,
            1,
            "signature",
            &optional_quoted(self.signature_value.as_deref()),
            true,
        );
        push_json_prop(
            &mut out,
            1,
            "signature_status",
            &quoted(&self.signature_status),
            true,
        );
        push_json_prop(
            &mut out,
            1,
            "check_only",
            if self.check_only { "true" } else { "false" },
            true,
        );
        push_string_array(&mut out, 1, "summary", &self.lines, true);
        push_string_array(&mut out, 1, "operation_log", &self.operation_log, true);
        push_string_array(&mut out, 1, "target_notes", &self.target_notes, true);
        push_string_array(
            &mut out,
            1,
            "expected_diagnostics",
            &self.expected_diagnostics,
            true,
        );
        push_metadata(&mut out, 1, &self.review_metadata, true);
        push_changes(&mut out, 1, self, true);
        push_diagnostics(&mut out, 1, diagnostics, false);
        out.push_str("\n}");
        out
    }
}

impl Default for ReviewReport {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct PatchEnvelope {
    version: String,
    author: Option<String>,
    mock_signature: Option<String>,
    signed_by: Option<String>,
    signature_algorithm: Option<String>,
    signature_value: Option<String>,
    target: Option<String>,
    preconditions: Vec<PatchPrecondition>,
    expected_diagnostics: Vec<String>,
    review_metadata: Vec<(String, String)>,
    operations: Vec<PatchOp>,
}

impl PatchEnvelope {
    fn v0(operations: Vec<PatchOp>) -> Self {
        Self {
            version: "0".to_string(),
            author: None,
            mock_signature: None,
            signed_by: None,
            signature_algorithm: None,
            signature_value: None,
            target: None,
            preconditions: Vec::new(),
            expected_diagnostics: Vec::new(),
            review_metadata: Vec::new(),
            operations,
        }
    }
}

#[derive(Clone, Debug)]
enum PatchPrecondition {
    Exists(String),
    Missing(String),
    Hash { node: String, hash: String },
}

pub fn apply_patch(program: Program, source: &str) -> Result<PatchResult, String> {
    apply_patch_with_options(program, source, PatchOptions::default())
}

pub fn apply_patch_with_options(
    mut program: Program,
    source: &str,
    options: PatchOptions,
) -> Result<PatchResult, String> {
    let envelope = parse_patch(source)?;
    let mut review = ReviewReport::new();
    review.patch_version = envelope.version.clone();
    review.author = envelope.author.clone();
    review.mock_signature = envelope.mock_signature.clone();
    review.signed_by = envelope.signed_by.clone();
    review.signature_algorithm = envelope.signature_algorithm.clone();
    review.signature_value = envelope.signature_value.clone();
    review.signature_status = signature_status(&envelope);
    review.expected_diagnostics = envelope.expected_diagnostics.clone();
    review.review_metadata = envelope.review_metadata.clone();
    review.check_only = options.check_only;
    validate_signature(&envelope, &mut review)?;
    validate_preconditions(&program, &envelope, &mut review)?;
    for operation in envelope.operations {
        match operation {
            PatchOp::AddStruct { name, fields } => {
                if program.find_struct_mut(&name).is_some() {
                    return Err(format!("struct already exists: {name}"));
                }
                program.items.push(Item::Struct(crate::ast::StructDecl::new(
                    name.clone(),
                    fields.clone(),
                )));
                review.public_api_changed = true;
                review.abi_changed = true;
                review.operation_log.push(format!("ADD_STRUCT {name}"));
                review.lines.push(format!("Added struct '{name}'."));
            }
            PatchOp::AddStructField { target, name, ty } => {
                let struct_name = target
                    .strip_prefix("@struct.")
                    .ok_or_else(|| format!("ADD_STRUCT_FIELD target must be struct: {target}"))?;
                let decl = program
                    .find_struct_mut(struct_name)
                    .ok_or_else(|| format!("unknown struct target: {target}"))?;
                if !decl.fields.iter().any(|field| field.name == name) {
                    decl.fields.push(Field {
                        name: name.clone(),
                        ty: Type::parse(&ty),
                        attrs: Vec::new(),
                    });
                    review.public_api_changed = true;
                    review.abi_changed = true;
                    review
                        .operation_log
                        .push(format!("ADD_STRUCT_FIELD {target}.{name}"));
                    review
                        .lines
                        .push(format!("Added field '{name}: {ty}' to {struct_name}."));
                }
            }
            PatchOp::RemoveStructField { target, name } => {
                let struct_name = target.strip_prefix("@struct.").ok_or_else(|| {
                    format!("REMOVE_STRUCT_FIELD target must be struct: {target}")
                })?;
                let decl = program
                    .find_struct_mut(struct_name)
                    .ok_or_else(|| format!("unknown struct target: {target}"))?;
                let before = decl.fields.len();
                decl.fields.retain(|field| field.name != name);
                if decl.fields.len() == before {
                    return Err(format!("unknown field target: {target}.field.{name}"));
                }
                review.public_api_changed = true;
                review.abi_changed = true;
                review
                    .operation_log
                    .push(format!("REMOVE_STRUCT_FIELD {target}.{name}"));
                review
                    .lines
                    .push(format!("Removed field '{name}' from {struct_name}."));
            }
            PatchOp::AddFunction {
                name,
                unsafe_fn,
                params,
                ret,
                effects,
                body,
            } => {
                if program.find_function(&name).is_some() {
                    return Err(format!("function already exists: {name}"));
                }
                let mut function = FunctionDecl::new(name.clone(), unsafe_fn, params, ret, body);
                function.effects = effects.clone();
                program.items.push(Item::Function(function));
                review.public_api_changed = true;
                review.abi_changed = true;
                if unsafe_fn {
                    review.unsafe_changed = true;
                }
                for effect in &effects {
                    mark_effect_change(&mut review, effect);
                }
                review.operation_log.push(format!("ADD_FUNCTION {name}"));
                review.lines.push(format!("Added function '{name}'."));
            }
            PatchOp::RemoveFunction { target } => {
                let function_name = function_target_name(&target)?.to_string();
                let before = program.items.len();
                program.items.retain(|item| {
                    !matches!(item, Item::Function(function) if function.name == function_name)
                });
                if program.items.len() == before {
                    return Err(format!("unknown function target: {target}"));
                }
                review.public_api_changed = true;
                review.abi_changed = true;
                review
                    .operation_log
                    .push(format!("REMOVE_FUNCTION {target}"));
                review
                    .lines
                    .push(format!("Removed function {function_name}."));
            }
            PatchOp::ReplaceReturnType { target, ty } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                function.ret = Type::parse(&ty);
                review.public_api_changed = true;
                review.abi_changed = true;
                review
                    .operation_log
                    .push(format!("REPLACE_RETURN_TYPE {target}"));
                review.lines.push(format!(
                    "Changed return type of {function_name} to {}.",
                    function.ret.to_agent()
                ));
            }
            PatchOp::ReplaceFunctionBody { target, body } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                function.body = body;
                review
                    .operation_log
                    .push(format!("REPLACE_FUNCTION_BODY {target}"));
                review
                    .lines
                    .push(format!("Replaced body of function {function_name}."));
            }
            PatchOp::ReplaceReturnExpr { target, expr } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                replace_first_return_expr(&mut function.body, expr)
                    .ok_or_else(|| format!("function has no return expression: {target}"))?;
                review
                    .operation_log
                    .push(format!("REPLACE_RETURN_EXPR {target}"));
                review.lines.push(format!(
                    "Replaced first return expression in {function_name}."
                ));
            }
            PatchOp::RenameFunction { target, new_name } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                function.name = new_name.clone();
                function.id = format!("@function.{new_name}");
                review.public_api_changed = true;
                review.abi_changed = true;
                review
                    .operation_log
                    .push(format!("RENAME_FUNCTION {target} -> {new_name}"));
                review
                    .lines
                    .push(format!("Renamed function {function_name} to {new_name}."));
            }
            PatchOp::MarkUnsafe { target } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                function.unsafe_fn = true;
                review.unsafe_changed = true;
                review.public_api_changed = true;
                review.operation_log.push(format!("MARK_UNSAFE {target}"));
                review
                    .lines
                    .push(format!("Marked function {function_name} unsafe."));
            }
            PatchOp::ClearUnsafe { target } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                function.unsafe_fn = false;
                review.unsafe_changed = true;
                review.public_api_changed = true;
                review.operation_log.push(format!("CLEAR_UNSAFE {target}"));
                review
                    .lines
                    .push(format!("Marked function {function_name} safe."));
            }
            PatchOp::AddEffect { target, effect } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                if !function.effects.iter().any(|existing| existing == &effect) {
                    function.effects.push(effect.clone());
                    function.effects.sort();
                    mark_effect_change(&mut review, &effect);
                    review
                        .operation_log
                        .push(format!("ADD_EFFECT {target} {effect}"));
                    review.lines.push(format!(
                        "Added effect/capability '{effect}' to {function_name}."
                    ));
                }
            }
            PatchOp::RemoveEffect { target, effect } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                let before = function.effects.len();
                function.effects.retain(|existing| existing != &effect);
                if function.effects.len() == before {
                    return Err(format!("effect not present on {target}: {effect}"));
                }
                mark_effect_change(&mut review, &effect);
                review
                    .operation_log
                    .push(format!("REMOVE_EFFECT {target} {effect}"));
                review.lines.push(format!(
                    "Removed effect/capability '{effect}' from {function_name}."
                ));
            }
            PatchOp::InsertFastPath {
                target,
                target_features,
                call,
            } => {
                let function_name = function_target_name(&target)?.to_string();
                let params = program
                    .find_function(&function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?
                    .params
                    .clone();
                let function = program
                    .find_function_mut(&function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                let fast_path = build_fast_path(&params, &call, &target_features)?;
                function.body.insert(0, fast_path);
                for effect in fast_path_effects(&target_features) {
                    if !function.effects.iter().any(|existing| existing == &effect) {
                        function.effects.push(effect.clone());
                        mark_effect_change(&mut review, &effect);
                        review.lines.push(format!(
                            "Added effect/capability '{effect}' to {function_name}."
                        ));
                    }
                }
                function.effects.sort();
                review.architecture_changed = true;
                review
                    .operation_log
                    .push(format!("INSERT_FAST_PATH {target} {target_features}"));
                review.lines.push(format!(
                    "Added {target_features} fast path to {function_name}."
                ));
                review
                    .lines
                    .push("Safe scalar fallback remains after the inserted fast path.".to_string());
                review.target_notes.push(format!(
                    "fast_path function={function_name} target_features={target_features} fallback=scalar"
                ));
            }
            PatchOp::AddAsmBlock {
                target,
                asm_target,
                op,
            } => {
                let function_name = function_target_name(&target)?;
                let function = program
                    .find_function_mut(function_name)
                    .ok_or_else(|| format!("unknown function target: {target}"))?;
                if !function.unsafe_fn {
                    return Err(format!("ADD_ASM_BLOCK target must be unsafe: {target}"));
                }
                function.body.insert(
                    0,
                    Stmt::Asm(AsmBlock {
                        target: asm_target.clone(),
                        inputs: Vec::new(),
                        outputs: Vec::new(),
                        clobbers: vec!["memory".to_string()],
                        ops: vec![op.split('_').map(str::to_string).collect()],
                    }),
                );
                review.unsafe_changed = true;
                review.architecture_changed = true;
                mark_effect_change(&mut review, "cpu.asm");
                mark_effect_change(&mut review, "memory.raw");
                review
                    .operation_log
                    .push(format!("ADD_ASM_BLOCK {target} {asm_target}"));
                review.lines.push(format!(
                    "Added structured assembly block to {function_name} for target {asm_target}."
                ));
                review.target_notes.push(format!(
                    "assembly function={function_name} target={asm_target} review_only=true"
                ));
            }
            PatchOp::AddUnsafeAsmStub {
                target,
                name,
                asm_target,
                effect,
            } => {
                let target_name = function_target_name(&target)?;
                if program.find_function(&name).is_none() {
                    let template = program
                        .find_function(target_name)
                        .ok_or_else(|| format!("unknown function target: {target}"))?
                        .clone();
                    let mut stub = unsafe_stub_from_template(&name, &asm_target, &template);
                    let note_effect = effect.clone().unwrap_or_else(|| "none".to_string());
                    if let Some(effect) = effect.as_ref() {
                        stub.effects.push(effect.clone());
                        mark_effect_change(&mut review, effect);
                        review
                            .lines
                            .push(format!("New target-specific effect required: {effect}."));
                    }
                    program.items.push(Item::Function(stub));
                    review.unsafe_changed = true;
                    review.architecture_changed = true;
                    review
                        .operation_log
                        .push(format!("ADD_UNSAFE_ASM_STUB {name} {asm_target}"));
                    review.lines.push(format!(
                        "Added unsafe structured assembly stub '{name}' for target {asm_target}."
                    ));
                    review.target_notes.push(format!(
                        "unsafe_stub function={name} target={asm_target} effect={note_effect} fallback=caller_retains_scalar_path"
                    ));
                }
            }
            PatchOp::AddTest { target, name } => {
                review
                    .operation_log
                    .push(format!("ADD_TEST {target} {name}"));
                review.lines.push(format!(
                    "Registered test intent '{name}' for target {target}."
                ));
            }
        }
    }
    Ok(PatchResult { program, review })
}

pub fn diagnose_patch_source(file: &str, source: &str) -> Vec<ParseDiagnostic> {
    match parse_patch(source) {
        Ok(_) => Vec::new(),
        Err(message) => {
            let span = patch_error_span(file, source, &message);
            let mut diagnostic = ParseDiagnostic::new(patch_error_code(&message), message).suggest(
                "start with PATCH @function.name or use a supported semantic patch operation",
            );
            if let Some(span) = span {
                diagnostic = diagnostic.with_span(span);
            }
            vec![diagnostic]
        }
    }
}

#[derive(Clone, Debug)]
enum PatchOp {
    AddStruct {
        name: String,
        fields: Vec<Field>,
    },
    AddStructField {
        target: String,
        name: String,
        ty: String,
    },
    RemoveStructField {
        target: String,
        name: String,
    },
    AddFunction {
        name: String,
        unsafe_fn: bool,
        params: Vec<Param>,
        ret: Type,
        effects: Vec<String>,
        body: Vec<Stmt>,
    },
    RemoveFunction {
        target: String,
    },
    ReplaceReturnType {
        target: String,
        ty: String,
    },
    ReplaceFunctionBody {
        target: String,
        body: Vec<Stmt>,
    },
    ReplaceReturnExpr {
        target: String,
        expr: Expr,
    },
    RenameFunction {
        target: String,
        new_name: String,
    },
    MarkUnsafe {
        target: String,
    },
    ClearUnsafe {
        target: String,
    },
    AddEffect {
        target: String,
        effect: String,
    },
    RemoveEffect {
        target: String,
        effect: String,
    },
    InsertFastPath {
        target: String,
        target_features: String,
        call: String,
    },
    AddAsmBlock {
        target: String,
        asm_target: String,
        op: String,
    },
    AddUnsafeAsmStub {
        target: String,
        name: String,
        asm_target: String,
        effect: Option<String>,
    },
    AddTest {
        target: String,
        name: String,
    },
}

fn parse_patch(source: &str) -> Result<PatchEnvelope, String> {
    let mut envelope = PatchEnvelope::v0(Vec::new());
    let mut saw_v1_field = false;
    let mut current_target: Option<String> = None;
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if line.is_empty() {
            continue;
        }
        if line == "PATCH_PROTOCOL v1" || line == "SEMANTIC_PATCH v1" {
            envelope.version = "1".to_string();
            saw_v1_field = true;
            continue;
        }
        if let Some(version) = line.strip_prefix("VERSION ") {
            envelope.version = version.trim().trim_start_matches('v').to_string();
            saw_v1_field = true;
            continue;
        }
        if let Some(author) = line.strip_prefix("AUTHOR ") {
            envelope.author = Some(author.trim().to_string());
            saw_v1_field = true;
            continue;
        }
        if let Some(signature) = line.strip_prefix("MOCK_SIGNATURE ") {
            envelope.mock_signature = Some(signature.trim().to_string());
            saw_v1_field = true;
            continue;
        }
        if let Some(signed_by) = line.strip_prefix("SIGNED_BY ") {
            envelope.signed_by = Some(signed_by.trim().to_string());
            saw_v1_field = true;
            continue;
        }
        if let Some(algorithm) = line.strip_prefix("SIGNATURE_ALGORITHM ") {
            envelope.signature_algorithm = Some(algorithm.trim().to_string());
            saw_v1_field = true;
            continue;
        }
        if let Some(signature) = line.strip_prefix("SIGNATURE ") {
            let signature = signature.trim();
            let values = key_values(signature);
            if values.is_empty() {
                envelope.signature_value = Some(signature.to_string());
            } else {
                for (key, value) in values {
                    match key.as_str() {
                        "signer" | "signed_by" => envelope.signed_by = Some(value),
                        "algorithm" | "alg" => envelope.signature_algorithm = Some(value),
                        "value" | "sig" => envelope.signature_value = Some(value),
                        _ => {}
                    }
                }
            }
            saw_v1_field = true;
            continue;
        }
        if let Some(target) = line.strip_prefix("TARGET ") {
            let target = target.trim().to_string();
            current_target = Some(target.clone());
            envelope.target = Some(target);
            saw_v1_field = true;
            continue;
        }
        if let Some(target) = line.strip_prefix("PATCH ") {
            current_target = Some(target.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("PRECONDITION ") {
            envelope
                .preconditions
                .push(parse_precondition(rest.trim(), current_target.as_deref())?);
            saw_v1_field = true;
            continue;
        }
        if let Some(diagnostic) = line.strip_prefix("EXPECT_DIAGNOSTIC ") {
            envelope
                .expected_diagnostics
                .push(diagnostic.trim().to_string());
            saw_v1_field = true;
            continue;
        }
        if let Some(metadata) = line.strip_prefix("REVIEW ") {
            envelope
                .review_metadata
                .extend(parse_review_metadata(metadata.trim()));
            saw_v1_field = true;
            continue;
        }
        let operation_line = line.strip_prefix("OP ").unwrap_or(line);
        envelope.operations.push(parse_patch_operation(
            operation_line,
            current_target.as_deref(),
        )?);
    }
    if saw_v1_field && envelope.version == "0" {
        envelope.version = "1".to_string();
    }
    Ok(envelope)
}

fn parse_precondition(
    source: &str,
    current_target: Option<&str>,
) -> Result<PatchPrecondition, String> {
    let (kind, rest) = source
        .split_once(char::is_whitespace)
        .map(|(kind, rest)| (kind, rest.trim()))
        .unwrap_or((source, ""));
    match kind {
        "NODE_EXISTS" => Ok(PatchPrecondition::Exists(patch_node_operand(
            rest,
            current_target,
            "NODE_EXISTS",
        )?)),
        "NODE_MISSING" => Ok(PatchPrecondition::Missing(patch_node_operand(
            rest,
            current_target,
            "NODE_MISSING",
        )?)),
        "NODE_HASH" | "HASH" => {
            let node = key_value(rest, "node")
                .or_else(|| key_value(rest, "target"))
                .map(str::to_string);
            let hash = key_value(rest, "hash").map(str::to_string);
            let (node, hash) = match (node, hash) {
                (Some(node), Some(hash)) => (node, hash),
                _ => {
                    let parts = rest.split_whitespace().collect::<Vec<_>>();
                    match parts.as_slice() {
                        [node, hash] => ((*node).to_string(), (*hash).to_string()),
                        [hash] => (
                            current_target
                                .ok_or_else(|| "NODE_HASH requires node target".to_string())?
                                .to_string(),
                            (*hash).to_string(),
                        ),
                        _ => {
                            return Err(format!(
                                "NODE_HASH requires node and hash operands: {source}"
                            ))
                        }
                    }
                }
            };
            Ok(PatchPrecondition::Hash { node, hash })
        }
        other => Err(format!("unknown patch precondition: {other}")),
    }
}

fn patch_node_operand(
    rest: &str,
    current_target: Option<&str>,
    kind: &str,
) -> Result<String, String> {
    key_value(rest, "node")
        .or_else(|| key_value(rest, "target"))
        .map(str::to_string)
        .or_else(|| {
            rest.split_whitespace()
                .next()
                .filter(|part| !part.is_empty())
                .map(str::to_string)
        })
        .or_else(|| current_target.map(str::to_string))
        .ok_or_else(|| format!("{kind} requires node target"))
}

fn parse_review_metadata(source: &str) -> Vec<(String, String)> {
    let values = key_values(source);
    if !values.is_empty() {
        return values;
    }
    source
        .split_once(char::is_whitespace)
        .map(|(key, value)| vec![(key.to_string(), value.trim().to_string())])
        .unwrap_or_else(|| vec![("note".to_string(), source.to_string())])
}

fn parse_patch_operation(line: &str, current_target: Option<&str>) -> Result<PatchOp, String> {
    let (op, rest) = line
        .split_once(char::is_whitespace)
        .map(|(op, rest)| (op, rest.trim()))
        .unwrap_or((line, ""));
    match op {
        "ADD_STRUCT" => {
            let name = key_value(rest, "name")
                .or_else(|| rest.split_whitespace().next())
                .ok_or_else(|| format!("ADD_STRUCT requires name: {line}"))?
                .to_string();
            let fields = key_value(rest, "fields")
                .map(parse_fields)
                .transpose()?
                .unwrap_or_default();
            Ok(PatchOp::AddStruct { name, fields })
        }
        "ADD_STRUCT_FIELD" => {
            let target = operation_target(rest, current_target, "ADD_STRUCT_FIELD")?;
            let name = key_value(rest, "name")
                .or_else(|| rest.split_whitespace().next())
                .ok_or_else(|| format!("missing field name in {line}"))?
                .to_string();
            let ty = key_value(rest, "type")
                .or_else(|| key_value(rest, "ty"))
                .map(str::to_string)
                .or_else(|| {
                    let mut parts = rest.split_whitespace();
                    parts.next()?;
                    let ty = parts.collect::<Vec<_>>().join(" ");
                    (!ty.is_empty()).then_some(ty)
                })
                .ok_or_else(|| format!("missing field type in {line}"))?;
            Ok(PatchOp::AddStructField { target, name, ty })
        }
        "REMOVE_STRUCT_FIELD" => {
            let target = operation_target(rest, current_target, "REMOVE_STRUCT_FIELD")?;
            let name = key_value(rest, "name")
                .or_else(|| rest.split_whitespace().next())
                .ok_or_else(|| format!("REMOVE_STRUCT_FIELD requires name: {line}"))?
                .to_string();
            Ok(PatchOp::RemoveStructField { target, name })
        }
        "ADD_FUNCTION" => {
            let name = key_value(rest, "name")
                .or_else(|| rest.split_whitespace().next())
                .ok_or_else(|| format!("ADD_FUNCTION requires name: {line}"))?
                .to_string();
            let unsafe_fn = parse_bool_value(key_value(rest, "unsafe")).unwrap_or(false)
                || rest.split_whitespace().any(|part| part == "unsafe");
            let params = key_value(rest, "params")
                .map(parse_params)
                .transpose()?
                .unwrap_or_default();
            let ret = Type::parse(
                key_value(rest, "ret")
                    .or_else(|| key_value(rest, "return_type"))
                    .unwrap_or("void"),
            );
            let effects = key_value(rest, "effects")
                .map(parse_effects)
                .unwrap_or_default();
            let body = parse_body(rest, &ret)?;
            Ok(PatchOp::AddFunction {
                name,
                unsafe_fn,
                params,
                ret,
                effects,
                body,
            })
        }
        "REMOVE_FUNCTION" => {
            let target = key_value(rest, "node")
                .or_else(|| key_value(rest, "target_node"))
                .or_else(|| key_value(rest, "target"))
                .map(str::to_string)
                .or_else(|| current_target.map(str::to_string))
                .or_else(|| {
                    key_value(rest, "name")
                        .or_else(|| rest.split_whitespace().next())
                        .map(|name| format!("@function.{name}"))
                })
                .ok_or_else(|| format!("REMOVE_FUNCTION requires target: {line}"))?;
            Ok(PatchOp::RemoveFunction { target })
        }
        "REPLACE_RETURN_TYPE" => {
            let target = operation_target(rest, current_target, "REPLACE_RETURN_TYPE")?;
            let ty = key_value(rest, "type")
                .or_else(|| key_value(rest, "ty"))
                .unwrap_or(rest)
                .trim()
                .to_string();
            if ty.is_empty() {
                return Err(format!("REPLACE_RETURN_TYPE requires type: {line}"));
            }
            Ok(PatchOp::ReplaceReturnType { target, ty })
        }
        "REPLACE_FUNCTION_BODY" => {
            let target = operation_target(rest, current_target, "REPLACE_FUNCTION_BODY")?;
            let body = parse_body(rest, &Type::Void)?;
            Ok(PatchOp::ReplaceFunctionBody { target, body })
        }
        "REPLACE_RETURN_EXPR" => {
            let target = operation_target(rest, current_target, "REPLACE_RETURN_EXPR")?;
            let expr_source = tail_key_value(rest, "expr")
                .or_else(|| tail_key_value(rest, "return"))
                .unwrap_or(rest)
                .trim();
            if expr_source.is_empty() {
                return Err(format!("REPLACE_RETURN_EXPR requires expr=<expr>: {line}"));
            }
            Ok(PatchOp::ReplaceReturnExpr {
                target,
                expr: expr_from_text(expr_source)?,
            })
        }
        "RENAME_FUNCTION" => {
            let target = operation_target(rest, current_target, "RENAME_FUNCTION")?;
            let new_name = key_value(rest, "new_name")
                .or_else(|| key_value(rest, "name"))
                .unwrap_or(rest)
                .trim()
                .to_string();
            if new_name.is_empty() {
                return Err(format!("RENAME_FUNCTION requires new name: {line}"));
            }
            Ok(PatchOp::RenameFunction { target, new_name })
        }
        "MARK_UNSAFE" => Ok(PatchOp::MarkUnsafe {
            target: operation_target(rest, current_target, "MARK_UNSAFE")?,
        }),
        "CLEAR_UNSAFE" => Ok(PatchOp::ClearUnsafe {
            target: operation_target(rest, current_target, "CLEAR_UNSAFE")?,
        }),
        "ADD_EFFECT" => {
            let target = operation_target(rest, current_target, "ADD_EFFECT")?;
            let effect = key_value(rest, "effect").unwrap_or(rest).trim().to_string();
            if effect.is_empty() {
                return Err(format!("ADD_EFFECT requires effect name: {line}"));
            }
            Ok(PatchOp::AddEffect { target, effect })
        }
        "REMOVE_EFFECT" => {
            let target = operation_target(rest, current_target, "REMOVE_EFFECT")?;
            let effect = key_value(rest, "effect").unwrap_or(rest).trim().to_string();
            if effect.is_empty() {
                return Err(format!("REMOVE_EFFECT requires effect name: {line}"));
            }
            Ok(PatchOp::RemoveEffect { target, effect })
        }
        "INSERT_FAST_PATH" => {
            let target = operation_target_node_only(rest, current_target, "INSERT_FAST_PATH")?;
            let target_features = key_value(rest, "target")
                .or_else(|| key_value(rest, "features"))
                .or_else(|| key_value(rest, "arch"))
                .unwrap_or("unknown")
                .to_string();
            let call = key_value(rest, "call")
                .ok_or_else(|| format!("INSERT_FAST_PATH requires call=<function>: {line}"))?
                .to_string();
            Ok(PatchOp::InsertFastPath {
                target,
                target_features,
                call,
            })
        }
        "ADD_ASM_BLOCK" => {
            let target = operation_target_node_only(rest, current_target, "ADD_ASM_BLOCK")?;
            let asm_target = key_value(rest, "target")
                .or_else(|| key_value(rest, "arch"))
                .unwrap_or("unknown")
                .to_string();
            let op = tail_key_value(rest, "op")
                .unwrap_or("demo_op")
                .trim()
                .replace(' ', "_");
            Ok(PatchOp::AddAsmBlock {
                target,
                asm_target,
                op,
            })
        }
        "ADD_UNSAFE_ASM_STUB" => {
            let target = operation_target_node_only(rest, current_target, "ADD_UNSAFE_ASM_STUB")?;
            let name = key_value(rest, "name")
                .ok_or_else(|| format!("ADD_UNSAFE_ASM_STUB requires name=<function>: {line}"))?
                .to_string();
            let asm_target = key_value(rest, "target")
                .or_else(|| key_value(rest, "arch"))
                .unwrap_or("unknown")
                .to_string();
            let effect = key_value(rest, "effect").map(|value| value.to_string());
            Ok(PatchOp::AddUnsafeAsmStub {
                target,
                name,
                asm_target,
                effect,
            })
        }
        "ADD_TEST" => {
            let target = operation_target(rest, current_target, "ADD_TEST")?;
            let name = key_value(rest, "name")
                .or_else(|| rest.split_whitespace().next())
                .ok_or_else(|| format!("ADD_TEST requires name: {line}"))?
                .to_string();
            Ok(PatchOp::AddTest { target, name })
        }
        _ => Err(format!("unknown patch operation: {line}")),
    }
}

fn operation_target(rest: &str, current_target: Option<&str>, op: &str) -> Result<String, String> {
    key_value(rest, "node")
        .or_else(|| key_value(rest, "target_node"))
        .or_else(|| key_value(rest, "target").filter(|target| target.starts_with('@')))
        .map(str::to_string)
        .or_else(|| current_target.map(str::to_string))
        .ok_or_else(|| format!("patch operation appears before PATCH target: {op}"))
}

fn operation_target_node_only(
    rest: &str,
    current_target: Option<&str>,
    op: &str,
) -> Result<String, String> {
    key_value(rest, "node")
        .or_else(|| key_value(rest, "target_node"))
        .map(str::to_string)
        .or_else(|| current_target.map(str::to_string))
        .ok_or_else(|| format!("patch operation appears before PATCH target: {op}"))
}

fn parse_fields(source: &str) -> Result<Vec<Field>, String> {
    let mut fields = Vec::new();
    for part in source
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (name, ty) = part
            .split_once(':')
            .ok_or_else(|| format!("field must use name:type syntax: {part}"))?;
        fields.push(Field {
            name: name.trim().to_string(),
            ty: Type::parse(ty),
            attrs: Vec::new(),
        });
    }
    Ok(fields)
}

fn parse_params(source: &str) -> Result<Vec<Param>, String> {
    let mut params = Vec::new();
    for part in source
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (name, ty) = part
            .split_once(':')
            .ok_or_else(|| format!("param must use name:type syntax: {part}"))?;
        params.push(Param {
            name: name.trim().to_string(),
            ty: Type::parse(ty),
        });
    }
    Ok(params)
}

fn parse_effects(source: &str) -> Vec<String> {
    source
        .split(',')
        .map(str::trim)
        .filter(|effect| !effect.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_body(rest: &str, ret: &Type) -> Result<Vec<Stmt>, String> {
    if let Some(return_value) = tail_key_value(rest, "return") {
        let return_value = return_value.trim();
        if return_value.is_empty() || matches!(return_value, "void" | "none" | "-") {
            return Ok(vec![Stmt::Return(None)]);
        }
        return Ok(vec![Stmt::Return(Some(expr_from_text(return_value)?))]);
    }
    if let Some(expr) = tail_key_value(rest, "expr") {
        return Ok(vec![Stmt::Expr(expr_from_text(expr.trim())?)]);
    }
    Ok(match ret {
        Type::Void => vec![Stmt::Return(None)],
        _ => vec![Stmt::Return(Some(default_value_for_type(ret)))],
    })
}

fn parse_bool_value(value: Option<&str>) -> Option<bool> {
    match value? {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn key_values(source: &str) -> Vec<(String, String)> {
    source
        .split_whitespace()
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn tail_key_value<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=");
    source
        .find(&needle)
        .map(|start| &source[start + needle.len()..])
}

fn signature_status(envelope: &PatchEnvelope) -> String {
    if envelope.version == "0" {
        "v0-compat".to_string()
    } else if envelope.signature_value.is_some()
        || envelope.signed_by.is_some()
        || envelope.signature_algorithm.is_some()
    {
        if signed_patch_is_valid(envelope) {
            "signed-mock-accepted".to_string()
        } else {
            "signed-mock-invalid".to_string()
        }
    } else if envelope.mock_signature.is_some() {
        "mock-accepted".to_string()
    } else {
        "mock-missing".to_string()
    }
}

fn signed_patch_is_valid(envelope: &PatchEnvelope) -> bool {
    matches!(envelope.signature_algorithm.as_deref(), Some("Ed25519"))
        && envelope
            .signature_value
            .as_deref()
            .map(|value| value.starts_with("mock-sig-"))
            .unwrap_or(false)
        && envelope
            .signed_by
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn validate_signature(envelope: &PatchEnvelope, review: &mut ReviewReport) -> Result<(), String> {
    if envelope.signature_value.is_none()
        && envelope.signed_by.is_none()
        && envelope.signature_algorithm.is_none()
    {
        return Ok(());
    }
    if !signed_patch_is_valid(envelope) {
        return Err(
            "signed patch verification failed: expected SIGNED_BY, SIGNATURE_ALGORITHM Ed25519, and SIGNATURE mock-sig-*"
                .to_string(),
        );
    }
    review.operation_log.push(format!(
        "SIGNATURE {} {} ok",
        envelope.signature_algorithm.as_deref().unwrap_or("unknown"),
        envelope.signed_by.as_deref().unwrap_or("unknown")
    ));
    review
        .lines
        .push("Verified signed semantic patch envelope with mock Ed25519 signature.".to_string());
    Ok(())
}

fn validate_preconditions(
    program: &Program,
    envelope: &PatchEnvelope,
    review: &mut ReviewReport,
) -> Result<(), String> {
    for precondition in &envelope.preconditions {
        match precondition {
            PatchPrecondition::Exists(node) => {
                if !node_exists(program, node) {
                    return Err(format!("precondition failed: node does not exist: {node}"));
                }
                review
                    .operation_log
                    .push(format!("PRECONDITION NODE_EXISTS {node} ok"));
            }
            PatchPrecondition::Missing(node) => {
                if node_exists(program, node) {
                    return Err(format!("precondition failed: node already exists: {node}"));
                }
                review
                    .operation_log
                    .push(format!("PRECONDITION NODE_MISSING {node} ok"));
            }
            PatchPrecondition::Hash { node, hash } => {
                let found = node_hash(program, node)
                    .ok_or_else(|| format!("precondition failed: node does not exist: {node}"))?;
                if &found != hash {
                    return Err(format!(
                        "patch conflict: node hash mismatch for {node}: expected {hash}, found {found}"
                    ));
                }
                review
                    .operation_log
                    .push(format!("PRECONDITION NODE_HASH {node} {hash} ok"));
            }
        }
    }
    Ok(())
}

fn node_exists(program: &Program, node: &str) -> bool {
    node_hash(program, node).is_some()
}

pub fn node_hash(program: &Program, node: &str) -> Option<String> {
    if let Some(rest) = node.strip_prefix("@struct.") {
        if let Some((struct_name, field_name)) = rest.split_once(".field.") {
            let decl = program.structs().find(|decl| decl.name == struct_name)?;
            let field = decl.fields.iter().find(|field| field.name == field_name)?;
            return Some(hash_text(&format!(
                "field:{}:{}",
                field.name,
                field.ty.to_agent()
            )));
        }
        let decl = program.structs().find(|decl| decl.name == rest)?;
        return Some(hash_text(
            &Program {
                items: vec![Item::Struct(decl.clone())],
            }
            .to_canonical_json(),
        ));
    }
    if let Some(rest) = node.strip_prefix("@function.") {
        if let Some((function_name, stmt_path)) = rest.split_once(".stmt.") {
            let function = program.find_function(function_name)?;
            let stmt = stmt_by_path(&function.body, stmt_path)?;
            return Some(hash_text(&format!("{stmt:?}")));
        }
        let function = program.find_function(rest)?;
        return Some(hash_text(
            &Program {
                items: vec![Item::Function(function.clone())],
            }
            .to_canonical_json(),
        ));
    }
    None
}

fn stmt_by_path<'a>(body: &'a [Stmt], path: &str) -> Option<&'a Stmt> {
    let mut parts = path.split('.');
    let index = parts.next()?.parse::<usize>().ok()?;
    let mut stmt = body.get(index)?;
    while let Some(segment) = parts.next() {
        match segment {
            "then" => {
                let index = parts.next()?.parse::<usize>().ok()?;
                match stmt {
                    Stmt::If { then_body, .. } => stmt = then_body.get(index)?,
                    _ => return None,
                }
            }
            "body" => {
                let index = parts.next()?.parse::<usize>().ok()?;
                match stmt {
                    Stmt::For { body, .. } => stmt = body.get(index)?,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    Some(stmt)
}

fn hash_text(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn mark_effect_change(review: &mut ReviewReport, effect: &str) {
    review.effect_changed = true;
    if effect == "alloc" || effect.starts_with("alloc.") {
        review.allocation_changed = true;
    }
    if effect.starts_with("fs.") || effect.starts_with("io.") {
        review.filesystem_changed = true;
    }
    if effect.starts_with("network.") {
        review.network_changed = true;
    }
    if effect.starts_with("cpu.") || effect.starts_with("target.") {
        review.architecture_changed = true;
    }
    if effect == "cpu.asm" || effect == "memory.raw" || effect.starts_with("unsafe.") {
        review.unsafe_changed = true;
    }
}

fn replace_first_return_expr(body: &mut [Stmt], expr: Expr) -> Option<()> {
    for stmt in body {
        match stmt {
            Stmt::Return(Some(return_expr)) => {
                *return_expr = expr;
                return Some(());
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                let replaced = replace_first_return_expr(then_body, expr.clone()).is_some()
                    || replace_first_return_expr(else_body, expr.clone()).is_some();
                if replaced {
                    return Some(());
                }
            }
            Stmt::For { body: nested, .. } => {
                let replaced = replace_first_return_expr(nested, expr.clone()).is_some();
                if replaced {
                    return Some(());
                }
            }
            _ => {}
        }
    }
    None
}

fn push_json_prop(out: &mut String, indent: usize, key: &str, value: &str, trailing: bool) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": {value}"));
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

fn push_metadata(out: &mut String, indent: usize, metadata: &[(String, String)], trailing: bool) {
    push_indent(out, indent);
    out.push_str("\"review_metadata\": [");
    for (idx, (key, value)) in metadata.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!(
            "{{\"key\": {}, \"value\": {}}}",
            quoted(key),
            quoted(value)
        ));
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_changes(out: &mut String, indent: usize, review: &ReviewReport, trailing: bool) {
    push_indent(out, indent);
    out.push_str("\"changes\": {\n");
    push_json_prop(
        out,
        indent + 1,
        "public_api",
        bool_json(review.public_api_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "unsafe_or_assembly",
        bool_json(review.unsafe_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "abi_or_layout",
        bool_json(review.abi_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "allocation",
        bool_json(review.allocation_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "filesystem",
        bool_json(review.filesystem_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "network",
        bool_json(review.network_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "effects",
        bool_json(review.effect_changed),
        true,
    );
    push_json_prop(
        out,
        indent + 1,
        "architecture",
        bool_json(review.architecture_changed),
        false,
    );
    push_indent(out, indent);
    out.push('}');
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_diagnostics(out: &mut String, indent: usize, diagnostics: &[Diagnostic], trailing: bool) {
    push_indent(out, indent);
    out.push_str("\"diagnostics\": [");
    for (idx, diagnostic) in diagnostics.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('\n');
        push_indent(out, indent + 1);
        out.push_str("{\n");
        push_json_prop(out, indent + 2, "error", &quoted(&diagnostic.code), true);
        push_json_prop(out, indent + 2, "node", &quoted(&diagnostic.node), true);
        push_json_prop(
            out,
            indent + 2,
            "message",
            &quoted(&diagnostic.message),
            true,
        );
        push_json_prop(
            out,
            indent + 2,
            "expected",
            &optional_quoted(diagnostic.expected.as_deref()),
            true,
        );
        push_json_prop(
            out,
            indent + 2,
            "found",
            &optional_quoted(diagnostic.found.as_deref()),
            true,
        );
        push_string_array(
            out,
            indent + 2,
            "suggested_patches",
            &diagnostic.suggested_patches,
            false,
        );
        push_indent(out, indent + 1);
        out.push('}');
    }
    if !diagnostics.is_empty() {
        out.push('\n');
        push_indent(out, indent);
    }
    out.push(']');
    if trailing {
        out.push(',');
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn optional_quoted(value: Option<&str>) -> String {
    value.map(quoted).unwrap_or_else(|| "null".to_string())
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape_json(value))
}

use crate::json::escape as escape_json;

fn build_fast_path(params: &[Param], call: &str, target_features: &str) -> Result<Stmt, String> {
    let mut cond = Expr::Call {
        name: feature_probe_name(target_features),
        args: Vec::new(),
    };
    for param in params {
        if matches!(param.ty, Type::Slice { .. }) {
            let aligned = Expr::Call {
                name: "aligned".to_string(),
                args: vec![Expr::Var(param.name.clone()), Expr::Int(32)],
            };
            cond = Expr::Binary {
                op: "&&".to_string(),
                left: Box::new(cond),
                right: Box::new(aligned),
            };
        }
    }
    let args = params
        .iter()
        .map(|param| Expr::Var(param.name.clone()))
        .collect::<Vec<_>>();
    let call_expr = Expr::Call {
        name: call.to_string(),
        args,
    };
    Ok(Stmt::If {
        cond,
        then_body: vec![Stmt::Expr(call_expr), Stmt::Return(None)],
        else_body: Vec::new(),
    })
}

fn feature_probe_name(target_features: &str) -> String {
    let feature = target_features
        .split(',')
        .map(str::trim)
        .find(|part| {
            !part.is_empty()
                && !matches!(
                    *part,
                    "x86" | "x86_64" | "arm" | "armv7" | "aarch64" | "riscv32" | "riscv64"
                )
        })
        .unwrap_or("unknown");
    format!("cpu_has_{feature}")
}

fn fast_path_effects(target_features: &str) -> Vec<String> {
    target_features
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter(|part| {
            !matches!(
                *part,
                "x86" | "x86_64" | "arm" | "armv7" | "aarch64" | "riscv32" | "riscv64"
            )
        })
        .map(|feature| format!("cpu.{feature}"))
        .collect()
}

fn unsafe_stub_from_template(
    name: &str,
    asm_target: &str,
    template: &FunctionDecl,
) -> FunctionDecl {
    let mut body = Vec::new();
    body.push(Stmt::Asm(AsmBlock {
        target: asm_target.to_string(),
        inputs: template
            .params
            .iter()
            .map(|param| crate::ast::AsmBinding {
                register: "arg".to_string(),
                value: param.name.clone(),
            })
            .collect(),
        outputs: Vec::new(),
        clobbers: vec!["memory".to_string(), "flags".to_string()],
        ops: vec![vec![
            "demo_fast_path_stub".to_string(),
            "not_lowered_in_v0".to_string(),
        ]],
    }));
    body.push(match &template.ret {
        Type::Void => Stmt::Return(None),
        _ => Stmt::Return(Some(default_value_for_type(&template.ret))),
    });
    FunctionDecl {
        id: format!("@function.{name}"),
        name: name.to_string(),
        type_params: Vec::new(),
        type_bounds: Vec::new(),
        unsafe_fn: true,
        params: template.params.clone(),
        ret: template.ret.clone(),
        effects: Vec::new(),
        attrs: Vec::new(),
        body,
    }
}

fn default_value_for_type(ty: &Type) -> Expr {
    match ty {
        Type::Bool => Expr::Bool(false),
        Type::Primitive(name) if name == "f32" || name == "f64" => Expr::Float(0.0),
        Type::Primitive(_) => Expr::Int(0),
        Type::Named(name) => Expr::Call {
            name: name.clone(),
            args: Vec::new(),
        },
        _ => Expr::Int(0),
    }
}

fn key_value<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    for part in source.split_whitespace() {
        let (candidate, value) = part.split_once('=')?;
        if candidate == key {
            return Some(value);
        }
    }
    None
}

fn function_target_name(target: &str) -> Result<&str, String> {
    target
        .strip_prefix("@function.")
        .ok_or_else(|| format!("target must be function: {target}"))
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn patch_error_code(message: &str) -> &'static str {
    if message.contains("before PATCH target") {
        "patch_missing_target"
    } else if message.contains("requires") || message.contains("missing") {
        "patch_missing_operand"
    } else if message.contains("unknown patch operation") {
        "patch_unknown_operation"
    } else {
        "patch_parse_error"
    }
}

fn patch_error_span(file: &str, source: &str, message: &str) -> Option<SourceSpan> {
    let fragment = message
        .rsplit_once(':')
        .map(|(_, after)| after.trim())
        .filter(|value| !value.is_empty());
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(line)
            .trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(fragment) = fragment {
            if trimmed.contains(fragment) {
                return Some(SourceSpan::new(file, idx + 1, 1, idx + 1, line.len() + 1));
            }
        }
        if !trimmed.starts_with("PATCH ") {
            return Some(SourceSpan::new(file, idx + 1, 1, idx + 1, line.len() + 1));
        }
    }
    None
}

#[allow(dead_code)]
fn expr_from_text(source: &str) -> Result<Expr, String> {
    parse_expr(source)
}
