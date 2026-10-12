//! Editing carried sections, and asking what changed.
//!
//! Many agents may work on one project file. Two properties make that safe
//! rather than lucky:
//!
//! - **Every edit can name what it expects to replace.** A caller passes the
//!   section's node hash (the same one `fgx fs ls --json` reports and a
//!   `NODE_HASH` patch precondition checks), and an edit whose section has moved
//!   since it was read is refused instead of overwriting someone else's work.
//! - **Every edit records who made it and when.** `modified` and `modified_by`
//!   are stamped here, so provenance is a property of the operation, not
//!   something each caller has to remember to update.
//!
//! The change feed answers "what moved since I last looked" without the file
//! keeping a history of its own: compare against an earlier copy of the file
//! (or a git revision of it) by node hash, or ask the recorded provenance for
//! everything touched after a timestamp.
//!
//! Nothing here reads a clock. Timestamps arrive as arguments, so every
//! function is deterministic; only the command line decides what "now" is.

use crate::ast::{item_node_id, Item, NoteNode, Program, SectionMeta};

/// Refuse an edit whose section has moved since the caller read it.
pub fn check_expected(program: &Program, id: &str, expect: Option<&str>) -> Result<(), String> {
    let Some(expect) = expect else {
        return Ok(());
    };
    let actual = crate::patch::node_hash(program, id)
        .ok_or_else(|| format!("no node with id '{id}' in this file"))?;
    if actual != expect {
        return Err(format!(
            "'{id}' has changed since it was read (expected hash {expect}, found {actual}); read it again and redo the edit"
        ));
    }
    Ok(())
}

fn stamp(meta: &mut SectionMeta, at: &str, by: &str) {
    meta.modified = Some(at.to_string());
    meta.modified_by = Some(by.to_string());
}

/// Replace a section's payload with `replacement`, which must be the same kind
/// of section. The section keeps its name, its creation record, and, for a data
/// section, its declared shape; the edit is recorded as a modification.
pub fn replace_payload(
    program: &mut Program,
    id: &str,
    replacement: Item,
    at: &str,
    by: &str,
) -> Result<(), String> {
    let slot = program
        .items
        .iter_mut()
        .find(|item| item_node_id(item) == id)
        .ok_or_else(|| format!("no section with node id '{id}' in this file"))?;
    match (slot, replacement) {
        (Item::Data(existing), Item::Data(new)) => {
            existing.content = new.content;
            stamp(&mut existing.meta, at, by);
        }
        (Item::Blob(existing), Item::Blob(new)) => {
            existing.payload = new.payload;
            existing.bytes = new.bytes;
            existing.sha256 = new.sha256;
            stamp(&mut existing.meta, at, by);
        }
        (Item::Data(_), Item::Blob(_)) => {
            return Err(format!(
                "'{id}' is a data section, but the replacement is not text of its shape; remove it and add the file as a blob instead"
            ))
        }
        (Item::Blob(_), Item::Data(_)) => {
            return Err(format!(
                "'{id}' is a blob, but the replacement is a data section"
            ))
        }
        (other, _) => {
            return Err(format!(
                "'{id}' is a {} and has no payload to replace",
                crate::ast::item_kind_name(other)
            ))
        }
    }
    Ok(())
}

/// Remove a section.
///
/// Notes about it would be left pointing at nothing, so removal is refused
/// while any exist unless `with_notes` is set — in which case they go too,
/// along with every reply beneath them. Returns the ids removed, the section
/// first.
pub fn remove_section(
    program: &mut Program,
    id: &str,
    with_notes: bool,
) -> Result<Vec<String>, String> {
    let item = program
        .items
        .iter()
        .find(|item| item_node_id(item) == id)
        .ok_or_else(|| format!("no node with id '{id}' in this file"))?;
    if !matches!(item, Item::Data(_) | Item::Blob(_) | Item::Note(_)) {
        return Err(format!(
            "'{id}' is a {}; `fs rm` removes carried sections, and code is edited with a semantic patch",
            crate::ast::item_kind_name(item)
        ));
    }

    // Everything that hangs off the removed node: notes on it, replies to any
    // of those, and so on down. A fixpoint, because replies can nest.
    let mut doomed = vec![id.to_string()];
    loop {
        let before = doomed.len();
        for note in program.notes() {
            let attached = doomed.contains(&note.target)
                || note
                    .reply_to
                    .as_ref()
                    .is_some_and(|parent| doomed.contains(parent));
            if attached && !doomed.contains(&note.id) {
                doomed.push(note.id.clone());
            }
        }
        if doomed.len() == before {
            break;
        }
    }
    if doomed.len() > 1 && !with_notes {
        return Err(format!(
            "'{id}' still has remarks attached ({}); pass --with-notes to remove them too",
            doomed[1..].join(", ")
        ));
    }
    program
        .items
        .retain(|item| !doomed.iter().any(|gone| gone == item_node_id(item)));
    Ok(doomed)
}

