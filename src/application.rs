//! Manifest-backed application execution.
//!
//! The first application runtime deliberately targets bounded file tools. The
//! package manifest fixes the entry point, effect set, path scopes, and
//! cumulative byte budgets. Runtime paths are resolved beneath those scopes
//! before `core.io` touches the filesystem.

use std::path::{Path, PathBuf};

use crate::ast::{Param, Program, Type};
use crate::backend::generate_backend_ir;
use crate::interp::{
    run_entry_with_scoped_io_with_ir, IoArtifactPolicy, IoArtifactReceipt, IoOp, IoPolicy, Value,
};
use crate::json;
use crate::project::{
    check_project, load_project, parse_compatibility_line, ApplicationArtifactManifest,
    ManifestPermission, Project,
};

pub const APPLICATION_RUN_SCHEMA: &str = "forgentex.application_run.v1";
pub const APPLICATION_ERROR_SCHEMA: &str = "forgentex.application_error.v1";

pub enum ApplicationDispatchReport {
    Hosted(Box<ApplicationRunReport>),
    Pipeline(Box<crate::pipeline_application::PipelineApplicationReport>),
}

pub fn run_dispatch(
    input: &Path,
    args: Vec<String>,
) -> Result<ApplicationDispatchReport, ApplicationError> {
    let (_, _, manifest) = crate::project::read_project_manifest(input).map_err(|message| {
        ApplicationError::new("application_manifest_invalid", "manifest", message)
    })?;
    let runtime = manifest
        .application
        .as_ref()
        .ok_or_else(|| {
            ApplicationError::new(
                "application_contract_missing",
                "manifest",
                "package has no [application] contract",
            )
        })?
        .runtime
        .as_str();
    match runtime {
        "hosted_io" => run(input, args)
            .map(Box::new)
            .map(ApplicationDispatchReport::Hosted),
        "pipeline" => crate::pipeline_application::run(input, args)
            .map(Box::new)
            .map(ApplicationDispatchReport::Pipeline),
        other => Err(ApplicationError::new(
            "application_runtime_unsupported",
            "manifest",
            format!("application runtime '{other}' is unsupported"),
        )),
    }
}

impl ApplicationDispatchReport {
    pub fn is_success(&self) -> bool {
        match self {
            Self::Hosted(report) => report.is_success(),
            Self::Pipeline(report) => report.is_success(),
        }
    }

