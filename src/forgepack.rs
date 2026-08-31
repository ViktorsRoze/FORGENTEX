//! Frozen `forgepack` v1 archive codec.
//!
//! This module owns byte-level validation only. Filesystem discovery,
//! publication, and extraction remain in the manifest-backed application
//! runtime so archive parsing cannot broaden filesystem authority.

use crate::interp::{
    atomic_publish_new, canonical_json, normalize_portable_path, parse_json_value, JsonValue,
};
use crate::{crypto, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAGIC: [u8; 8] = *b"FGXPACK\0";
pub const VERSION: u16 = 1;
pub const FLAGS: u16 = 0;
pub const HEADER_BYTES: usize = 24;
pub const MANIFEST_SCHEMA: &str = "forgentex.forgepack_manifest.v1";
pub const MAX_ENTRIES: usize = 4_096;
pub const MAX_PATH_BYTES: usize = 1_048_576;
pub const MAX_MANIFEST_BYTES: usize = 4_194_304;
pub const MAX_PAYLOAD_BYTES: usize = 1_073_741_824;
pub const BUILD_SPEC_SCHEMA: &str = "forgentex.forgepack_build_spec.v1";
pub const BUILD_REPORT_SCHEMA: &str = "forgentex.forgepack_build.v1";
pub const INSPECT_REPORT_SCHEMA: &str = "forgentex.forgepack_inspect.v1";
pub const VERIFY_REPORT_SCHEMA: &str = "forgentex.forgepack_verify.v1";
pub const EXTRACT_REPORT_SCHEMA: &str = "forgentex.forgepack_extract.v1";
pub const EVIDENCE_REPORT_SCHEMA: &str = "forgentex.forgepack_evidence.v1";

static EVIDENCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildEntry {
    pub path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub media_type: String,
    pub offset: u32,
    pub length: u32,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive<'a> {
    pub version: u16,
    pub flags: u16,
    pub manifest_bytes: &'a [u8],
    pub manifest: Manifest,
    pub payload: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSpecEntry {
    pub source: String,
    pub path: String,
    pub media_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildReport {
    pub archive: String,
    pub entries: usize,
    pub manifest_bytes: usize,
    pub payload_bytes: usize,
    pub archive_bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub archive: String,
    pub entries: usize,
    pub manifest_bytes: usize,
    pub payload_bytes: usize,
    pub archive_bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectReport {
    pub archive: String,
    pub version: u16,
    pub flags: u16,
    pub entries: Vec<Entry>,
    pub manifest_bytes: usize,
    pub payload_bytes: usize,
    pub archive_bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedEntry {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractReport {
    pub archive: String,
    pub directory: String,
    pub entries: Vec<ExtractedEntry>,
    pub bytes: usize,
    pub archive_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceCheck {
    pub id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceReport {
    pub checks: Vec<EvidenceCheck>,
}

impl BuildReport {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"status\":\"ok\",\"archive\":{},\"entries\":{},\"manifest_bytes\":{},\"payload_bytes\":{},\"archive_bytes\":{},\"sha256\":{}}}",
            json::string(BUILD_REPORT_SCHEMA),
            json::string(&self.archive),
            self.entries,
            self.manifest_bytes,
            self.payload_bytes,
            self.archive_bytes,
            json::string(&self.sha256)
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "forgepack build ok\n  archive={}\n  entries={} manifest_bytes={} payload_bytes={} archive_bytes={}\n  sha256={}\n",
            self.archive,
            self.entries,
            self.manifest_bytes,
            self.payload_bytes,
            self.archive_bytes,
            self.sha256
        )
    }
}

impl VerifyReport {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"status\":\"ok\",\"archive\":{},\"entries\":{},\"manifest_bytes\":{},\"payload_bytes\":{},\"archive_bytes\":{},\"sha256\":{},\"hashes_verified\":{}}}",
            json::string(VERIFY_REPORT_SCHEMA),
            json::string(&self.archive),
            self.entries,
            self.manifest_bytes,
            self.payload_bytes,
            self.archive_bytes,
            json::string(&self.sha256),
            self.entries
        )
    }

    pub fn to_text(&self) -> String {
        format!(
            "forgepack verify ok\n  archive={}\n  entries={} hashes_verified={} manifest_bytes={} payload_bytes={} archive_bytes={}\n  sha256={}\n",
            self.archive,
            self.entries,
            self.entries,
            self.manifest_bytes,
            self.payload_bytes,
            self.archive_bytes,
            self.sha256
        )
    }
}

impl InspectReport {
    pub fn to_json(&self) -> String {
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{{\"path\":{},\"media_type\":{},\"offset\":{},\"length\":{},\"sha256\":{}}}",
                    json::string(&entry.path),
                    json::string(&entry.media_type),
                    entry.offset,
                    entry.length,
                    json::string(&entry.sha256)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":{},\"status\":\"ok\",\"archive\":{},\"version\":{},\"flags\":{},\"structure_valid\":true,\"hashes_verified\":false,\"entries\":[{}],\"manifest_bytes\":{},\"payload_bytes\":{},\"archive_bytes\":{},\"sha256\":{}}}",
            json::string(INSPECT_REPORT_SCHEMA),
            json::string(&self.archive),
            self.version,
            self.flags,
            entries,
            self.manifest_bytes,
            self.payload_bytes,
            self.archive_bytes,
            json::string(&self.sha256)
        )
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "forgepack inspect ok\n  archive={}\n  version={} flags={} structure_valid=true hashes_verified=false\n  entries={} manifest_bytes={} payload_bytes={} archive_bytes={}\n  sha256={}\n",
            self.archive,
            self.version,
            self.flags,
            self.entries.len(),
            self.manifest_bytes,
            self.payload_bytes,
            self.archive_bytes,
            self.sha256
        );
        for entry in &self.entries {
            out.push_str(&format!(
                "  entry path={} media_type={} offset={} length={} sha256={}\n",
                entry.path, entry.media_type, entry.offset, entry.length, entry.sha256
            ));
        }
        out
    }
}

impl ExtractReport {
    pub fn to_json(&self) -> String {
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{{\"path\":{},\"bytes\":{},\"sha256\":{}}}",
                    json::string(&entry.path),
                    entry.bytes,
                    json::string(&entry.sha256)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":{},\"status\":\"ok\",\"archive\":{},\"directory\":{},\"entries\":[{}],\"entry_count\":{},\"bytes\":{},\"archive_sha256\":{},\"publication\":\"create_new\",\"transactional\":false}}",
            json::string(EXTRACT_REPORT_SCHEMA),
            json::string(&self.archive),
            json::string(&self.directory),
            entries,
            self.entries.len(),
            self.bytes,
            json::string(&self.archive_sha256)
        )
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "forgepack extract ok\n  archive={}\n  directory={}\n  entries={} bytes={} publication=create_new transactional=false\n  archive_sha256={}\n",
            self.archive,
            self.directory,
            self.entries.len(),
            self.bytes,
            self.archive_sha256
        );
        for entry in &self.entries {
            out.push_str(&format!(
                "  output path={} bytes={} sha256={}\n",
                entry.path, entry.bytes, entry.sha256
            ));
        }
        out
    }
}

impl EvidenceReport {
    pub fn is_success(&self) -> bool {
        self.checks.iter().all(|check| check.status == "pass")
    }

    pub fn gaps(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|check| check.status != "pass")
            .map(|check| format!("{}: {}", check.id, check.detail))
            .collect()
    }

    pub fn to_json(&self) -> String {
        let checks = self
            .checks
            .iter()
            .map(|check| {
                format!(
                    "{{\"id\":{},\"status\":{},\"detail\":{}}}",
                    json::string(&check.id),
                    json::string(&check.status),
                    json::string(&check.detail)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let gaps = self
            .gaps()
            .iter()
            .map(|gap| json::string(gap))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":{},\"status\":{},\"checks_pass\":{},\"checks_fail\":{},\"checks\":[{}],\"gaps\":[{}]}}",
            json::string(EVIDENCE_REPORT_SCHEMA),
            json::string(if self.is_success() { "pass" } else { "fail" }),
            self.checks.iter().filter(|check| check.status == "pass").count(),
            self.checks.iter().filter(|check| check.status != "pass").count(),
            checks,
            gaps
        )
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "forgepack workflow evidence status={} checks_pass={} checks_fail={}\n",
            if self.is_success() { "pass" } else { "fail" },
            self.checks
                .iter()
                .filter(|check| check.status == "pass")
                .count(),
            self.checks
                .iter()
                .filter(|check| check.status != "pass")
                .count()
        );
        for check in &self.checks {
            out.push_str(&format!(
                "  {} status={} detail={}\n",
                check.id, check.status, check.detail
            ));
        }
        out
    }
}

pub fn build_file(spec_path: &Path, output: &str) -> Result<BuildReport, String> {
    let spec_path = fs::canonicalize(spec_path).map_err(|source| {
        error(
            "spec_read",
            format!("failed to resolve {}: {source}", spec_path.display()),
        )
    })?;
    if !spec_path.is_file() {
        return Err(error(
            "spec_read",
            format!("build spec is not a file: {}", spec_path.display()),
        ));
    }
    let root = spec_path
        .parent()
        .ok_or_else(|| error("spec_read", "build spec has no parent directory"))?
        .to_path_buf();
    let spec_bytes = read_bounded(&spec_path, MAX_MANIFEST_BYTES, "build spec")?;
    let spec_text = std::str::from_utf8(&spec_bytes)
        .map_err(|_| error("spec_utf8", "build spec is not valid UTF-8"))?;
    let entries = parse_build_spec(spec_text)?;

    let mut inputs = Vec::with_capacity(entries.len());
    let mut total = 0usize;
    for entry in entries {
        let source = resolve_existing_beneath(&root, &entry.source, "source")?;
        if !source.is_file() {
            return Err(error(
                "source_read",
                format!("source is not a regular file: {}", source.display()),
            ));
        }
        let remaining = MAX_PAYLOAD_BYTES
            .checked_sub(total)
            .ok_or_else(|| error("payload_limit", "payload limit exhausted"))?;
        let bytes = read_bounded(&source, remaining, "source")?;
        total += bytes.len();
        inputs.push(BuildEntry {
            path: entry.path,
            media_type: entry.media_type,
            bytes,
        });
    }
    let bytes = build(inputs)?;
    let archive = inspect(&bytes)?;
    let output_path = resolve_new_beneath(&root, output)?;
    atomic_publish_new(&output_path, &bytes)
        .map_err(|detail| error("publish", strip_trap_prefix(&detail)))?;
    Ok(BuildReport {
        archive: output.to_string(),
        entries: archive.manifest.entries.len(),
        manifest_bytes: archive.manifest_bytes.len(),
        payload_bytes: archive.payload.len(),
        archive_bytes: bytes.len(),
        sha256: crypto::sha256_hex(&bytes),
    })
}

pub fn verify_file(path: &Path) -> Result<VerifyReport, String> {
    let bytes = read_archive_file(path)?;
    let archive = verify(&bytes)?;
    Ok(VerifyReport {
        archive: display_path(path),
        entries: archive.manifest.entries.len(),
        manifest_bytes: archive.manifest_bytes.len(),
        payload_bytes: archive.payload.len(),
        archive_bytes: bytes.len(),
        sha256: crypto::sha256_hex(&bytes),
    })
}

pub fn inspect_file(path: &Path) -> Result<InspectReport, String> {
    let bytes = read_archive_file(path)?;
    let archive = inspect(&bytes)?;
    Ok(InspectReport {
        archive: display_path(path),
        version: archive.version,
        flags: archive.flags,
        entries: archive.manifest.entries,
        manifest_bytes: archive.manifest_bytes.len(),
        payload_bytes: archive.payload.len(),
        archive_bytes: bytes.len(),
        sha256: crypto::sha256_hex(&bytes),
    })
}

pub fn extract_file(archive_path: &Path, directory: &Path) -> Result<ExtractReport, String> {
    let bytes = read_archive_file(archive_path)?;
    let archive = verify(&bytes)?;
    let root = fs::canonicalize(directory).map_err(|source| {
        error(
            "extract_root",
            format!(
                "failed to resolve extraction root {}: {source}",
                directory.display()
            ),
        )
    })?;
    if !root.is_dir() {
        return Err(error(
            "extract_root",
            format!("extraction root is not a directory: {}", root.display()),
        ));
    }

    for entry in &archive.manifest.entries {
        preflight_extract_target(&root, &entry.path)?;
    }

    let mut extracted = Vec::with_capacity(archive.manifest.entries.len());
    let mut extracted_bytes = 0usize;
    for entry in &archive.manifest.entries {
        let parent = ensure_extract_parent(&root, &entry.path)?;
        let name = Path::new(&entry.path)
            .file_name()
            .ok_or_else(|| error("extract_path", "entry path has no file name"))?;
        let target = parent.join(name);
        let start = entry.offset as usize;
        let end = start + entry.length as usize;
        atomic_publish_new(&target, &archive.payload[start..end])
            .map_err(|detail| error("extract_publish", strip_trap_prefix(&detail)))?;
        extracted_bytes += entry.length as usize;
        extracted.push(ExtractedEntry {
            path: entry.path.clone(),
            bytes: entry.length as usize,
            sha256: entry.sha256.clone(),
        });
    }
    Ok(ExtractReport {
        archive: display_path(archive_path),
        directory: display_path(directory),
        entries: extracted,
        bytes: extracted_bytes,
        archive_sha256: crypto::sha256_hex(&bytes),
    })
}

pub fn workflow_evidence() -> EvidenceReport {
    let mut checks = Vec::new();
    let root = match create_evidence_root() {
        Ok(root) => {
            pass(&mut checks, "fixture_setup", "isolated fixture created");
            root
        }
        Err(detail) => {
            fail(&mut checks, "fixture_setup", detail);
            return EvidenceReport { checks };
        }
    };

    let setup = (|| -> Result<(), String> {
        fs::create_dir(root.join("input"))
            .map_err(|source| format!("failed to create input directory: {source}"))?;
        fs::create_dir(root.join("output"))
            .map_err(|source| format!("failed to create output directory: {source}"))?;
        fs::create_dir(root.join("extract"))
            .map_err(|source| format!("failed to create extraction directory: {source}"))?;
        fs::write(root.join("input/readme.txt"), b"forgepack clean install\n")
            .map_err(|source| format!("failed to write text fixture: {source}"))?;
        fs::write(root.join("input/data.bin"), [0, 1, 2, 255])
            .map_err(|source| format!("failed to write binary fixture: {source}"))?;
        fs::write(
            root.join("build.json"),
            br#"{"entries":[{"media_type":"text/plain","path":"docs/readme.txt","source":"input/readme.txt"},{"media_type":"application/octet-stream","path":"payload/data.bin","source":"input/data.bin"}],"schema":"forgentex.forgepack_build_spec.v1"}"#,
        )
        .map_err(|source| format!("failed to write build spec: {source}"))?;
        Ok(())
    })();
    if let Err(detail) = setup {
        fail(&mut checks, "fixture_materialization", detail);
        let _ = fs::remove_dir_all(&root);
        return EvidenceReport { checks };
    }
    pass(
        &mut checks,
        "fixture_materialization",
        "text, binary, and build-spec fixtures written",
    );

    let spec = root.join("build.json");
    let first_path = root.join("output/first.fpk");
    let second_path = root.join("output/second.fpk");
    let first = build_file(&spec, "output/first.fpk");
    match &first {
        Ok(report) if report.entries == 2 => pass(&mut checks, "build", "two-entry archive built"),
        Ok(report) => fail(
            &mut checks,
            "build",
            format!("builder reported {} entries", report.entries),
        ),
        Err(detail) => fail(&mut checks, "build", detail),
    }

    let second = build_file(&spec, "output/second.fpk");
    match (&first, &second) {
        (Ok(left), Ok(right)) if left.sha256 == right.sha256 => {
            let byte_equal = fs::read(&first_path)
                .and_then(|left_bytes| fs::read(&second_path).map(|right| left_bytes == right))
                .unwrap_or(false);
            if byte_equal {
                pass(
                    &mut checks,
                    "reproducible_build",
                    "repeated archives and SHA-256 values are identical",
                );
            } else {
                fail(
                    &mut checks,
                    "reproducible_build",
                    "reported hashes match but archive bytes differ",
                );
            }
        }
        (Ok(_), Ok(_)) => fail(
            &mut checks,
            "reproducible_build",
            "repeated archive hashes differ",
        ),
        (_, Err(detail)) => fail(&mut checks, "reproducible_build", detail),
        (Err(detail), _) => fail(&mut checks, "reproducible_build", detail),
    }

    match inspect_file(&first_path) {
        Ok(report) if report.entries.len() == 2 => pass(
            &mut checks,
            "inspect",
            "structure and two entries reported without integrity overclaim",
        ),
        Ok(report) => fail(
            &mut checks,
            "inspect",
            format!("inspector reported {} entries", report.entries.len()),
        ),
        Err(detail) => fail(&mut checks, "inspect", detail),
    }

    match verify_file(&first_path) {
        Ok(report) if report.entries == 2 => {
            pass(&mut checks, "verify", "both entry hashes verified")
        }
        Ok(report) => fail(
            &mut checks,
            "verify",
            format!("verifier reported {} entries", report.entries),
        ),
        Err(detail) => fail(&mut checks, "verify", detail),
    }

    match extract_file(&first_path, &root.join("extract")) {
        Ok(report)
            if report.entries.len() == 2
                && fs::read(root.join("extract/docs/readme.txt")).ok()
                    == Some(b"forgepack clean install\n".to_vec())
                && fs::read(root.join("extract/payload/data.bin")).ok()
                    == Some(vec![0, 1, 2, 255]) =>
        {
            pass(
                &mut checks,
                "extract",
                "verified text and binary outputs match inputs",
            )
        }
        Ok(_) => fail(
            &mut checks,
            "extract",
            "extracted outputs do not match embedded fixtures",
        ),
        Err(detail) => fail(&mut checks, "extract", detail),
    }

    match extract_file(&first_path, &root.join("extract")) {
        Err(detail) if detail.contains("forgepack_extract_exists") => pass(
            &mut checks,
            "no_clobber_denial",
            "second extraction refused existing destinations",
        ),
        Err(detail) => fail(&mut checks, "no_clobber_denial", detail),
        Ok(_) => fail(
            &mut checks,
            "no_clobber_denial",
            "second extraction unexpectedly replaced or accepted outputs",
        ),
    }

    let corruption = (|| -> Result<(), String> {
        let mut bytes = fs::read(&first_path)
            .map_err(|source| format!("failed to read archive for corruption check: {source}"))?;
        let last = bytes
            .last_mut()
            .ok_or_else(|| "built archive is empty".to_string())?;
        *last ^= 1;
        let corrupt = root.join("output/corrupt.fpk");
        fs::write(&corrupt, bytes)
            .map_err(|source| format!("failed to write corrupt archive: {source}"))?;
        match verify_file(&corrupt) {
            Err(detail) if detail.contains("forgepack_hash_mismatch") => Ok(()),
            Err(detail) => Err(detail),
            Ok(_) => Err("corrupt payload unexpectedly verified".to_string()),
        }
    })();
    match corruption {
        Ok(()) => pass(
            &mut checks,
            "corruption_denial",
            "payload corruption failed strict verification",
        ),
        Err(detail) => fail(&mut checks, "corruption_denial", detail),
    }

    if let Err(source) = fs::remove_dir_all(&root) {
        fail(
            &mut checks,
            "fixture_cleanup",
            format!("failed to remove evidence workspace: {source}"),
        );
    } else {
        pass(&mut checks, "fixture_cleanup", "isolated fixture removed");
    }
    EvidenceReport { checks }
}

pub fn build(mut inputs: Vec<BuildEntry>) -> Result<Vec<u8>, String> {
    if inputs.len() > MAX_ENTRIES {
        return Err(error(
            "entry_limit",
            format!("entry count {} exceeds {MAX_ENTRIES}", inputs.len()),
        ));
    }
    inputs.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));

    let mut seen = BTreeSet::new();
    let mut path_bytes = 0usize;
    let mut payload_bytes = 0usize;
    let mut entries = Vec::with_capacity(inputs.len());
    let mut payload = Vec::new();
    let mut previous_path: Option<String> = None;

    for input in inputs {
        validate_path(&input.path)?;
        validate_media_type(&input.media_type)?;
        if previous_path
            .as_deref()
            .is_some_and(|previous| is_path_prefix(previous, &input.path))
        {
            return Err(error(
                "path_conflict",
                format!(
                    "file path {} is an ancestor of {}",
                    json::string(previous_path.as_deref().unwrap_or_default()),
                    json::string(&input.path)
                ),
            ));
        }
        if !seen.insert(input.path.clone()) {
            return Err(error(
                "duplicate_path",
                format!("path {} appears more than once", json::string(&input.path)),
            ));
        }
        path_bytes = path_bytes
            .checked_add(input.path.len())
            .ok_or_else(|| error("path_limit", "cumulative path length overflow"))?;
        if path_bytes > MAX_PATH_BYTES {
            return Err(error(
                "path_limit",
                format!("cumulative path bytes exceed {MAX_PATH_BYTES}"),
            ));
        }

        let offset = u32::try_from(payload_bytes)
            .map_err(|_| error("payload_limit", "payload offset exceeds u32"))?;
        payload_bytes = payload_bytes
            .checked_add(input.bytes.len())
            .ok_or_else(|| error("payload_limit", "payload length overflow"))?;
        if payload_bytes > MAX_PAYLOAD_BYTES {
            return Err(error(
                "payload_limit",
                format!("payload bytes exceed {MAX_PAYLOAD_BYTES}"),
            ));
        }
        let length = u32::try_from(input.bytes.len())
            .map_err(|_| error("payload_limit", "entry length exceeds u32"))?;
        previous_path = Some(input.path.clone());
        entries.push(Entry {
            path: input.path,
            media_type: input.media_type,
            offset,
            length,
            sha256: crypto::sha256_hex(&input.bytes),
        });
        payload.extend_from_slice(&input.bytes);
    }

    let manifest = Manifest { entries };
    let manifest_text = manifest_json(&manifest);
    if manifest_text.len() > MAX_MANIFEST_BYTES {
        return Err(error(
            "manifest_limit",
            format!("manifest bytes exceed {MAX_MANIFEST_BYTES}"),
        ));
    }
    let manifest_len = u32::try_from(manifest_text.len())
        .map_err(|_| error("manifest_limit", "manifest length exceeds u32"))?;
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| error("payload_limit", "payload length exceeds u32"))?;
    let entry_count = u32::try_from(manifest.entries.len())
        .map_err(|_| error("entry_limit", "entry count exceeds u32"))?;

    let capacity = HEADER_BYTES
        .checked_add(manifest_text.len())
        .and_then(|size| size.checked_add(payload.len()))
        .ok_or_else(|| error("archive_limit", "archive length overflow"))?;
    let mut archive = Vec::with_capacity(capacity);
    archive.extend_from_slice(&MAGIC);
    archive.extend_from_slice(&VERSION.to_le_bytes());
    archive.extend_from_slice(&FLAGS.to_le_bytes());
    archive.extend_from_slice(&entry_count.to_le_bytes());
    archive.extend_from_slice(&manifest_len.to_le_bytes());
    archive.extend_from_slice(&payload_len.to_le_bytes());
    archive.extend_from_slice(manifest_text.as_bytes());
    archive.extend_from_slice(&payload);
    Ok(archive)
}

pub fn inspect(bytes: &[u8]) -> Result<Archive<'_>, String> {
    if bytes.len() < HEADER_BYTES {
        return Err(error(
            "truncated_header",
            format!(
                "archive has {} bytes; header requires {HEADER_BYTES}",
                bytes.len()
            ),
        ));
    }
    if bytes[..8] != MAGIC {
        return Err(error("bad_magic", "archive magic is not FGXPACK\\0"));
    }

    let version = read_u16(bytes, 8);
    if version != VERSION {
        return Err(error(
            "unsupported_version",
            format!("archive version {version} is unsupported; expected {VERSION}"),
        ));
    }
    let flags = read_u16(bytes, 10);
    if flags != FLAGS {
        return Err(error(
            "unsupported_flags",
            format!("archive flags 0x{flags:04x} are unsupported"),
        ));
    }
    let entry_count = read_u32(bytes, 12) as usize;
    if entry_count > MAX_ENTRIES {
        return Err(error(
            "entry_limit",
            format!("entry count {entry_count} exceeds {MAX_ENTRIES}"),
        ));
    }
    let manifest_len = read_u32(bytes, 16) as usize;
    if manifest_len > MAX_MANIFEST_BYTES {
        return Err(error(
            "manifest_limit",
            format!("manifest bytes {manifest_len} exceed {MAX_MANIFEST_BYTES}"),
        ));
    }
    let payload_len = read_u32(bytes, 20) as usize;
    if payload_len > MAX_PAYLOAD_BYTES {
        return Err(error(
            "payload_limit",
            format!("payload bytes {payload_len} exceed {MAX_PAYLOAD_BYTES}"),
        ));
    }
    let expected_len = HEADER_BYTES
        .checked_add(manifest_len)
        .and_then(|size| size.checked_add(payload_len))
        .ok_or_else(|| error("archive_limit", "declared archive length overflow"))?;
    if bytes.len() != expected_len {
        return Err(error(
            "length_mismatch",
            format!(
                "archive has {} bytes but header declares {expected_len}",
                bytes.len()
            ),
        ));
    }

    let manifest_bytes = &bytes[HEADER_BYTES..HEADER_BYTES + manifest_len];
    let manifest_text = std::str::from_utf8(manifest_bytes)
        .map_err(|_| error("manifest_utf8", "manifest is not valid UTF-8"))?;
    let canonical = canonical_json(manifest_text)
        .map_err(|detail| error("manifest_json", strip_trap_prefix(&detail)))?;
    if canonical.as_bytes() != manifest_bytes {
        return Err(error(
            "manifest_noncanonical",
            "manifest bytes are not canonical JSON",
        ));
    }
    let value = parse_json_value(manifest_text)
        .map_err(|detail| error("manifest_json", strip_trap_prefix(&detail)))?;
    let manifest = manifest_from_json(value)?;
    if manifest.entries.len() != entry_count {
        return Err(error(
            "entry_count_mismatch",
            format!(
                "header declares {entry_count} entries but manifest contains {}",
                manifest.entries.len()
            ),
        ));
    }
    validate_manifest(&manifest, payload_len)?;
    let payload = &bytes[HEADER_BYTES + manifest_len..];
    Ok(Archive {
        version,
        flags,
        manifest_bytes,
        manifest,
        payload,
    })
}

