//! Manifest-backed bridge from `app-run` to the governed pipeline runtime.

use std::path::{Path, PathBuf};

use crate::application::{
    combined_program, ensure_compatible_version, permission_scopes, stable_error_code,
    validate_effect_scope, ApplicationError,
};
use crate::interp::{IoArtifactPolicy, IoArtifactReceipt, IoContext, IoOp, IoPolicy};
use crate::json;
use crate::pipeline::{PipelineControl, PipelineLimits, PipelineReport, PipelineSchedule};
use crate::project::{check_project, load_project};

pub const PIPELINE_APPLICATION_SCHEMA: &str = "forgentex.pipeline_application_run.v1";

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineApplicationReport {
    pub schema: String,
    pub package: String,
    pub version: String,
    pub forgentex_required: String,
    pub forgentex_version: String,
    pub entry: String,
    pub status: String,
    pub input_shares: Vec<String>,
    pub output_share: String,
    pub output_path: String,
    pub effects: Vec<String>,
    pub pipeline: PipelineReport,
    pub ops: Vec<IoOp>,
    pub artifact: Option<IoArtifactReceipt>,
    pub manifest_sha256: String,
    pub error: Option<ApplicationError>,
}

pub fn run(input: &Path, args: Vec<String>) -> Result<PipelineApplicationReport, ApplicationError> {
    crate::security::enforce_package_security(input)
        .map_err(|message| ApplicationError::new("application_security", "security", message))?;
    let project = load_project(input).map_err(|message| {
        ApplicationError::new("application_manifest_invalid", "manifest", message)
    })?;
    let application = project.manifest.application.as_ref().ok_or_else(|| {
        ApplicationError::new(
            "application_contract_missing",
            "manifest",
            "package has no [application] contract",
        )
    })?;
    let configuration = application.pipeline.as_ref().ok_or_else(|| {
        ApplicationError::new(
            "application_pipeline_missing",
            "manifest",
            "pipeline application has no [application.pipeline] contract",
        )
    })?;
    let forgentex_version = env!("CARGO_PKG_VERSION").to_string();
    ensure_compatible_version(&application.forgentex, &forgentex_version).map_err(|message| {
        ApplicationError::new(
            stable_error_code(&message, "application_version_incompatible"),
            "compatibility",
            message,
        )
    })?;
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
                "application project has {} diagnostic(s)",
                diagnostics.len()
            ),
        ));
    }
    if configuration.inputs.len() != args.len() {
        return Err(ApplicationError::new(
            "application_argument_count",
            "arguments",
            format!(
                "pipeline application expects {} argument(s), received {}",
                configuration.inputs.len(),
                args.len()
            ),
        ));
    }
    let overrides = configuration
        .inputs
        .iter()
        .zip(args.iter())
        .map(|(name, value)| {
            let mut parsed = crate::parser::parse_capsule_values(value).map_err(|message| {
                ApplicationError::new("application_argument_invalid", "arguments", message)
            })?;
            if parsed.len() != 1 {
                return Err(ApplicationError::new(
                    "application_argument_invalid",
                    "arguments",
                    format!("pipeline input '{name}' requires exactly one scalar value"),
                ));
            }
            Ok((name.clone(), parsed.remove(0)))
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;

    let program = combined_program(&project);
    let declarations = program.pipelines().collect::<Vec<_>>();
    if declarations.len() != 1 || declarations[0].name != application.entry {
        return Err(ApplicationError::new(
            "application_pipeline_entry_invalid",
            "contract",
            format!(
                "pipeline application entry '{}' must name the project's sole pipeline declaration",
                application.entry
            ),
        ));
    }
    let write_scopes = permission_scopes(&project.manifest.security.permissions, "fs.write");
    validate_effect_scope("fs.write", &application.effects, &write_scopes).map_err(|message| {
        ApplicationError::new("application_permission_invalid", "policy", message)
    })?;

    let limits = PipelineLimits {
        max_stages: configuration.max_stages,
        max_wall_ms: configuration.max_wall_ms,
        max_cpu_ms: configuration.max_cpu_ms,
        max_memory_bytes: configuration.max_memory_bytes,
        max_owned_bytes: configuration.max_owned_bytes,
        max_fs_bytes: None,
        max_net_bytes: None,
    };
    let file = project
        .modules
        .first()
        .map(|module| module.path.display().to_string())
        .unwrap_or_else(|| project.manifest_path.display().to_string());
    let pipeline = crate::pipeline::run_with_control(
        &file,
        &program,
        overrides,
        Some(application.effects.clone()),
        limits,
        PipelineSchedule::SourceGraph,
        PipelineControl::default(),
    );

    let mut ops = Vec::new();
    let mut artifact = None;
    let mut error = None;
    if pipeline.is_success() {
        match publish_pipeline_output(
            &project.package_root,
            application,
            configuration,
            &write_scopes,
            &pipeline,
        ) {
            Ok((published_ops, published_artifact)) => {
                ops = published_ops;
                artifact = Some(published_artifact);
            }
            Err((published_ops, message)) => {
                ops = published_ops;
                error = Some(ApplicationError::new(
                    stable_error_code(&message, "application_pipeline_publish_failed"),
                    "io",
                    message,
                ));
            }
        }
    } else {
        error = Some(ApplicationError::new(
            "application_pipeline_failed",
            "runtime",
            if pipeline.diagnostics.is_empty() {
                format!("pipeline completed with status {}", pipeline.status)
            } else {
                pipeline.diagnostics.join("; ")
            },
        ));
    }
    let status = if error.is_none() {
        "ok"
    } else if ops.iter().any(|operation| operation.decision == "deny") {
        "denied"
    } else if ops
        .iter()
        .any(|operation| operation.decision == "budget_exhausted")
    {
        "budget_exhausted"
    } else {
        "error"
    };

    Ok(PipelineApplicationReport {
        schema: PIPELINE_APPLICATION_SCHEMA.to_string(),
        package: project.manifest.name.clone(),
        version: project.manifest.version.clone(),
        forgentex_required: application.forgentex.clone(),
        forgentex_version,
        entry: application.entry.clone(),
        status: status.to_string(),
        input_shares: configuration.inputs.clone(),
        output_share: configuration.output_share.clone(),
        output_path: configuration.output_path.clone(),
        effects: application.effects.clone(),
        pipeline,
        ops,
        artifact,
        manifest_sha256: crate::crypto::sha256_hex(&manifest_bytes),
        error,
    })
}

fn publish_pipeline_output(
    package_root: &Path,
    application: &crate::project::ApplicationManifest,
    configuration: &crate::project::ApplicationPipelineManifest,
    write_scopes: &[PathBuf],
    pipeline: &PipelineReport,
) -> Result<(Vec<IoOp>, IoArtifactReceipt), (Vec<IoOp>, String)> {
    let bytes = pipeline
        .owned_u8_output(&configuration.output_share)
        .map_err(|message| (Vec::new(), message))?;
    let declaration = application
        .artifacts
        .first()
        .expect("pipeline manifests require exactly one artifact");
    let evidence = pipeline
        .stages
        .iter()
        .find(|stage| stage.target == configuration.output_share)
        .and_then(|stage| stage.artifact.as_ref())
        .ok_or_else(|| {
            (
                Vec::new(),
                "pipeline output has no governed native artifact evidence".to_string(),
            )
        })?;
    if evidence.media_type != declaration.media_type
        || evidence.bytes != bytes.len()
        || evidence.sha256 != crate::crypto::sha256_hex(&bytes)
    {
        return Err((
            Vec::new(),
            "pipeline output bytes do not match the declared artifact media type and native receipt"
                .to_string(),
        ));
    }
    let policy = IoPolicy::scoped(
        package_root,
        &[],
        write_scopes,
        None,
        application.max_write_bytes,
        None,
        vec![IoArtifactPolicy {
            name: declaration.name.clone(),
            root: PathBuf::from(&declaration.root),
            media_type: declaration.media_type.clone(),
            max_bytes: declaration.max_bytes,
            required: declaration.required,
            publication: declaration.publication.clone(),
        }],
    )
    .map_err(|message| (Vec::new(), message))?;
    let io = IoContext::from_policy(policy);
    if let Err(message) = io.publish_artifact(
        "publish_pipeline_artifact",
        &declaration.name,
        &configuration.output_path,
        &bytes,
    ) {
        return Err((io.ops(), message));
    }
    let published = io
        .artifacts()
        .into_iter()
        .next()
        .expect("successful pipeline artifact publication records a receipt");
    Ok((io.ops(), published))
}

impl PipelineApplicationReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "Forgentex pipeline application\nschema={} package={} version={} entry={} status={}\noutput_share={} output_path={}\n",
            self.schema,
            self.package,
            self.version,
            self.entry,
            self.status,
            self.output_share,
            self.output_path
        );
        out.push_str(&self.pipeline.to_text());
        if let Some(artifact) = &self.artifact {
            out.push_str(&format!(
                "application artifact={} media_type={} bytes={} sha256={} path={}\n",
                artifact.name, artifact.media_type, artifact.bytes, artifact.sha256, artifact.path
            ));
        }
        if let Some(error) = &self.error {
            out.push_str(&format!(
                "application error code={} phase={} message={}\n",
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
        let artifact = self
            .artifact
            .as_ref()
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
            .unwrap_or_else(|| "null".to_string());
        let error = self
            .error
            .as_ref()
            .map(ApplicationError::to_json_object)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"schema\":{},\"package\":{},\"version\":{},\"forgentex_required\":{},\"forgentex_version\":{},\"entry\":{},\"runtime\":\"pipeline\",\"status\":{},\"input_shares\":{},\"output_share\":{},\"output_path\":{},\"effects\":{},\"ops\":[{}],\"artifact\":{},\"error\":{},\"manifest_sha256\":{},\"pipeline\":{}}}",
            json::string(&self.schema),
            json::string(&self.package),
            json::string(&self.version),
            json::string(&self.forgentex_required),
            json::string(&self.forgentex_version),
            json::string(&self.entry),
            json::string(&self.status),
            json::string_array(&self.input_shares),
            json::string(&self.output_share),
            json::string(&self.output_path),
            json::string_array(&self.effects),
            ops,
            artifact,
            error,
            json::string(&self.manifest_sha256),
            self.pipeline.to_json().trim()
        )
    }
}
