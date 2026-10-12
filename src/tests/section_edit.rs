// Editing carried sections, and the change feed.
//
// What these protect is the multi-agent case: an edit made against a section
// that has since moved must be refused rather than overwrite someone else's
// work, every edit must say who made it, and an agent coming back to a file
// must be able to learn exactly what moved without rereading all of it.

use crate::ast::{Item, Program};
use crate::parser::parse_human;
use crate::section_edit::{
    add_note, changes_between, changes_since, check_expected, is_timestamp, remove_section,
    replace_payload, resolve_note, NewNote,
};

const PROJECT: &str = "\
data sales csv:
    created \"2026-09-20T09:00:00Z\" by \"human:viktor\"
    content:
        region,units
        north,120

note n1 on \"@data.sales\":
    by \"agent:claude-opus-5\"
    created \"2026-09-20T10:20:00Z\"
    status \"open\"
    text:
        North looks doubled.

fn total(a: i32, b: i32) -> i32:
    return a + b
";

fn project() -> Program {
    parse_human(PROJECT).expect("project should parse")
}

fn csv(content: &[&str]) -> Item {
    let mut section = crate::ast::DataSection::new("sales".to_string(), "csv".to_string());
    section.content = content.iter().map(|line| line.to_string()).collect();
    Item::Data(section)
}

fn hash(program: &Program, id: &str) -> String {
    crate::patch::node_hash(program, id).expect("every item has a node hash")
}

#[test]
fn every_item_kind_has_a_node_hash() {
    // `NODE_HASH` used to resolve only functions and structs, so a patch or an
    // edit could not name the version of a section it expected to replace.
    let program = project();
    for id in ["@data.sales", "@note.n1", "@function.total"] {
        assert!(crate::patch::node_hash(&program, id).is_some(), "{id}");
    }
    assert!(crate::patch::node_hash(&program, "@data.missing").is_none());
}

#[test]
fn an_edit_against_a_section_that_moved_is_refused() {
    let mut program = project();
    let read = hash(&program, "@data.sales");
    check_expected(&program, "@data.sales", Some(&read)).expect("unchanged section is fine");

    // Someone else edits first.
    replace_payload(
        &mut program,
        "@data.sales",
        csv(&["region,units", "north,60"]),
        "2026-09-21T08:00:00Z",
        "agent:other",
    )
    .unwrap();

    let error = check_expected(&program, "@data.sales", Some(&read)).unwrap_err();
    assert!(error.contains("has changed since it was read"), "{error}");
    check_expected(&program, "@data.sales", None).expect("no expectation, no check");
}

#[test]
fn replacing_a_payload_keeps_identity_and_records_the_edit() {
    let mut program = project();
    replace_payload(
        &mut program,
        "@data.sales",
        csv(&["region,units", "north,60"]),
        "2026-09-21T08:00:00Z",
        "agent:claude-opus-5",
    )
    .unwrap();
    let sales = program.data_sections().next().unwrap();
    assert_eq!(sales.content, vec!["region,units", "north,60"]);
    assert_eq!(sales.kind, "csv", "the shape belongs to the section");
    assert_eq!(
        sales.meta.created.as_deref(),
        Some("2026-09-20T09:00:00Z"),
        "an edit is not a creation"
    );
    assert_eq!(sales.meta.modified.as_deref(), Some("2026-09-21T08:00:00Z"));
    assert_eq!(
        sales.meta.modified_by.as_deref(),
        Some("agent:claude-opus-5")
    );

    let error = replace_payload(
        &mut program,
        "@function.total",
        csv(&["x"]),
        "2026-09-21T08:00:00Z",
        "agent:a",
    )
    .unwrap_err();
    assert!(error.contains("no payload to replace"), "{error}");
}