pub fn verify(bytes: &[u8]) -> Result<Archive<'_>, String> {
    let archive = inspect(bytes)?;
    for entry in &archive.manifest.entries {
        let start = entry.offset as usize;
        let end = start + entry.length as usize;
        let actual = crypto::sha256_hex(&archive.payload[start..end]);
        if actual != entry.sha256 {
            return Err(error(
                "hash_mismatch",
                format!(
                    "entry {} declares SHA-256 {} but payload hashes to {actual}",
                    json::string(&entry.path),
                    entry.sha256
                ),
            ));
        }
    }
    Ok(archive)
}

pub fn manifest_json(manifest: &Manifest) -> String {
    let mut entries = String::new();
    for (index, entry) in manifest.entries.iter().enumerate() {
        if index > 0 {
            entries.push(',');
        }
        entries.push_str(&format!(
            "{{\"length\":{},\"media_type\":{},\"offset\":{},\"path\":{},\"sha256\":{}}}",
            entry.length,
            json::string(&entry.media_type),
            entry.offset,
            json::string(&entry.path),
            json::string(&entry.sha256)
        ));
    }
    format!(
        "{{\"entries\":[{entries}],\"schema\":{}}}",
        json::string(MANIFEST_SCHEMA)
    )
}

fn manifest_from_json(value: JsonValue) -> Result<Manifest, String> {
    let mut root = expect_object(value, "manifest")?;
    reject_unknown_keys(&root, &["entries", "schema"], "manifest")?;
    let schema = expect_string(take(&mut root, "schema", "manifest")?, "schema")?;
    if schema != MANIFEST_SCHEMA {
        return Err(error(
            "manifest_schema",
            format!(
                "manifest schema {} is unsupported; expected {}",
                json::string(&schema),
                json::string(MANIFEST_SCHEMA)
            ),
        ));
    }
    let values = expect_array(take(&mut root, "entries", "manifest")?, "entries")?;
    if values.len() > MAX_ENTRIES {
        return Err(error(
            "entry_limit",
            format!("entry count {} exceeds {MAX_ENTRIES}", values.len()),
        ));
    }
    let entries = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| entry_from_json(index, value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Manifest { entries })
}