/// What a new note says, and where it hangs.
pub struct NewNote<'a> {
    pub on: &'a str,
    pub text: &'a str,
    pub reply_to: Option<&'a str>,
    /// A chosen name; when absent the next free `n<k>` is used.
    pub name: Option<&'a str>,
    pub status: &'a str,
}

/// Add a remark to the annotation graph. Returns the new note's id.
pub fn add_note(
    program: &mut Program,
    note: NewNote,
    at: &str,
    by: &str,
) -> Result<String, String> {
    let exists = |id: &str| program.items.iter().any(|item| item_node_id(item) == id);
    if !exists(note.on) {
        return Err(format!(
            "'{}' is not a node in this file, so there is nothing to remark on",
            note.on
        ));
    }
    if let Some(parent) = note.reply_to {
        if !program.notes().any(|existing| existing.id == parent) {
            return Err(format!("'{parent}' is not a note in this file"));
        }
    }
    if !by.starts_with("agent:") && !by.starts_with("human:") {
        return Err(format!(
            "author '{by}' must be agent:<name> or human:<name>, so readers can tell who wrote it"
        ));
    }
    if !matches!(note.status, "open" | "resolved") {
        return Err(format!("status '{}' must be open or resolved", note.status));
    }
    let name = match note.name {
        Some(name) => name.trim_start_matches("@note.").to_string(),
        // One past the highest number in use rather than a count, so removing
        // a note never makes the next one reuse a name that a commit message,
        // a log, or another agent's memory may still refer to.
        None => {
            let highest = program
                .notes()
                .filter_map(|existing| existing.name.strip_prefix('n')?.parse::<u64>().ok())
                .max()
                .unwrap_or(0);
            format!("n{}", highest + 1)
        }
    };
    let mut node = NoteNode::new(name, note.on.to_string(), by.to_string());
    if exists(&node.id) {
        return Err(format!("'{}' is already in this file", node.id));
    }
    node.created = Some(at.to_string());
    node.reply_to = note.reply_to.map(str::to_string);
    node.status = note.status.to_string();
    node.text = note
        .text
        .replace("\r\n", "\n")
        .split('\n')
        .map(str::to_string)
        .collect();
    let id = node.id.clone();
    program.items.push(Item::Note(node));
    Ok(id)
}

/// Mark a note resolved.
///
/// A note records only when it was written, so the resolution is kept as a
/// resolved reply carrying who closed the thread, when, and why. Without it a
/// status flip would be invisible to the change feed and unattributed.
/// Returns the reply's id.
pub fn resolve_note(
    program: &mut Program,
    id: &str,
    reason: Option<&str>,
    at: &str,
    by: &str,
) -> Result<String, String> {
    let target = {
        let note = program
            .items
            .iter()
            .find_map(|item| match item {
                Item::Note(note) if note.id == id => Some(note),
                _ => None,
            })
            .ok_or_else(|| format!("'{id}' is not a note in this file"))?;
        if note.status == "resolved" {
            return Err(format!("'{id}' is already resolved"));
        }
        note.target.clone()
    };
    // Validate the reply before changing anything, so a refused author leaves
    // the note open rather than resolved by nobody.
    let reply = add_note(
        program,
        NewNote {
            on: &target,
            text: reason.unwrap_or("Resolved."),
            reply_to: Some(id),
            name: None,
            status: "resolved",
        },
        at,
        by,
    )?;
    if let Some(Item::Note(note)) = program
        .items
        .iter_mut()
        .find(|item| item_node_id(item) == id)
    {
        note.status = "resolved".to_string();
    }
    Ok(reply)
}

