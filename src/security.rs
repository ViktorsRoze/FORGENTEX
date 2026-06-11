use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::effects::analyze as analyze_effects;
use crate::package::generate_lock;
use crate::project::{load_project, read_project_manifest, ProjectManifest, ProjectSecurity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupplyChainIssue {
    pub package: String,
    pub severity: String,
    pub code: String,
    pub message: String,
    pub required_permission: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageSecurityReview {
    pub name: String,
    pub version: String,
    pub trust: String,
    pub permissions: Vec<String>,
    pub sandbox: Vec<String>,
    pub actions: Vec<String>,
    pub signature_status: String,
    pub reproducible_hash: String,
    pub issues: Vec<SupplyChainIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityAuditReport {
    pub schema: String,
    pub status: String,
    pub packages: Vec<PackageSecurityReview>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageVerifyResult {
    pub name: String,
    pub version: String,
    pub signed_by: Option<String>,
    pub signature_algorithm: Option<String>,
    pub signature_status: String,
    pub reproducible: bool,
    pub reproducible_hash: String,
    pub checks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub schema: String,
    pub status: String,
    pub packages: Vec<PackageVerifyResult>,
}

pub fn audit_project(input: &Path) -> Result<SecurityAuditReport, String> {
    let lock = generate_lock(input)?;
    let mut packages = Vec::new();
    for package in &lock.packages {
        let (_, root, manifest) = read_project_manifest(&package.root)?;
        packages.push(review_package(&root, manifest)?);
    }
    let status = report_status(packages.iter().flat_map(|package| package.issues.iter()));
    Ok(SecurityAuditReport {
        schema: "forgentex.audit.v1".to_string(),
        status,
        packages,
    })
}

pub fn verify_project(input: &Path) -> Result<VerifyReport, String> {
    let lock = generate_lock(input)?;
    let mut packages = Vec::new();
    for package in &lock.packages {
        let (_, root, manifest) = read_project_manifest(&package.root)?;
        let signature_status = package_signature_status(&manifest);
        let mut checks = Vec::new();
        checks.push(format!("signature={signature_status}"));
        checks.push(format!(
            "reproducible={}",
            if manifest.security.reproducible {
                "declared"
            } else {
                "missing"
            }
        ));
        checks.push("registry=local-path-only".to_string());
        packages.push(PackageVerifyResult {
            name: manifest.name,
            version: manifest.version,
            signed_by: manifest.security.signed_by,
            signature_algorithm: manifest.security.signature_algorithm,
            signature_status,
            reproducible: manifest.security.reproducible,
            reproducible_hash: reproducible_hash(&root)?,
            checks,
        });
    }
    let status = if packages
        .iter()
        .all(|package| package.signature_status == "mock-accepted" && package.reproducible)
    {
        "pass"
    } else {
        "fail"
    };
    Ok(VerifyReport {
        schema: "forgentex.verify.v1".to_string(),
        status: status.to_string(),
        packages,
    })
}

pub fn enforce_package_security(input: &Path) -> Result<(), String> {
    let report = audit_project(input)?;
    if report.is_success() {
        Ok(())
    } else {
        Err(format!(
            "supply-chain audit failed before package action:\n{}",
            report.to_text()
        ))
    }
}

pub fn package_signature_status(manifest: &ProjectManifest) -> String {
    signature_status(&manifest.security)
}

impl SecurityAuditReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass" || self.status == "warn"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex supply-chain audit\n");
        out.push_str(&format!("schema={} status={}\n", self.schema, self.status));
        for package in &self.packages {
            out.push_str(&format!(
                "  - {} {} trust={} signature={} reproducible_hash={}\n",
                package.name,
                package.version,
                package.trust,
                package.signature_status,
                package.reproducible_hash
            ));
            out.push_str(&format!(
                "      permissions=[{}]\n",
                package.permissions.join(",")
            ));
            out.push_str(&format!("      sandbox=[{}]\n", package.sandbox.join(",")));
            out.push_str(&format!("      actions=[{}]\n", package.actions.join(",")));
            for issue in &package.issues {
                out.push_str(&format!(
                    "      issue severity={} code={} message={}\n",
                    issue.severity, issue.code, issue.message
                ));
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"status\": {},\n", quoted(&self.status)));
        out.push_str("  \"packages\": [");
        for (idx, package) in self.packages.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"name\": {}, \"version\": {}, \"trust\": {}, \"signature_status\": {}, \"reproducible_hash\": {}, ",
                quoted(&package.name),
                quoted(&package.version),
                quoted(&package.trust),
                quoted(&package.signature_status),
                quoted(&package.reproducible_hash)
            ));
            push_string_array_inline(&mut out, "permissions", &package.permissions);
            out.push_str(", ");
            push_string_array_inline(&mut out, "sandbox", &package.sandbox);
            out.push_str(", ");
            push_string_array_inline(&mut out, "actions", &package.actions);
            out.push_str(", \"issues\": [");
            for (issue_idx, issue) in package.issues.iter().enumerate() {
                if issue_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"severity\": {}, \"code\": {}, \"message\": {}, \"required_permission\": {}}}",
                    quoted(&issue.severity),
                    quoted(&issue.code),
                    quoted(&issue.message),
                    optional_quoted(issue.required_permission.as_deref())
                ));
            }
            out.push_str("]}");
        }
        if !self.packages.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