fn parse_build_spec(source: &str) -> Result<Vec<BuildSpecEntry>, String> {
    let value = parse_json_value(source)
        .map_err(|detail| error("spec_json", strip_trap_prefix(&detail)))?;
    let mut root = expect_object(value, "build spec")?;
    reject_unknown_keys(&root, &["entries", "schema"], "build spec")?;
    let schema = expect_string(take(&mut root, "schema", "build spec")?, "schema")?;
    if schema != BUILD_SPEC_SCHEMA {
        return Err(error(
            "spec_schema",
            format!(
                "build spec schema {} is unsupported; expected {}",
                json::string(&schema),
                json::string(BUILD_SPEC_SCHEMA)
            ),
        ));
    }
    let values = expect_array(take(&mut root, "entries", "build spec")?, "entries")?;
    if values.len() > MAX_ENTRIES {
        return Err(error(
            "entry_limit",
            format!("entry count {} exceeds {MAX_ENTRIES}", values.len()),
        ));
    }
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let context = format!("entries[{index}]");
            let mut object = expect_object(value, &context)?;
            reject_unknown_keys(&object, &["media_type", "path", "source"], &context)?;
            let source = expect_string(take(&mut object, "source", &context)?, "source")?;
            normalize_portable_path(&source)
                .map_err(|detail| error("source_path_invalid", strip_trap_prefix(&detail)))?;
            Ok(BuildSpecEntry {
                source,
                path: expect_string(take(&mut object, "path", &context)?, "path")?,
                media_type: expect_string(
                    take(&mut object, "media_type", &context)?,
                    "media_type",
                )?,
            })
        })
        .collect()
}

