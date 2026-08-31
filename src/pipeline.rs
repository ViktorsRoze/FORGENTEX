//! Deterministic runtime pipeline (Milestone C).
//!
//! A stage is a checked capsule with one ordered `bind <source> read` per input
//! and exactly one `bind <target> rw`. Its scalar, reviewed `@repr(C)` struct,
//! or reviewed owned-vector inputs are snapshotted from the Forgentex-owned
//! shared store and its output is atomically written to the target. Stages run one at a time in
//! source-declaration order or in a deterministic dependency order derived from
//! their bindings. Reviewed owned integer and finite floating-point vectors may
//! feed later matching immutable slices. Dependency scheduling permits bounded
//! immutable fan-out and ordered fan-in while still excluding mutable buffers,
//! implicit globals, and parallel scheduling. Optional
//! pipeline-wide limits bound stage count, the sum of declared native wall
//! limits, and committed owned-vector bytes.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::ast::{Capsule, CapsuleValue, Program, ShareMode, Type};
use crate::interp::{
    invoke_capsule_live_transactional_args, value_to_json, CapsuleCall, CapsuleLiveMutation, Value,
};
use crate::json;

pub const PIPELINE_RUN_SCHEMA: &str = "forgentex.pipeline_run.v1";

const PREVIEW_MAX_WIDTH: i64 = 1920;
const PREVIEW_MAX_HEIGHT: i64 = 1080;
const FINAL_MAX_WIDTH: i64 = 3840;
const FINAL_MAX_HEIGHT: i64 = 2160;
const MAX_RENDER_FRAME_COUNT: i64 = 1_000_000;
const PREVIEW_MAX_SAMPLES_PER_PIXEL: i64 = 64;
const FINAL_MAX_SAMPLES_PER_PIXEL: i64 = 4096;
const MAX_RENDER_SHUTTER_Q16: i64 = 65_536;
const MAX_RENDER_ADAPTIVE_NOISE_Q16: i64 = 65_536;

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineValue {
    pub name: String,
    pub version: u64,
    pub ty: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineStage {
    pub capsule: String,
    /// Ordered shared inputs, matching capsule input declaration order.
    pub sources: Vec<String>,
    /// Compatibility projection: the sole source, or a comma-separated fan-in.
    pub source: String,
    /// Sources declared for transactional consumption by this stage.
    pub move_sources: Vec<String>,
    /// Sources declared for fixed-length transactional mutation.
    pub update_sources: Vec<String>,
    /// True only when move-source removal and target insertion both committed.
    pub handoff_committed: bool,
    pub target: String,
    pub decision: String,
    pub input: String,
    pub redacted_inputs: Vec<String>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub native: bool,
    pub duration_ms: Option<u128>,
    pub cache_identity: Option<PipelineCacheIdentityReceipt>,
    pub cache_restore: Option<PipelineCacheRestoreReceipt>,
    pub io: Option<PipelineStageIoReceipt>,
    pub buffer: Option<PipelineBufferReceipt>,
    pub updates: Vec<PipelineUpdateReceipt>,
    pub artifact: Option<crate::ffi::NativeArtifactReceipt>,
    pub render: Option<RenderReceipt>,
    pub encode: Option<EncodeReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineUpdateReceipt {
    pub source: String,
    pub source_version: u64,
    pub target_version: u64,
    pub ty: String,
    pub elements: usize,
    pub bytes: usize,
    pub encoding: String,
    pub before_sha256: String,
    pub after_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PipelineLimits {
    pub max_stages: Option<usize>,
    pub max_wall_ms: Option<u128>,
    pub max_cpu_ms: Option<u128>,
    pub max_memory_bytes: Option<u64>,
    pub max_owned_bytes: Option<usize>,
    pub max_fs_bytes: Option<u64>,
    pub max_net_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PipelineSchedule {
    #[default]
    Declaration,
    DependencyGraph,
    SourceGraph,
}

#[derive(Clone, Debug, Default)]
pub struct PipelineCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl PipelineCancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PipelineControl {
    pub cancellation: Option<PipelineCancellationToken>,
    /// Deterministically cancel before dispatching stage N+1.
    pub stop_after_stages: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineGraphEdge {
    pub from: String,
    pub to: String,
    pub share: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineGraphReceipt {
    pub schema: String,
    pub origin: String,
    pub declaration: Option<String>,
    pub nodes: Vec<String>,
    pub edges: Vec<PipelineGraphEdge>,
    pub roots: Vec<String>,
    pub fan_out_shares: Vec<String>,
    pub execution_order: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineBudgetReceipt {
    pub schema: String,
    pub max_stages: Option<usize>,
    pub max_wall_ms: Option<u128>,
    pub max_cpu_ms: Option<u128>,
    pub max_memory_bytes: Option<u64>,
    pub max_owned_bytes: Option<usize>,
    pub max_fs_bytes: Option<u64>,
    pub max_net_bytes: Option<u64>,
    pub planned_stages: usize,
    pub planned_wall_ms: Option<u128>,
    pub planned_cpu_ms: Option<u128>,
    pub planned_peak_memory_bytes: Option<u64>,
    pub used_stages: usize,
    pub used_wall_ms: u128,
    pub used_owned_bytes: usize,
    pub used_fs_bytes: Option<u64>,
    pub used_net_bytes: Option<u64>,
    pub decision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineCancellationReceipt {
    pub schema: String,
    pub source: String,
    pub decision: String,
    pub committed_stages: usize,
    pub after_stage: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineBufferReceipt {
    pub schema: String,
    pub element_type: String,
    pub elements: usize,
    pub bytes: usize,
    pub encoding: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineCacheIdentityReceipt {
    pub schema: String,
    pub key_sha256: String,
    pub input_sha256: String,
    pub code_sha256: String,
    pub policy_sha256: String,
    pub limits_sha256: String,
    pub output_contract_sha256: String,
    pub toolchain: String,
    pub reuse: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineCacheRestoreReceipt {
    pub schema: String,
    pub request_sha256: String,
    pub output_sha256: String,
    pub output_bytes: usize,
    pub storage: String,
    pub decision: String,
}

#[derive(Clone, Debug)]
struct PipelineCacheEntry {
    identity: PipelineCacheIdentityReceipt,
    output: Value,
    output_sha256: String,
    output_bytes: usize,
    storage: String,
}

struct PipelineCacheAuthenticationKey(Vec<u8>);

impl std::fmt::Debug for PipelineCacheAuthenticationKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PipelineCacheAuthenticationKey([REDACTED])")
    }
}

impl Drop for PipelineCacheAuthenticationKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// A bounded cache of already validated native stage outputs. By default it is
/// process-local; [`PipelineCache::persistent`] adds an explicit, trusted
/// directory grant with strict content-addressed integrity checks.
#[derive(Debug)]
pub struct PipelineCache {
    entries: BTreeMap<String, PipelineCacheEntry>,
    stored_bytes: usize,
    max_entries: usize,
    max_bytes: usize,
    persistent_root: Option<PathBuf>,
    authentication_key: Option<PipelineCacheAuthenticationKey>,
}

impl Default for PipelineCache {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            stored_bytes: 0,
            max_entries: 256,
            max_bytes: 64 * 1024 * 1024,
            persistent_root: None,
            authentication_key: None,
        }
    }
}

impl PipelineCache {
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Result<Self, String> {
        if max_entries == 0 || max_entries > 1024 {
            return Err("pipeline cache max_entries must be between 1 and 1024".to_string());
        }
        if max_bytes == 0 || max_bytes > 1024 * 1024 * 1024 {
            return Err("pipeline cache max_bytes must be between 1 and 1073741824".to_string());
        }
        Ok(Self {
            entries: BTreeMap::new(),
            stored_bytes: 0,
            max_entries,
            max_bytes,
            persistent_root: None,
            authentication_key: None,
        })
    }

    /// Open an existing directory as an explicit persistent-cache grant. The
    /// root itself must be a real directory, not a symlink; entries use a
    /// strict integrity-checked content-addressed format. The directory is a
    /// trusted root: hashes detect corruption, but do not authenticate origin
    /// against a writer that can replace files and recompute those hashes.
    pub fn persistent(root: &Path) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(root).map_err(|error| {
            format!(
                "pipeline cache directory '{}' is unavailable: {error}",
                root.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "pipeline cache directory '{}' must be an existing non-symlink directory",
                root.display()
            ));
        }
        let canonical = std::fs::canonicalize(root).map_err(|error| {
            format!(
                "pipeline cache directory '{}' cannot be resolved: {error}",
                root.display()
            )
        })?;
        Ok(Self {
            persistent_root: Some(canonical),
            ..Self::default()
        })
    }

    /// Open an existing directory for HMAC-SHA256 authenticated persistent
    /// reuse. The shared key must contain 32 through 4096 bytes and is redacted
    /// from debug output, receipts, record names, and serialized content.
    pub fn authenticated_persistent(root: &Path, key: &[u8]) -> Result<Self, String> {
        if !(32..=4096).contains(&key.len()) {
            return Err(
                "pipeline cache authentication key must contain between 32 and 4096 bytes"
                    .to_string(),
            );
        }
        let mut cache = Self::persistent(root)?;
        cache.authentication_key = Some(PipelineCacheAuthenticationKey(key.to_vec()));
        Ok(cache)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn insert(&mut self, request_sha256: String, mut entry: PipelineCacheEntry) -> Option<String> {
        if entry.output_bytes > self.max_bytes
            || (!self.entries.contains_key(&request_sha256)
                && self.entries.len() >= self.max_entries)
        {
            return None;
        }
        let replaced_bytes = self
            .entries
            .get(&request_sha256)
            .map_or(0, |existing| existing.output_bytes);
        let next_bytes = self
            .stored_bytes
            .saturating_sub(replaced_bytes)
            .saturating_add(entry.output_bytes);
        if next_bytes > self.max_bytes {
            return None;
        }
        if let Some(root) = self.persistent_root.as_ref() {
            let authentication_key = self.authentication_key.as_ref().map(|key| key.0.as_slice());
            if persist_cache_entry(root, &request_sha256, &entry, authentication_key) {
                entry.storage = if authentication_key.is_some() {
                    "authenticated_hmac_sha256_content_addressed_file"
                } else {
                    "integrity_checked_content_addressed_file"
                }
                .to_string();
            }
        }
        let storage = entry.storage.clone();
        self.entries.insert(request_sha256, entry);
        self.stored_bytes = next_bytes;
        Some(storage)
    }

    fn hydrate(&mut self, request_sha256: &str) {
        if self.entries.contains_key(request_sha256) {
            return;
        }
        let Some(root) = self.persistent_root.as_ref() else {
            return;
        };
        let authentication_key = self.authentication_key.as_ref().map(|key| key.0.as_slice());
        let Some(entry) = load_cache_entry(root, request_sha256, authentication_key) else {
            return;
        };
        let replaced_bytes = self
            .entries
            .get(request_sha256)
            .map_or(0, |existing| existing.output_bytes);
        let next_bytes = self
            .stored_bytes
            .saturating_sub(replaced_bytes)
            .saturating_add(entry.output_bytes);
        if self.entries.len() < self.max_entries
            && entry.output_bytes <= self.max_bytes
            && next_bytes <= self.max_bytes
        {
            self.entries.insert(request_sha256.to_string(), entry);
            self.stored_bytes = next_bytes;
        }
    }

    #[cfg(test)]
    pub(crate) fn corrupt_all_entries_for_test(&mut self) {
        for entry in self.entries.values_mut() {
            entry.output_sha256 = "0".repeat(64);
        }
    }
}

const PIPELINE_CACHE_INTEGRITY_MAGIC: &[u8] = b"FGXPCACHE\x01";
const PIPELINE_CACHE_AUTHENTICATED_MAGIC: &[u8] = b"FGXPCACHE\x02";
const MAX_PERSISTENT_CACHE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PERSISTENT_CACHE_TOOLCHAIN_BYTES: usize = 4096;
const MAX_PERSISTENT_CACHE_FILE_BYTES: u64 = (MAX_PERSISTENT_CACHE_OUTPUT_BYTES + 16 * 1024) as u64;
static PIPELINE_CACHE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn serialize_cache_entry(
    request_sha256: &str,
    entry: &PipelineCacheEntry,
    authentication_key: Option<&[u8]>,
) -> Option<(Vec<u8>, String)> {
    let identity = &entry.identity;
    let hashes = [
        request_sha256,
        &identity.key_sha256,
        &identity.input_sha256,
        &identity.code_sha256,
        &identity.policy_sha256,
        &identity.limits_sha256,
        &identity.output_contract_sha256,
        &entry.output_sha256,
    ];
    if !hashes.iter().all(|hash| valid_sha256(hash))
        || entry.output_bytes > MAX_PERSISTENT_CACHE_OUTPUT_BYTES
        || identity.toolchain.len() > MAX_PERSISTENT_CACHE_TOOLCHAIN_BYTES
    {
        return None;
    }
    let output = value_to_json(&entry.output);
    if output.len() != entry.output_bytes
        || crate::crypto::sha256_hex(output.as_bytes()) != entry.output_sha256
    {
        return None;
    }
    let magic = if authentication_key.is_some() {
        PIPELINE_CACHE_AUTHENTICATED_MAGIC
    } else {
        PIPELINE_CACHE_INTEGRITY_MAGIC
    };
    let mut bytes = Vec::with_capacity(
        magic.len()
            + 8 * 64
            + identity.toolchain.len()
            + output.len()
            + if authentication_key.is_some() {
                144
            } else {
                80
            },
    );
    bytes.extend_from_slice(magic);
    for hash in hashes {
        bytes.extend_from_slice(hash.as_bytes());
    }
    bytes.extend_from_slice(&(identity.toolchain.len() as u16).to_le_bytes());
    bytes.extend_from_slice(identity.toolchain.as_bytes());
    bytes.extend_from_slice(&(output.len() as u32).to_le_bytes());
    bytes.extend_from_slice(output.as_bytes());
    let digest = crate::crypto::sha256_hex(&bytes);
    bytes.extend_from_slice(digest.as_bytes());
    if let Some(key) = authentication_key {
        let authentication = crate::crypto::hmac_sha256_hex(key, &bytes);
        bytes.extend_from_slice(authentication.as_bytes());
    }
    Some((bytes, digest))
}

fn persist_cache_entry(
    root: &Path,
    request_sha256: &str,
    entry: &PipelineCacheEntry,
    authentication_key: Option<&[u8]>,
) -> bool {
    let Ok(root_metadata) = std::fs::symlink_metadata(root) else {
        return false;
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return false;
    }
    let Some((bytes, digest)) = serialize_cache_entry(request_sha256, entry, authentication_key)
    else {
        return false;
    };
    let exact_target = root.join(format!("{request_sha256}.{digest}.fgpc"));
    if std::fs::symlink_metadata(&exact_target).is_ok_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && std::fs::read(&exact_target).is_ok_and(|existing| existing == bytes)
    }) {
        return true;
    }
    let counter = PIPELINE_CACHE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let target = if std::fs::symlink_metadata(&exact_target).is_ok() {
        root.join(format!(
            "{request_sha256}.{digest}.{}.{}.fgpc",
            std::process::id(),
            counter
        ))
    } else {
        exact_target
    };
    if std::fs::symlink_metadata(&target).is_ok() {
        return std::fs::symlink_metadata(&target).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && std::fs::read(&target).is_ok_and(|existing| existing == bytes)
        });
    }
    let temporary = root.join(format!(
        ".{request_sha256}.{}.{}.tmp",
        std::process::id(),
        counter
    ));
    let result = (|| -> Result<(), ()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| ())?;
        file.write_all(&bytes).map_err(|_| ())?;
        file.sync_all().map_err(|_| ())?;
        std::fs::hard_link(&temporary, &target).map_err(|_| ())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary);
    if result.is_ok() {
        true
    } else {
        std::fs::read(&target).is_ok_and(|existing| existing == bytes)
    }
}

fn read_exact_slice<'a>(bytes: &'a [u8], offset: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(len)?;
    let slice = bytes.get(*offset..end)?;
    *offset = end;
    Some(slice)
}

fn parse_cache_entry(
    bytes: &[u8],
    expected_request: &str,
    authentication_key: Option<&[u8]>,
) -> Option<PipelineCacheEntry> {
    let trailer_len = if authentication_key.is_some() {
        128
    } else {
        64
    };
    let expected_magic = if authentication_key.is_some() {
        PIPELINE_CACHE_AUTHENTICATED_MAGIC
    } else {
        PIPELINE_CACHE_INTEGRITY_MAGIC
    };
    if bytes.len() < expected_magic.len() + 8 * 64 + 2 + 4 + trailer_len
        || bytes.len() as u64 > MAX_PERSISTENT_CACHE_FILE_BYTES
    {
        return None;
    }
    let content_len = bytes.len().checked_sub(trailer_len)?;
    let content = bytes.get(..content_len)?;
    let recorded_digest = bytes.get(content_len..content_len + 64)?;
    let recorded_digest = std::str::from_utf8(recorded_digest).ok()?;
    if !valid_sha256(recorded_digest) || crate::crypto::sha256_hex(content) != recorded_digest {
        return None;
    }
    if let Some(key) = authentication_key {
        let recorded_authentication = bytes.get(content_len + 64..)?;
        let expected_authentication =
            crate::crypto::hmac_sha256_hex(key, bytes.get(..content_len + 64)?);
        if !crate::crypto::constant_time_eq(
            recorded_authentication,
            expected_authentication.as_bytes(),
        ) {
            return None;
        }
    }
    let mut offset = 0usize;
    if read_exact_slice(content, &mut offset, expected_magic.len())? != expected_magic {
        return None;
    }
    let (
        request_sha256,
        key_sha256,
        input_sha256,
        code_sha256,
        policy_sha256,
        limits_sha256,
        output_contract_sha256,
        output_sha256,
    ) = {
        let mut next_hash = || {
            std::str::from_utf8(read_exact_slice(content, &mut offset, 64)?)
                .ok()
                .filter(|hash| valid_sha256(hash))
                .map(str::to_string)
        };
        (
            next_hash()?,
            next_hash()?,
            next_hash()?,
            next_hash()?,
            next_hash()?,
            next_hash()?,
            next_hash()?,
            next_hash()?,
        )
    };
    if request_sha256 != expected_request {
        return None;
    }
    let toolchain_len =
        u16::from_le_bytes(read_exact_slice(content, &mut offset, 2)?.try_into().ok()?) as usize;
    if toolchain_len > MAX_PERSISTENT_CACHE_TOOLCHAIN_BYTES {
        return None;
    }
    let toolchain = std::str::from_utf8(read_exact_slice(content, &mut offset, toolchain_len)?)
        .ok()?
        .to_string();
    let output_len =
        u32::from_le_bytes(read_exact_slice(content, &mut offset, 4)?.try_into().ok()?) as usize;
    if output_len > MAX_PERSISTENT_CACHE_OUTPUT_BYTES {
        return None;
    }
    let output_text =
        std::str::from_utf8(read_exact_slice(content, &mut offset, output_len)?).ok()?;
    if offset != content.len() || crate::crypto::sha256_hex(output_text.as_bytes()) != output_sha256
    {
        return None;
    }
    let output = crate::interp::value_from_json_text(output_text).ok()?;
    Some(PipelineCacheEntry {
        identity: PipelineCacheIdentityReceipt {
            schema: "forgentex.pipeline_cache_identity.v1".to_string(),
            key_sha256,
            input_sha256,
            code_sha256,
            policy_sha256,
            limits_sha256,
            output_contract_sha256,
            toolchain,
            reuse: if authentication_key.is_some() {
                "loaded_authenticated_file"
            } else {
                "loaded_integrity_checked_file"
            }
            .to_string(),
        },
        output,
        output_sha256,
        output_bytes: output_len,
        storage: if authentication_key.is_some() {
            "authenticated_hmac_sha256_content_addressed_file"
        } else {
            "integrity_checked_content_addressed_file"
        }
        .to_string(),
    })
}

fn load_cache_entry(
    root: &Path,
    request_sha256: &str,
    authentication_key: Option<&[u8]>,
) -> Option<PipelineCacheEntry> {
    let root_metadata = std::fs::symlink_metadata(root).ok()?;
    if !valid_sha256(request_sha256)
        || root_metadata.file_type().is_symlink()
        || !root_metadata.is_dir()
    {
        return None;
    }
    let prefix = format!("{request_sha256}.");
    let mut candidates = Vec::new();
    for item in std::fs::read_dir(root).ok()?.take(4097) {
        let Ok(item) = item else {
            continue;
        };
        let file_name = item.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if let Some(suffix) = name
            .strip_prefix(&prefix)
            .and_then(|name| name.strip_suffix(".fgpc"))
        {
            let digest = suffix.split('.').next().unwrap_or_default();
            if valid_sha256(digest) {
                candidates.push((item.path(), digest.to_string()));
            }
        }
        if candidates.len() > 1024 {
            return None;
        }
    }
    candidates.sort();
    for (path, expected_digest) in candidates {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_PERSISTENT_CACHE_FILE_BYTES
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let digest_offset = bytes.len().checked_sub(if authentication_key.is_some() {
            128
        } else {
            64
        });
        if digest_offset.is_none_or(|offset| {
            std::str::from_utf8(&bytes[offset..offset + 64]).ok() != Some(&expected_digest)
        }) {
            continue;
        }
        if let Some(entry) = parse_cache_entry(&bytes, request_sha256, authentication_key) {
            return Some(entry);
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineStageIoReceipt {
    pub schema: String,
    pub read_transfer_bytes: u64,
    pub write_transfer_bytes: u64,
    pub other_transfer_bytes: u64,
    pub total_transfer_bytes: u64,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderReceipt {
    pub schema: String,
    pub profile: String,
    pub color_space: Option<String>,
    pub device: Option<String>,
    pub render_pass: Option<String>,
    pub engine: Option<String>,
    pub engine_version: Option<String>,
    pub sequence_id: Option<String>,
    pub frame_cache_key_sha256: Option<String>,
    pub renderer: String,
    pub scene_type: String,
    pub scene_sha256: String,
    pub seed: i64,
    pub width: u32,
    pub height: u32,
    pub frame_index: u32,
    pub frame_count: u32,
    pub frames_per_second: Option<u32>,
    pub samples_per_pixel: Option<u32>,
    pub shutter_q16: Option<u32>,
    pub adaptive_min_samples: Option<u32>,
    pub adaptive_noise_q16: Option<u32>,
    pub effects: Vec<String>,
    pub boundary_hash: String,
    pub toolchain: String,
    pub renderer_binary_sha256: String,
    pub wall_limit_ms: Option<u128>,
    pub cpu_limit_ms: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub artifact: crate::ffi::NativeArtifactReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeReceipt {
    pub schema: String,
    pub profile: String,
    pub color_space: Option<String>,
    pub input_pass: Option<String>,
    pub engine: Option<String>,
    pub engine_version: Option<String>,
    pub compression: Option<String>,
    pub compression_level: Option<u32>,
    pub pixel_format: Option<String>,
    pub interlace: Option<String>,
    pub scanline_filter: Option<String>,
    pub chunk_checksum: Option<String>,
    pub stream_checksum: Option<String>,
    pub chunk_order: Option<String>,
    pub encoder: String,
    pub input_sha256: String,
    pub input_bytes: u64,
    pub input_width: u32,
    pub input_height: u32,
    pub input_format: String,
    pub effects: Vec<String>,
    pub boundary_hash: String,
    pub toolchain: String,
    pub encoder_binary_sha256: String,
    pub wall_limit_ms: Option<u128>,
    pub cpu_limit_ms: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub artifact: crate::ffi::NativeArtifactReceipt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineReport {
    pub schema: String,
    pub file: String,
    pub mode: String,
    pub status: String,
    pub diagnostics: Vec<String>,
    pub stages: Vec<PipelineStage>,
    pub shared: Vec<PipelineValue>,
    pub budget: PipelineBudgetReceipt,
    pub graph: Option<PipelineGraphReceipt>,
    pub cancellation: Option<PipelineCancellationReceipt>,
}

/// Execute the bounded single-threaded pipeline. `overrides` replaces declared
/// share defaults for one run; it never mutates the source program.
pub fn run(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
) -> PipelineReport {
    run_with_options(
        file,
        program,
        overrides,
        allow,
        PipelineLimits::default(),
        PipelineSchedule::Declaration,
    )
}

pub fn run_with_limits(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
    limits: PipelineLimits,
) -> PipelineReport {
    run_with_options(
        file,
        program,
        overrides,
        allow,
        limits,
        PipelineSchedule::Declaration,
    )
}

pub fn run_with_options(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
    limits: PipelineLimits,
    schedule: PipelineSchedule,
) -> PipelineReport {
    run_with_control(
        file,
        program,
        overrides,
        allow,
        limits,
        schedule,
        PipelineControl::default(),
    )
}

pub fn run_with_control(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
    limits: PipelineLimits,
    schedule: PipelineSchedule,
    control: PipelineControl,
) -> PipelineReport {
    run_internal(
        file, program, overrides, allow, limits, schedule, control, None,
    )
}

/// Execute with a caller-owned bounded output cache. A default cache survives
/// multiple calls in this process; [`PipelineCache::persistent`] additionally
/// restores integrity-checked entries from its explicitly granted directory.
#[allow(clippy::too_many_arguments)]
pub fn run_with_cache_control(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
    limits: PipelineLimits,
    schedule: PipelineSchedule,
    control: PipelineControl,
    cache: &mut PipelineCache,
) -> PipelineReport {
    run_internal(
        file,
        program,
        overrides,
        allow,
        limits,
        schedule,
        control,
        Some(cache),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_internal(
    file: &str,
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
    allow: Option<Vec<String>>,
    limits: PipelineLimits,
    schedule: PipelineSchedule,
    control: PipelineControl,
    mut cache: Option<&mut PipelineCache>,
) -> PipelineReport {
    let report_context = ReportContext {
        program,
        limits: &limits,
        schedule,
        control: &control,
        cancellation_reason: Cell::new(None),
    };
    let mut graph = None;
    let diagnostics = crate::typecheck::check(program);
    if !diagnostics.is_empty() {
        return report(
            file,
            "rejected",
            diagnostics.into_iter().map(|item| item.code).collect(),
            Vec::new(),
            BTreeMap::new(),
            &report_context,
            graph.as_ref(),
        );
    }

    if let Err((status, error)) = validate_pipeline_limits(program, &limits) {
        return report(
            file,
            status,
            vec![error],
            Vec::new(),
            BTreeMap::new(),
            &report_context,
            graph.as_ref(),
        );
    }
    if let Err(error) = validate_transactional_handoffs(program) {
        return report(
            file,
            "rejected",
            vec![error],
            Vec::new(),
            BTreeMap::new(),
            &report_context,
            graph.as_ref(),
        );
    }
    if control
        .stop_after_stages
        .is_some_and(|value| value == 0 || value > 1024)
    {
        return report(
            file,
            "rejected",
            vec!["pipeline stop_after_stages must be between 1 and 1024".to_string()],
            Vec::new(),
            BTreeMap::new(),
            &report_context,
            graph.as_ref(),
        );
    }

    let mut shared = match initial_shared_values(program, overrides) {
        Ok(values) => values,
        Err(error) => {
            return report(
                file,
                "rejected",
                vec![error],
                Vec::new(),
                BTreeMap::new(),
                &report_context,
                graph.as_ref(),
            )
        }
    };
    let execution_order = match schedule {
        PipelineSchedule::Declaration => program
            .capsules()
            .filter(|capsule| !capsule.bindings.is_empty())
            .map(|capsule| capsule.name.clone())
            .collect(),
        PipelineSchedule::DependencyGraph => match dependency_graph_plan(program, &shared) {
            Ok((order, receipt)) => {
                graph = Some(receipt);
                order
            }
            Err(error) => {
                return report(
                    file,
                    "rejected",
                    vec![error],
                    Vec::new(),
                    shared,
                    &report_context,
                    None,
                )
            }
        },
        PipelineSchedule::SourceGraph => match source_graph_plan(program, &shared) {
            Ok((order, receipt)) => {
                graph = Some(receipt);
                order
            }
            Err(error) => {
                return report(
                    file,
                    "rejected",
                    vec![error],
                    Vec::new(),
                    shared,
                    &report_context,
                    None,
                )
            }
        },
    };
    let mut stages = Vec::new();
    let mut sensitive_shares = BTreeSet::new();
    for capsule in program.capsules() {
        let redacted = crate::sandbox::analyze_capsule(capsule).redacted_inputs;
        for (input_index, input) in capsule.inputs.iter().enumerate() {
            if !redacted.contains(&input.name) {
                continue;
            }
            if let Some(binding) = capsule
                .bindings
                .iter()
                .filter(|binding| binding.mode.is_source())
                .nth(input_index)
            {
                sensitive_shares.insert(binding.name.clone());
                sensitive_shares.insert(share_reference(&binding.name).0.to_string());
            }
        }
    }
    if let Some(reason) = pipeline_cancellation_reason(&control, stages.len()) {
        report_context.cancellation_reason.set(Some(reason));
        return report(
            file,
            "cancelled",
            Vec::new(),
            stages,
            shared,
            &report_context,
            graph.as_ref(),
        );
    }
    for capsule_name in execution_order {
        if let Some(reason) = pipeline_cancellation_reason(&control, stages.len()) {
            report_context.cancellation_reason.set(Some(reason));
            return report(
                file,
                "cancelled",
                Vec::new(),
                stages,
                shared,
                &report_context,
                graph.as_ref(),
            );
        }
        let capsule = program
            .capsules()
            .find(|capsule| capsule.name == capsule_name)
            .expect("scheduled pipeline capsule must exist");
        let contract = match stage_contract(capsule, program) {
            Ok(contract) => contract,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    sources: Vec::new(),
                    source: String::new(),
                    move_sources: Vec::new(),
                    update_sources: Vec::new(),
                    handoff_committed: false,
                    target: String::new(),
                    decision: "rejected".to_string(),
                    input: "null".to_string(),
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    duration_ms: None,
                    cache_identity: None,
                    cache_restore: None,
                    io: None,
                    buffer: None,
                    updates: Vec::new(),
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(
                    file,
                    "rejected",
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
        };
        let sources = contract.sources;
        let move_sources = contract.move_sources;
        let update_sources = contract.update_sources;
        let target = contract.target;
        let source = sources.join(",");
        let sensitive_input = sources.iter().any(|source| {
            sensitive_shares.contains(source)
                || sensitive_shares.contains(share_reference(source).0)
        });
        let inputs = match sources
            .iter()
            .map(|source| {
                shared_value(&shared, source).cloned().ok_or_else(|| {
                    format!(
                        "pipeline source '{source}' has no default, override, or preceding stage output"
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(inputs) => inputs,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    sources,
                    source,
                    move_sources,
                    update_sources,
                    handoff_committed: false,
                    target,
                    decision: "rejected".to_string(),
                    input: "null".to_string(),
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    duration_ms: None,
                    cache_identity: None,
                    cache_restore: None,
                    io: None,
                    buffer: None,
                    updates: Vec::new(),
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(
                    file,
                    "rejected",
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
        };
        let input_values_json = inputs.iter().map(value_to_json).collect::<Vec<_>>();
        let input_json = if input_values_json.len() == 1 {
            input_values_json[0].clone()
        } else {
            format!("[{}]", input_values_json.join(","))
        };
        let first_input = inputs.first().expect("stage contract requires an input");
        let first_input_json = &input_values_json[0];
        let render_context = match render_context(capsule, first_input, first_input_json) {
            Ok(context) => context,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    sources,
                    source,
                    move_sources,
                    update_sources,
                    handoff_committed: false,
                    target,
                    decision: "rejected".to_string(),
                    input: input_json,
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    duration_ms: None,
                    cache_identity: None,
                    cache_restore: None,
                    io: None,
                    buffer: None,
                    updates: Vec::new(),
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(
                    file,
                    "rejected",
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
        };
        let encode_context = match encode_context(capsule, first_input) {
            Ok(context) => context,
            Err(error) => {
                stages.push(PipelineStage {
                    capsule: capsule.name.clone(),
                    sources,
                    source,
                    move_sources,
                    update_sources,
                    handoff_committed: false,
                    target,
                    decision: "rejected".to_string(),
                    input: input_json,
                    redacted_inputs: Vec::new(),
                    output: None,
                    error: Some(error),
                    native: false,
                    duration_ms: None,
                    cache_identity: None,
                    cache_restore: None,
                    io: None,
                    buffer: None,
                    updates: Vec::new(),
                    artifact: None,
                    render: None,
                    encode: None,
                });
                return report(
                    file,
                    "rejected",
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
        };
        let cache_request = cache.as_deref().and_then(|_| {
            cache_request_sha256(
                program,
                capsule,
                &sources,
                &input_values_json,
                &target,
                &limits,
                &allow,
                sensitive_input,
            )
        });
        if let (Some(request), Some(cache)) = (cache_request.as_deref(), cache.as_deref_mut()) {
            cache.hydrate(request);
        }
        let cached_entry = cache_request
            .as_deref()
            .and_then(|request| {
                cache.as_deref().and_then(|cache| {
                    validated_cache_entry(cache, request, program, capsule, &target)
                })
            })
            .cloned();
        if let (Some(request_sha256), Some(entry)) = (cache_request.as_ref(), cached_entry) {
            let output = entry.output;
            let output_json = value_to_json(&output);
            let mut identity = entry.identity;
            identity.reuse = match entry.storage.as_str() {
                "bounded_process_local_memory" => "verified_process_local_hit",
                "authenticated_hmac_sha256_content_addressed_file" => {
                    "verified_authenticated_file_hit"
                }
                _ => "verified_integrity_checked_file_hit",
            }
            .to_string();
            let mut event = stage_event(
                capsule,
                sources,
                move_sources,
                update_sources,
                target.clone(),
                None,
                input_json,
                Some(output_json),
                None,
                None,
                None,
            );
            event.decision = "cache_hit".to_string();
            event.duration_ms = Some(0);
            event.cache_identity = Some(identity);
            event.cache_restore = Some(PipelineCacheRestoreReceipt {
                schema: "forgentex.pipeline_cache_restore.v1".to_string(),
                request_sha256: request_sha256.clone(),
                output_sha256: entry.output_sha256,
                output_bytes: entry.output_bytes,
                storage: entry.storage,
                decision: "verified_hit".to_string(),
            });
            event.buffer = buffer_receipt(
                share_type(program, &target).expect("validated cache target"),
                &output,
                None,
            )
            .expect("validated cache output must retain its reviewed layout");
            event.artifact = restored_cache_artifact(capsule, &output)
                .expect("validated cache artifact must retain its reviewed policy");
            if control
                .cancellation
                .as_ref()
                .is_some_and(PipelineCancellationToken::is_cancelled)
            {
                event.output = None;
                event.error =
                    Some("pipeline cancellation observed before cached target commit".to_string());
                stages.push(event);
                report_context
                    .cancellation_reason
                    .set(Some("external_token"));
                return report(
                    file,
                    "cancelled",
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
            if let Some(max_owned_bytes) = limits.max_owned_bytes {
                let used_owned_bytes = stages
                    .iter()
                    .map(stage_owned_bytes)
                    .sum::<usize>()
                    .saturating_add(stage_owned_bytes(&event));
                if used_owned_bytes > max_owned_bytes {
                    event.output = None;
                    event.error = Some(format!(
                        "pipeline budget max_owned_bytes {max_owned_bytes} exceeded by {used_owned_bytes} bytes"
                    ));
                    stages.push(event);
                    return report(
                        file,
                        "budget_exhausted",
                        Vec::new(),
                        stages,
                        shared,
                        &report_context,
                        graph.as_ref(),
                    );
                }
            }
            shared.insert(target, output);
            event.handoff_committed = true;
            stages.push(event);
            continue;
        }
        let (outcome, calls) = invoke_capsule_live_transactional_args(
            program,
            &capsule.name,
            inputs.clone(),
            allow.clone(),
        );
        let call = calls.last();
        match outcome {
            Ok(outcome)
                if value_matches_type(
                    program,
                    &outcome.output,
                    share_type(program, &target).unwrap(),
                ) =>
            {
                let output = outcome.output;
                let output_json = value_to_json(&output);
                let mut event = stage_event(
                    capsule,
                    sources,
                    move_sources,
                    update_sources,
                    target.clone(),
                    call,
                    input_json,
                    Some(output_json),
                    None,
                    render_context.as_ref(),
                    encode_context.as_ref(),
                );
                if let Some(context) = render_context.as_ref() {
                    if let Err(error) = validate_render_output(capsule, context, &output) {
                        event.output = None;
                        event.error = Some(error);
                        event.render = None;
                        stages.push(event);
                        return report(
                            file,
                            "error",
                            Vec::new(),
                            stages,
                            shared,
                            &report_context,
                            graph.as_ref(),
                        );
                    }
                }
                if control
                    .cancellation
                    .as_ref()
                    .is_some_and(PipelineCancellationToken::is_cancelled)
                {
                    event.output = None;
                    event.error =
                        Some("pipeline cancellation observed before target commit".to_string());
                    stages.push(event);
                    report_context
                        .cancellation_reason
                        .set(Some("external_token"));
                    return report(
                        file,
                        "cancelled",
                        Vec::new(),
                        stages,
                        shared,
                        &report_context,
                        graph.as_ref(),
                    );
                }
                event.buffer = match buffer_receipt(
                    share_type(program, &target).unwrap(),
                    &output,
                    call.and_then(|item| item.native.as_ref()),
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        event.output = None;
                        event.error = Some(error);
                        stages.push(event);
                        return report(
                            file,
                            "error",
                            Vec::new(),
                            stages,
                            shared,
                            &report_context,
                            graph.as_ref(),
                        );
                    }
                };
                let updates = match transactional_update_receipts(
                    program,
                    capsule,
                    &event.sources,
                    &inputs,
                    &outcome.mutations,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        event.output = None;
                        event.error = Some(error);
                        stages.push(event);
                        return report(
                            file,
                            "error",
                            Vec::new(),
                            stages,
                            shared,
                            &report_context,
                            graph.as_ref(),
                        );
                    }
                };
                event.updates = updates.receipts;
                if let Some(max_owned_bytes) = limits.max_owned_bytes {
                    let used_owned_bytes = stages
                        .iter()
                        .map(stage_owned_bytes)
                        .sum::<usize>()
                        .saturating_add(stage_owned_bytes(&event));
                    if used_owned_bytes > max_owned_bytes {
                        event.output = None;
                        event.error = Some(format!(
                            "pipeline budget max_owned_bytes {max_owned_bytes} exceeded by {used_owned_bytes} bytes"
                        ));
                        stages.push(event);
                        return report(
                            file,
                            "budget_exhausted",
                            Vec::new(),
                            stages,
                            shared,
                            &report_context,
                            graph.as_ref(),
                        );
                    }
                }
                event.io = stage_io_receipt(call);
                if let Some(max_fs_bytes) = limits.max_fs_bytes {
                    let Some(stage_io) = event.io.as_ref() else {
                        event.output = None;
                        event.error = Some(
                            "pipeline filesystem accounting became unavailable before target commit"
                                .to_string(),
                        );
                        stages.push(event);
                        return report(
                            file,
                            "error",
                            Vec::new(),
                            stages,
                            shared,
                            &report_context,
                            graph.as_ref(),
                        );
                    };
                    let used_fs_bytes = stages
                        .iter()
                        .filter_map(|stage| stage.io.as_ref())
                        .map(|io| io.total_transfer_bytes)
                        .fold(0u64, u64::saturating_add)
                        .saturating_add(stage_io.total_transfer_bytes);
                    if used_fs_bytes > max_fs_bytes {
                        event.output = None;
                        event.error = Some(format!(
                            "pipeline budget max_fs_bytes {max_fs_bytes} exceeded by {used_fs_bytes} bytes"
                        ));
                        stages.push(event);
                        return report(
                            file,
                            "budget_exhausted",
                            Vec::new(),
                            stages,
                            shared,
                            &report_context,
                            graph.as_ref(),
                        );
                    }
                }
                if render_context.is_some() && event.render.is_none() {
                    event.output = None;
                    event.error =
                        Some("render stage completed without native artifact evidence".to_string());
                    stages.push(event);
                    return report(
                        file,
                        "error",
                        Vec::new(),
                        stages,
                        shared,
                        &report_context,
                        graph.as_ref(),
                    );
                }
                if encode_context.is_some() && event.encode.is_none() {
                    event.output = None;
                    event.error =
                        Some("encode stage completed without native artifact evidence".to_string());
                    stages.push(event);
                    return report(
                        file,
                        "error",
                        Vec::new(),
                        stages,
                        shared,
                        &report_context,
                        graph.as_ref(),
                    );
                }
                event.cache_identity = (!sensitive_input)
                    .then(|| {
                        cache_identity_receipt(
                            capsule,
                            &event.sources,
                            &input_values_json,
                            &target,
                            share_type(program, &target).expect("validated pipeline target"),
                            call,
                            &limits,
                        )
                    })
                    .flatten();
                if let (Some(request_sha256), Some(cache), Some(identity)) = (
                    cache_request,
                    cache.as_deref_mut(),
                    event.cache_identity.clone(),
                ) {
                    let cached_output_json = value_to_json(&output);
                    let cached_output_bytes = cached_output_json.len();
                    if let Some(storage) = cache.insert(
                        request_sha256,
                        PipelineCacheEntry {
                            identity,
                            output: output.clone(),
                            output_sha256: crate::crypto::sha256_hex(cached_output_json.as_bytes()),
                            output_bytes: cached_output_bytes,
                            storage: "bounded_process_local_memory".to_string(),
                        },
                    ) {
                        if let Some(identity) = &mut event.cache_identity {
                            identity.reuse = match storage.as_str() {
                                "integrity_checked_content_addressed_file" => {
                                    "stored_process_local_and_integrity_checked_file"
                                }
                                "authenticated_hmac_sha256_content_addressed_file" => {
                                    "stored_process_local_and_authenticated_file"
                                }
                                _ => "stored_process_local",
                            }
                            .to_string();
                        }
                    }
                }
                for source in &event.move_sources {
                    if let Some(key) = shared_value_key(&shared, source) {
                        shared.remove(&key);
                    }
                }
                for (source, value) in updates.values {
                    if sensitive_input {
                        sensitive_shares.insert(source.clone());
                    }
                    shared.insert(source, value);
                }
                if sensitive_input {
                    sensitive_shares.insert(target.clone());
                    sensitive_shares.insert(share_reference(&target).0.to_string());
                }
                shared.insert(target.clone(), output);
                event.handoff_committed = true;
                stages.push(event);
            }
            Ok(outcome) => {
                stages.push(stage_event(
                    capsule,
                    sources,
                    move_sources,
                    update_sources,
                    target.clone(),
                    call,
                    input_json,
                    None,
                    Some(format!(
                        "stage output does not match shared '{}' type {}; got {}",
                        target,
                        share_type(program, &target).unwrap().to_agent(),
                        value_to_json(&outcome.output)
                    )),
                    render_context.as_ref(),
                    encode_context.as_ref(),
                ));
                return report(
                    file,
                    "error",
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
            Err(error) => {
                let status = if call.map(|item| item.decision.as_str()) == Some("denied") {
                    "denied"
                } else {
                    "error"
                };
                stages.push(stage_event(
                    capsule,
                    sources,
                    move_sources,
                    update_sources,
                    target,
                    call,
                    input_json,
                    None,
                    Some(error),
                    render_context.as_ref(),
                    encode_context.as_ref(),
                ));
                return report(
                    file,
                    status,
                    Vec::new(),
                    stages,
                    shared,
                    &report_context,
                    graph.as_ref(),
                );
            }
        }
    }
    report(
        file,
        "ok",
        Vec::new(),
        stages,
        shared,
        &report_context,
        graph.as_ref(),
    )
}

fn validate_pipeline_limits(
    program: &Program,
    limits: &PipelineLimits,
) -> Result<(), (&'static str, String)> {
    const MAX_STAGES: usize = 1024;
    const MAX_WALL_MS: u128 = 86_400_000;
    const MAX_CPU_MS: u128 = 86_400_000;
    const MAX_MEMORY_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
    const MAX_OWNED_BYTES: usize = 1024 * 1024 * 1024;
    const MAX_IO_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

    if limits
        .max_stages
        .is_some_and(|value| value == 0 || value > MAX_STAGES)
    {
        return Err((
            "rejected",
            format!("pipeline max_stages must be between 1 and {MAX_STAGES}"),
        ));
    }
    if limits
        .max_fs_bytes
        .is_some_and(|value| value == 0 || value > MAX_IO_BYTES)
    {
        return Err((
            "rejected",
            format!("pipeline max_fs_bytes must be between 1 and {MAX_IO_BYTES}"),
        ));
    }
    if limits
        .max_net_bytes
        .is_some_and(|value| value == 0 || value > MAX_IO_BYTES)
    {
        return Err((
            "rejected",
            format!("pipeline max_net_bytes must be between 1 and {MAX_IO_BYTES}"),
        ));
    }
    if limits.max_fs_bytes.is_some() && !cfg!(windows) {
        return Err((
            "rejected",
            "pipeline max_fs_bytes requires aggregate OS accounting, which is currently available only through Windows Job Objects"
                .to_string(),
        ));
    }
    if limits.max_net_bytes.is_some() {
        return Err((
            "rejected",
            "pipeline max_net_bytes requires aggregate network accounting, which is unavailable on this host"
                .to_string(),
        ));
    }
    if limits
        .max_wall_ms
        .is_some_and(|value| value == 0 || value > MAX_WALL_MS)
    {
        return Err((
            "rejected",
            format!("pipeline max_wall_ms must be between 1 and {MAX_WALL_MS}"),
        ));
    }
    if limits
        .max_owned_bytes
        .is_some_and(|value| value == 0 || value > MAX_OWNED_BYTES)
    {
        return Err((
            "rejected",
            format!("pipeline max_owned_bytes must be between 1 and {MAX_OWNED_BYTES}"),
        ));
    }
    if limits
        .max_cpu_ms
        .is_some_and(|value| value == 0 || value > MAX_CPU_MS)
    {
        return Err((
            "rejected",
            format!("pipeline max_cpu_ms must be between 1 and {MAX_CPU_MS}"),
        ));
    }
    if limits
        .max_memory_bytes
        .is_some_and(|value| value == 0 || value > MAX_MEMORY_BYTES)
    {
        return Err((
            "rejected",
            format!("pipeline max_memory_bytes must be between 1 and {MAX_MEMORY_BYTES}"),
        ));
    }

    let stages = program
        .capsules()
        .filter(|capsule| !capsule.bindings.is_empty())
        .collect::<Vec<_>>();
    if limits.max_fs_bytes.is_some() {
        let unsupported = stages
            .iter()
            .filter(|capsule| {
                !matches!(
                    capsule.language.as_str(),
                    "c" | "cpp" | "c++" | "rust" | "zig"
                )
            })
            .map(|capsule| capsule.name.clone())
            .collect::<Vec<_>>();
        if !unsupported.is_empty() {
            return Err((
                "rejected",
                format!(
                    "pipeline filesystem budget requires native Job-accounted stages; unsupported [{}]",
                    unsupported.join(",")
                ),
            ));
        }
    }
    if let Some(max_stages) = limits.max_stages {
        if stages.len() > max_stages {
            return Err((
                "budget_exhausted",
                format!(
                    "pipeline budget max_stages {max_stages} is smaller than planned stage count {}",
                    stages.len()
                ),
            ));
        }
    }
    if let Some(max_wall_ms) = limits.max_wall_ms {
        let missing = stages
            .iter()
            .filter(|capsule| capsule_wall_limit_ms(capsule).is_none())
            .map(|capsule| capsule.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err((
                "rejected",
                format!(
                    "pipeline wall budget requires limit.wall_ms on every stage; missing [{}]",
                    missing.join(",")
                ),
            ));
        }
        let planned = stages
            .iter()
            .filter_map(|capsule| capsule_wall_limit_ms(capsule))
            .fold(0u128, u128::saturating_add);
        if planned > max_wall_ms {
            return Err((
                "budget_exhausted",
                format!(
                    "pipeline budget max_wall_ms {max_wall_ms} is smaller than declared stage total {planned}"
                ),
            ));
        }
    }
    if let Some(max_cpu_ms) = limits.max_cpu_ms {
        let missing = stages
            .iter()
            .filter(|capsule| capsule_cpu_limit_ms(capsule).is_none())
            .map(|capsule| capsule.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err((
                "rejected",
                format!(
                    "pipeline CPU budget requires limit.cpu_ms on every stage; missing [{}]",
                    missing.join(",")
                ),
            ));
        }
        let planned = stages
            .iter()
            .filter_map(|capsule| capsule_cpu_limit_ms(capsule))
            .fold(0u128, u128::saturating_add);
        if planned > max_cpu_ms {
            return Err((
                "budget_exhausted",
                format!(
                    "pipeline budget max_cpu_ms {max_cpu_ms} is smaller than declared stage total {planned}"
                ),
            ));
        }
    }
    if let Some(max_memory_bytes) = limits.max_memory_bytes {
        let missing = stages
            .iter()
            .filter(|capsule| capsule_memory_limit_bytes(capsule).is_none())
            .map(|capsule| capsule.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err((
                "rejected",
                format!(
                    "pipeline memory budget requires limit.mem_mb on every stage; missing [{}]",
                    missing.join(",")
                ),
            ));
        }
        let planned_peak = stages
            .iter()
            .filter_map(|capsule| capsule_memory_limit_bytes(capsule))
            .max()
            .unwrap_or(0);
        if planned_peak > max_memory_bytes {
            return Err((
                "budget_exhausted",
                format!(
                    "pipeline budget max_memory_bytes {max_memory_bytes} is smaller than declared sequential peak {planned_peak}"
                ),
            ));
        }
    }
    Ok(())
}

fn validate_transactional_handoffs(program: &Program) -> Result<(), String> {
    let mut consumers = BTreeMap::<String, Vec<(String, String, ShareMode, Option<u64>)>>::new();
    for capsule in program.capsules() {
        for binding in capsule
            .bindings
            .iter()
            .filter(|binding| binding.mode.is_source())
        {
            let (base, version) = share_reference(&binding.name);
            consumers.entry(base.to_string()).or_default().push((
                capsule.name.clone(),
                binding.name.clone(),
                binding.mode,
                version,
            ));
        }
        let transactional_sources = capsule
            .bindings
            .iter()
            .filter(|binding| matches!(binding.mode, ShareMode::Move | ShareMode::Update))
            .map(|binding| share_reference(&binding.name).0)
            .collect::<BTreeSet<_>>();
        if let Some(shared_name) = capsule
            .bindings
            .iter()
            .filter(|binding| binding.mode == ShareMode::Rw)
            .map(|binding| share_reference(&binding.name).0)
            .find(|target| transactional_sources.contains(*target))
        {
            return Err(format!(
                "pipeline stage '{}' cannot move or update shared '{}' with the same rw target",
                capsule.name, shared_name
            ));
        }
    }

    for (shared_name, bindings) in consumers {
        let explicit = bindings.iter().any(|(_, _, _, version)| version.is_some());
        if explicit && bindings.iter().any(|(_, _, _, version)| version.is_none()) {
            return Err(format!(
                "pipeline share '{shared_name}' mixes explicit and implicit source versions"
            ));
        }
        if explicit {
            let mut by_reference =
                BTreeMap::<String, Vec<&(String, String, ShareMode, Option<u64>)>>::new();
            for binding in &bindings {
                by_reference
                    .entry(binding.1.clone())
                    .or_default()
                    .push(binding);
            }
            for (reference, consumers) in by_reference {
                let moves = consumers
                    .iter()
                    .filter(|binding| binding.2 == ShareMode::Move)
                    .count();
                let updates = consumers
                    .iter()
                    .filter(|binding| binding.2 == ShareMode::Update)
                    .count();
                if moves > 0 && (moves != 1 || consumers.len() != 1) {
                    return Err(format!(
                        "pipeline move source '{reference}' must have exactly one exclusive consumer"
                    ));
                }
                if updates > 1 {
                    return Err(format!(
                        "pipeline update source '{reference}' may have only one version producer"
                    ));
                }
                if updates == 1 {
                    let version = consumers
                        .iter()
                        .find(|binding| binding.2 == ShareMode::Update)
                        .and_then(|binding| binding.3)
                        .expect("explicit version validated");
                    version.checked_add(1).ok_or_else(|| {
                        format!(
                            "pipeline update source '{reference}' cannot advance beyond u64::MAX"
                        )
                    })?;
                }
            }
            continue;
        }

        if bindings
            .iter()
            .any(|(_, _, mode, _)| *mode == ShareMode::Move)
            && (bindings.len() != 1 || bindings[0].2 != ShareMode::Move)
        {
            let consumers = bindings
                .iter()
                .map(|(capsule, _, mode, _)| format!("{}:{}", capsule, mode.as_str()))
                .collect::<Vec<_>>();
            return Err(format!(
                "pipeline move source '{}' must have exactly one exclusive consumer; found [{}]",
                shared_name,
                consumers.join(",")
            ));
        }
        if bindings
            .iter()
            .any(|(_, _, mode, _)| *mode == ShareMode::Update)
            && (bindings.len() != 1 || bindings[0].2 != ShareMode::Update)
        {
            let consumers = bindings
                .iter()
                .map(|(capsule, _, mode, _)| format!("{}:{}", capsule, mode.as_str()))
                .collect::<Vec<_>>();
            return Err(format!(
                "pipeline update source '{}' must have exactly one exclusive consumer; found [{}]",
                shared_name,
                consumers.join(",")
            ));
        }
    }
    Ok(())
}

fn capsule_wall_limit_ms(capsule: &Capsule) -> Option<u128> {
    crate::sandbox::analyze_capsule(capsule)
        .limits
        .into_iter()
        .find_map(|(key, value)| (key == "wall_ms").then(|| value.parse().ok()).flatten())
}

fn capsule_cpu_limit_ms(capsule: &Capsule) -> Option<u128> {
    crate::sandbox::analyze_capsule(capsule)
        .limits
        .into_iter()
        .find_map(|(key, value)| {
            (key == "cpu_ms")
                .then(|| value.parse::<u64>().ok().map(u128::from))
                .flatten()
        })
}

fn capsule_memory_limit_bytes(capsule: &Capsule) -> Option<u64> {
    crate::sandbox::analyze_capsule(capsule)
        .limits
        .into_iter()
        .find_map(|(key, value)| {
            (key == "mem_mb")
                .then(|| {
                    value
                        .parse::<u64>()
                        .ok()
                        .map(|megabytes| megabytes.saturating_mul(1024 * 1024))
                })
                .flatten()
        })
}

fn initial_shared_values(
    program: &Program,
    overrides: Vec<(String, CapsuleValue)>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut values = BTreeMap::new();
    for share in program.shares() {
        if let Some(value) = &share.default {
            values.insert(share.name.clone(), capsule_value_to_value(value));
        }
    }
    for (name, value) in overrides {
        let share = program
            .shares()
            .find(|share| share.name == name)
            .ok_or_else(|| format!("pipeline override names unknown shared value '{name}'"))?;
        if !value.matches_type(&share.ty) {
            return Err(format!(
                "pipeline override for '{}' does not match {}",
                name,
                share.ty.to_agent()
            ));
        }
        values.insert(name, capsule_value_to_value(&value));
    }
    Ok(values)
}

fn dependency_graph_plan(
    program: &Program,
    shared: &BTreeMap<String, Value>,
) -> Result<(Vec<String>, PipelineGraphReceipt), String> {
    let capsules = program
        .capsules()
        .filter(|capsule| !capsule.bindings.is_empty())
        .collect::<Vec<_>>();
    let contracts = capsules
        .iter()
        .map(|capsule| stage_contract(capsule, program))
        .collect::<Result<Vec<_>, _>>()?;

    let mut writers = BTreeMap::new();
    let mut base_writers = BTreeMap::new();
    for (index, contract) in contracts.iter().enumerate() {
        let target_base = share_reference(&contract.target).0.to_string();
        if let Some(previous) = base_writers.insert(target_base.clone(), index) {
            return Err(format!(
                "pipeline dependency graph has multiple writers for share '{}': '{}' and '{}'",
                target_base, capsules[previous].name, capsules[index].name
            ));
        }
        writers.insert(target_base.clone(), index);
        writers.insert(explicit_share_reference(&target_base, 0), index);
        for source in &contract.update_sources {
            let (_, version) = share_reference(source);
            let Some(version) = version else {
                continue;
            };
            let output = explicit_share_reference(
                source,
                version
                    .checked_add(1)
                    .expect("version overflow rejected during handoff validation"),
            );
            if let Some(previous) = writers.insert(output.clone(), index) {
                return Err(format!(
                    "pipeline dependency graph has multiple producers for share version '{}': '{}' and '{}'",
                    output, capsules[previous].name, capsules[index].name
                ));
            }
        }
    }

    let mut roots = BTreeSet::new();
    let mut source_readers = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut edges = Vec::new();
    let mut outgoing = vec![Vec::new(); capsules.len()];
    let mut indegree = vec![0usize; capsules.len()];
    for (reader, contract) in contracts.iter().enumerate() {
        let mut planned_sources = BTreeSet::new();
        for source in &contract.sources {
            if !planned_sources.insert(source) {
                continue;
            }
            source_readers
                .entry(source.clone())
                .or_default()
                .insert(reader);
            if let Some(&writer) = writers.get(source) {
                edges.push(PipelineGraphEdge {
                    from: capsules[writer].name.clone(),
                    to: capsules[reader].name.clone(),
                    share: source.clone(),
                });
                outgoing[writer].push(reader);
                indegree[reader] = indegree[reader].saturating_add(1);
            } else if shared_value(shared, source).is_some() {
                roots.insert(source.clone());
            } else {
                return Err(format!(
                    "pipeline dependency graph source '{source}' for stage '{}' has no initializer or writer",
                    capsules[reader].name
                ));
            }
        }
    }

    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(capsules.len());
    while let Some(index) = ready.pop_first() {
        order.push(index);
        for dependent in &outgoing[index] {
            indegree[*dependent] -= 1;
            if indegree[*dependent] == 0 {
                ready.insert(*dependent);
            }
        }
    }
    if order.len() != capsules.len() {
        let cycle = indegree
            .iter()
            .enumerate()
            .filter(|(_, degree)| **degree > 0)
            .map(|(index, _)| capsules[index].name.clone())
            .collect::<Vec<_>>();
        return Err(format!(
            "pipeline dependency graph contains a cycle involving [{}]",
            cycle.join(",")
        ));
    }

    let execution_order = order
        .into_iter()
        .map(|index| capsules[index].name.clone())
        .collect::<Vec<_>>();
    let fan_out_shares = source_readers
        .into_iter()
        .filter_map(|(share, readers)| (readers.len() > 1).then_some(share))
        .collect::<Vec<_>>();
    Ok((
        execution_order.clone(),
        PipelineGraphReceipt {
            schema: "forgentex.pipeline_graph.v1".to_string(),
            origin: "derived_bindings".to_string(),
            declaration: None,
            nodes: capsules
                .iter()
                .map(|capsule| capsule.name.clone())
                .collect(),
            edges,
            roots: roots.into_iter().collect(),
            fan_out_shares,
            execution_order,
        },
    ))
}

fn source_graph_plan(
    program: &Program,
    shared: &BTreeMap<String, Value>,
) -> Result<(Vec<String>, PipelineGraphReceipt), String> {
    let declarations = program.pipelines().collect::<Vec<_>>();
    let declaration = match declarations.as_slice() {
        [declaration] => *declaration,
        [] => {
            return Err(
                "source graph scheduling requires exactly one pipeline declaration; found 0"
                    .to_string(),
            )
        }
        _ => {
            return Err(format!(
                "source graph scheduling requires exactly one pipeline declaration; found {}",
                declarations.len()
            ))
        }
    };
    if declaration.name.is_empty() {
        return Err("pipeline declaration name must not be empty".to_string());
    }
    if declaration.stages.is_empty() {
        return Err(format!(
            "pipeline '{}' must declare at least one stage",
            declaration.name
        ));
    }

    let mut stage_indexes = BTreeMap::new();
    for (index, stage) in declaration.stages.iter().enumerate() {
        if stage.is_empty() {
            return Err(format!(
                "pipeline '{}' contains an empty stage name",
                declaration.name
            ));
        }
        if stage_indexes.insert(stage.clone(), index).is_some() {
            return Err(format!(
                "pipeline '{}' declares stage '{}' more than once",
                declaration.name, stage
            ));
        }
    }

    let (_, mut receipt) = dependency_graph_plan(program, shared)?;
    let derived_nodes = receipt.nodes.iter().cloned().collect::<BTreeSet<_>>();
    let declared_nodes = declaration.stages.iter().cloned().collect::<BTreeSet<_>>();
    let missing_nodes = derived_nodes
        .difference(&declared_nodes)
        .cloned()
        .collect::<Vec<_>>();
    let unexpected_nodes = declared_nodes
        .difference(&derived_nodes)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_nodes.is_empty() || !unexpected_nodes.is_empty() {
        return Err(format!(
            "pipeline '{}' stage manifest does not match bound capsules: missing=[{}] unexpected=[{}]",
            declaration.name,
            missing_nodes.join(","),
            unexpected_nodes.join(",")
        ));
    }

    let declared_edges = declaration.edges.iter().cloned().collect::<BTreeSet<_>>();
    if declared_edges.len() != declaration.edges.len() {
        return Err(format!(
            "pipeline '{}' declares a duplicate edge",
            declaration.name
        ));
    }
    for edge in &declaration.edges {
        if edge.from == edge.to {
            return Err(format!(
                "pipeline '{}' declares a self-edge for stage '{}'",
                declaration.name, edge.from
            ));
        }
        if !stage_indexes.contains_key(&edge.from) || !stage_indexes.contains_key(&edge.to) {
            return Err(format!(
                "pipeline '{}' edge '{} -> {} via {}' names a stage outside its manifest",
                declaration.name, edge.from, edge.to, edge.share
            ));
        }
    }
    let derived_edges = receipt
        .edges
        .iter()
        .map(|edge| crate::ast::PipelineEdgeDecl {
            from: edge.from.clone(),
            to: edge.to.clone(),
            share: edge.share.clone(),
        })
        .collect::<BTreeSet<_>>();
    let missing_edges = derived_edges
        .difference(&declared_edges)
        .map(render_declared_edge)
        .collect::<Vec<_>>();
    let unexpected_edges = declared_edges
        .difference(&derived_edges)
        .map(render_declared_edge)
        .collect::<Vec<_>>();
    if !missing_edges.is_empty() || !unexpected_edges.is_empty() {
        return Err(format!(
            "pipeline '{}' edge manifest does not match capsule bindings: missing=[{}] unexpected=[{}]",
            declaration.name,
            missing_edges.join(","),
            unexpected_edges.join(",")
        ));
    }

    // The declaration's stage order is the deterministic tie-breaker among
    // independent ready nodes. Exact edge validation above prevents it from
    // changing the binding-derived dependency relation.
    let mut outgoing = vec![Vec::new(); declaration.stages.len()];
    let mut indegree = vec![0usize; declaration.stages.len()];
    for edge in &declaration.edges {
        let from = stage_indexes[&edge.from];
        let to = stage_indexes[&edge.to];
        outgoing[from].push(to);
        indegree[to] = indegree[to].saturating_add(1);
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut execution_order = Vec::with_capacity(declaration.stages.len());
    while let Some(index) = ready.pop_first() {
        execution_order.push(declaration.stages[index].clone());
        for dependent in &outgoing[index] {
            indegree[*dependent] -= 1;
            if indegree[*dependent] == 0 {
                ready.insert(*dependent);
            }
        }
    }
    if execution_order.len() != declaration.stages.len() {
        return Err(format!("pipeline '{}' contains a cycle", declaration.name));
    }
    receipt.origin = "source_declaration".to_string();
    receipt.declaration = Some(declaration.name.clone());
    receipt.nodes = declaration.stages.clone();
    receipt.edges = declaration
        .edges
        .iter()
        .map(|edge| PipelineGraphEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            share: edge.share.clone(),
        })
        .collect();
    receipt.execution_order = execution_order.clone();
    Ok((execution_order, receipt))
}

fn render_declared_edge(edge: &crate::ast::PipelineEdgeDecl) -> String {
    format!("{}->{} via {}", edge.from, edge.to, edge.share)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StageContract {
    sources: Vec<String>,
    move_sources: Vec<String>,
    update_sources: Vec<String>,
    target: String,
}

fn stage_contract(capsule: &Capsule, program: &Program) -> Result<StageContract, String> {
    if capsule.inputs.is_empty() || capsule.outputs.len() != 1 {
        return Err(format!(
            "pipeline stage '{}' requires one or more inputs and exactly one output",
            capsule.name
        ));
    }
    let sources = capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode.is_source())
        .map(|binding| binding.name.clone())
        .collect::<Vec<_>>();
    let move_sources = capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode == ShareMode::Move)
        .map(|binding| binding.name.clone())
        .collect::<Vec<_>>();
    let update_sources = capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode == ShareMode::Update)
        .map(|binding| binding.name.clone())
        .collect::<Vec<_>>();
    let source_modes = capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode.is_source())
        .map(|binding| binding.mode)
        .collect::<Vec<_>>();
    let targets = capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode == ShareMode::Rw)
        .map(|binding| binding.name.clone())
        .collect::<Vec<_>>();
    if sources.len() != capsule.inputs.len() || targets.len() != 1 {
        return Err(format!(
            "pipeline stage '{}' requires one ordered read/move/update binding per input (expected {}, found {}) and exactly one rw binding (found {})",
            capsule.name,
            capsule.inputs.len(),
            sources.len(),
            targets.len()
        ));
    }
    let target = targets.into_iter().next().expect("one target validated");
    let target_ty = share_type(program, &target).ok_or_else(|| {
        format!(
            "pipeline stage '{}' writes unknown shared '{}",
            capsule.name, target
        )
    })?;
    if !is_pipeline_value(program, target_ty) {
        return Err(format!(
            "pipeline stage '{}' only supports scalar, restricted @repr(C) struct, or reviewed owned-vector shared values",
            capsule.name
        ));
    }
    for (index, ((source, mode), input)) in sources
        .iter()
        .zip(&source_modes)
        .zip(&capsule.inputs)
        .enumerate()
    {
        let source_ty = share_type(program, source).ok_or_else(|| {
            format!(
                "pipeline stage '{}' reads unknown shared '{}'",
                capsule.name, source
            )
        })?;
        if !is_pipeline_value(program, source_ty) {
            return Err(format!(
                "pipeline stage '{}' input {} shared '{}' is not a supported pipeline value",
                capsule.name, index, source
            ));
        }
        let compatible = if *mode == ShareMode::Update {
            pipeline_update_input_compatible(source_ty, &input.ty)
        } else {
            pipeline_input_compatible(source_ty, &input.ty)
        };
        if !compatible {
            return Err(format!(
                "pipeline stage '{}' boundary must match shared '{}' type {} for input '{}'",
                capsule.name,
                source,
                source_ty.to_agent(),
                input.name
            ));
        }
    }
    if &capsule.outputs[0].ty != target_ty {
        return Err(format!(
            "pipeline stage '{}' output must match shared '{}' type {}",
            capsule.name,
            target,
            target_ty.to_agent()
        ));
    }
    validate_render_stage(capsule, program)?;
    validate_encode_stage(capsule, program, &sources[0])?;
    Ok(StageContract {
        sources,
        move_sources,
        update_sources,
        target,
    })
}

fn validate_encode_stage(capsule: &Capsule, program: &Program, source: &str) -> Result<(), String> {
    let Some(policy) = crate::capsule::encode_policy(capsule)? else {
        return Ok(());
    };
    if capsule.inputs.len() != 1 {
        return Err("encode stage requires exactly one raw-frame input".to_string());
    }
    let producer = program
        .capsules()
        .take_while(|candidate| candidate.name != capsule.name)
        .filter(|candidate| {
            candidate.bindings.iter().any(|binding| {
                binding.mode == ShareMode::Rw
                    && share_reference(&binding.name).0 == share_reference(source).0
            })
        })
        .last()
        .ok_or_else(|| {
            format!(
                "encode stage '{}' requires a preceding producer for shared raw frame '{}'",
                capsule.name, source
            )
        })?;
    let artifact = crate::capsule::artifact_policy(producer)?
        .ok_or_else(|| "encode input producer requires an artifact policy".to_string())?;
    if artifact.media_type != "application/x-forgentex-rgb" {
        return Err(format!(
            "encode input producer '{}' must publish application/x-forgentex-rgb, found '{}'",
            producer.name, artifact.media_type
        ));
    }
    let producer_policy = crate::capsule::render_policy(producer)?;
    let producer_color_space = producer_policy
        .as_ref()
        .and_then(|producer_policy| producer_policy.color_space.as_deref());
    if policy.color_space.as_deref() != producer_color_space {
        return Err(format!(
            "encode color space {:?} must match raw-frame producer '{}' color space {:?}",
            policy.color_space, producer.name, producer_color_space
        ));
    }
    let producer_render_pass = producer_policy
        .as_ref()
        .and_then(|producer_policy| producer_policy.render_pass.as_deref());
    if policy.input_pass.as_deref() != producer_render_pass {
        return Err(format!(
            "encode input pass {:?} must match raw-frame producer '{}' render pass {:?}",
            policy.input_pass, producer.name, producer_render_pass
        ));
    }
    Ok(())
}

fn validate_render_stage(capsule: &Capsule, program: &Program) -> Result<(), String> {
    let Some(policy) = crate::capsule::render_policy(capsule)? else {
        return Ok(());
    };
    if capsule.inputs.len() != 1 {
        return Err("render stage requires exactly one scene input".to_string());
    }
    let Type::Named(scene_type) = &capsule.inputs[0].ty else {
        return Err("render stage requires one named scene struct".to_string());
    };
    let scene = program
        .structs()
        .find(|decl| decl.name == *scene_type)
        .ok_or_else(|| format!("render scene struct '{scene_type}' is not declared"))?;
    let mut field_names = vec![policy.seed_field.as_str(), "width", "height"];
    if let Some(field) = policy.frame_index_field.as_deref() {
        field_names.push(field);
    }
    if let Some(field) = policy.frame_count_field.as_deref() {
        field_names.push(field);
    }
    if let Some(field) = policy.samples_field.as_deref() {
        field_names.push(field);
    }
    if let Some(field) = policy.shutter_q16_field.as_deref() {
        field_names.push(field);
    }
    if let Some(field) = policy.adaptive_min_samples_field.as_deref() {
        field_names.push(field);
    }
    if let Some(field) = policy.adaptive_noise_q16_field.as_deref() {
        field_names.push(field);
    }
    for field_name in field_names {
        let field = scene
            .fields
            .iter()
            .find(|field| field.name == field_name)
            .ok_or_else(|| {
                format!(
                    "render scene struct '{scene_type}' is missing integer field '{field_name}'"
                )
            })?;
        if !matches!(&field.ty, Type::Primitive(name) if crate::ast::is_integer_primitive(name)) {
            return Err(format!(
                "render scene field '{scene_type}.{field_name}' must be an integer"
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct RenderContext {
    profile: String,
    color_space: Option<String>,
    device: Option<String>,
    render_pass: Option<String>,
    engine: Option<String>,
    engine_version: Option<String>,
    sequence_id: Option<String>,
    scene_type: String,
    scene_sha256: String,
    seed: i64,
    width: u32,
    height: u32,
    frame_index: u32,
    frame_count: u32,
    frames_per_second: Option<u32>,
    samples_per_pixel: Option<u32>,
    shutter_q16: Option<u32>,
    adaptive_min_samples: Option<u32>,
    adaptive_noise_q16: Option<u32>,
}

#[derive(Clone, Debug)]
struct EncodeContext {
    profile: String,
    color_space: Option<String>,
    input_pass: Option<String>,
    engine: Option<String>,
    engine_version: Option<String>,
    compression: Option<String>,
    compression_level: Option<u32>,
    pixel_format: Option<String>,
    interlace: Option<String>,
    scanline_filter: Option<String>,
    chunk_checksum: Option<String>,
    stream_checksum: Option<String>,
    chunk_order: Option<String>,
    input_sha256: String,
    input_bytes: u64,
    input_width: u32,
    input_height: u32,
    input_format: String,
}

fn render_context(
    capsule: &Capsule,
    input: &Value,
    input_json: &str,
) -> Result<Option<RenderContext>, String> {
    let Some(policy) = crate::capsule::render_policy(capsule)? else {
        return Ok(None);
    };
    let Value::Struct(scene_type, fields) = input else {
        return Err("render stage input is not a scene struct".to_string());
    };
    let integer = |name: &str| match fields.get(name) {
        Some(Value::Int(value)) => Ok(*value),
        _ => Err(format!(
            "render scene field '{scene_type}.{name}' is not an integer"
        )),
    };
    let seed = integer(&policy.seed_field)?;
    let width = integer("width")?;
    let height = integer("height")?;
    let (max_width, max_height) = match policy.profile.as_str() {
        "preview" => (PREVIEW_MAX_WIDTH, PREVIEW_MAX_HEIGHT),
        "final" => (FINAL_MAX_WIDTH, FINAL_MAX_HEIGHT),
        _ => unreachable!("render_policy validates the profile"),
    };
    if !(1..=max_width).contains(&width) || !(1..=max_height).contains(&height) {
        return Err(format!(
            "render {} dimensions {width}x{height} exceed the {max_width}x{max_height} {} profile",
            policy.profile, policy.profile
        ));
    }
    let (frame_index, frame_count, frames_per_second) = match (
        policy.frame_index_field.as_deref(),
        policy.frame_count_field.as_deref(),
        policy.frames_per_second,
    ) {
        (Some(index_field), Some(count_field), Some(fps)) => {
            let frame_index = integer(index_field)?;
            let frame_count = integer(count_field)?;
            if !(1..=MAX_RENDER_FRAME_COUNT).contains(&frame_count) {
                return Err(format!(
                    "render frame count {frame_count} must be from 1 through {MAX_RENDER_FRAME_COUNT}"
                ));
            }
            if !(0..frame_count).contains(&frame_index) {
                return Err(format!(
                    "render frame index {frame_index} must be from 0 through {}",
                    frame_count - 1
                ));
            }
            (frame_index as u32, frame_count as u32, Some(fps))
        }
        _ => (0, 1, None),
    };
    let (samples_per_pixel, shutter_q16) = match (
        policy.samples_field.as_deref(),
        policy.shutter_q16_field.as_deref(),
    ) {
        (Some(samples_field), Some(shutter_field)) => {
            let samples = integer(samples_field)?;
            let max_samples = match policy.profile.as_str() {
                "preview" => PREVIEW_MAX_SAMPLES_PER_PIXEL,
                "final" => FINAL_MAX_SAMPLES_PER_PIXEL,
                _ => unreachable!("render_policy validates the profile"),
            };
            if !(1..=max_samples).contains(&samples) {
                return Err(format!(
                    "render {} samples per pixel {samples} must be from 1 through {max_samples}",
                    policy.profile
                ));
            }
            let shutter = integer(shutter_field)?;
            if !(0..=MAX_RENDER_SHUTTER_Q16).contains(&shutter) {
                return Err(format!(
                    "render shutter Q16 value {shutter} must be from 0 through {MAX_RENDER_SHUTTER_Q16}"
                ));
            }
            (Some(samples as u32), Some(shutter as u32))
        }
        _ => (None, None),
    };
    let (adaptive_min_samples, adaptive_noise_q16) = match (
        policy.adaptive_min_samples_field.as_deref(),
        policy.adaptive_noise_q16_field.as_deref(),
    ) {
        (Some(min_samples_field), Some(noise_q16_field)) => {
            let max_samples =
                i64::from(samples_per_pixel.expect("adaptive manifest requires quality manifest"));
            let min_samples = integer(min_samples_field)?;
            if !(1..=max_samples).contains(&min_samples) {
                return Err(format!(
                    "render adaptive minimum samples {min_samples} must be from 1 through {max_samples}"
                ));
            }
            let noise_q16 = integer(noise_q16_field)?;
            if !(1..=MAX_RENDER_ADAPTIVE_NOISE_Q16).contains(&noise_q16) {
                return Err(format!(
                    "render adaptive noise Q16 value {noise_q16} must be from 1 through {MAX_RENDER_ADAPTIVE_NOISE_Q16}"
                ));
            }
            (Some(min_samples as u32), Some(noise_q16 as u32))
        }
        _ => (None, None),
    };
    Ok(Some(RenderContext {
        profile: policy.profile,
        color_space: policy.color_space,
        device: policy.device,
        render_pass: policy.render_pass,
        engine: policy.engine,
        engine_version: policy.engine_version,
        sequence_id: policy.sequence_id,
        scene_type: scene_type.clone(),
        scene_sha256: crate::crypto::sha256_hex(input_json.as_bytes()),
        seed,
        width: width as u32,
        height: height as u32,
        frame_index,
        frame_count,
        frames_per_second,
        samples_per_pixel,
        shutter_q16,
        adaptive_min_samples,
        adaptive_noise_q16,
    }))
}

fn validate_render_output(
    capsule: &Capsule,
    context: &RenderContext,
    output: &Value,
) -> Result<(), String> {
    let artifact = crate::capsule::artifact_policy(capsule)?
        .expect("render policy requires an artifact policy");
    if artifact.media_type != "application/x-forgentex-rgb" {
        return Ok(());
    }
    let (_, width, height) = raw_rgb_frame(output)?;
    if (width, height) != (context.width, context.height) {
        return Err(format!(
            "raw RGB frame dimensions {width}x{height} do not match render scene dimensions {}x{}",
            context.width, context.height
        ));
    }
    Ok(())
}

fn encode_context(capsule: &Capsule, input: &Value) -> Result<Option<EncodeContext>, String> {
    let Some(policy) = crate::capsule::encode_policy(capsule)? else {
        return Ok(None);
    };
    let (bytes, width, height) = raw_rgb_frame(input)?;
    Ok(Some(EncodeContext {
        profile: policy.profile,
        color_space: policy.color_space,
        input_pass: policy.input_pass,
        engine: policy.engine,
        engine_version: policy.engine_version,
        compression: policy.compression,
        compression_level: policy.compression_level,
        pixel_format: policy.pixel_format,
        interlace: policy.interlace,
        scanline_filter: policy.scanline_filter,
        chunk_checksum: policy.chunk_checksum,
        stream_checksum: policy.stream_checksum,
        chunk_order: policy.chunk_order,
        input_sha256: crate::crypto::sha256_hex(&bytes),
        input_bytes: bytes.len() as u64,
        input_width: width,
        input_height: height,
        input_format: "rgb8".to_string(),
    }))
}

fn raw_rgb_frame(input: &Value) -> Result<(Vec<u8>, u32, u32), String> {
    let Value::Array(values) = input else {
        return Err("raw RGB input is not an owned byte handoff".to_string());
    };
    let bytes = values
        .iter()
        .map(|value| match value {
            Value::Int(byte) if (0..=255).contains(byte) => Ok(*byte as u8),
            _ => Err("raw RGB input contains a non-byte value".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let header: [u8; 8] = bytes
        .get(..8)
        .and_then(|header| header.try_into().ok())
        .ok_or_else(|| "raw RGB frame requires an 8-byte dimension header".to_string())?;
    let width = u32::from_le_bytes(header[..4].try_into().unwrap());
    let height = u32::from_le_bytes(header[4..].try_into().unwrap());
    if width == 0 || height == 0 {
        return Err("raw RGB frame dimensions must be positive".to_string());
    }
    let payload_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| "raw RGB frame dimensions overflow the host byte size".to_string())?;
    let expected_bytes = 8usize
        .checked_add(payload_bytes)
        .ok_or_else(|| "raw RGB frame dimensions overflow the host byte size".to_string())?;
    if bytes.len() != expected_bytes {
        return Err(format!(
            "raw RGB frame {width}x{height} requires {expected_bytes} bytes, found {}",
            bytes.len()
        ));
    }
    Ok((bytes, width, height))
}

fn reviewed_owned_element(ty: &Type) -> Option<(&str, usize)> {
    let Type::Generic { name, args } = ty else {
        return None;
    };
    if name != "Vec" {
        return None;
    }
    match args.as_slice() {
        [Type::Primitive(element)] if element == "u8" => Some(("u8", 1)),
        [Type::Primitive(element)] if element == "i8" => Some(("i8", 1)),
        [Type::Primitive(element)] if element == "u16" => Some(("u16", 2)),
        [Type::Primitive(element)] if element == "i16" => Some(("i16", 2)),
        [Type::Primitive(element)] if element == "u32" => Some(("u32", 4)),
        [Type::Primitive(element)] if element == "i32" => Some(("i32", 4)),
        [Type::Primitive(element)] if element == "u64" => Some(("u64", 8)),
        [Type::Primitive(element)] if element == "i64" => Some(("i64", 8)),
        [Type::Primitive(element)] if element == "isize" => {
            Some(("isize", std::mem::size_of::<isize>()))
        }
        [Type::Primitive(element)] if element == "f32" => Some(("f32", 4)),
        [Type::Primitive(element)] if element == "f64" => Some(("f64", 8)),
        _ => None,
    }
}

fn pipeline_input_compatible(shared: &Type, input: &Type) -> bool {
    if shared == input {
        return true;
    }
    let Some((shared_element, _)) = reviewed_owned_element(shared) else {
        return false;
    };
    matches!(
        input,
        Type::Slice {
            mutable: false,
            elem
        } if matches!(elem.as_ref(), Type::Primitive(name) if name == shared_element)
    )
}

fn pipeline_update_input_compatible(shared: &Type, input: &Type) -> bool {
    let Some((shared_element, _)) = reviewed_owned_element(shared) else {
        return false;
    };
    matches!(
        input,
        Type::Slice {
            mutable: true,
            elem
        } if matches!(elem.as_ref(), Type::Primitive(name) if name == shared_element)
    )
}

fn buffer_receipt(
    ty: &Type,
    value: &Value,
    native: Option<&crate::ffi::NativeReceipt>,
) -> Result<Option<PipelineBufferReceipt>, String> {
    let Some((element_type, element_bytes)) = reviewed_owned_element(ty) else {
        return Ok(None);
    };
    let Value::Array(values) = value else {
        return Err("pipeline owned-vector output is not an array value".to_string());
    };
    let mut canonical = Vec::with_capacity(values.len().saturating_mul(element_bytes));
    for value in values {
        match (element_type, value) {
            ("u8", Value::Int(value)) => canonical.push(u8::try_from(*value).map_err(|_| {
                "pipeline Vec[u8] output contains an out-of-range value".to_string()
            })?),
            ("i8", Value::Int(value)) => {
                canonical.push(i8::try_from(*value).map_err(|_| {
                    "pipeline Vec[i8] output contains an out-of-range value".to_string()
                })? as u8)
            }
            ("u16", Value::Int(value)) => canonical.extend_from_slice(
                &u16::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[u16] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            ("i16", Value::Int(value)) => canonical.extend_from_slice(
                &i16::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[i16] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            ("u32", Value::Int(value)) => canonical.extend_from_slice(
                &u32::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[u32] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            ("i32", Value::Int(value)) => canonical.extend_from_slice(
                &i32::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[i32] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            ("u64", Value::Int(value)) => canonical.extend_from_slice(
                &u64::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[u64] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            ("u64", Value::UInt(value)) => canonical.extend_from_slice(&value.to_le_bytes()),
            ("i64", Value::Int(value)) => canonical.extend_from_slice(&value.to_le_bytes()),
            ("isize", Value::Int(value)) => canonical.extend_from_slice(
                &isize::try_from(*value)
                    .map_err(|_| {
                        "pipeline Vec[isize] output contains an out-of-range value".to_string()
                    })?
                    .to_le_bytes(),
            ),
            ("f32", Value::Float(value)) if finite_exact_f32(*value) => {
                canonical.extend_from_slice(&(*value as f32).to_bits().to_le_bytes());
            }
            ("f64", Value::Float(value)) if value.is_finite() => {
                canonical.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            ("f32" | "f64", Value::Float(_)) => {
                return Err(format!(
                    "pipeline Vec[{element_type}] output contains a non-finite or width-invalid value"
                ));
            }
            ("f32" | "f64", _) => {
                return Err(format!(
                    "pipeline Vec[{element_type}] output contains a non-float value"
                ));
            }
            (_, _) => {
                return Err(format!(
                    "pipeline Vec[{element_type}] output contains a non-integer value"
                ));
            }
        }
    }
    if let Some(reported) = native.and_then(|receipt| receipt.owned_output_bytes) {
        if reported != canonical.len() {
            return Err(format!(
                "pipeline owned-vector byte accounting mismatch: runtime reported {reported}, canonical handoff has {}",
                canonical.len()
            ));
        }
    }
    Ok(Some(PipelineBufferReceipt {
        schema: "forgentex.pipeline_buffer.v1".to_string(),
        element_type: element_type.to_string(),
        elements: values.len(),
        bytes: canonical.len(),
        encoding: if matches!(element_type, "f32" | "f64") {
            "little_endian_ieee754_v1"
        } else {
            "little_endian_integer_v1"
        }
        .to_string(),
        sha256: crate::crypto::sha256_hex(&canonical),
    }))
}

struct TransactionalUpdates {
    receipts: Vec<PipelineUpdateReceipt>,
    values: Vec<(String, Value)>,
}

fn transactional_update_receipts(
    program: &Program,
    capsule: &Capsule,
    sources: &[String],
    inputs: &[Value],
    mutations: &[CapsuleLiveMutation],
) -> Result<TransactionalUpdates, String> {
    let source_bindings = capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode.is_source())
        .collect::<Vec<_>>();
    let mut actual = BTreeMap::new();
    for mutation in mutations {
        if actual
            .insert(mutation.input_index, &mutation.value)
            .is_some()
        {
            return Err(format!(
                "pipeline stage '{}' returned duplicate mutation for input {}",
                capsule.name, mutation.input_index
            ));
        }
    }
    let expected = source_bindings
        .iter()
        .filter(|binding| binding.mode == ShareMode::Update)
        .count();
    if actual.len() != expected {
        return Err(format!(
            "pipeline stage '{}' returned {} mutable copy-back value(s); expected {}",
            capsule.name,
            actual.len(),
            expected
        ));
    }

    let mut receipts = Vec::with_capacity(expected);
    let mut values = Vec::with_capacity(expected);
    for (input_index, ((source, binding), before)) in
        sources.iter().zip(source_bindings).zip(inputs).enumerate()
    {
        if binding.mode != ShareMode::Update {
            if actual.contains_key(&input_index) {
                return Err(format!(
                    "pipeline stage '{}' mutated non-update source '{}'",
                    capsule.name, source
                ));
            }
            continue;
        }
        let after = actual.remove(&input_index).ok_or_else(|| {
            format!(
                "pipeline stage '{}' omitted mutable copy-back for update source '{}'",
                capsule.name, source
            )
        })?;
        let ty = share_type(program, source).expect("validated update source");
        if !value_matches_type(program, after, ty) {
            return Err(format!(
                "pipeline stage '{}' update for '{}' does not match {}",
                capsule.name,
                source,
                ty.to_agent()
            ));
        }
        let before_receipt = buffer_receipt(ty, before, None)?.ok_or_else(|| {
            format!(
                "pipeline stage '{}' update source '{}' is not a reviewed vector",
                capsule.name, source
            )
        })?;
        let after_receipt = buffer_receipt(ty, after, None)?.ok_or_else(|| {
            format!(
                "pipeline stage '{}' update result '{}' is not a reviewed vector",
                capsule.name, source
            )
        })?;
        if before_receipt.elements != after_receipt.elements
            || before_receipt.bytes != after_receipt.bytes
        {
            return Err(format!(
                "pipeline stage '{}' update source '{}' changed layout",
                capsule.name, source
            ));
        }
        let (_, explicit_version) = share_reference(source);
        let source_version = explicit_version.unwrap_or(0);
        let target_version = explicit_version
            .map(|version| {
                version
                    .checked_add(1)
                    .expect("version overflow rejected before execution")
            })
            .unwrap_or(source_version);
        receipts.push(PipelineUpdateReceipt {
            source: source.clone(),
            source_version,
            target_version,
            ty: ty.to_agent(),
            elements: after_receipt.elements,
            bytes: after_receipt.bytes,
            encoding: after_receipt.encoding,
            before_sha256: before_receipt.sha256,
            after_sha256: after_receipt.sha256,
        });
        values.push((
            explicit_version
                .map(|_| explicit_share_reference(source, target_version))
                .unwrap_or_else(|| source.clone()),
            (*after).clone(),
        ));
    }
    if !actual.is_empty() {
        return Err(format!(
            "pipeline stage '{}' returned mutation for an unknown input",
            capsule.name
        ));
    }
    Ok(TransactionalUpdates { receipts, values })
}

fn stage_owned_bytes(stage: &PipelineStage) -> usize {
    stage
        .buffer
        .as_ref()
        .map_or(0, |buffer| buffer.bytes)
        .saturating_add(
            stage
                .updates
                .iter()
                .map(|update| update.bytes)
                .sum::<usize>(),
        )
}

fn finite_exact_f32(value: f64) -> bool {
    value.is_finite() && f64::from(value as f32).to_bits() == value.to_bits()
}

fn cache_identity_receipt(
    capsule: &Capsule,
    sources: &[String],
    input_values_json: &[String],
    target: &str,
    target_ty: &Type,
    call: Option<&CapsuleCall>,
    pipeline_limits: &PipelineLimits,
) -> Option<PipelineCacheIdentityReceipt> {
    let call = call?;
    if !call.redacted_inputs.is_empty() {
        return None;
    }
    let native = call.native.as_ref()?;

    let mut input_fields = vec![("count".to_string(), sources.len().to_string())];
    for (index, ((source, binding), value)) in sources
        .iter()
        .zip(&capsule.inputs)
        .zip(input_values_json)
        .enumerate()
    {
        input_fields.push((format!("{index}.source"), source.clone()));
        let source_mode = capsule
            .bindings
            .iter()
            .filter(|binding| binding.mode.is_source())
            .nth(index)
            .map(|binding| binding.mode.as_str())
            .unwrap_or("invalid");
        input_fields.push((format!("{index}.mode"), source_mode.to_string()));
        input_fields.push((format!("{index}.name"), binding.name.clone()));
        input_fields.push((format!("{index}.type"), binding.ty.to_agent()));
        input_fields.push((format!("{index}.value"), value.clone()));
    }
    let input_sha256 = identity_fields_sha256(&input_fields);

    let code_sha256 = identity_fields_sha256(&[
        ("capsule".to_string(), capsule.name.clone()),
        ("language".to_string(), capsule.language.clone()),
        ("version".to_string(), capsule.version.clone()),
        (
            "environment".to_string(),
            capsule.env.clone().unwrap_or_default(),
        ),
        ("boundary_hash".to_string(), native.boundary_hash.clone()),
        (
            "runner_artifact_sha256".to_string(),
            native.runner_artifact_sha256.clone(),
        ),
        ("toolchain".to_string(), native.toolchain.clone()),
    ]);

    let mut effects = capsule.effects.clone();
    effects.sort();
    effects.dedup();
    let mut sandbox = capsule.sandbox.clone();
    sandbox.sort();
    sandbox.dedup();
    let mut policy_fields = vec![
        ("effects".to_string(), effects.join("\n")),
        ("sandbox".to_string(), sandbox.join("\n")),
        (
            "runner_capability_profile".to_string(),
            native.runner_capability_profile.clone(),
        ),
        ("containment".to_string(), native.containment.clone()),
        (
            "filesystem_isolation".to_string(),
            native.filesystem_isolation.clone(),
        ),
        (
            "network_isolation".to_string(),
            native.network_isolation.clone(),
        ),
        (
            "execution_model".to_string(),
            native.runner_execution_model.clone(),
        ),
        (
            "environment_policy".to_string(),
            native.runner_environment_policy.clone(),
        ),
        (
            "working_directory_policy".to_string(),
            native.runner_working_directory_policy.clone(),
        ),
        (
            "process_identity_policy".to_string(),
            native.runner_process_identity_policy.clone(),
        ),
        (
            "ui_isolation_source".to_string(),
            native.ui_isolation_source.clone(),
        ),
        (
            "windows_ui_restrictions".to_string(),
            option_u32(native.windows_ui_restrictions),
        ),
    ];
    for (index, dependency) in capsule.deps.iter().enumerate() {
        policy_fields.push((format!("dependency.{index}"), dependency.source.clone()));
    }
    let policy_sha256 = identity_fields_sha256(&policy_fields);

    let limits_sha256 = identity_fields_sha256(&[
        (
            "native.wall_ms".to_string(),
            option_u128(native.wall_limit_ms),
        ),
        (
            "native.wall_source".to_string(),
            native.wall_limit_source.clone(),
        ),
        ("native.cpu_ms".to_string(), option_u64(native.cpu_limit_ms)),
        (
            "native.cpu_source".to_string(),
            native.cpu_limit_source.clone(),
        ),
        (
            "native.memory_bytes".to_string(),
            option_u64(native.memory_limit_bytes),
        ),
        (
            "native.memory_source".to_string(),
            native.memory_limit_source.clone(),
        ),
        (
            "native.output_bytes".to_string(),
            native.output_limit_bytes.to_string(),
        ),
        (
            "native.output_source".to_string(),
            native.output_limit_source.clone(),
        ),
        (
            "native.processes".to_string(),
            option_u32(native.process_limit),
        ),
        (
            "native.process_source".to_string(),
            native.process_limit_source.clone(),
        ),
        (
            "native.io_bytes".to_string(),
            option_u64(native.io_limit_bytes),
        ),
        (
            "native.io_source".to_string(),
            native.io_limit_source.clone(),
        ),
        (
            "pipeline.max_stages".to_string(),
            option_usize(pipeline_limits.max_stages),
        ),
        (
            "pipeline.max_wall_ms".to_string(),
            option_u128(pipeline_limits.max_wall_ms),
        ),
        (
            "pipeline.max_cpu_ms".to_string(),
            option_u128(pipeline_limits.max_cpu_ms),
        ),
        (
            "pipeline.max_memory_bytes".to_string(),
            option_u64(pipeline_limits.max_memory_bytes),
        ),
        (
            "pipeline.max_owned_bytes".to_string(),
            option_usize(pipeline_limits.max_owned_bytes),
        ),
        (
            "pipeline.max_fs_bytes".to_string(),
            option_u64(pipeline_limits.max_fs_bytes),
        ),
        (
            "pipeline.max_net_bytes".to_string(),
            option_u64(pipeline_limits.max_net_bytes),
        ),
    ]);

    let output = capsule.outputs.first()?;
    let mut output_fields = vec![
        ("target".to_string(), target.to_string()),
        ("share_type".to_string(), target_ty.to_agent()),
        ("output_name".to_string(), output.name.clone()),
        ("output_type".to_string(), output.ty.to_agent()),
    ];
    for (index, (source, input)) in capsule
        .bindings
        .iter()
        .filter(|binding| binding.mode.is_source())
        .zip(&capsule.inputs)
        .filter(|(binding, _)| binding.mode == ShareMode::Update)
        .enumerate()
    {
        output_fields.push((format!("update.{index}.source"), source.name.clone()));
        output_fields.push((format!("update.{index}.type"), input.ty.to_agent()));
    }
    let output_contract_sha256 = identity_fields_sha256(&output_fields);
    let key_sha256 = compose_cache_identity_key(
        &input_sha256,
        &code_sha256,
        &policy_sha256,
        &limits_sha256,
        &output_contract_sha256,
    );

    Some(PipelineCacheIdentityReceipt {
        schema: "forgentex.pipeline_cache_identity.v1".to_string(),
        key_sha256,
        input_sha256,
        code_sha256,
        policy_sha256,
        limits_sha256,
        output_contract_sha256,
        toolchain: native.toolchain.clone(),
        reuse: "not_attempted_identity_only".to_string(),
    })
}

fn stage_io_receipt(call: Option<&CapsuleCall>) -> Option<PipelineStageIoReceipt> {
    let native = call?.native.as_ref()?;
    let read_transfer_bytes = native.job_read_transfer_bytes?;
    let write_transfer_bytes = native.job_write_transfer_bytes?;
    let other_transfer_bytes = native.job_other_transfer_bytes?;
    Some(PipelineStageIoReceipt {
        schema: "forgentex.pipeline_stage_io.v1".to_string(),
        read_transfer_bytes,
        write_transfer_bytes,
        other_transfer_bytes,
        total_transfer_bytes: read_transfer_bytes
            .saturating_add(write_transfer_bytes)
            .saturating_add(other_transfer_bytes),
        source: "windows_job_object_accounting".to_string(),
    })
}

fn compose_cache_identity_key(
    input_sha256: &str,
    code_sha256: &str,
    policy_sha256: &str,
    limits_sha256: &str,
    output_contract_sha256: &str,
) -> String {
    identity_fields_sha256(&[
        (
            "schema".to_string(),
            "forgentex.pipeline_cache_identity.v1".to_string(),
        ),
        ("input".to_string(), input_sha256.to_string()),
        ("code".to_string(), code_sha256.to_string()),
        ("policy".to_string(), policy_sha256.to_string()),
        ("limits".to_string(), limits_sha256.to_string()),
        (
            "output_contract".to_string(),
            output_contract_sha256.to_string(),
        ),
    ])
}

fn identity_fields_sha256(fields: &[(String, String)]) -> String {
    let mut canonical = Vec::new();
    for (name, value) in fields {
        canonical.extend_from_slice(&(name.len() as u64).to_le_bytes());
        canonical.extend_from_slice(name.as_bytes());
        canonical.extend_from_slice(&(value.len() as u64).to_le_bytes());
        canonical.extend_from_slice(value.as_bytes());
    }
    crate::crypto::sha256_hex(&canonical)
}

#[allow(clippy::too_many_arguments)]
fn cache_request_sha256(
    program: &Program,
    capsule: &Capsule,
    sources: &[String],
    input_values_json: &[String],
    target: &str,
    limits: &PipelineLimits,
    allow: &Option<Vec<String>>,
    sensitive_input: bool,
) -> Option<String> {
    let artifact_policy = crate::capsule::artifact_policy(capsule).ok()?;
    if sensitive_input
        || !capsule
            .bindings
            .iter()
            .all(|binding| !binding.mode.is_source() || binding.mode == ShareMode::Read)
        || !capsule
            .effects
            .iter()
            .all(|effect| matches!(effect.as_str(), "alloc" | "cpu"))
        || !crate::sandbox::analyze_capsule(capsule)
            .redacted_inputs
            .is_empty()
        || limits.max_fs_bytes.is_some()
        || limits.max_net_bytes.is_some()
        || artifact_policy
            .as_ref()
            .is_some_and(|policy| policy.path.is_some())
        || crate::capsule::render_policy(capsule)
            .ok()
            .flatten()
            .is_some()
        || crate::capsule::encode_policy(capsule)
            .ok()
            .flatten()
            .is_some()
    {
        return None;
    }
    if allow.as_ref().is_some_and(|allowed| {
        capsule
            .effects
            .iter()
            .any(|effect| !allowed.contains(effect))
    }) {
        return None;
    }
    let toolchain = crate::ffi::live_native_toolchain_identity(capsule).ok()?;
    let mut allowed = allow.clone().unwrap_or_default();
    allowed.sort();
    allowed.dedup();
    let mut fields = vec![
        (
            "program".to_string(),
            crate::crypto::sha256_hex(program.to_canonical_json().as_bytes()),
        ),
        ("capsule".to_string(), capsule.name.clone()),
        ("toolchain".to_string(), toolchain),
        ("target".to_string(), target.to_string()),
        ("allow".to_string(), allowed.join("\n")),
        ("max_stages".to_string(), option_usize(limits.max_stages)),
        ("max_wall_ms".to_string(), option_u128(limits.max_wall_ms)),
        ("max_cpu_ms".to_string(), option_u128(limits.max_cpu_ms)),
        (
            "max_memory_bytes".to_string(),
            option_u64(limits.max_memory_bytes),
        ),
        (
            "max_owned_bytes".to_string(),
            option_usize(limits.max_owned_bytes),
        ),
    ];
    for (index, (source, value)) in sources.iter().zip(input_values_json).enumerate() {
        fields.push((format!("input.{index}.source"), source.clone()));
        fields.push((format!("input.{index}.value"), value.clone()));
    }
    Some(identity_fields_sha256(&fields))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validated_cache_entry<'a>(
    cache: &'a PipelineCache,
    request_sha256: &str,
    program: &Program,
    capsule: &Capsule,
    target: &str,
) -> Option<&'a PipelineCacheEntry> {
    let entry = cache.entries.get(request_sha256)?;
    let identity = &entry.identity;
    if ![
        &identity.key_sha256,
        &identity.input_sha256,
        &identity.code_sha256,
        &identity.policy_sha256,
        &identity.limits_sha256,
        &identity.output_contract_sha256,
    ]
    .into_iter()
    .all(|hash| valid_sha256(hash))
        || !value_matches_type(program, &entry.output, share_type(program, target)?)
        || restored_cache_artifact(capsule, &entry.output).is_none()
    {
        return None;
    }
    let output_json = value_to_json(&entry.output);
    (output_json.len() == entry.output_bytes
        && crate::crypto::sha256_hex(output_json.as_bytes()) == entry.output_sha256)
        .then_some(entry)
}

fn restored_cache_artifact(
    capsule: &Capsule,
    output: &Value,
) -> Option<Option<crate::ffi::NativeArtifactReceipt>> {
    let Some(policy) = crate::capsule::artifact_policy(capsule).ok()? else {
        return Some(None);
    };
    if policy.path.is_some() {
        return None;
    }
    let Value::Array(values) = output else {
        return None;
    };
    if values.len() > policy.max_bytes {
        return None;
    }
    let bytes = values
        .iter()
        .map(|value| match value {
            Value::Int(byte) => u8::try_from(*byte).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Some(crate::ffi::NativeArtifactReceipt {
        schema: "forgentex.native_artifact.v1".to_string(),
        media_type: policy.media_type,
        bytes: bytes.len(),
        sha256: crate::crypto::sha256_hex(&bytes),
        path: None,
    }))
}

fn share_reference(name: &str) -> (&str, Option<u64>) {
    crate::ast::split_share_reference(name)
        .expect("shared binding references are validated before pipeline execution")
}

fn explicit_share_reference(name: &str, version: u64) -> String {
    format!("{}@{version}", share_reference(name).0)
}

fn shared_value_key(shared: &BTreeMap<String, Value>, reference: &str) -> Option<String> {
    if shared.contains_key(reference) {
        return Some(reference.to_string());
    }
    let (base, version) = share_reference(reference);
    (version == Some(0) && shared.contains_key(base)).then(|| base.to_string())
}

fn shared_value<'a>(shared: &'a BTreeMap<String, Value>, reference: &str) -> Option<&'a Value> {
    shared_value_key(shared, reference).and_then(|key| shared.get(&key))
}

fn share_type<'a>(program: &'a Program, name: &str) -> Option<&'a Type> {
    let (base, _) = crate::ast::split_share_reference(name).ok()?;
    program
        .shares()
        .find(|share| share.name == base)
        .map(|share| &share.ty)
}

fn is_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Bool)
        || matches!(ty, Type::Primitive(name) if crate::ast::is_integer_primitive(name) || crate::ast::is_float_primitive(name) || name == "str")
        || matches!(ty, Type::Named(name) if name == "str")
}

fn is_pipeline_value(program: &Program, ty: &Type) -> bool {
    if is_scalar(ty) || reviewed_owned_element(ty).is_some() {
        return true;
    }
    let Type::Named(name) = ty else {
        return false;
    };
    let Some(decl) = program.structs().find(|decl| decl.name == *name) else {
        return false;
    };
    let repr_c = decl
        .attrs
        .iter()
        .any(|attr| attr.name == "repr" && attr.args.iter().any(|arg| arg == "C"));
    repr_c
        && !decl.fields.is_empty()
        && !decl.attrs.iter().any(|attr| {
            attr.name == "packed"
                || attr.name == "aligned"
                || (attr.name == "repr" && attr.args.iter().any(|arg| arg == "packed"))
        })
        && decl.fields.iter().all(|field| {
            field.attrs.is_empty()
                && matches!(&field.ty, Type::Primitive(name) if crate::ast::is_integer_primitive(name) || crate::ast::is_float_primitive(name))
        })
}

fn capsule_value_to_value(value: &CapsuleValue) -> Value {
    match value {
        CapsuleValue::Int(value) => Value::Int(*value),
        CapsuleValue::UInt(value) => Value::UInt(*value),
        CapsuleValue::Float(value) => Value::Float(*value),
        CapsuleValue::Bool(value) => Value::Bool(*value),
        CapsuleValue::Str(value) => Value::String(value.clone()),
    }
}

fn value_matches_type(program: &Program, value: &Value, ty: &Type) -> bool {
    match (value, ty) {
        (Value::Bool(_), Type::Bool) => true,
        (Value::Int(_), Type::Primitive(name)) => crate::ast::is_integer_primitive(name),
        (Value::UInt(_), Type::Primitive(name)) => name == "u64" || name == "usize",
        (Value::Float(value), Type::Primitive(name)) => {
            crate::ast::is_float_primitive(name) && value.is_finite()
        }
        (Value::String(_), Type::Primitive(name) | Type::Named(name)) => name == "str",
        (Value::Array(values), ty) => reviewed_owned_element(ty).is_some_and(|(element, _)| {
            values.iter().all(|value| match (element, value) {
                ("u8", Value::Int(value)) => u8::try_from(*value).is_ok(),
                ("i8", Value::Int(value)) => i8::try_from(*value).is_ok(),
                ("u16", Value::Int(value)) => u16::try_from(*value).is_ok(),
                ("i16", Value::Int(value)) => i16::try_from(*value).is_ok(),
                ("u32", Value::Int(value)) => u32::try_from(*value).is_ok(),
                ("i32", Value::Int(value)) => i32::try_from(*value).is_ok(),
                ("u64", Value::Int(value)) => u64::try_from(*value).is_ok(),
                ("u64", Value::UInt(_)) => true,
                ("i64", Value::Int(_)) => true,
                ("isize", Value::Int(value)) => isize::try_from(*value).is_ok(),
                ("f32", Value::Float(value)) => finite_exact_f32(*value),
                ("f64", Value::Float(value)) => value.is_finite(),
                _ => false,
            })
        }),
        (Value::Struct(name, fields), Type::Named(struct_name)) if name == struct_name => program
            .structs()
            .find(|decl| decl.name == *struct_name)
            .map(|decl| {
                fields.len() == decl.fields.len()
                    && decl.fields.iter().all(|field| {
                        fields
                            .get(&field.name)
                            .map(|field_value| value_matches_type(program, field_value, &field.ty))
                            .unwrap_or(false)
                    })
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn render_frame_cache_key(
    capsule: &Capsule,
    context: &RenderContext,
    native: &crate::ffi::NativeReceipt,
    artifact: &crate::ffi::NativeArtifactReceipt,
) -> Option<String> {
    let sequence_id = context.sequence_id.as_ref()?;
    Some(identity_fields_sha256(&[
        (
            "schema".to_string(),
            "forgentex.render_frame_cache_key.v1".to_string(),
        ),
        ("sequence_id".to_string(), sequence_id.clone()),
        ("renderer".to_string(), capsule.name.clone()),
        ("profile".to_string(), context.profile.clone()),
        (
            "color_space".to_string(),
            context.color_space.as_deref().unwrap_or("none").to_string(),
        ),
        (
            "device".to_string(),
            context.device.as_deref().unwrap_or("none").to_string(),
        ),
        (
            "render_pass".to_string(),
            context.render_pass.as_deref().unwrap_or("none").to_string(),
        ),
        (
            "engine".to_string(),
            context.engine.as_deref().unwrap_or("none").to_string(),
        ),
        (
            "engine_version".to_string(),
            context
                .engine_version
                .as_deref()
                .unwrap_or("none")
                .to_string(),
        ),
        ("scene_type".to_string(), context.scene_type.clone()),
        ("scene_sha256".to_string(), context.scene_sha256.clone()),
        ("seed".to_string(), context.seed.to_string()),
        ("width".to_string(), context.width.to_string()),
        ("height".to_string(), context.height.to_string()),
        ("frame_index".to_string(), context.frame_index.to_string()),
        ("frame_count".to_string(), context.frame_count.to_string()),
        (
            "frames_per_second".to_string(),
            option_u32(context.frames_per_second),
        ),
        (
            "samples_per_pixel".to_string(),
            option_u32(context.samples_per_pixel),
        ),
        ("shutter_q16".to_string(), option_u32(context.shutter_q16)),
        (
            "adaptive_min_samples".to_string(),
            option_u32(context.adaptive_min_samples),
        ),
        (
            "adaptive_noise_q16".to_string(),
            option_u32(context.adaptive_noise_q16),
        ),
        ("effects".to_string(), capsule.effects.join(",")),
        ("boundary_hash".to_string(), native.boundary_hash.clone()),
        ("toolchain".to_string(), native.toolchain.clone()),
        (
            "runner_artifact_sha256".to_string(),
            native.runner_artifact_sha256.clone(),
        ),
        (
            "artifact_media_type".to_string(),
            artifact.media_type.clone(),
        ),
        (
            "artifact_path".to_string(),
            artifact.path.as_deref().unwrap_or("none").to_string(),
        ),
    ]))
}

#[allow(clippy::too_many_arguments)]
fn stage_event(
    capsule: &Capsule,
    sources: Vec<String>,
    move_sources: Vec<String>,
    update_sources: Vec<String>,
    target: String,
    call: Option<&CapsuleCall>,
    input: String,
    output: Option<String>,
    error: Option<String>,
    render_context: Option<&RenderContext>,
    encode_context: Option<&EncodeContext>,
) -> PipelineStage {
    let source = sources.join(",");
    let redacted_inputs = call
        .map(|item| item.redacted_inputs.clone())
        .unwrap_or_default();
    let input = if redacted_inputs.is_empty() {
        input
    } else {
        "{\"redacted\":true}".to_string()
    };
    let error = if redacted_inputs.is_empty() {
        error
    } else {
        error.map(|_| {
            call.and_then(|item| item.native_failure.as_ref())
                .map(|receipt| format!("{}: diagnostic redacted by input policy", receipt.kind))
                .unwrap_or_else(|| "capsule diagnostic redacted by input policy".to_string())
        })
    };
    let render = render_context.and_then(|context| {
        let native = call.and_then(|item| item.native.as_ref())?;
        let artifact = native.artifact.clone()?;
        let frame_cache_key_sha256 = render_frame_cache_key(capsule, context, native, &artifact);
        Some(RenderReceipt {
            schema: "forgentex.render_receipt.v1".to_string(),
            profile: context.profile.clone(),
            color_space: context.color_space.clone(),
            device: context.device.clone(),
            render_pass: context.render_pass.clone(),
            engine: context.engine.clone(),
            engine_version: context.engine_version.clone(),
            sequence_id: context.sequence_id.clone(),
            frame_cache_key_sha256,
            renderer: capsule.name.clone(),
            scene_type: context.scene_type.clone(),
            scene_sha256: context.scene_sha256.clone(),
            seed: context.seed,
            width: context.width,
            height: context.height,
            frame_index: context.frame_index,
            frame_count: context.frame_count,
            frames_per_second: context.frames_per_second,
            samples_per_pixel: context.samples_per_pixel,
            shutter_q16: context.shutter_q16,
            adaptive_min_samples: context.adaptive_min_samples,
            adaptive_noise_q16: context.adaptive_noise_q16,
            effects: capsule.effects.clone(),
            boundary_hash: native.boundary_hash.clone(),
            toolchain: native.toolchain.clone(),
            renderer_binary_sha256: native.runner_artifact_sha256.clone(),
            wall_limit_ms: native.wall_limit_ms,
            cpu_limit_ms: native.cpu_limit_ms,
            memory_limit_bytes: native.memory_limit_bytes,
            artifact,
        })
    });
    let encode = encode_context.and_then(|context| {
        let native = call.and_then(|item| item.native.as_ref())?;
        let artifact = native.artifact.clone()?;
        Some(EncodeReceipt {
            schema: "forgentex.encode_receipt.v1".to_string(),
            profile: context.profile.clone(),
            color_space: context.color_space.clone(),
            input_pass: context.input_pass.clone(),
            engine: context.engine.clone(),
            engine_version: context.engine_version.clone(),
            compression: context.compression.clone(),
            compression_level: context.compression_level,
            pixel_format: context.pixel_format.clone(),
            interlace: context.interlace.clone(),
            scanline_filter: context.scanline_filter.clone(),
            chunk_checksum: context.chunk_checksum.clone(),
            stream_checksum: context.stream_checksum.clone(),
            chunk_order: context.chunk_order.clone(),
            encoder: capsule.name.clone(),
            input_sha256: context.input_sha256.clone(),
            input_bytes: context.input_bytes,
            input_width: context.input_width,
            input_height: context.input_height,
            input_format: context.input_format.clone(),
            effects: capsule.effects.clone(),
            boundary_hash: native.boundary_hash.clone(),
            toolchain: native.toolchain.clone(),
            encoder_binary_sha256: native.runner_artifact_sha256.clone(),
            wall_limit_ms: native.wall_limit_ms,
            cpu_limit_ms: native.cpu_limit_ms,
            memory_limit_bytes: native.memory_limit_bytes,
            artifact,
        })
    });
    PipelineStage {
        capsule: capsule.name.clone(),
        sources,
        source,
        move_sources,
        update_sources,
        handoff_committed: false,
        target,
        decision: call
            .map(|item| item.decision.clone())
            .unwrap_or_else(|| "error".to_string()),
        input,
        redacted_inputs,
        output,
        error,
        native: call.and_then(|item| item.native.as_ref()).is_some(),
        duration_ms: call.and_then(|item| {
            item.native
                .as_ref()
                .map(|receipt| receipt.duration_ms)
                .or_else(|| {
                    item.native_failure
                        .as_ref()
                        .map(|receipt| receipt.duration_ms)
                })
        }),
        cache_identity: None,
        cache_restore: None,
        io: None,
        buffer: None,
        updates: Vec::new(),
        artifact: call
            .and_then(|item| item.native.as_ref())
            .and_then(|receipt| receipt.artifact.clone()),
        render,
        encode,
    }
}

struct ReportContext<'a> {
    program: &'a Program,
    limits: &'a PipelineLimits,
    schedule: PipelineSchedule,
    control: &'a PipelineControl,
    cancellation_reason: Cell<Option<&'static str>>,
}

fn report(
    file: &str,
    status: &str,
    diagnostics: Vec<String>,
    stages: Vec<PipelineStage>,
    shared: BTreeMap<String, Value>,
    context: &ReportContext<'_>,
    graph: Option<&PipelineGraphReceipt>,
) -> PipelineReport {
    let program = context.program;
    let redacted_shares = program
        .capsules()
        .filter(|capsule| {
            !crate::sandbox::analyze_capsule(capsule)
                .redacted_inputs
                .is_empty()
        })
        .flat_map(|capsule| {
            capsule
                .bindings
                .iter()
                .filter(|binding| binding.mode.is_source())
                .map(|binding| share_reference(&binding.name).0.to_string())
        })
        .collect::<BTreeSet<_>>();
    let shared = shared
        .into_iter()
        .filter_map(|(name, value)| {
            let (base_name, explicit_version) = share_reference(&name);
            let base_name = base_name.to_string();
            share_type(program, &name).map(|ty| PipelineValue {
                value: if redacted_shares.contains(&base_name) {
                    "{\"redacted\":true}".to_string()
                } else {
                    value_to_json(&value)
                },
                version: explicit_version.unwrap_or(0),
                name,
                ty: ty.to_agent(),
            })
        })
        .collect();
    let budget = pipeline_budget_receipt(program, &stages, context.limits, status);
    let cancellation = (context.control.cancellation.is_some()
        || context.control.stop_after_stages.is_some())
    .then(|| PipelineCancellationReceipt {
        schema: "forgentex.pipeline_cancellation.v1".to_string(),
        source: context
            .cancellation_reason
            .get()
            .unwrap_or(if context.control.cancellation.is_some() {
                "external_token"
            } else {
                "stage_limit"
            })
            .to_string(),
        decision: if status == "cancelled" {
            "cancelled"
        } else {
            "completed"
        }
        .to_string(),
        committed_stages: stages
            .iter()
            .filter(|stage| stage.output.is_some() && stage.error.is_none())
            .count(),
        after_stage: stages.last().map(|stage| stage.capsule.clone()),
    });
    PipelineReport {
        schema: PIPELINE_RUN_SCHEMA.to_string(),
        file: file.to_string(),
        mode: match context.schedule {
            PipelineSchedule::Declaration => "deterministic_single_threaded",
            PipelineSchedule::DependencyGraph => "deterministic_dependency_graph",
            PipelineSchedule::SourceGraph => "deterministic_source_graph",
        }
        .to_string(),
        status: status.to_string(),
        diagnostics,
        stages,
        shared,
        budget,
        graph: graph.cloned(),
        cancellation,
    }
}

fn pipeline_cancellation_reason(
    control: &PipelineControl,
    completed_stages: usize,
) -> Option<&'static str> {
    if control
        .cancellation
        .as_ref()
        .is_some_and(PipelineCancellationToken::is_cancelled)
    {
        Some("external_token")
    } else if control
        .stop_after_stages
        .is_some_and(|limit| completed_stages >= limit)
    {
        Some("stage_limit")
    } else {
        None
    }
}

fn pipeline_budget_receipt(
    program: &Program,
    stages: &[PipelineStage],
    limits: &PipelineLimits,
    status: &str,
) -> PipelineBudgetReceipt {
    let planned = program
        .capsules()
        .filter(|capsule| !capsule.bindings.is_empty())
        .collect::<Vec<_>>();
    let declared_wall = planned
        .iter()
        .map(|capsule| capsule_wall_limit_ms(capsule))
        .collect::<Vec<_>>();
    let planned_wall_ms = declared_wall.iter().all(Option::is_some).then(|| {
        declared_wall
            .iter()
            .flatten()
            .copied()
            .fold(0u128, u128::saturating_add)
    });
    let declared_cpu = planned
        .iter()
        .map(|capsule| capsule_cpu_limit_ms(capsule))
        .collect::<Vec<_>>();
    let planned_cpu_ms = declared_cpu.iter().all(Option::is_some).then(|| {
        declared_cpu
            .iter()
            .flatten()
            .copied()
            .fold(0u128, u128::saturating_add)
    });
    let declared_memory = planned
        .iter()
        .map(|capsule| capsule_memory_limit_bytes(capsule))
        .collect::<Vec<_>>();
    let planned_peak_memory_bytes = declared_memory
        .iter()
        .all(Option::is_some)
        .then(|| declared_memory.iter().flatten().copied().max().unwrap_or(0));
    let requested = limits.max_stages.is_some()
        || limits.max_wall_ms.is_some()
        || limits.max_cpu_ms.is_some()
        || limits.max_memory_bytes.is_some()
        || limits.max_owned_bytes.is_some()
        || limits.max_fs_bytes.is_some()
        || limits.max_net_bytes.is_some();
    let decision = match status {
        "budget_exhausted" => "exhausted",
        "ok" if requested => "within_budget",
        "ok" => "not_requested",
        _ => "incomplete",
    };
    PipelineBudgetReceipt {
        schema: "forgentex.pipeline_budget.v1".to_string(),
        max_stages: limits.max_stages,
        max_wall_ms: limits.max_wall_ms,
        max_cpu_ms: limits.max_cpu_ms,
        max_memory_bytes: limits.max_memory_bytes,
        max_owned_bytes: limits.max_owned_bytes,
        max_fs_bytes: limits.max_fs_bytes,
        max_net_bytes: limits.max_net_bytes,
        planned_stages: planned.len(),
        planned_wall_ms,
        planned_cpu_ms,
        planned_peak_memory_bytes,
        used_stages: stages.len(),
        used_wall_ms: stages
            .iter()
            .filter_map(|stage| stage.duration_ms)
            .fold(0u128, u128::saturating_add),
        used_owned_bytes: stages
            .iter()
            .map(stage_owned_bytes)
            .fold(0usize, usize::saturating_add),
        used_fs_bytes: (!stages.is_empty() && stages.iter().all(|stage| stage.io.is_some())).then(
            || {
                stages
                    .iter()
                    .filter_map(|stage| stage.io.as_ref())
                    .map(|io| io.total_transfer_bytes)
                    .fold(0u64, u64::saturating_add)
            },
        ),
        used_net_bytes: None,
        decision: decision.to_string(),
    }
}

impl PipelineReport {
    pub fn is_success(&self) -> bool {
        self.status == "ok"
    }

    /// Recover one final reviewed byte-vector value for a package-level
    /// application publisher. Pipeline values are retained as canonical JSON
    /// in the public report, so this conversion is deliberately narrow.
    pub fn owned_u8_output(&self, name: &str) -> Result<Vec<u8>, String> {
        let value = self
            .shared
            .iter()
            .find(|value| value.name == name)
            .ok_or_else(|| format!("pipeline output share '{name}' was not produced"))?;
        if value.ty != "Vec[u8]" {
            return Err(format!(
                "pipeline output share '{name}' has type {}; expected Vec[u8]",
                value.ty
            ));
        }
        let body = value
            .value
            .trim()
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .ok_or_else(|| format!("pipeline output share '{name}' is not a JSON byte array"))?;
        if body.trim().is_empty() {
            return Ok(Vec::new());
        }
        body.split(',')
            .map(|item| {
                item.trim().parse::<u8>().map_err(|_| {
                    format!("pipeline output share '{name}' contains a non-byte value")
                })
            })
            .collect()
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "Forgentex deterministic pipeline\nschema={} file={} mode={} status={}\nbudget={} decision={} max_stages={} max_wall_ms={} max_cpu_ms={} max_memory_bytes={} max_owned_bytes={} max_fs_bytes={} max_net_bytes={} planned_stages={} planned_wall_ms={} planned_cpu_ms={} planned_peak_memory_bytes={} used_stages={} used_wall_ms={} used_owned_bytes={} used_fs_bytes={} used_net_bytes={}\n",
            self.schema,
            self.file,
            self.mode,
            self.status,
            self.budget.schema,
            self.budget.decision,
            option_usize(self.budget.max_stages),
            option_u128(self.budget.max_wall_ms),
            option_u128(self.budget.max_cpu_ms),
            option_u64(self.budget.max_memory_bytes),
            option_usize(self.budget.max_owned_bytes),
            option_u64(self.budget.max_fs_bytes),
            option_u64(self.budget.max_net_bytes),
            self.budget.planned_stages,
            option_u128(self.budget.planned_wall_ms),
            option_u128(self.budget.planned_cpu_ms),
            option_u64(self.budget.planned_peak_memory_bytes),
            self.budget.used_stages,
            self.budget.used_wall_ms,
            self.budget.used_owned_bytes,
            option_u64(self.budget.used_fs_bytes),
            option_u64(self.budget.used_net_bytes),
        );
        if let Some(graph) = &self.graph {
            out.push_str(&format!(
                "graph={} origin={} declaration={} nodes=[{}] roots=[{}] fan_out=[{}] order=[{}] edges={}\n",
                graph.schema,
                graph.origin,
                graph.declaration.as_deref().unwrap_or("-"),
                graph.nodes.join(","),
                graph.roots.join(","),
                graph.fan_out_shares.join(","),
                graph.execution_order.join(","),
                graph.edges.len(),
            ));
        }
        if let Some(cancellation) = &self.cancellation {
            out.push_str(&format!(
                "cancellation={} source={} decision={} committed_stages={} after_stage={}\n",
                cancellation.schema,
                cancellation.source,
                cancellation.decision,
                cancellation.committed_stages,
                cancellation.after_stage.as_deref().unwrap_or("null"),
            ));
        }
        for stage in &self.stages {
            out.push_str(&format!(
                "  {}: [{}] -> {} move=[{}] update=[{}] handoff_committed={} decision={} duration_ms={} input={} redacted=[{}] output={}{}{}\n",
                stage.capsule,
                stage.sources.join(","),
                stage.target,
                stage.move_sources.join(","),
                stage.update_sources.join(","),
                stage.handoff_committed,
                stage.decision,
                option_u128(stage.duration_ms),
                stage.input,
                stage.redacted_inputs.join(","),
                stage.output.as_deref().unwrap_or("null"),
                stage
                    .error
                    .as_ref()
                    .map(|error| format!(" error={error}"))
                    .unwrap_or_default(),
                stage
                    .buffer
                    .as_ref()
                    .map(|buffer| {
                        format!(
                            " buffer={} elements={} bytes={} sha256={}",
                            buffer.element_type, buffer.elements, buffer.bytes, buffer.sha256
                        )
                    })
                    .unwrap_or_default()
                    + &stage
                        .artifact
                        .as_ref()
                        .map(|artifact| format!(
                            " artifact={} bytes={} sha256={}",
                            artifact.media_type, artifact.bytes, artifact.sha256
                        ))
                        .unwrap_or_default()
                    + &stage
                        .io
                        .as_ref()
                        .map(|io| {
                            format!(
                                " io={} read={} write={} other={} total={}",
                                io.source,
                                io.read_transfer_bytes,
                                io.write_transfer_bytes,
                                io.other_transfer_bytes,
                                io.total_transfer_bytes
                            )
                        })
                        .unwrap_or_default()
                    + &stage
                        .render
                        .as_ref()
                        .map(|render| {
                            format!(
                                " render={} color={} device={} pass={} engine={}@{} binary_sha256={} sequence={} frame_cache_key={} {}x{} frame={}/{} fps={} samples={} shutter_q16={} adaptive_min_samples={} adaptive_noise_q16={} seed={} scene_sha256={}",
                                render.profile,
                                render.color_space.as_deref().unwrap_or("none"),
                                render.device.as_deref().unwrap_or("none"),
                                render.render_pass.as_deref().unwrap_or("none"),
                                render.engine.as_deref().unwrap_or("none"),
                                render.engine_version.as_deref().unwrap_or("none"),
                                render.renderer_binary_sha256,
                                render.sequence_id.as_deref().unwrap_or("none"),
                                render.frame_cache_key_sha256.as_deref().unwrap_or("none"),
                                render.width,
                                render.height,
                                render.frame_index,
                                render.frame_count,
                                option_u32(render.frames_per_second),
                                option_u32(render.samples_per_pixel),
                                option_u32(render.shutter_q16),
                                option_u32(render.adaptive_min_samples),
                                option_u32(render.adaptive_noise_q16),
                                render.seed,
                                render.scene_sha256
                            )
                        })
                        .unwrap_or_default()
                    + &stage
                        .encode
                        .as_ref()
                        .map(|encode| {
                            format!(
                                " encode={} color={} input_pass={} engine={}@{} compression={} level={} pixel_format={} interlace={} scanline_filter={} chunk_checksum={} stream_checksum={} chunk_order={} binary_sha256={} input={}x{}:{} input_bytes={} input_sha256={}",
                                encode.profile,
                                encode.color_space.as_deref().unwrap_or("none"),
                                encode.input_pass.as_deref().unwrap_or("none"),
                                encode.engine.as_deref().unwrap_or("none"),
                                encode.engine_version.as_deref().unwrap_or("none"),
                                encode.compression.as_deref().unwrap_or("none"),
                                option_u32(encode.compression_level),
                                encode.pixel_format.as_deref().unwrap_or("none"),
                                encode.interlace.as_deref().unwrap_or("none"),
                                encode.scanline_filter.as_deref().unwrap_or("none"),
                                encode.chunk_checksum.as_deref().unwrap_or("none"),
                                encode.stream_checksum.as_deref().unwrap_or("none"),
                                encode.chunk_order.as_deref().unwrap_or("none"),
                                encode.encoder_binary_sha256,
                                encode.input_width,
                                encode.input_height,
                                encode.input_format,
                                encode.input_bytes,
                                encode.input_sha256
                            )
                        })
                        .unwrap_or_default()
                    + &stage
                        .cache_identity
                        .as_ref()
                        .map(|cache| {
                            format!(
                                " cache_identity={} key={} reuse={}",
                                cache.schema, cache.key_sha256, cache.reuse
                            )
                        })
                        .unwrap_or_default()
                    + &stage
                        .cache_restore
                        .as_ref()
                        .map(|restore| {
                            format!(
                                " cache_restore={} request={} decision={}",
                                restore.schema, restore.request_sha256, restore.decision
                            )
                        })
                        .unwrap_or_default()
            ));
        }
        for value in &self.shared {
            out.push_str(&format!(
                "  shared {}: {} = {}\n",
                value.name, value.ty, value.value
            ));
        }
        if !self.diagnostics.is_empty() {
            out.push_str(&format!("  diagnostics=[{}]\n", self.diagnostics.join(",")));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let stages = self
            .stages
            .iter()
            .map(|stage| {
                let artifact = stage.artifact.as_ref().map(|artifact| format!(
                    "{{\"schema\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"path\":{}}}",
                    json::string(&artifact.schema), json::string(&artifact.media_type), artifact.bytes,
                    json::string(&artifact.sha256), json::opt_string(artifact.path.as_deref())
                )).unwrap_or_else(|| "null".to_string());
                let buffer = stage.buffer.as_ref().map(|buffer| format!(
                    "{{\"schema\":{},\"element_type\":{},\"elements\":{},\"bytes\":{},\"encoding\":{},\"sha256\":{}}}",
                    json::string(&buffer.schema), json::string(&buffer.element_type), buffer.elements,
                    buffer.bytes, json::string(&buffer.encoding), json::string(&buffer.sha256)
                )).unwrap_or_else(|| "null".to_string());
                let render = stage.render.as_ref().map(|render| format!(
                    "{{\"schema\":{},\"profile\":{},\"color_space\":{},\"device\":{},\"render_pass\":{},\"engine\":{},\"engine_version\":{},\"sequence_id\":{},\"frame_cache_key_sha256\":{},\"renderer\":{},\"renderer_binary_sha256\":{},\"scene_type\":{},\"scene_sha256\":{},\"seed\":{},\"width\":{},\"height\":{},\"frame_index\":{},\"frame_count\":{},\"frames_per_second\":{},\"samples_per_pixel\":{},\"shutter_q16\":{},\"adaptive_min_samples\":{},\"adaptive_noise_q16\":{},\"effects\":{},\"boundary_hash\":{},\"toolchain\":{},\"wall_limit_ms\":{},\"cpu_limit_ms\":{},\"memory_limit_bytes\":{},\"artifact\":{{\"schema\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"path\":{}}}}}",
                    json::string(&render.schema), json::string(&render.profile), json::opt_string(render.color_space.as_deref()), json::opt_string(render.device.as_deref()), json::opt_string(render.render_pass.as_deref()), json::opt_string(render.engine.as_deref()), json::opt_string(render.engine_version.as_deref()), json::opt_string(render.sequence_id.as_deref()), json::opt_string(render.frame_cache_key_sha256.as_deref()), json::string(&render.renderer),
                    json::string(&render.renderer_binary_sha256), json::string(&render.scene_type), json::string(&render.scene_sha256), render.seed,
                    render.width, render.height, render.frame_index, render.frame_count,
                    render.frames_per_second.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.samples_per_pixel.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.shutter_q16.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.adaptive_min_samples.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.adaptive_noise_q16.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    json::string_array(&render.effects),
                    json::string(&render.boundary_hash), json::string(&render.toolchain),
                    render.wall_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    render.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    json::string(&render.artifact.schema), json::string(&render.artifact.media_type),
                    render.artifact.bytes, json::string(&render.artifact.sha256),
                    json::opt_string(render.artifact.path.as_deref())
                )).unwrap_or_else(|| "null".to_string());
                let encode = stage.encode.as_ref().map(|encode| format!(
                    "{{\"schema\":{},\"profile\":{},\"color_space\":{},\"input_pass\":{},\"engine\":{},\"engine_version\":{},\"compression\":{},\"compression_level\":{},\"pixel_format\":{},\"interlace\":{},\"scanline_filter\":{},\"chunk_checksum\":{},\"stream_checksum\":{},\"chunk_order\":{},\"encoder\":{},\"encoder_binary_sha256\":{},\"input_sha256\":{},\"input_bytes\":{},\"input_width\":{},\"input_height\":{},\"input_format\":{},\"effects\":{},\"boundary_hash\":{},\"toolchain\":{},\"wall_limit_ms\":{},\"cpu_limit_ms\":{},\"memory_limit_bytes\":{},\"artifact\":{{\"schema\":{},\"media_type\":{},\"bytes\":{},\"sha256\":{},\"path\":{}}}}}",
                    json::string(&encode.schema), json::string(&encode.profile), json::opt_string(encode.color_space.as_deref()), json::opt_string(encode.input_pass.as_deref()), json::opt_string(encode.engine.as_deref()), json::opt_string(encode.engine_version.as_deref()), json::opt_string(encode.compression.as_deref()), encode.compression_level.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()), json::opt_string(encode.pixel_format.as_deref()), json::opt_string(encode.interlace.as_deref()), json::opt_string(encode.scanline_filter.as_deref()), json::opt_string(encode.chunk_checksum.as_deref()), json::opt_string(encode.stream_checksum.as_deref()), json::opt_string(encode.chunk_order.as_deref()), json::string(&encode.encoder),
                    json::string(&encode.encoder_binary_sha256), json::string(&encode.input_sha256), encode.input_bytes, encode.input_width,
                    encode.input_height, json::string(&encode.input_format), json::string_array(&encode.effects),
                    json::string(&encode.boundary_hash), json::string(&encode.toolchain),
                    encode.wall_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    encode.cpu_limit_ms.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    encode.memory_limit_bytes.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
                    json::string(&encode.artifact.schema), json::string(&encode.artifact.media_type),
                    encode.artifact.bytes, json::string(&encode.artifact.sha256),
                    json::opt_string(encode.artifact.path.as_deref())
                )).unwrap_or_else(|| "null".to_string());
                let cache_identity = stage.cache_identity.as_ref().map(|cache| format!(
                    "{{\"schema\":{},\"key_sha256\":{},\"input_sha256\":{},\"code_sha256\":{},\"policy_sha256\":{},\"limits_sha256\":{},\"output_contract_sha256\":{},\"toolchain\":{},\"reuse\":{}}}",
                    json::string(&cache.schema), json::string(&cache.key_sha256),
                    json::string(&cache.input_sha256), json::string(&cache.code_sha256),
                    json::string(&cache.policy_sha256), json::string(&cache.limits_sha256),
                    json::string(&cache.output_contract_sha256), json::string(&cache.toolchain),
                    json::string(&cache.reuse)
                )).unwrap_or_else(|| "null".to_string());
                let cache_restore = stage.cache_restore.as_ref().map(|restore| format!(
                    "{{\"schema\":{},\"request_sha256\":{},\"output_sha256\":{},\"output_bytes\":{},\"storage\":{},\"decision\":{}}}",
                    json::string(&restore.schema), json::string(&restore.request_sha256),
                    json::string(&restore.output_sha256), restore.output_bytes,
                    json::string(&restore.storage), json::string(&restore.decision)
                )).unwrap_or_else(|| "null".to_string());
                let io = stage.io.as_ref().map(|io| format!(
                    "{{\"schema\":{},\"read_transfer_bytes\":{},\"write_transfer_bytes\":{},\"other_transfer_bytes\":{},\"total_transfer_bytes\":{},\"source\":{}}}",
                    json::string(&io.schema), io.read_transfer_bytes, io.write_transfer_bytes,
                    io.other_transfer_bytes, io.total_transfer_bytes, json::string(&io.source)
                )).unwrap_or_else(|| "null".to_string());
                let updates = stage.updates.iter().map(|update| format!(
                    "{{\"source\":{},\"source_version\":{},\"target_version\":{},\"type\":{},\"elements\":{},\"bytes\":{},\"encoding\":{},\"before_sha256\":{},\"after_sha256\":{}}}",
                    json::string(&update.source), update.source_version, update.target_version,
                    json::string(&update.ty), update.elements,
                    update.bytes, json::string(&update.encoding), json::string(&update.before_sha256),
                    json::string(&update.after_sha256)
                )).collect::<Vec<_>>().join(",");
                format!(
                "{{\"capsule\":{},\"source\":{},\"sources\":{},\"move_sources\":{},\"update_sources\":{},\"handoff_committed\":{},\"target\":{},\"decision\":{},\"duration_ms\":{},\"input\":{},\"redacted_inputs\":{},\"output\":{},\"error\":{},\"native\":{},\"cache_identity\":{},\"cache_restore\":{},\"io\":{},\"buffer\":{},\"updates\":[{}],\"artifact\":{},\"render\":{},\"encode\":{}}}",
                json::string(&stage.capsule), json::string(&stage.source), json::string_array(&stage.sources), json::string_array(&stage.move_sources), json::string_array(&stage.update_sources), stage.handoff_committed, json::string(&stage.target),
                json::string(&stage.decision), option_u128(stage.duration_ms), stage.input, json::string_array(&stage.redacted_inputs), stage.output.as_deref().unwrap_or("null"),
                json::opt_string(stage.error.as_deref()), stage.native, cache_identity, cache_restore, io, buffer, updates, artifact, render, encode
            )})
            .collect::<Vec<_>>()
            .join(",");
        let shared = self
            .shared
            .iter()
            .map(|value| {
                format!(
                    "{{\"name\":{},\"type\":{},\"value\":{},\"version\":{}}}",
                    json::string(&value.name),
                    json::string(&value.ty),
                    value.value,
                    value.version
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let budget = format!(
            "{{\"schema\":{},\"max_stages\":{},\"max_wall_ms\":{},\"max_cpu_ms\":{},\"max_memory_bytes\":{},\"max_owned_bytes\":{},\"max_fs_bytes\":{},\"max_net_bytes\":{},\"planned_stages\":{},\"planned_wall_ms\":{},\"planned_cpu_ms\":{},\"planned_peak_memory_bytes\":{},\"used_stages\":{},\"used_wall_ms\":{},\"used_owned_bytes\":{},\"used_fs_bytes\":{},\"used_net_bytes\":{},\"decision\":{}}}",
            json::string(&self.budget.schema),
            option_usize(self.budget.max_stages),
            option_u128(self.budget.max_wall_ms),
            option_u128(self.budget.max_cpu_ms),
            option_u64(self.budget.max_memory_bytes),
            option_usize(self.budget.max_owned_bytes),
            option_u64(self.budget.max_fs_bytes),
            option_u64(self.budget.max_net_bytes),
            self.budget.planned_stages,
            option_u128(self.budget.planned_wall_ms),
            option_u128(self.budget.planned_cpu_ms),
            option_u64(self.budget.planned_peak_memory_bytes),
            self.budget.used_stages,
            self.budget.used_wall_ms,
            self.budget.used_owned_bytes,
            option_u64(self.budget.used_fs_bytes),
            option_u64(self.budget.used_net_bytes),
            json::string(&self.budget.decision),
        );
        let graph = self
            .graph
            .as_ref()
            .map(|graph| {
                let edges = graph
                    .edges
                    .iter()
                    .map(|edge| {
                        format!(
                            "{{\"from\":{},\"to\":{},\"share\":{}}}",
                            json::string(&edge.from),
                            json::string(&edge.to),
                            json::string(&edge.share),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"schema\":{},\"origin\":{},\"declaration\":{},\"nodes\":{},\"edges\":[{}],\"roots\":{},\"fan_out_shares\":{},\"execution_order\":{}}}",
                    json::string(&graph.schema),
                    json::string(&graph.origin),
                    json::opt_string(graph.declaration.as_deref()),
                    json::string_array(&graph.nodes),
                    edges,
                    json::string_array(&graph.roots),
                    json::string_array(&graph.fan_out_shares),
                    json::string_array(&graph.execution_order),
                )
            })
            .unwrap_or_else(|| "null".to_string());
        let cancellation = self
            .cancellation
            .as_ref()
            .map(|cancellation| {
                format!(
                    "{{\"schema\":{},\"source\":{},\"decision\":{},\"committed_stages\":{},\"after_stage\":{}}}",
                    json::string(&cancellation.schema),
                    json::string(&cancellation.source),
                    json::string(&cancellation.decision),
                    cancellation.committed_stages,
                    json::opt_string(cancellation.after_stage.as_deref()),
                )
            })
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"schema\":{},\"file\":{},\"mode\":{},\"status\":{},\"diagnostics\":{},\"budget\":{},\"graph\":{},\"cancellation\":{},\"stages\":[{}],\"shared\":[{}]}}",
            json::string(&self.schema), json::string(&self.file), json::string(&self.mode), json::string(&self.status),
            json::string_array(&self.diagnostics), budget, graph, cancellation, stages, shared
        )
    }
}

fn option_u128(value: Option<u128>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn option_usize(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn option_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn option_u32(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

#[cfg(test)]
mod reviewed_buffer_tests {
    use super::{
        buffer_receipt, compose_cache_identity_key, pipeline_input_compatible, value_matches_type,
    };
    use crate::ast::{Program, Type};
    use crate::interp::Value;

    fn vector_type(element: &str) -> Type {
        Type::Generic {
            name: "Vec".to_string(),
            args: vec![Type::Primitive(element.to_string())],
        }
    }

    fn immutable_slice(element: &str) -> Type {
        Type::Slice {
            mutable: false,
            elem: Box::new(Type::Primitive(element.to_string())),
        }
    }

    #[test]
    fn every_reviewed_integer_layout_has_canonical_pipeline_receipts() {
        let cases: Vec<(&str, Vec<i64>, Vec<u8>)> = vec![
            ("u8", vec![0, 255], vec![0, 255]),
            ("i8", vec![-128, 127], vec![128, 127]),
            ("u16", vec![0, 65_535], vec![0, 0, 255, 255]),
            ("i16", vec![-32_768, 32_767], vec![0, 128, 255, 127]),
            (
                "u32",
                vec![0, 4_000_000_000],
                [0_u32.to_le_bytes(), 4_000_000_000_u32.to_le_bytes()].concat(),
            ),
            (
                "i32",
                vec![-2_000_000_000, 2_000_000_000],
                [
                    (-2_000_000_000_i32).to_le_bytes(),
                    2_000_000_000_i32.to_le_bytes(),
                ]
                .concat(),
            ),
            (
                "u64",
                vec![0, i64::MAX],
                [0_u64.to_le_bytes(), (i64::MAX as u64).to_le_bytes()].concat(),
            ),
            (
                "i64",
                vec![-5_000_000_000, 5_000_000_000],
                [
                    (-5_000_000_000_i64).to_le_bytes(),
                    5_000_000_000_i64.to_le_bytes(),
                ]
                .concat(),
            ),
            (
                "isize",
                vec![-2_000_000_000, 2_000_000_000],
                [
                    (-2_000_000_000_isize).to_le_bytes().as_slice(),
                    2_000_000_000_isize.to_le_bytes().as_slice(),
                ]
                .concat(),
            ),
        ];

        for (element, values, canonical) in cases {
            let ty = vector_type(element);
            let value = Value::Array(values.into_iter().map(Value::Int).collect());
            let receipt = buffer_receipt(&ty, &value, None)
                .expect("reviewed buffer")
                .expect("buffer receipt");
            assert_eq!(receipt.element_type, element);
            assert_eq!(receipt.elements, 2);
            assert_eq!(receipt.bytes, canonical.len());
            assert_eq!(receipt.sha256, crate::crypto::sha256_hex(&canonical));
            assert!(pipeline_input_compatible(&ty, &immutable_slice(element)));
        }
    }

    #[test]
    fn full_range_u64_keeps_exact_pipeline_bytes_and_type() {
        let ty = vector_type("u64");
        let value = Value::Array(vec![Value::UInt(u64::MAX)]);
        let receipt = buffer_receipt(&ty, &value, None)
            .expect("reviewed buffer")
            .expect("buffer receipt");
        assert_eq!(receipt.elements, 1);
        assert_eq!(receipt.bytes, 8);
        assert_eq!(
            receipt.sha256,
            crate::crypto::sha256_hex(&u64::MAX.to_le_bytes())
        );
        assert!(value_matches_type(
            &Program { items: Vec::new() },
            &value,
            &ty
        ));
    }

    #[test]
    fn reviewed_pipeline_buffers_reject_mutable_views_and_out_of_range_values() {
        let u16_vector = vector_type("u16");
        let mutable_u16_slice = Type::Slice {
            mutable: true,
            elem: Box::new(Type::Primitive("u16".to_string())),
        };
        assert!(!pipeline_input_compatible(&u16_vector, &mutable_u16_slice));

        let out_of_range = Value::Array(vec![Value::Int(65_536)]);
        assert!(buffer_receipt(&u16_vector, &out_of_range, None).is_err());
        assert!(!value_matches_type(
            &Program { items: Vec::new() },
            &out_of_range,
            &u16_vector
        ));

        let u64_vector = vector_type("u64");
        let negative_unsigned = Value::Array(vec![Value::Int(-1)]);
        assert!(buffer_receipt(&u64_vector, &negative_unsigned, None).is_err());
        assert!(!value_matches_type(
            &Program { items: Vec::new() },
            &negative_unsigned,
            &u64_vector
        ));
    }

    #[test]
    fn reviewed_float_layouts_have_bit_exact_pipeline_receipts() {
        let cases = [
            (
                "f32",
                vec![f64::from(-0.0_f32), f64::from(0.1_f32)],
                [
                    (-0.0_f32).to_bits().to_le_bytes().as_slice(),
                    0.1_f32.to_bits().to_le_bytes().as_slice(),
                ]
                .concat(),
            ),
            (
                "f64",
                vec![-0.0_f64, std::f64::consts::PI],
                [
                    (-0.0_f64).to_bits().to_le_bytes().as_slice(),
                    std::f64::consts::PI.to_bits().to_le_bytes().as_slice(),
                ]
                .concat(),
            ),
        ];

        for (element, values, canonical) in cases {
            let ty = vector_type(element);
            let value = Value::Array(values.into_iter().map(Value::Float).collect());
            let receipt = buffer_receipt(&ty, &value, None)
                .expect("reviewed float buffer")
                .expect("buffer receipt");
            assert_eq!(receipt.element_type, element);
            assert_eq!(receipt.elements, 2);
            assert_eq!(receipt.bytes, canonical.len());
            assert_eq!(receipt.encoding, "little_endian_ieee754_v1");
            assert_eq!(receipt.sha256, crate::crypto::sha256_hex(&canonical));
            assert!(pipeline_input_compatible(&ty, &immutable_slice(element)));
        }
    }

    #[test]
    fn reviewed_float_buffers_reject_mutable_non_finite_and_width_invalid_values() {
        let f32_vector = vector_type("f32");
        let mutable_f32_slice = Type::Slice {
            mutable: true,
            elem: Box::new(Type::Primitive("f32".to_string())),
        };
        assert!(!pipeline_input_compatible(&f32_vector, &mutable_f32_slice));

        for invalid in [Value::Float(f64::NAN), Value::Float(f64::INFINITY)] {
            let value = Value::Array(vec![invalid]);
            assert!(buffer_receipt(&f32_vector, &value, None).is_err());
            assert!(!value_matches_type(
                &Program { items: Vec::new() },
                &value,
                &f32_vector
            ));
        }

        let not_exact_at_f32_width = Value::Array(vec![Value::Float(0.1_f64)]);
        assert!(buffer_receipt(&f32_vector, &not_exact_at_f32_width, None).is_err());
        assert!(!value_matches_type(
            &Program { items: Vec::new() },
            &not_exact_at_f32_width,
            &f32_vector
        ));
    }

    #[test]
    fn cache_identity_key_binds_every_component_digest() {
        let baseline = compose_cache_identity_key("input", "code", "policy", "limits", "output");
        assert_eq!(baseline.len(), 64);
        for changed in [
            compose_cache_identity_key("changed", "code", "policy", "limits", "output"),
            compose_cache_identity_key("input", "changed", "policy", "limits", "output"),
            compose_cache_identity_key("input", "code", "changed", "limits", "output"),
            compose_cache_identity_key("input", "code", "policy", "changed", "output"),
            compose_cache_identity_key("input", "code", "policy", "limits", "changed"),
        ] {
            assert_ne!(changed, baseline);
        }
    }
}
