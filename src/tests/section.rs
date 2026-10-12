// Carried sections: the project filesystem inside a Forgentex file.
//
// The property that makes this worth having is that a payload is an ordinary
// item: it round-trips through both syntaxes, carries a canonical node id, and
// is checked. A format that quietly loses a byte of a payload, or lets a note
// point at nothing, would be worse than keeping the files on disk.

use crate::ast::{BlobPayload, Item};
use crate::parser::{parse_agent, parse_human};
use crate::section::{decode_base64, encode_base64, validate};

const PROJECT: &str = "\
data sales csv:
    created \"2026-09-20T09:00:00Z\" by \"human:viktor\"
    modified \"2026-09-20T10:15:00Z\" by \"agent:claude-opus-5\"
    content:
        region,units
        north,120

blob logo image/png:
    created \"2026-09-20T09:05:00Z\" by \"human:viktor\"
    bytes 6
    sha256 \"c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2\"
    inline:
        Zm9vYmFy

note n1 on \"@data.sales\":
    by \"agent:claude-opus-5\"
    created \"2026-09-20T10:20:00Z\"
    status \"open\"
    text:
        North looks doubled after the Q3 import.

note n2 on \"@data.sales\":
    by \"human:viktor\"
    created \"2026-09-20T10:31:00Z\"
    reply-to \"@note.n1\"
    status \"resolved\"
    text:
        Confirmed and fixed at source.

fn total(a: i32, b: i32) -> i32:
    return a + b
";

#[test]
fn a_project_file_carries_data_bytes_notes_and_code_together() {
    let program = parse_human(PROJECT).expect("project should parse");
    assert_eq!(program.data_sections().count(), 1);
    assert_eq!(program.blobs().count(), 1);
    assert_eq!(program.notes().count(), 2);
    assert_eq!(program.functions().count(), 1);

    let sales = program.data_sections().next().unwrap();
    assert_eq!(sales.id, "@data.sales");
    assert_eq!(sales.kind, "csv");
    assert_eq!(sales.content, vec!["region,units", "north,120"]);
    assert_eq!(sales.meta.created_by.as_deref(), Some("human:viktor"));
    assert_eq!(
        sales.meta.modified_by.as_deref(),
        Some("agent:claude-opus-5"),
        "a section records who changed it last, not just that it changed"
    );

    let logo = program.blobs().next().unwrap();
    assert_eq!(logo.id, "@blob.logo");
    assert_eq!(logo.media_type, "image/png");
    assert_eq!(logo.bytes, 6);
    assert!(matches!(logo.payload, BlobPayload::Inline(_)));
}

#[test]
fn both_syntaxes_round_trip_a_payload_byte_for_byte() {
    // A payload is verbatim content, so the formatter must return it exactly —
    // a CSV that gains a space or a base64 line that gets rewrapped is a
    // corrupted file, not a formatting difference.
    let program = parse_human(PROJECT).unwrap();
    let human = crate::format::format_human(&program);
    assert_eq!(human, PROJECT, "human round-trip must be byte-identical");

    let agent = crate::format::format_agent(&program);
    let reparsed = parse_agent(&agent).expect("the agent view should parse back");
    assert_eq!(
        crate::format::format_human(&reparsed),
        PROJECT,
        "the dense view must carry the same payloads back"
    );
    assert_eq!(
        program.to_canonical_json(),
        reparsed.to_canonical_json(),
        "both syntaxes must describe one canonical program"
    );
}

#[test]
fn a_carried_payload_is_part_of_the_canonical_ast() {
    // If sections were left out of the canonical AST, a round-trip that dropped
    // one would still compare equal — the check would be checking nothing.
    let program = parse_human(PROJECT).unwrap();
    let canonical = program.to_canonical_json();
    assert!(canonical.contains("\"kind\": \"data\""), "{canonical}");
    assert!(canonical.contains("region,units"), "{canonical}");
    assert!(canonical.contains("\"kind\": \"blob\""), "{canonical}");
    assert!(canonical.contains("\"kind\": \"note\""), "{canonical}");

    let mut without = program.clone();
    without.items.retain(|item| !matches!(item, Item::Data(_)));
    assert_ne!(
        canonical,
        without.to_canonical_json(),
        "dropping a section has to change the canonical form"
    );
}

#[test]
fn every_section_is_addressable_by_a_node_id() {
    // Jumping straight to a section is the whole point: the id an agent lists
    // is the id it reads, and the id a semantic patch targets.
    let program = parse_human(PROJECT).unwrap();
    let ids: Vec<&str> = program.items.iter().map(crate::ast::item_node_id).collect();
    for expected in [
        "@data.sales",
        "@blob.logo",
        "@note.n1",
        "@note.n2",
        "@function.total",
    ] {
        assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
    }
}