fn entry_from_json(index: usize, value: JsonValue) -> Result<Entry, String> {
    let context = format!("entries[{index}]");
    let mut object = expect_object(value, &context)?;
    reject_unknown_keys(
        &object,
        &["length", "media_type", "offset", "path", "sha256"],
        &context,
    )?;
    Ok(Entry {
        length: expect_u32(take(&mut object, "length", &context)?, "length")?,
        media_type: expect_string(take(&mut object, "media_type", &context)?, "media_type")?,
        offset: expect_u32(take(&mut object, "offset", &context)?, "offset")?,
        path: expect_string(take(&mut object, "path", &context)?, "path")?,
        sha256: expect_string(take(&mut object, "sha256", &context)?, "sha256")?,
    })
}

fn validate_manifest(manifest: &Manifest, payload_len: usize) -> Result<(), String> {
    let mut previous_path: Option<&str> = None;
    let mut expected_offset = 0usize;
    let mut path_bytes = 0usize;

    for entry in &manifest.entries {
        validate_path(&entry.path)?;
        validate_media_type(&entry.media_type)?;
        validate_sha256(&entry.sha256)?;
        if let Some(previous) = previous_path {
            match previous.as_bytes().cmp(entry.path.as_bytes()) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => {
                    return Err(error(
                        "duplicate_path",
                        format!("path {} appears more than once", json::string(&entry.path)),
                    ));
                }
                std::cmp::Ordering::Greater => {
                    return Err(error(
                        "entry_order",
                        "manifest entries are not in ascending UTF-8 path order",
                    ));
                }
            }
            if is_path_prefix(previous, &entry.path) {
                return Err(error(
                    "path_conflict",
                    format!(
                        "file path {} is an ancestor of {}",
                        json::string(previous),
                        json::string(&entry.path)
                    ),
                ));
            }
        }
        previous_path = Some(&entry.path);
        path_bytes = path_bytes
            .checked_add(entry.path.len())
            .ok_or_else(|| error("path_limit", "cumulative path length overflow"))?;
        if path_bytes > MAX_PATH_BYTES {
            return Err(error(
                "path_limit",
                format!("cumulative path bytes exceed {MAX_PATH_BYTES}"),
            ));
        }

        if entry.offset as usize != expected_offset {
            return Err(error(
                "payload_layout",
                format!(
                    "entry {} starts at {} but contiguous layout requires {expected_offset}",
                    json::string(&entry.path),
                    entry.offset
                ),
            ));
        }
        expected_offset = expected_offset
            .checked_add(entry.length as usize)
            .ok_or_else(|| error("payload_layout", "entry range overflow"))?;
        if expected_offset > payload_len {
            return Err(error(
                "payload_layout",
                format!(
                    "entry {} extends beyond the payload",
                    json::string(&entry.path)
                ),
            ));
        }
    }
    if expected_offset != payload_len {
        return Err(error(
            "payload_layout",
            format!(
                "entries cover {expected_offset} payload bytes but header declares {payload_len}"
            ),
        ));
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), String> {
    let normalized = normalize_portable_path(path)
        .map_err(|detail| error("path_invalid", strip_trap_prefix(&detail)))?;
    if normalized != path {
        return Err(error(
            "path_invalid",
            format!("path {} is not normalized", json::string(path)),
        ));
    }
    Ok(())
}