impl VerifyReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex package verification\n");
        out.push_str(&format!("schema={} status={}\n", self.schema, self.status));
        for package in &self.packages {
            out.push_str(&format!(
                "  - {} {} signature={} reproducible={} hash={} checks=[{}]\n",
                package.name,
                package.version,
                package.signature_status,
                package.reproducible,
                package.reproducible_hash,
                package.checks.join(",")
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", quoted(&self.schema)));
        out.push_str(&format!("  \"status\": {},\n", quoted(&self.status)));
        out.push_str("  \"packages\": [");
        for (idx, package) in self.packages.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "\n    {{\"name\": {}, \"version\": {}, \"signed_by\": {}, \"signature_algorithm\": {}, \"signature_status\": {}, \"reproducible\": {}, \"reproducible_hash\": {}, ",
                quoted(&package.name),
                quoted(&package.version),
                optional_quoted(package.signed_by.as_deref()),
                optional_quoted(package.signature_algorithm.as_deref()),
                quoted(&package.signature_status),
                bool_json(package.reproducible),
                quoted(&package.reproducible_hash)
            ));
            push_string_array_inline(&mut out, "checks", &package.checks);
            out.push('}');
        }
        if !self.packages.is_empty() {
            out.push('\n');
            out.push_str("  ");
        }
        out.push_str("]\n}");
        out
    }
}

fn review_package(root: &Path, manifest: ProjectManifest) -> Result<PackageSecurityReview, String> {
    let signature_status = signature_status(&manifest.security);
    let mut issues = Vec::new();
    let permission_names = permission_names(&manifest.security);
    let untrusted = manifest.security.trust == "untrusted";

    if !matches!(manifest.security.trust.as_str(), "trusted" | "untrusted") {
        issues.push(issue(
            &manifest.name,
            "high",
            "unknown_trust_level",
            format!("unknown trust level '{}'", manifest.security.trust),
            None,
        ));
    }
    if signature_status == "invalid" {
        issues.push(issue(
            &manifest.name,
            "high",
            "invalid_package_signature",
            "package signature metadata is incomplete or invalid".to_string(),
            Some("crypto.verify".to_string()),
        ));
    } else if untrusted && signature_status == "missing" {
        issues.push(issue(
            &manifest.name,
            "high",
            "missing_package_signature",
            "untrusted package must carry signed package metadata".to_string(),
            Some("crypto.verify".to_string()),
        ));
    }
    if !manifest.security.reproducible {
        issues.push(issue(
            &manifest.name,
            "medium",
            "missing_reproducible_metadata",
            "package has not declared reproducible build metadata".to_string(),
            None,
        ));
    }

    for (action, value, permission) in manifest.security.actions.declared() {
        if untrusted && !has_permission(&manifest.security, permission) {
            issues.push(issue(
                &manifest.name,
                "high",
                "undeclared_permission",
                format!("untrusted package action '{action}' uses '{value}'"),
                Some(permission.to_string()),
            ));
        }
        if untrusted && sandbox_for_action(&manifest.security, action) == "deny" {
            issues.push(issue(
                &manifest.name,
                "high",
                "sandbox_denies_action",
                format!("sandbox policy denies action '{action}'"),
                Some(permission.to_string()),
            ));
        }
    }

    for (effect, permission) in required_effect_permissions(root, &manifest.name)? {
        if untrusted && !has_permission(&manifest.security, &permission) {
            issues.push(issue(
                &manifest.name,
                "high",
                "missing_effect_permission",
                format!("effect '{effect}' requires manifest permission '{permission}'"),
                Some(permission),
            ));
        }
    }

    Ok(PackageSecurityReview {
        name: manifest.name.clone(),
        version: manifest.version,
        trust: manifest.security.trust.clone(),
        permissions: permission_names,
        sandbox: sandbox_entries(&manifest.security),
        actions: manifest
            .security
            .actions
            .declared()
            .into_iter()
            .map(|(name, value, _)| format!("{name}={value}"))
            .collect(),
        signature_status,
        reproducible_hash: reproducible_hash(root)?,
        issues,
    })
}

