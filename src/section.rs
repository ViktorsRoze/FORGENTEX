//! Carried sections: the project filesystem inside the file.
//!
//! A Forgentex file already holds foreign source in capsules. `data`, `blob`,
//! and `note` extend that to the rest of a project: text payloads (CSV, JSON,
//! YAML, XML, SQL, Markdown), byte payloads (images, audio, video,
//! spreadsheets, columnar files, compiled artifacts), and an annotation graph
//! over any of it.
//!
//! They are items in the same AST as everything else, which is the point: each
//! one gets a canonical node id, a source position, an entry in the project
//! map, a slot in a context packet, and a node hash that a `NODE_HASH` patch
//! precondition or an `fgx fs --expect` edit can check — none of which a
//! separate container format would have. Editing lives in `section_edit`.
//!
//! This module owns what those sections need that code does not: base64
//! transport for bytes, resolution of out-of-line payloads, and the checks that
//! keep a carried payload honest.

use crate::ast::{BlobPayload, BlobSection, Item, NoteNode, Program};
use crate::diag::Diagnostic;

/// Bytes at or below this size are carried inline, as base64, in the project
/// file. Larger payloads go to the content-addressed store: a file people read
/// and diff should not contain a megabyte of base64, and base64 inflates by a
/// third besides.
pub const MAX_INLINE_BYTES: u64 = 256 * 1024;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode base64, rejecting anything malformed rather than guessing.
///
/// Whitespace between groups is ignored, because an inline payload is wrapped
/// across lines to stay readable.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut symbols = Vec::with_capacity(text.len());
    let mut padding = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch == '=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err("base64 has a symbol after its padding".to_string());
        }
        let value = ALPHABET
            .iter()
            .position(|candidate| *candidate as char == ch)
            .ok_or_else(|| format!("base64 contains an invalid character {ch:?}"))?;
        symbols.push(value as u8);
    }
    if padding > 2 {
        return Err("base64 has more than two padding characters".to_string());
    }
    if !(symbols.len() + padding).is_multiple_of(4) {
        return Err("base64 length is not a multiple of four".to_string());
    }
    let mut out = Vec::with_capacity(symbols.len() / 4 * 3);
    for group in symbols.chunks(4) {
        let mut triple = 0u32;
        for (index, symbol) in group.iter().enumerate() {
            triple |= (*symbol as u32) << (18 - 6 * index);
        }
        out.push((triple >> 16) as u8);
        if group.len() > 2 {
            out.push((triple >> 8) as u8);
        }
        if group.len() > 3 {
            out.push(triple as u8);
        }
    }
    Ok(out)
}

/// The bytes a blob stands for.
///
/// An inline payload decodes from the file itself. An out-of-line one is read
/// from the store, relative to the directory holding the project file, and is
/// verified against the declared hash before it is returned — a store entry is
/// ordinary content on disk and must not be trusted merely for being there.
pub fn blob_bytes(section: &BlobSection, root: &std::path::Path) -> Result<Vec<u8>, String> {
    let bytes = match &section.payload {
        BlobPayload::Inline(lines) => decode_base64(&lines.join(""))
            .map_err(|err| format!("blob '{}': {err}", section.name))?,
        BlobPayload::Store(path) => {
            let resolved = root.join(path);
            std::fs::read(&resolved).map_err(|err| {
                format!(
                    "blob '{}': could not read {}: {err}",
                    section.name,
                    resolved.display()
                )
            })?
        }
    };
    if let Some(expected) = &section.sha256 {
        let actual = crate::crypto::sha256_hex(&bytes);
        if &actual != expected {
            return Err(format!(
                "blob '{}': content does not match its declared sha256 (declared {expected}, found {actual})",
                section.name
            ));
        }
    }
    Ok(bytes)
}