fn validate_media_type(media_type: &str) -> Result<(), String> {
    let Some((type_name, subtype)) = media_type.split_once('/') else {
        return Err(error(
            "media_type_invalid",
            format!(
                "media type {} must contain type/subtype",
                json::string(media_type)
            ),
        ));
    };
    let token = |value: &str| {
        !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                    )
            })
    };
    if media_type.len() <= 127
        && media_type.bytes().all(|byte| !byte.is_ascii_uppercase())
        && token(type_name)
        && token(subtype)
    {
        Ok(())
    } else {
        Err(error(
            "media_type_invalid",
            format!(
                "media type {} must be a lowercase ASCII type/subtype token of at most 127 bytes",
                json::string(media_type)
            ),
        ))
    }
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(error(
            "hash_invalid",
            "entry SHA-256 must contain exactly 64 lowercase hexadecimal digits",
        ))
    }
}

fn expect_object(value: JsonValue, context: &str) -> Result<BTreeMap<String, JsonValue>, String> {
    if let JsonValue::Object(value) = value {
        Ok(value)
    } else {
        Err(error(
            "manifest_shape",
            format!("{context} must be a JSON object"),
        ))
    }
}

fn expect_array(value: JsonValue, context: &str) -> Result<Vec<JsonValue>, String> {
    if let JsonValue::Array(value) = value {
        Ok(value)
    } else {
        Err(error(
            "manifest_shape",
            format!("{context} must be a JSON array"),
        ))
    }
}