fn required_effect_permissions(
    root: &Path,
    package_name: &str,
) -> Result<BTreeSet<(String, String)>, String> {
    let project = load_project(root)?;
    let mut items = Vec::new();
    for module in &project.modules {
        if module.package == package_name && !module.dependency && !module.standard {
            items.extend(module.program.items.clone());
        }
    }
    let mut required = BTreeSet::new();
    for function in analyze_effects(&Program { items }) {
        for effect in function
            .declared
            .iter()
            .chain(function.inferred.iter())
            .chain(function.capabilities.iter())
        {
            if let Some(permission) = permission_for_effect(effect) {
                required.insert((effect.clone(), permission.to_string()));
            }
        }
    }
    Ok(required)
}

fn permission_for_effect(effect: &str) -> Option<&'static str> {
    if effect.starts_with("fs.") {
        Some("fs")
    } else if effect.starts_with("network.") {
        Some("network")
    } else if effect.starts_with("crypto.") {
        Some("crypto")
    } else if effect.starts_with("io.") {
        Some("io")
    } else if effect.starts_with("agent.") || effect.starts_with("ledger.") {
        Some("agent")
    } else {
        None
    }
}

fn has_permission(security: &ProjectSecurity, permission: &str) -> bool {
    security.permissions.iter().any(|candidate| {
        candidate.name == permission
            || candidate
                .name
                .strip_prefix(permission)
                .is_some_and(|rest| rest.starts_with('.'))
            || permission
                .strip_prefix(&candidate.name)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

fn permission_names(security: &ProjectSecurity) -> Vec<String> {
    security
        .permissions
        .iter()
        .map(|permission| {
            if permission.values.is_empty() {
                permission.name.clone()
            } else {
                format!("{}=[{}]", permission.name, permission.values.join("|"))
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sandbox_entries(security: &ProjectSecurity) -> Vec<String> {
    vec![
        format!("build_scripts={}", security.sandbox.build_scripts),
        format!("codegen={}", security.sandbox.codegen),
        format!("tests={}", security.sandbox.tests),
        format!("agent_tools={}", security.sandbox.agent_tools),
    ]
}

fn sandbox_for_action<'a>(security: &'a ProjectSecurity, action: &str) -> &'a str {
    match action {
        "build_script" => &security.sandbox.build_scripts,
        "codegen" => &security.sandbox.codegen,
        "test_runner" => &security.sandbox.tests,
        "agent_tool" => &security.sandbox.agent_tools,
        _ => "deny",
    }
}

fn signature_status(security: &ProjectSecurity) -> String {
    let has_any = security.signed_by.is_some()
        || security.signature_algorithm.is_some()
        || security.signature.is_some();
    if !has_any {
        return "missing".to_string();
    }
    if security.signed_by.is_some()
        && matches!(security.signature_algorithm.as_deref(), Some("Ed25519"))
        && security
            .signature
            .as_deref()
            .is_some_and(|signature| signature.starts_with("mock-sig-"))
    {
        "mock-accepted".to_string()
    } else {
        "invalid".to_string()
    }
}

fn reproducible_hash(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_reproducible_files(root, &mut files)?;
    files.sort();
    let mut material = String::new();
    for file in files {
        let relative = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string()
            .replace('\\', "/");
        let source = fs::read_to_string(&file)
            .map_err(|err| format!("failed to read {}: {err}", file.display()))?;
        material.push_str(&relative);
        material.push('\n');
        material.push_str(&source);
        material.push('\n');
    }
    Ok(stable_hash(&material))
}

fn collect_reproducible_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if root.is_file() {
        if is_reproducible_file(root) {
            files.push(root.to_path_buf());
        }
        return Ok(());
    }
    for entry in
        fs::read_dir(root).map_err(|err| format!("failed to read {}: {err}", root.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if matches!(name, "target" | "build" | ".git") {
            continue;
        }
        if path.is_dir() {
            collect_reproducible_files(&path, files)?;
        } else if is_reproducible_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_reproducible_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("Forgentex.toml")
        || matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("fgx") | Some("fgai") | Some("fgp") | Some("fgmap")
        )
}

fn report_status<'a>(issues: impl Iterator<Item = &'a SupplyChainIssue>) -> String {
    let severities = issues
        .map(|issue| issue.severity.as_str())
        .collect::<BTreeSet<_>>();
    if severities.contains("high") {
        "fail"
    } else if severities.contains("medium") {
        "warn"
    } else {
        "pass"
    }
    .to_string()
}

fn issue(
    package: &str,
    severity: &str,
    code: &str,
    message: String,
    required_permission: Option<String>,
) -> SupplyChainIssue {
    SupplyChainIssue {
        package: package.to_string(),
        severity: severity.to_string(),
        code: code.to_string(),
        message,
        required_permission,
    }
}

fn stable_hash(source: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn push_string_array_inline(out: &mut String, key: &str, values: &[String]) {
    out.push_str(&format!("\"{key}\": ["));
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&quoted(value));
    }
    out.push(']');
}

fn optional_quoted(value: Option<&str>) -> String {
    value.map(quoted).unwrap_or_else(|| "null".to_string())
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