#[test]
fn removing_a_section_does_not_strand_its_remarks() {
    let mut program = project();
    add_note(
        &mut program,
        NewNote {
            on: "@data.sales",
            text: "Agreed.",
            reply_to: Some("@note.n1"),
            name: None,
            status: "open",
        },
        "2026-09-21T08:00:00Z",
        "human:viktor",
    )
    .unwrap();

    let refused = remove_section(&mut program.clone(), "@data.sales", false).unwrap_err();
    assert!(refused.contains("@note.n1"), "{refused}");
    assert!(refused.contains("@note.n2"), "{refused}");

    let removed = remove_section(&mut program, "@data.sales", true).unwrap();
    assert_eq!(removed, vec!["@data.sales", "@note.n1", "@note.n2"]);
    assert!(
        crate::section::validate(&program).is_empty(),
        "what remains must still check"
    );

    let code = remove_section(&mut program, "@function.total", true).unwrap_err();
    assert!(code.contains("semantic patch"), "{code}");
}

#[test]
fn removing_a_note_takes_its_replies_but_not_its_siblings() {
    let mut program = project();
    let at = "2026-09-21T08:00:00Z";
    let note = |on, reply_to| NewNote {
        on,
        text: "x",
        reply_to,
        name: None,
        status: "open",
    };
    let reply = add_note(
        &mut program,
        note("@data.sales", Some("@note.n1")),
        at,
        "human:v",
    )
    .unwrap();
    let sibling = add_note(&mut program, note("@data.sales", None), at, "human:v").unwrap();
    let removed = remove_section(&mut program, "@note.n1", true).unwrap();
    assert_eq!(removed, vec!["@note.n1".to_string(), reply]);
    assert!(program.notes().any(|note| note.id == sibling));
}

#[test]
fn a_new_note_never_reuses_a_removed_name() {
    // A name may still be referenced from outside the file — a commit message,
    // a log, another agent's memory — so reuse would silently repoint it.
    let mut program = project();
    let at = "2026-09-21T08:00:00Z";
    let note = || NewNote {
        on: "@data.sales",
        text: "x",
        reply_to: None,
        name: None,
        status: "open",
    };
    assert_eq!(
        add_note(&mut program, note(), at, "agent:a").unwrap(),
        "@note.n2"
    );
    assert_eq!(
        add_note(&mut program, note(), at, "agent:a").unwrap(),
        "@note.n3"
    );
    remove_section(&mut program, "@note.n2", false).unwrap();
    assert_eq!(
        add_note(&mut program, note(), at, "agent:a").unwrap(),
        "@note.n4"
    );
}

#[test]
fn a_note_is_refused_before_it_could_fail_the_checker() {
    let mut program = project();
    let at = "2026-09-21T08:00:00Z";
    let attempt = |program: &mut Program, on, reply_to, by| {
        add_note(
            program,
            NewNote {
                on,
                text: "x",
                reply_to,
                name: None,
                status: "open",
            },
            at,
            by,
        )
    };
    assert!(attempt(&mut program, "@data.gone", None, "agent:a").is_err());
    assert!(attempt(&mut program, "@data.sales", Some("@note.none"), "agent:a").is_err());
    assert!(attempt(&mut program, "@data.sales", None, "claude").is_err());
    assert_eq!(program.notes().count(), 1, "a refused note adds nothing");
}

#[test]
fn resolving_records_who_closed_the_thread() {
    let mut program = project();
    let reply = resolve_note(
        &mut program,
        "@note.n1",
        None,
        "2026-09-21T08:05:00Z",
        "human:viktor",
    )
    .unwrap();
    let notes: Vec<_> = program.notes().collect();
    assert_eq!(notes[0].status, "resolved");
    let reply = notes.iter().find(|note| note.id == reply).unwrap();
    assert_eq!(reply.author, "human:viktor");
    assert_eq!(reply.reply_to.as_deref(), Some("@note.n1"));
    assert_eq!(reply.status, "resolved");

    let again = resolve_note(
        &mut program,
        "@note.n1",
        None,
        "2026-09-21T08:06:00Z",
        "human:v",
    );
    assert!(again.unwrap_err().contains("already resolved"));

    // A refused author must not leave the note resolved by nobody.
    let mut fresh = project();
    assert!(resolve_note(
        &mut fresh,
        "@note.n1",
        None,
        "2026-09-21T08:05:00Z",
        "viktor"
    )
    .is_err());
    assert_eq!(fresh.notes().next().unwrap().status, "open");
}