/// Check the carried sections of a program.
///
/// These are content checks, not type checks: names must be unique, a note must
/// point at something that exists, and a blob's payload must be exactly what it
/// says it is. The last one matters most — a blob whose bytes, length, and hash
/// disagree is the failure that silently corrupts whatever reads it.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let known: std::collections::BTreeSet<&str> = program.items.iter().map(item_id).collect::<_>();
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

    for item in &program.items {
        let id = item_id(item);
        if matches!(item, Item::Data(_) | Item::Blob(_) | Item::Note(_)) && !seen.insert(id) {
            diagnostics.push(
                Diagnostic::new(
                    "section_duplicate_name",
                    id,
                    format!("two carried sections share the node id '{id}'"),
                )
                .suggest(
                    "rename one of them; a section's name is how everything else refers to it",
                ),
            );
        }
        match item {
            Item::Data(section) => validate_timestamps(id, &section.meta, &mut diagnostics),
            Item::Blob(section) => {
                validate_timestamps(id, &section.meta, &mut diagnostics);
                validate_blob(section, &mut diagnostics);
            }
            Item::Note(note) => {
                validate_timestamp(id, "created", note.created.as_deref(), &mut diagnostics);
                validate_note(note, &known, &mut diagnostics);
            }
            _ => {}
        }
    }
    diagnostics
}

fn validate_timestamps(
    id: &str,
    meta: &crate::ast::SectionMeta,
    diagnostics: &mut Vec<Diagnostic>,
) {
    validate_timestamp(id, "created", meta.created.as_deref(), diagnostics);
    validate_timestamp(id, "modified", meta.modified.as_deref(), diagnostics);
}

/// Provenance timestamps are fixed-width UTC so they sort as text: the change
/// feed compares them that way, and a local offset or fractional second would
/// quietly put an edit on the wrong side of `--since`.
fn validate_timestamp(
    id: &str,
    field: &str,
    value: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(value) = value else { return };
    if !crate::section_edit::is_timestamp(value) {
        diagnostics.push(
            Diagnostic::new(
                "section_timestamp_invalid",
                id,
                format!("'{id}' records {field} as '{value}'"),
            )
            .suggest("write UTC as YYYY-MM-DDTHH:MM:SSZ, for example 2026-09-20T10:15:00Z"),
        );
    }
}

fn validate_blob(section: &BlobSection, diagnostics: &mut Vec<Diagnostic>) {
    match &section.payload {
        BlobPayload::Inline(lines) => {
            let decoded = match decode_base64(&lines.join("")) {
                Ok(decoded) => decoded,
                Err(err) => {
                    diagnostics.push(
                        Diagnostic::new(
                            "blob_payload_invalid",
                            &section.id,
                            format!("blob '{}' is not valid base64: {err}", section.name),
                        )
                        .suggest("re-encode the payload as base64"),
                    );
                    return;
                }
            };
            if decoded.len() as u64 != section.bytes {
                diagnostics.push(
                    Diagnostic::new(
                        "blob_size_mismatch",
                        &section.id,
                        format!(
                            "blob '{}' declares {} bytes but its payload decodes to {}",
                            section.name,
                            section.bytes,
                            decoded.len()
                        ),
                    )
                    .suggest("correct the declared byte count to match the payload"),
                );
            }
            if let Some(expected) = &section.sha256 {
                let actual = crate::crypto::sha256_hex(&decoded);
                if &actual != expected {
                    diagnostics.push(
                        Diagnostic::new(
                            "blob_hash_mismatch",
                            &section.id,
                            format!(
                                "blob '{}' does not match its declared sha256 (declared {expected}, found {actual})",
                                section.name
                            ),
                        )
                        .suggest("the payload and the hash were edited independently; recompute the hash over the payload"),
                    );
                }
            }
            if section.bytes > MAX_INLINE_BYTES {
                diagnostics.push(
                    Diagnostic::new(
                        "blob_inline_too_large",
                        &section.id,
                        format!(
                            "blob '{}' carries {} bytes inline, above the {MAX_INLINE_BYTES}-byte limit",
                            section.name, section.bytes
                        ),
                    )
                    .suggest("move it to the content-addressed store so the project file stays readable"),
                );
            }
        }
        BlobPayload::Store(path) => {
            if section.sha256.is_none() {
                diagnostics.push(
                    Diagnostic::new(
                        "blob_store_unverifiable",
                        &section.id,
                        format!(
                            "blob '{}' lives in the store at '{path}' but declares no sha256",
                            section.name
                        ),
                    )
                    .suggest("declare the hash: an out-of-line payload is ordinary content on disk and is otherwise unverifiable"),
                );
            }
        }
    }
}