fn expect_string(value: JsonValue, context: &str) -> Result<String, String> {
    if let JsonValue::String(value) = value {
        Ok(value)
    } else {
        Err(error(
            "manifest_shape",
            format!("{context} must be a JSON string"),
        ))
    }
}

fn expect_u32(value: JsonValue, context: &str) -> Result<u32, String> {
    if let JsonValue::Int(value) = value {
        u32::try_from(value).map_err(|_| {
            error(
                "manifest_shape",
                format!("{context} must be an unsigned 32-bit integer"),
            )
        })
    } else {
        Err(error(
            "manifest_shape",
            format!("{context} must be an unsigned 32-bit integer"),
        ))
    }
}

fn take(
    object: &mut BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> Result<JsonValue, String> {
    object.remove(key).ok_or_else(|| {
        error(
            "manifest_shape",
            format!("{context} is missing required key {}", json::string(key)),
        )
    })
}

fn reject_unknown_keys(
    object: &BTreeMap<String, JsonValue>,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Err(error(
            "manifest_shape",
            format!("{context} contains unknown key {}", json::string(key)),
        ))
    } else {
        Ok(())
    }
}

fn read_bounded(path: &Path, maximum: usize, label: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|source| {
        error(
            "read",
            format!("failed to inspect {label} {}: {source}", path.display()),
        )
    })?;
    if metadata.len() > maximum as u64 {
        return Err(error(
            "read_limit",
            format!(
                "{label} {} has {} bytes; limit is {maximum}",
                path.display(),
                metadata.len()
            ),
        ));
    }
    let bytes = fs::read(path).map_err(|source| {
        error(
            "read",
            format!("failed to read {label} {}: {source}", path.display()),
        )
    })?;
    if bytes.len() > maximum {
        return Err(error(
            "read_limit",
            format!(
                "{label} {} grew to {} bytes; limit is {maximum}",
                path.display(),
                bytes.len()
            ),
        ));
    }
    Ok(bytes)
}