#[test]
fn the_change_feed_reports_what_moved_between_two_versions() {
    let before = project();
    let mut after = before.clone();
    replace_payload(
        &mut after,
        "@data.sales",
        csv(&["region,units", "north,60"]),
        "2026-09-21T08:00:00Z",
        "agent:claude-opus-5",
    )
    .unwrap();
    resolve_note(
        &mut after,
        "@note.n1",
        Some("Halved at source."),
        "2026-09-21T08:05:00Z",
        "human:viktor",
    )
    .unwrap();

    let feed = changes_between(&before, &after);
    let summary: Vec<(&str, &str)> = feed
        .iter()
        .map(|change| (change.change, change.id.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("changed", "@data.sales"),
            ("changed", "@note.n1"),
            ("added", "@note.n2"),
        ],
        "the untouched function must not appear"
    );
    assert_eq!(feed[0].by.as_deref(), Some("agent:claude-opus-5"));
    assert_eq!(
        feed[0].before.as_deref(),
        Some(hash(&before, "@data.sales").as_str())
    );
    assert_eq!(
        feed[0].after.as_deref(),
        Some(hash(&after, "@data.sales").as_str())
    );

    let removed = changes_between(&after, &before);
    assert!(removed
        .iter()
        .any(|change| change.change == "removed" && change.id == "@note.n2"));
    assert!(changes_between(&before, &before).is_empty());
}

#[test]
fn code_changes_are_in_the_feed_too() {
    // An agent resuming work wants to know that a function it depends on
    // moved, not only that a payload did.
    let before = project();
    let after = parse_human(&PROJECT.replace("return a + b", "return b + a")).unwrap();
    let feed = changes_between(&before, &after);
    assert_eq!(feed.len(), 1, "{feed:?}");
    assert_eq!(feed[0].id, "@function.total");
    assert_eq!(feed[0].change, "changed");
}

#[test]
fn the_since_feed_reads_recorded_provenance() {
    let mut program = project();
    replace_payload(
        &mut program,
        "@data.sales",
        csv(&["region,units"]),
        "2026-09-21T08:00:00Z",
        "agent:a",
    )
    .unwrap();
    resolve_note(
        &mut program,
        "@note.n1",
        None,
        "2026-09-21T09:00:00Z",
        "human:v",
    )
    .unwrap();

    let feed = changes_since(&program, "2026-09-21T00:00:00Z").unwrap();
    let summary: Vec<(&str, &str)> = feed
        .iter()
        .map(|change| (change.change, change.id.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![("changed", "@data.sales"), ("added", "@note.n2")]
    );

    // Strictly after: an edit at the cut-off was already seen.
    assert_eq!(
        changes_since(&program, "2026-09-21T09:00:00Z")
            .unwrap()
            .len(),
        0
    );
    assert!(changes_since(&program, "2026-09-21").is_err());
}

#[test]
fn timestamps_are_fixed_width_utc() {
    assert!(is_timestamp("2026-09-21T08:00:00Z"));
    for bad in [
        "2026-09-21 08:00:00Z",
        "2026-09-21T08:00:00",
        "2026-09-21T08:00:00.5Z",
        "2026-09-21T08:00:00+02:00",
        "2026-9-21T08:00:00Z",
    ] {
        assert!(!is_timestamp(bad), "{bad}");
    }

    // And the checker holds the file to the same form the feed relies on.
    let local = PROJECT.replace("2026-09-20T09:00:00Z", "2026-09-20 09:00");
    let codes: Vec<String> = crate::section::validate(&parse_human(&local).unwrap())
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(codes, vec!["section_timestamp_invalid".to_string()]);
}