fn validate_note(
    note: &NoteNode,
    known: &std::collections::BTreeSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !note.author.starts_with("agent:") && !note.author.starts_with("human:") {
        diagnostics.push(
            Diagnostic::new(
                "note_author_unqualified",
                &note.id,
                format!(
                    "note '{}' records its author as '{}', which says nothing about whether a person or an agent wrote it",
                    note.name, note.author
                ),
            )
            .suggest("write it as agent:<name> or human:<name>"),
        );
    }
    if !matches!(note.status.as_str(), "open" | "resolved") {
        diagnostics.push(
            Diagnostic::new(
                "note_status_unknown",
                &note.id,
                format!("note '{}' has status '{}'", note.name, note.status),
            )
            .suggest("use open or resolved"),
        );
    }
    // A note that points at nothing is worse than no note: an agent following
    // the graph would read it as commentary on code that no longer exists.
    if !known.contains(note.target.as_str()) {
        diagnostics.push(
            Diagnostic::new(
                "note_target_missing",
                &note.id,
                format!(
                    "note '{}' is on '{}', which is not a node in this file",
                    note.name, note.target
                ),
            )
            .suggest("point it at an existing node id, or remove the note"),
        );
    }
    if let Some(reply_to) = &note.reply_to {
        if !known.contains(reply_to.as_str()) {
            diagnostics.push(
                Diagnostic::new(
                    "note_reply_missing",
                    &note.id,
                    format!(
                        "note '{}' replies to '{reply_to}', which is not a node in this file",
                        note.name
                    ),
                )
                .suggest("point it at an existing note, or drop the reply-to"),
            );
        } else if reply_to == &note.id {
            diagnostics.push(
                Diagnostic::new(
                    "note_reply_cycle",
                    &note.id,
                    format!("note '{}' replies to itself", note.name),
                )
                .suggest("drop the reply-to"),
            );
        }
    }
}

fn item_id(item: &Item) -> &str {
    match item {
        Item::Struct(decl) => &decl.id,
        Item::Enum(decl) => &decl.id,
        Item::Trait(decl) => &decl.id,
        Item::TraitImpl(decl) => &decl.id,
        Item::Function(function) => &function.id,
        Item::Test(test) => &test.id,
        Item::Capsule(capsule) => &capsule.id,
        Item::Share(share) => &share.id,
        Item::Pipeline(pipeline) => &pipeline.id,
        Item::Data(section) => &section.id,
        Item::Blob(section) => &section.id,
        Item::Note(note) => &note.id,
    }
}

// ---- Writing: adding payloads, and putting them back on disk --------------

/// Where out-of-line bytes live, relative to the project file.
pub const STORE_DIR: &str = ".fgstore";