#[test]
fn notes_form_a_graph_and_name_whether_an_agent_wrote_them() {
    let program = parse_human(PROJECT).unwrap();
    let notes: Vec<_> = program.notes().collect();
    assert!(notes[0].is_agent(), "n1 was written by an agent");
    assert!(!notes[1].is_agent(), "n2 was written by a person");
    assert_eq!(notes[1].reply_to.as_deref(), Some("@note.n1"));
    assert_eq!(notes[0].status, "open");
    assert_eq!(notes[1].status, "resolved");
}

#[test]
fn a_payload_that_disagrees_with_its_own_declaration_is_rejected() {
    // The failure that silently corrupts whatever reads the file: bytes, size,
    // and hash that do not describe each other.
    let wrong_hash = PROJECT.replace(
        "c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    let codes = codes_for(&wrong_hash);
    assert!(
        codes.contains(&"blob_hash_mismatch".to_string()),
        "{codes:?}"
    );

    let wrong_size = PROJECT.replace("bytes 6", "bytes 9");
    assert!(codes_for(&wrong_size).contains(&"blob_size_mismatch".to_string()));

    let bad_base64 = PROJECT.replace("Zm9vYmFy", "not base64!!");
    assert!(codes_for(&bad_base64).contains(&"blob_payload_invalid".to_string()));
}

#[test]
fn a_note_must_point_at_something_that_exists() {
    // A note on a vanished node reads as commentary on code that is still
    // there, which is worse than no note at all.
    let dangling = PROJECT.replace("note n1 on \"@data.sales\"", "note n1 on \"@data.gone\"");
    assert!(codes_for(&dangling).contains(&"note_target_missing".to_string()));

    let bad_reply = PROJECT.replace("reply-to \"@note.n1\"", "reply-to \"@note.nowhere\"");
    assert!(codes_for(&bad_reply).contains(&"note_reply_missing".to_string()));

    let unqualified = PROJECT.replace("by \"agent:claude-opus-5\"", "by \"claude\"");
    assert!(codes_for(&unqualified).contains(&"note_author_unqualified".to_string()));
}

#[test]
fn a_clean_project_reports_nothing() {
    // The guard against a validator that simply complains: the example above is
    // correct and must pass in full, through the real checker.
    let program = parse_human(PROJECT).unwrap();
    assert!(validate(&program).is_empty());
    let diagnostics = crate::typecheck::check(&program);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn base64_transport_round_trips_every_byte_value() {
    // Blob payloads are arbitrary bytes, including the ones that break a naive
    // encoder: every value, and every length remainder.
    let all_bytes: Vec<u8> = (0..=255u8).collect();
    for length in [0usize, 1, 2, 3, 4, 255, 256] {
        let sample = &all_bytes[..length.min(all_bytes.len())];
        let encoded = encode_base64(sample);
        let decoded = decode_base64(&encoded)
            .unwrap_or_else(|err| panic!("length {length} should decode: {err}"));
        assert_eq!(decoded, sample, "length {length} did not survive");
    }
    // Wrapped across lines, as an inline payload is written.
    let encoded = encode_base64(&all_bytes);
    let wrapped = encoded
        .as_bytes()
        .chunks(60)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(decode_base64(&wrapped).unwrap(), all_bytes);

    for malformed in ["=abc", "a", "ab=c", "abc!"] {
        assert!(
            decode_base64(malformed).is_err(),
            "{malformed:?} must be rejected rather than guessed at"
        );
    }
}

fn codes_for(source: &str) -> Vec<String> {
    let program = parse_human(source).expect("source should parse");
    validate(&program)
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

// --- Writing: adding payloads and putting them back on disk ----------------

use crate::section::{file_name_for, section_for_payload, store_path_for, MAX_INLINE_BYTES};

#[test]
fn a_text_payload_is_carried_as_readable_content() {
    let (item, store) = section_for_payload(
        "sales",
        std::path::Path::new("sales.csv"),
        b"region,units\r\nnorth,120\r\n".to_vec(),
        "2026-09-20T12:00:00Z",
        "agent:claude-opus-5",
        false,
    )
    .unwrap();
    assert!(store.is_none(), "text stays in the file");
    let Item::Data(section) = item else {
        panic!("a csv should become a data section");
    };
    assert_eq!(section.kind, "csv");
    // Normalised to LF, so a payload does not change shape according to the
    // platform that added it, and the trailing newline does not become a line.
    assert_eq!(section.content, vec!["region,units", "north,120"]);
    assert_eq!(
        section.meta.created_by.as_deref(),
        Some("agent:claude-opus-5")
    );
    assert_eq!(
        section.meta.modified.as_deref(),
        Some("2026-09-20T12:00:00Z")
    );
}

#[test]
fn bytes_stay_inline_until_they_are_too_big_to_read() {
    let small = vec![7u8; 1024];
    let (item, store) = section_for_payload(
        "icon",
        std::path::Path::new("icon.png"),
        small.clone(),
        "2026-09-20T12:00:00Z",
        "human:viktor",
        false,
    )
    .unwrap();
    assert!(store.is_none(), "a small payload needs no store");
    let Item::Blob(section) = item else {
        panic!("a png should become a blob");
    };
    assert_eq!(section.media_type, "image/png");
    assert_eq!(section.bytes, 1024);
    assert_eq!(
        section.sha256.as_deref(),
        Some(crate::crypto::sha256_hex(&small).as_str())
    );
    match &section.payload {
        BlobPayload::Inline(lines) => {
            assert!(
                lines.iter().all(|line| line.len() <= 76),
                "inline base64 is wrapped so the file stays readable"
            );
        }
        BlobPayload::Store(_) => panic!("1 KB should not reach the store"),
    }

    let large = vec![9u8; MAX_INLINE_BYTES as usize + 1];
    let (item, store) = section_for_payload(
        "clip",
        std::path::Path::new("clip.mp4"),
        large.clone(),
        "2026-09-20T12:00:00Z",
        "human:viktor",
        false,
    )
    .unwrap();
    let Item::Blob(section) = item else {
        panic!("an mp4 should become a blob");
    };
    let (path, bytes) = store.expect("a large payload goes to the store");
    assert_eq!(bytes, large);
    // Content-addressed: the path is derived from the bytes, so identical
    // payloads are stored once and no name has to be de-duplicated.
    assert_eq!(path, store_path_for(&large));
    assert!(path.starts_with(".fgstore/"), "{path}");
    assert_eq!(section.payload, BlobPayload::Store(path));
    assert_eq!(section.bytes, large.len() as u64);
    assert!(
        section.sha256.is_some(),
        "a store-backed payload must be verifiable"
    );
}

#[test]
fn a_payload_added_as_a_blob_keeps_every_byte() {
    // The round-trip that matters for images, audio, and binaries: add, then
    // read back through exactly the path `fgx fs cat` and `fs extract` use.
    let bytes: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    let (item, _) = section_for_payload(
        "sample",
        std::path::Path::new("sample.wav"),
        bytes.clone(),
        "2026-09-20T12:00:00Z",
        "human:viktor",
        false,
    )
    .unwrap();
    let Item::Blob(section) = &item else {
        panic!("expected a blob");
    };
    assert_eq!(section.media_type, "audio/wav");
    let read_back = crate::section::blob_bytes(section, std::path::Path::new(".")).unwrap();
    assert_eq!(read_back, bytes);

    // And it survives being written into a project and parsed back out.
    let mut program = parse_human("fn noop() -> void:\n    return\n").unwrap();
    program.items.push(item);
    let rendered = crate::format::format_human(&program);
    let reparsed = parse_human(&rendered).expect("a written project must parse back");
    let section = reparsed.blobs().next().unwrap();
    assert_eq!(
        crate::section::blob_bytes(section, std::path::Path::new(".")).unwrap(),
        bytes
    );
    assert!(validate(&reparsed).is_empty());
}

#[test]
fn text_that_is_not_utf8_is_refused_rather_than_mangled() {
    let err = section_for_payload(
        "broken",
        std::path::Path::new("broken.csv"),
        vec![0xff, 0xfe, 0x00],
        "2026-09-20T12:00:00Z",
        "human:viktor",
        false,
    )
    .unwrap_err();
    assert!(err.contains("not valid UTF-8"), "{err}");
    assert!(
        err.contains("--as blob"),
        "the way out has to be named: {err}"
    );

    // Forcing it to a blob is exactly that way out.
    let (item, _) = section_for_payload(
        "broken",
        std::path::Path::new("broken.csv"),
        vec![0xff, 0xfe, 0x00],
        "2026-09-20T12:00:00Z",
        "human:viktor",
        true,
    )
    .unwrap();
    assert!(matches!(item, Item::Blob(_)));
}

#[test]
fn extracted_file_names_are_derived_from_the_section() {
    let program = parse_human(PROJECT).unwrap();
    let names: Vec<String> = program.items.iter().filter_map(file_name_for).collect();
    assert_eq!(names, vec!["sales.csv".to_string(), "logo.png".to_string()]);
    // A media type with no known extension still gets a usable name.
    let (item, _) = section_for_payload(
        "opaque",
        std::path::Path::new("opaque.unknownext"),
        vec![1, 2, 3],
        "2026-09-20T12:00:00Z",
        "human:viktor",
        false,
    )
    .unwrap();
    assert_eq!(file_name_for(&item).as_deref(), Some("opaque.bin"));
}

// Reading a carried payload from code.
//
// Until this existed, the project filesystem was tooling-only: a file could
// carry its data and bytes, and the program beside them could not see any of
// it. Reading a payload is deliberately not I/O — the bytes are in the program
// — so it needs no effect and grants no authority, which is why an out-of-line
// payload must be refused rather than quietly reaching outside the file.

const CARRIED: &str = "\
data sales csv:
    content:
        region,units
        north,120

blob logo image/png:
    bytes 6
    sha256 \"c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2\"
    inline:
        Zm9vYmFy

note n1 on \"@data.sales\":
    by \"human:viktor\"
    status \"open\"
    text:
        A remark.

fn text_size() -> usize:
    return str_len(section_text(\"@data.sales\"))

fn first_byte() -> u8:
    let bytes: Vec[u8] = section_bytes(\"@blob.logo\")
    return bytes[0]
";

/// Run an entry and return the reported result, which carries a trap in its
/// own report rather than as an error.
fn run(program: &crate::ast::Program, entry: &str) -> String {
    crate::interp::run_entry_json(program, entry, "{\"args\": {}}")
        .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}"))
}

#[test]
fn a_program_can_read_the_payloads_its_file_carries() {
    let program = parse_human(CARRIED).unwrap();
    assert!(crate::typecheck::check(&program).is_empty());

    // "region,units\nnorth,120" — the payload as authored, newline-joined.
    let text_size = run(&program, "text_size");
    assert!(text_size.contains("\"return\": 22"), "{text_size}");
    // 'f', the first byte of "foobar".
    let first_byte = run(&program, "first_byte");
    assert!(first_byte.contains("\"return\": 102"), "{first_byte}");
}

#[test]
fn a_payload_reference_is_checked_rather_than_trusted() {
    let codes = |source: &str| -> Vec<String> {
        let program = parse_human(source).unwrap();
        crate::typecheck::check(&program)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    };

    // A section that is not there is a mistake the checker can rule out
    // completely, so it does so.
    assert!(codes(&CARRIED.replace("@data.sales\")", "@data.gone\")"))
        .contains(&"section_ref_unknown".to_string()));

    // A note carries text, but it is not a payload to read.
    assert!(codes(&CARRIED.replace(
        "section_text(\"@data.sales\")",
        "section_text(\"@note.n1\")"
    ))
    .contains(&"section_ref_not_a_payload".to_string()));

    // A computed id would move the failure to runtime for no benefit.
    let computed = CARRIED.replace(
        "section_text(\"@data.sales\")",
        "section_text(str_concat(\"@data.\", \"sales\"))",
    );
    assert!(codes(&computed).contains(&"section_ref_not_literal".to_string()));
}

#[test]
fn a_payload_that_disagrees_with_its_declaration_is_not_readable() {
    // `fgx check` catches this too; the interpreter checks again because a
    // program reached through the library API may never have been checked, and
    // bytes that disagree with what the file says they are must not be read.
    let wrong_hash = CARRIED.replace(
        "c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    let program = parse_human(&wrong_hash).unwrap();
    let report = run(&program, "first_byte");
    assert!(report.contains("blob_hash_mismatch"), "{report}");

    let wrong_size = CARRIED.replace("bytes 6", "bytes 9");
    let program = parse_human(&wrong_size).unwrap();
    let report = run(&program, "first_byte");
    assert!(report.contains("blob_size_mismatch"), "{report}");
}

#[test]
fn an_out_of_line_payload_is_refused_rather_than_read_from_disk() {
    // Reading a store entry is a file read. Allowing it here would hand a
    // function that declares no effects the authority to read a path, so it is
    // refused with the way out named.
    let stored = CARRIED.replace(
        "    inline:\n        Zm9vYmFy",
        "    store \".fgstore/c3/ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2\"",
    );
    let program = parse_human(&stored).unwrap();
    let report = run(&program, "first_byte");
    assert!(report.contains("section_payload_out_of_line"), "{report}");
    assert!(
        report.contains("fgx fs extract"),
        "the refusal names a way out: {report}"
    );
}

#[test]
fn reading_a_payload_is_blocked_for_native_lowering_rather_than_ignored() {
    // A carried payload lives in the source file; emitting it as a static
    // constant is sound and not built. An unsupported construct must produce an
    // explicit blocker, never lower with weaker semantics.
    let dir =
        std::env::temp_dir().join(format!("forgentex_{}_section_blockers", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"sections\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.fgx"), CARRIED).unwrap();

    let error = crate::c_backend::build_c_project(&dir, &dir.join("out-c")).unwrap_err();
    assert!(error.contains("intrinsic:section_text"), "{error}");
    assert!(error.contains("intrinsic:section_bytes"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