    pub fn status(&self) -> &str {
        match self {
            Self::Hosted(report) => &report.status,
            Self::Pipeline(report) => &report.status,
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Self::Hosted(report) => report.to_text(),
            Self::Pipeline(report) => report.to_text(),
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            Self::Hosted(report) => report.to_json(),
            Self::Pipeline(report) => report.to_json(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApplicationResult {
    pub type_name: String,
    pub value_text: String,
    value_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationError {
    pub code: String,
    pub phase: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApplicationRunReport {
    pub schema: String,
    pub package: String,
    pub version: String,
    pub forgentex_required: String,
    pub forgentex_version: String,
    pub entry: String,
    pub runtime: String,
    pub effects: Vec<String>,
    pub read_scopes: Vec<String>,
    pub write_scopes: Vec<String>,
    pub max_read_bytes: Option<usize>,
    pub max_write_bytes: Option<usize>,
    pub max_directory_entries: Option<usize>,
    pub status: String,
    pub ops: Vec<IoOp>,
    pub read_bytes: usize,
    pub write_bytes: usize,
    pub directory_entries: usize,
    pub artifact_declarations: Vec<ApplicationArtifactManifest>,
    pub artifacts: Vec<IoArtifactReceipt>,
    pub result: Option<ApplicationResult>,
    pub manifest_sha256: String,
    pub error: Option<ApplicationError>,
}

pub fn run(input: &Path, args: Vec<String>) -> Result<ApplicationRunReport, ApplicationError> {
    crate::security::enforce_package_security(input)
        .map_err(|message| ApplicationError::new("application_security", "security", message))?;
    let project = load_project(input).map_err(|message| {
        ApplicationError::new("application_manifest_invalid", "manifest", message)
    })?;
    let application = project.manifest.application.as_ref().ok_or_else(|| {
        ApplicationError::new(
            "application_contract_missing",
            "manifest",
            "package has no [application] contract; declare forgentex, entry, runtime, effects, and limits",
        )
    })?;
    let forgentex_version = env!("CARGO_PKG_VERSION").to_string();
    ensure_compatible_version(&application.forgentex, &forgentex_version)
        .map_err(|message| ApplicationError::from_prefixed("compatibility", message))?;
    let manifest_bytes = std::fs::read(&project.manifest_path).map_err(|error| {
        ApplicationError::new(
            "application_manifest_unreadable",
            "manifest",
            format!(
                "failed to read application manifest {}: {error}",
                project.manifest_path.display()
            ),
        )
    })?;
    let diagnostics = check_project(input)
        .map_err(|message| ApplicationError::new("application_project_load", "project", message))?;
    if !diagnostics.is_empty() {
        return Err(ApplicationError::new(
            "application_project_invalid",
            "project",
            format!(
                "application project has {} diagnostic(s): {:?}",
                diagnostics.len(),
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code.as_str())
                    .collect::<Vec<_>>()
            ),
        ));
    }

    let program = combined_program(&project);
    let entry = program.find_function(&application.entry).ok_or_else(|| {
        ApplicationError::new(
            "application_entry_missing",
            "contract",
            format!("application entry '{}' was not found", application.entry),
        )
    })?;
    if entry.params.len() != args.len() {
        return Err(ApplicationError::new(
            "application_argument_count",
            "arguments",
            format!(
                "application entry '{}' expects {} argument(s), received {}",
                application.entry,
                entry.params.len(),
                args.len()
            ),
        ));
    }
    let values = entry
        .params
        .iter()
        .zip(args.iter())
        .enumerate()
        .map(|(index, (param, value))| parse_application_argument(index, param, value))
        .collect::<Result<Vec<_>, String>>()
        .map_err(|message| ApplicationError::from_prefixed("arguments", message))?;
    if !application_result_type_supported(&entry.ret) {
        return Err(ApplicationError::new(
            "application_result_type_unsupported",
            "contract",
            format!(
                "application entry '{}' has unsupported result type {}; expected bool, str, Path, an integer primitive, f32, f64, or void",
                application.entry,
                entry.ret.to_human()
            ),
        ));
    }
    for effect in &entry.effects {
        if !application.effects.contains(effect) {
            return Err(ApplicationError::new(
                "application_effect_undeclared",
                "contract",
                format!(
                    "application entry effect '{effect}' is not declared in [application].effects"
                ),
            ));
        }
    }

    let read_scopes = permission_scopes(&project.manifest.security.permissions, "fs.read");
    let write_scopes = permission_scopes(&project.manifest.security.permissions, "fs.write");
    validate_effect_scope("fs.read", &application.effects, &read_scopes).map_err(|message| {
        ApplicationError::new("application_permission_invalid", "policy", message)
    })?;
    validate_effect_scope("fs.write", &application.effects, &write_scopes).map_err(|message| {
        ApplicationError::new("application_permission_invalid", "policy", message)
    })?;

    let policy = IoPolicy::scoped(
        &project.package_root,
        &read_scopes,
        &write_scopes,
        application.max_read_bytes,
        application.max_write_bytes,
        application.max_directory_entries,
        application
            .artifacts
            .iter()
            .map(|artifact| IoArtifactPolicy {
                name: artifact.name.clone(),
                root: PathBuf::from(&artifact.root),
                media_type: artifact.media_type.clone(),
                max_bytes: artifact.max_bytes,
                required: artifact.required,
                publication: artifact.publication.clone(),
            })
            .collect(),
    )
    .map_err(|message| ApplicationError::from_prefixed("policy", message))?;
    let ir = generate_backend_ir(input)
        .map_err(|message| ApplicationError::new("application_ir_invalid", "ir", message))?;
    let (mut value, ops, usage, artifacts, mut error) =
        run_entry_with_scoped_io_with_ir(&program, &ir, &application.entry, values, policy);
    if error.is_none() {
        let missing = application
            .artifacts
            .iter()
            .filter(|declaration| declaration.required)
            .filter(|declaration| {
                !artifacts
                    .iter()
                    .any(|artifact| artifact.name == declaration.name)
            })
            .map(|declaration| declaration.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            value = None;
            error = Some(format!(
                "artifact_required_missing: application did not publish required artifact(s): {}",
                missing.join(", ")
            ));
        }
    }
    let mut structured_error = error.as_deref().map(|message| runtime_error(&ops, message));
    let result = if structured_error.is_none() {
        match value.as_ref() {
            Some(value) => match application_result(&entry.ret, value) {
                Ok(result) => Some(result),
                Err(result_error) => {
                    structured_error = Some(result_error);
                    None
                }
            },
            None => {
                structured_error = Some(ApplicationError::new(
                    "application_result_missing",
                    "result",
                    "application completed without a result value",
                ));
                None
            }
        }
    } else {
        None
    };
    let status = application_status(&ops, structured_error.as_ref());

    Ok(ApplicationRunReport {
        schema: APPLICATION_RUN_SCHEMA.to_string(),
        package: project.manifest.name,
        version: project.manifest.version,
        forgentex_required: application.forgentex.clone(),
        forgentex_version,
        entry: application.entry.clone(),
        runtime: application.runtime.clone(),
        effects: application.effects.clone(),
        read_scopes: display_scopes(&read_scopes),
        write_scopes: display_scopes(&write_scopes),
        max_read_bytes: application.max_read_bytes,
        max_write_bytes: application.max_write_bytes,
        max_directory_entries: application.max_directory_entries,
        status: status.to_string(),
        ops,
        read_bytes: usage.read_bytes,
        write_bytes: usage.write_bytes,
        directory_entries: usage.directory_entries,
        artifact_declarations: application.artifacts.clone(),
        artifacts,
        result,
        manifest_sha256: crate::crypto::sha256_hex(&manifest_bytes),
        error: structured_error,
    })
}

fn parse_application_argument(index: usize, param: &Param, source: &str) -> Result<Value, String> {
    let position = index + 1;
    match &param.ty {
        Type::Bool => match source {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(argument_invalid(
                position,
                param,
                source,
                "expected lowercase true or false",
            )),
        },
        Type::Primitive(name) | Type::Named(name) if name == "str" => {
            Ok(Value::String(source.to_string()))
        }
        Type::Named(name) if name == "Path" => crate::interp::normalize_portable_path(source)
            .map(Value::String)
            .map_err(|detail| argument_invalid(position, param, source, &detail)),
        Type::Primitive(name) if integer_bounds(name).is_some() => {
            let (min, max) = integer_bounds(name).expect("checked integer type");
            let signed = min < 0;
            if !integer_syntax(source, signed) {
                return Err(argument_invalid(
                    position,
                    param,
                    source,
                    if signed {
                        "expected an optional minus sign followed by decimal digits"
                    } else {
                        "expected unsigned decimal digits"
                    },
                ));
            }
            let parsed = source.parse::<i128>().map_err(|_| {
                argument_out_of_range(position, param, source, min, max)
            })?;
            if parsed < min || parsed > max {
                return Err(argument_out_of_range(position, param, source, min, max));
            }
            Ok(Value::Int(parsed as i64))
        }
        Type::Primitive(name) if name == "f32" || name == "f64" => {
            let parsed = source.parse::<f64>().map_err(|_| {
                argument_invalid(position, param, source, "expected a finite decimal float")
            })?;
            if !parsed.is_finite() {
                return Err(argument_invalid(
                    position,
                    param,
                    source,
                    "expected a finite decimal float",
                ));
            }
            if name == "f32" {
                let narrowed = parsed as f32;
                if !narrowed.is_finite() {
                    return Err(argument_out_of_range_text(
                        position,
                        param,
                        source,
                        "the finite f32 range",
                    ));
                }
                Ok(Value::Float(f64::from(narrowed)))
            } else {
                Ok(Value::Float(parsed))
            }
        }
        _ => Err(format!(
            "application_argument_type_unsupported: argument {position} '{}' has unsupported CLI type {}; expected bool, str, Path, an integer primitive, f32, or f64",
            param.name,
            param.ty.to_human()
        )),
    }
}

fn integer_bounds(name: &str) -> Option<(i128, i128)> {
    match name {
        "u8" => Some((0, i128::from(u8::MAX))),
        "u16" => Some((0, i128::from(u16::MAX))),
        "u32" => Some((0, i128::from(u32::MAX))),
        "u64" | "usize" => Some((0, i128::from(i64::MAX))),
        "i8" => Some((i128::from(i8::MIN), i128::from(i8::MAX))),
        "i16" => Some((i128::from(i16::MIN), i128::from(i16::MAX))),
        "i32" => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
        "i64" | "isize" => Some((i128::from(i64::MIN), i128::from(i64::MAX))),
        _ => None,
    }
}

fn integer_syntax(source: &str, signed: bool) -> bool {
    let digits = if signed {
        source.strip_prefix('-').unwrap_or(source)
    } else {
        source
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn application_result_type_supported(ty: &Type) -> bool {
    match ty {
        Type::Bool | Type::Void => true,
        Type::Primitive(name) => {
            name == "str" || name == "f32" || name == "f64" || integer_bounds(name).is_some()
        }
        Type::Named(name) => name == "str" || name == "Path",
        _ => false,
    }
}

fn application_result(ty: &Type, value: &Value) -> Result<ApplicationResult, ApplicationError> {
    let mismatch = || {
        ApplicationError::new(
            "application_result_type_mismatch",
            "result",
            format!(
                "application returned a runtime value incompatible with declared type {}",
                ty.to_human()
            ),
        )
    };
    let (value_text, value_json) = match (ty, value) {
        (Type::Bool, Value::Bool(value)) => (value.to_string(), value.to_string()),
        (Type::Primitive(name), Value::Int(value)) if integer_bounds(name).is_some() => {
            (value.to_string(), value.to_string())
        }
        (Type::Primitive(name), Value::Float(value)) if name == "f32" || name == "f64" => {
            if !value.is_finite() {
                return Err(ApplicationError::new(
                    "application_result_non_finite",
                    "result",
                    format!("application returned a non-finite {name} value"),
                ));
            }
            (value.to_string(), value.to_string())
        }
        (Type::Primitive(name) | Type::Named(name), Value::String(value)) if name == "str" => {
            (value.clone(), json::string(value))
        }
        (Type::Named(name), Value::String(value)) if name == "Path" => {
            let path = crate::interp::normalize_portable_path(value).map_err(|detail| {
                ApplicationError::new(
                    "application_result_path_invalid",
                    "result",
                    format!("application returned an invalid Path: {detail}"),
                )
            })?;
            (path.clone(), json::string(&path))
        }
        (Type::Void, Value::Void) => ("void".to_string(), "null".to_string()),
        _ => return Err(mismatch()),
    };
    Ok(ApplicationResult {
        type_name: ty.to_human(),
        value_text,
        value_json,
    })
}

fn argument_invalid(position: usize, param: &Param, source: &str, detail: &str) -> String {
    format!(
        "application_argument_invalid: argument {position} '{}' expected {}, received '{}': {detail}",
        param.name,
        param.ty.to_human(),
        source.escape_default()
    )
}

fn argument_out_of_range(
    position: usize,
    param: &Param,
    source: &str,
    min: i128,
    max: i128,
) -> String {
    format!(
        "application_argument_out_of_range: argument {position} '{}' expected {} in {min}..{max}, received '{}'",
        param.name,
        param.ty.to_human(),
        source.escape_default()
    )
}

fn argument_out_of_range_text(position: usize, param: &Param, source: &str, range: &str) -> String {
    format!(
        "application_argument_out_of_range: argument {position} '{}' expected {} in {range}, received '{}'",
        param.name,
        param.ty.to_human(),
        source.escape_default()
    )
}

pub(crate) fn ensure_compatible_version(required: &str, running: &str) -> Result<(), String> {
    let required_line = parse_compatibility_line(required)?;
    let running_core = running
        .split(['-', '+'])
        .next()
        .unwrap_or(running)
        .split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".");
    let running_line = parse_compatibility_line(&running_core).map_err(|_| {
        format!(
            "application_version_runtime_invalid: running Forgentex version '{running}' is invalid"
        )
    })?;
    if required_line == running_line {
        Ok(())
    } else {
        Err(format!(
            "application_version_incompatible: application requires Forgentex {required}, running {running}"
        ))
    }
}

fn runtime_error(ops: &[IoOp], message: &str) -> ApplicationError {
    let code = stable_error_code(message, "application_runtime_error");
    let phase = if code == "artifact_required_missing" {
        "postflight"
    } else if ops
        .iter()
        .any(|operation| operation.decision == "deny" || operation.decision == "budget_exhausted")
    {
        "io"
    } else {
        "runtime"
    };
    ApplicationError::new(code, phase, message)
}

pub(crate) fn stable_error_code(message: &str, fallback: &str) -> String {
    let candidate = message
        .strip_prefix("trap: ")
        .unwrap_or(message)
        .split_once(':')
        .map(|(code, _)| code)
        .unwrap_or_default();
    if !candidate.is_empty()
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        candidate.to_string()
    } else {
        fallback.to_string()
    }
}

pub(crate) fn combined_program(project: &Project) -> Program {
    Program {
        items: project
            .modules
            .iter()
            .flat_map(|module| module.program.items.iter().cloned())
            .collect(),
    }
}

pub(crate) fn permission_scopes(permissions: &[ManifestPermission], name: &str) -> Vec<PathBuf> {
    permissions
        .iter()
        .filter(|permission| permission.name == name)
        .flat_map(|permission| permission.values.iter())
        .map(PathBuf::from)
        .collect()
}

pub(crate) fn validate_effect_scope(
    effect: &str,
    effects: &[String],
    scopes: &[PathBuf],
) -> Result<(), String> {
    let declared = effects.iter().any(|candidate| candidate == effect);
    if declared && scopes.is_empty() {
        return Err(format!(
            "application effect '{effect}' requires at least one [permissions] scope"
        ));
    }
    if !declared && !scopes.is_empty() {
        return Err(format!(
            "[permissions].{effect} scopes require '{effect}' in [application].effects"
        ));
    }
    Ok(())
}

fn application_status(ops: &[IoOp], error: Option<&ApplicationError>) -> &'static str {
    if error.is_none() {
        return "ok";
    }
    if ops
        .iter()
        .any(|operation| operation.decision == "budget_exhausted")
    {
        "budget_exhausted"
    } else if ops.iter().any(|operation| operation.decision == "deny") {
        "denied"
    } else {
        "error"
    }
}

fn display_scopes(scopes: &[PathBuf]) -> Vec<String> {
    scopes
        .iter()
        .map(|scope| scope.to_string_lossy().replace('\\', "/"))
        .collect()
}

impl ApplicationResult {
    pub(crate) fn to_json_object(&self) -> String {
        format!(
            "{{\"type\":{},\"value\":{}}}",
            json::string(&self.type_name),
            self.value_json
        )
    }
}

impl ApplicationError {
    pub fn new(
        code: impl Into<String>,
        phase: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            phase: phase.into(),
            message: message.into(),
        }
    }

    fn from_prefixed(phase: &str, message: String) -> Self {
        let code = stable_error_code(&message, "application_preflight_error");
        Self::new(code, phase, message)
    }

    pub fn contains(&self, value: &str) -> bool {
        self.code.contains(value) || self.phase.contains(value) || self.message.contains(value)
    }

    pub(crate) fn to_json_object(&self) -> String {
        format!(
            "{{\"code\":{},\"phase\":{},\"message\":{}}}",
            json::string(&self.code),
            json::string(&self.phase),
            json::string(&self.message)
        )
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"status\":\"error\",\"error\":{}}}",
            json::string(APPLICATION_ERROR_SCHEMA),
            self.to_json_object()
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex application error\nschema={} status=error code={} phase={}\n  {}\n",
            APPLICATION_ERROR_SCHEMA, self.code, self.phase, self.message
        )
    }
}

impl std::fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} phase={}: {}",
            self.code, self.phase, self.message
        )
    }
}