fn read_archive_file(path: &Path) -> Result<Vec<u8>, String> {
    let maximum = HEADER_BYTES
        .checked_add(MAX_MANIFEST_BYTES)
        .and_then(|size| size.checked_add(MAX_PAYLOAD_BYTES))
        .ok_or_else(|| error("archive_limit", "maximum archive size overflow"))?;
    read_bounded(path, maximum, "archive")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn create_evidence_root() -> Result<PathBuf, String> {
    for _ in 0..100 {
        let sequence = EVIDENCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "forgentex_forgepack_evidence_{}_{}",
            std::process::id(),
            sequence
        ));
        match fs::create_dir(&root) {
            Ok(()) => return Ok(root),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(format!(
                    "failed to create isolated evidence directory: {source}"
                ));
            }
        }
    }
    Err("could not reserve an isolated evidence directory".to_string())
}

fn pass(checks: &mut Vec<EvidenceCheck>, id: &str, detail: impl Into<String>) {
    checks.push(EvidenceCheck {
        id: id.to_string(),
        status: "pass".to_string(),
        detail: detail.into(),
    });
}

fn fail(checks: &mut Vec<EvidenceCheck>, id: &str, detail: impl Into<String>) {
    checks.push(EvidenceCheck {
        id: id.to_string(),
        status: "fail".to_string(),
        detail: detail.into(),
    });
}