/// Shapes carried as text. Anything else is carried as bytes.
///
/// The split is by what a person can usefully read and diff inside the project
/// file, not by whether the bytes happen to be UTF-8: a 40 MB JSON export is
/// still better off as a blob.
const TEXT_SHAPES: &[(&str, &str)] = &[
    ("csv", "csv"),
    ("tsv", "tsv"),
    ("json", "json"),
    ("ndjson", "ndjson"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("xml", "xml"),
    ("sql", "sql"),
    ("md", "md"),
    ("markdown", "md"),
    ("txt", "txt"),
    ("toml", "toml"),
    ("ini", "ini"),
];

/// The media type recorded for a file extension. An unknown extension gets the
/// generic type rather than a guess.
const MEDIA_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("wav", "audio/wav"),
    ("mp3", "audio/mpeg"),
    ("flac", "audio/flac"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("parquet", "application/vnd.apache.parquet"),
    ("avro", "application/vnd.apache.avro"),
    ("orc", "application/vnd.apache.orc"),
    ("h5", "application/x-hdf5"),
    ("hdf5", "application/x-hdf5"),
    ("pb", "application/x-protobuf"),
    ("zip", "application/zip"),
    ("pdf", "application/pdf"),
    ("exe", "application/vnd.microsoft.portable-executable"),
    ("wasm", "application/wasm"),
];

fn extension_of(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The text shape for an extension, or `None` when the payload is bytes.
pub fn text_shape_for(path: &std::path::Path) -> Option<&'static str> {
    let ext = extension_of(path);
    TEXT_SHAPES
        .iter()
        .find(|(candidate, _)| *candidate == ext)
        .map(|(_, shape)| *shape)
}

pub fn media_type_for(path: &std::path::Path) -> String {
    let ext = extension_of(path);
    MEDIA_TYPES
        .iter()
        .find(|(candidate, _)| *candidate == ext)
        .map(|(_, media)| (*media).to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

/// A conventional file name for a section, used when materialising to disk.
pub fn file_name_for(item: &Item) -> Option<String> {
    match item {
        Item::Data(section) => Some(format!("{}.{}", section.name, section.kind)),
        Item::Blob(section) => {
            let ext = MEDIA_TYPES
                .iter()
                .find(|(_, media)| *media == section.media_type)
                .map(|(ext, _)| (*ext).to_string())
                .unwrap_or_else(|| "bin".to_string());
            Some(format!("{}.{ext}", section.name))
        }
        _ => None,
    }
}

/// The store path for some bytes: content-addressed, so identical payloads are
/// stored once and no path has to be invented or de-duplicated by name.
pub fn store_path_for(bytes: &[u8]) -> String {
    let hash = crate::crypto::sha256_hex(bytes);
    format!("{STORE_DIR}/{}/{}", &hash[..2], &hash[2..])
}

/// A section ready to be added, and the store write it needs, if any: the
/// relative path to write and the bytes to put there.
pub type AddedSection = (Item, Option<(String, Vec<u8>)>);

/// Build the section for a payload being added to a project, plus the store
/// write it needs, if any.
///
/// Text shapes are carried inline as content; bytes are carried inline while
/// they are small and go to the store when they are not. The timestamp and
/// author are passed in rather than read here, so this stays deterministic and
/// only the command line touches the wall clock.
pub fn section_for_payload(
    name: &str,
    source: &std::path::Path,
    bytes: Vec<u8>,
    at: &str,
    by: &str,
    force_blob: bool,
) -> Result<AddedSection, String> {
    let meta = crate::ast::SectionMeta {
        created: Some(at.to_string()),
        created_by: Some(by.to_string()),
        modified: Some(at.to_string()),
        modified_by: Some(by.to_string()),
    };
    if !force_blob {
        if let Some(shape) = text_shape_for(source) {
            let text = String::from_utf8(bytes).map_err(|_| {
                format!(
                    "{} looks like {shape} but is not valid UTF-8; add it with --as blob",
                    source.display()
                )
            })?;
            let mut section = crate::ast::DataSection::new(name.to_string(), shape.to_string());
            section.meta = meta;
            // Normalised to LF so a payload does not change shape according to
            // the platform that added it.
            section.content = text
                .replace("\r\n", "\n")
                .split('\n')
                .map(str::to_string)
                .collect();
            while section.content.last().is_some_and(|line| line.is_empty()) {
                section.content.pop();
            }
            return Ok((Item::Data(section), None));
        }
    }
    let hash = crate::crypto::sha256_hex(&bytes);
    let size = bytes.len() as u64;
    let (payload, store_write) = if size <= MAX_INLINE_BYTES {
        // Wrapped so the project file stays readable; the decoder ignores the
        // line breaks.
        let lines = encode_base64(&bytes)
            .as_bytes()
            .chunks(76)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<Vec<_>>();
        (BlobPayload::Inline(lines), None)
    } else {
        let path = store_path_for(&bytes);
        (BlobPayload::Store(path.clone()), Some((path, bytes)))
    };
    let mut section =
        crate::ast::BlobSection::new(name.to_string(), media_type_for(source), payload);
    section.meta = meta;
    section.bytes = size;
    section.sha256 = Some(hash);
    Ok((Item::Blob(section), store_write))
}