impl std::error::Error for ApplicationError {}

impl ApplicationRunReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex application run\n");
        out.push_str(&format!(
            "schema={} package={} version={} entry={} status={}\n",
            self.schema, self.package, self.version, self.entry, self.status
        ));
        out.push_str(&format!(
            "  forgentex_required={} forgentex_version={} compatibility=compatible\n",
            self.forgentex_required, self.forgentex_version
        ));
        out.push_str(&format!(
            "  runtime={} effects=[{}]\n",
            self.runtime,
            self.effects.join(", ")
        ));
        out.push_str(&format!(
            "  fs.read=[{}] max_read_bytes={}\n",
            self.read_scopes.join(", "),
            option_usize(self.max_read_bytes)
        ));
        out.push_str(&format!(
            "  fs.write=[{}] max_write_bytes={}\n",
            self.write_scopes.join(", "),
            option_usize(self.max_write_bytes)
        ));
        out.push_str(&format!(
            "  max_directory_entries={}\n",
            option_usize(self.max_directory_entries)
        ));
        for artifact in &self.artifact_declarations {
            out.push_str(&format!(
                "  artifact.declared name={} root={} media_type={} max_bytes={} required={} publication={}\n",
                artifact.name,
                artifact.root,
                artifact.media_type,
                artifact.max_bytes,
                artifact.required,
                artifact.publication
            ));
        }
        for operation in &self.ops {
            out.push_str(&format!(
                "  {:<6} {:<16} {} ({} bytes)\n",
                operation.op, operation.decision, operation.path, operation.bytes
            ));
        }
        out.push_str(&format!(
            "  read_bytes={} write_bytes={} directory_entries={} manifest_sha256={}\n",
            self.read_bytes, self.write_bytes, self.directory_entries, self.manifest_sha256
        ));
        for artifact in &self.artifacts {
            out.push_str(&format!(
                "  artifact.published name={} path={} media_type={} bytes={} sha256={} publication={}\n",
                artifact.name,
                artifact.path,
                artifact.media_type,
                artifact.bytes,
                artifact.sha256,
                artifact.publication
            ));
        }
        if let Some(result) = &self.result {
            out.push_str(&format!(
                "  result type={} value={}\n",
                result.type_name, result.value_text
            ));
        }
        if let Some(error) = &self.error {
            out.push_str(&format!(
                "  error code={} phase={} message={}\n",
                error.code, error.phase, error.message
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let ops = self
            .ops
            .iter()
            .map(|operation| {
                format!(
                    "{{\"op\":{},\"path\":{},\"bytes\":{},\"decision\":{}}}",
                    json::string(&operation.op),
                    json::string(&operation.path),
                    operation.bytes,
                    json::string(&operation.decision)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let artifact_declarations = self
            .artifact_declarations
            .iter()
            .map(|artifact| {
                format!(
                    "{{\"name\":{},\"root\":{},\"media_type\":{},\"max_bytes\":{},\"required\":{},\"publication\":{}}}",
                    json::string(&artifact.name),
                    json::string(&artifact.root),
                    json::string(&artifact.media_type),
                    artifact.max_bytes,
                    artifact.required,
                    json::string(&artifact.publication)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let artifacts = self
            .artifacts
            .iter()
            .map(|artifact| {
                format!(
                    "{{\"name\":{},\"path\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"publication\":{}}}",
                    json::string(&artifact.name),
                    json::string(&artifact.path),
                    json::string(&artifact.media_type),
                    artifact.bytes,
                    json::string(&artifact.sha256),
                    json::string(&artifact.publication)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let result = self
            .result
            .as_ref()
            .map(ApplicationResult::to_json_object)
            .unwrap_or_else(|| "null".to_string());
        let error = self
            .error
            .as_ref()
            .map(ApplicationError::to_json_object)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"schema\":{},\"package\":{},\"version\":{},\"forgentex_required\":{},\"forgentex_version\":{},\"forgentex_compatibility\":\"compatible\",\"entry\":{},\"runtime\":{},\"effects\":{},\"permissions\":{{\"fs.read\":{},\"fs.write\":{}}},\"limits\":{{\"max_read_bytes\":{},\"max_write_bytes\":{},\"max_directory_entries\":{}}},\"artifact_declarations\":[{}],\"status\":{},\"ops\":[{}],\"read_bytes\":{},\"write_bytes\":{},\"directory_entries\":{},\"artifacts\":[{}],\"result\":{},\"error\":{},\"manifest_sha256\":{}}}",
            json::string(&self.schema),
            json::string(&self.package),
            json::string(&self.version),
            json::string(&self.forgentex_required),
            json::string(&self.forgentex_version),
            json::string(&self.entry),
            json::string(&self.runtime),
            json::string_array(&self.effects),
            json::string_array(&self.read_scopes),
            json::string_array(&self.write_scopes),
            option_usize(self.max_read_bytes),
            option_usize(self.max_write_bytes),
            option_usize(self.max_directory_entries),
            artifact_declarations,
            json::string(&self.status),
            ops,
            self.read_bytes,
            self.write_bytes,
            self.directory_entries,
            artifacts,
            result,
            error,
            json::string(&self.manifest_sha256),
        )
    }
}

fn option_usize(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}