fn is_path_prefix(parent: &str, child: &str) -> bool {
    child
        .strip_prefix(parent)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn preflight_extract_target(root: &Path, portable: &str) -> Result<(), String> {
    validate_path(portable)?;
    let components = portable.split('/').collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(error(
                        "extract_scope",
                        format!(
                            "entry {} crosses symbolic link {}",
                            json::string(portable),
                            current.display()
                        ),
                    ));
                }
                if index + 1 == components.len() {
                    return Err(error(
                        "extract_exists",
                        format!("destination already exists for {}", json::string(portable)),
                    ));
                }
                if !metadata.is_dir() {
                    return Err(error(
                        "extract_conflict",
                        format!(
                            "parent component is not a directory for {}",
                            json::string(portable)
                        ),
                    ));
                }
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(error(
                    "extract_preflight",
                    format!(
                        "failed to inspect destination for {}: {source}",
                        json::string(portable)
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn ensure_extract_parent(root: &Path, portable: &str) -> Result<PathBuf, String> {
    let relative = Path::new(portable);
    let mut current = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(error(
                            "extract_scope",
                            format!(
                                "extraction parent is not a real directory: {}",
                                current.display()
                            ),
                        ));
                    }
                }
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current).map_err(|source| {
                        error(
                            "extract_directory",
                            format!("failed to create {}: {source}", current.display()),
                        )
                    })?;
                }
                Err(source) => {
                    return Err(error(
                        "extract_directory",
                        format!("failed to inspect {}: {source}", current.display()),
                    ));
                }
            }
            let canonical = fs::canonicalize(&current).map_err(|source| {
                error(
                    "extract_scope",
                    format!("failed to resolve {}: {source}", current.display()),
                )
            })?;
            if !canonical.starts_with(root) {
                return Err(error(
                    "extract_scope",
                    format!("extraction parent escapes root: {}", current.display()),
                ));
            }
            current = canonical;
        }
    }
    Ok(current)
}

fn resolve_existing_beneath(root: &Path, portable: &str, label: &str) -> Result<PathBuf, String> {
    let normalized = normalize_portable_path(portable)
        .map_err(|detail| error("source_path_invalid", strip_trap_prefix(&detail)))?;
    let resolved = fs::canonicalize(root.join(&normalized)).map_err(|source| {
        error(
            "source_read",
            format!(
                "failed to resolve {label} {}: {source}",
                json::string(portable)
            ),
        )
    })?;
    if !resolved.starts_with(root) {
        return Err(error(
            "source_scope",
            format!(
                "{label} {} resolves outside the spec directory",
                json::string(portable)
            ),
        ));
    }
    Ok(resolved)
}

fn resolve_new_beneath(root: &Path, portable: &str) -> Result<PathBuf, String> {
    let normalized = normalize_portable_path(portable)
        .map_err(|detail| error("output_path_invalid", strip_trap_prefix(&detail)))?;
    let relative = Path::new(&normalized);
    let parent = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let resolved_parent = fs::canonicalize(root.join(parent)).map_err(|source| {
        error(
            "output_scope",
            format!(
                "failed to resolve output parent for {}: {source}",
                json::string(portable)
            ),
        )
    })?;
    if !resolved_parent.starts_with(root) {
        return Err(error(
            "output_scope",
            format!(
                "output {} resolves outside the spec directory",
                json::string(portable)
            ),
        ));
    }
    let name = relative
        .file_name()
        .ok_or_else(|| error("output_path_invalid", "output has no file name"))?;
    Ok(resolved_parent.join(name))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn error(code: &str, detail: impl AsRef<str>) -> String {
    format!("forgepack_{code}: {}", detail.as_ref())
}

fn strip_trap_prefix(detail: &str) -> &str {
    detail.split_once(": ").map_or(detail, |(_, rest)| rest)
}