/// One entry in the change feed.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: String,
    pub kind: &'static str,
    /// `added`, `removed`, or `changed`.
    pub change: &'static str,
    pub before: Option<String>,
    pub after: Option<String>,
    /// Who recorded the latest edit, when the item carries provenance.
    pub by: Option<String>,
    pub at: Option<String>,
}

fn provenance(item: &Item) -> (Option<String>, Option<String>) {
    match item {
        Item::Data(section) => latest(&section.meta),
        Item::Blob(section) => latest(&section.meta),
        Item::Note(note) => (note.created.clone(), Some(note.author.clone())),
        _ => (None, None),
    }
}

fn latest(meta: &SectionMeta) -> (Option<String>, Option<String>) {
    if meta.modified.is_some() {
        (meta.modified.clone(), meta.modified_by.clone())
    } else {
        (meta.created.clone(), meta.created_by.clone())
    }
}

/// Every node that differs between two versions of a file, by node hash.
///
/// This covers code as well as sections — an agent resuming work wants to
/// know that a function it depends on moved, not only that a payload did. The
/// order is the file's own: removed nodes where they were, then the rest where
/// they now are.
pub fn changes_between(before: &Program, after: &Program) -> Vec<Change> {
    let hash = |program: &Program, id: &str| crate::patch::node_hash(program, id);
    let mut out = Vec::new();
    for item in &before.items {
        let id = item_node_id(item);
        if !after.items.iter().any(|other| item_node_id(other) == id) {
            out.push(Change {
                id: id.to_string(),
                kind: crate::ast::item_kind_name(item),
                change: "removed",
                before: hash(before, id),
                after: None,
                by: None,
                at: None,
            });
        }
    }
    for item in &after.items {
        let id = item_node_id(item);
        let old = hash(before, id);
        let new = hash(after, id);
        let change = match &old {
            None => "added",
            Some(old) if Some(old) != new.as_ref() => "changed",
            Some(_) => continue,
        };
        let (at, by) = provenance(item);
        out.push(Change {
            id: id.to_string(),
            kind: crate::ast::item_kind_name(item),
            change,
            before: old,
            after: new,
            by,
            at,
        });
    }
    out
}

/// Every section and note whose recorded provenance is later than `since`.
///
/// This trusts what the file says about itself, which is what makes it work
/// without a prior copy — and why `changes_between` is the stronger answer
/// when one exists. Code carries no timestamps and is not reported.
pub fn changes_since(program: &Program, since: &str) -> Result<Vec<Change>, String> {
    if !is_timestamp(since) {
        return Err(format!(
            "'{since}' is not a timestamp of the form YYYY-MM-DDTHH:MM:SSZ"
        ));
    }
    let mut out = Vec::new();
    for item in &program.items {
        let (created, (at, by)) = match item {
            Item::Data(section) => (section.meta.created.clone(), latest(&section.meta)),
            Item::Blob(section) => (section.meta.created.clone(), latest(&section.meta)),
            Item::Note(note) => (
                note.created.clone(),
                (note.created.clone(), Some(note.author.clone())),
            ),
            _ => continue,
        };
        // The fixed-width UTC form sorts as text, which `is_timestamp`
        // guarantees for `since` and `validate` for what the file records.
        let Some(when) = at.as_deref() else { continue };
        if when <= since {
            continue;
        }
        let id = item_node_id(item);
        out.push(Change {
            id: id.to_string(),
            kind: crate::ast::item_kind_name(item),
            change: if created.as_deref().is_some_and(|c| c > since) {
                "added"
            } else {
                "changed"
            },
            before: None,
            after: crate::patch::node_hash(program, id),
            by,
            at,
        });
    }
    Ok(out)
}

/// Whether `text` is exactly `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Deliberately narrow: this form compares correctly as a string, and a
/// fractional second or a local offset would silently break that.
pub fn is_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 20 {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    digits.iter().all(|&i| bytes[i].is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
}
