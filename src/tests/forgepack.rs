use crate::forgepack::{self, BuildEntry};
use std::fs;
use std::path::PathBuf;

fn golden_inputs() -> Vec<BuildEntry> {
    vec![
        BuildEntry {
            path: "images/pixel.bin".to_string(),
            media_type: "application/octet-stream".to_string(),
            bytes: vec![0, 1, 2, 3, 255],
        },
        BuildEntry {
            path: "docs/readme.txt".to_string(),
            media_type: "text/plain".to_string(),
            bytes: b"Forgentex forgepack v1\n".to_vec(),
        },
    ]
}

fn decode_hex(source: &str) -> Vec<u8> {
    let compact: String = source.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert_eq!(compact.len() % 2, 0);
    compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .expect("golden fixture must be hexadecimal")
        })
        .collect()
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_forgepack_{name}", std::process::id()))
}

#[test]
fn forgepack_v1_build_is_byte_identical_to_the_golden_archive() {
    let actual = forgepack::build(golden_inputs()).unwrap();
    let expected = decode_hex(include_str!(
        "../../examples/apps/forgepack/golden/two_entries.fpk.hex"
    ));
    assert_eq!(actual, expected);
}

#[test]
fn forgepack_v1_inspect_and_verify_share_one_strict_parser() {
    let bytes = forgepack::build(golden_inputs()).unwrap();
    let inspected = forgepack::inspect(&bytes).unwrap();
    let verified = forgepack::verify(&bytes).unwrap();
    assert_eq!(inspected, verified);
    assert_eq!(verified.version, 1);
    assert_eq!(verified.flags, 0);
    assert_eq!(verified.manifest.entries.len(), 2);
    assert_eq!(verified.manifest.entries[0].path, "docs/readme.txt");
    assert_eq!(verified.manifest.entries[0].offset, 0);
    assert_eq!(verified.manifest.entries[0].length, 23);
    assert_eq!(verified.manifest.entries[1].path, "images/pixel.bin");
    assert_eq!(verified.manifest.entries[1].offset, 23);
    assert_eq!(verified.manifest.entries[1].length, 5);
}

#[test]
fn forgepack_v1_build_rejects_duplicate_and_unportable_paths() {
    let mut duplicate = golden_inputs();
    duplicate[0].path = duplicate[1].path.clone();
    let error = forgepack::build(duplicate).unwrap_err();
    assert!(error.contains("forgepack_duplicate_path"), "{error}");

    let error = forgepack::build(vec![BuildEntry {
        path: "../escape.bin".to_string(),
        media_type: "application/octet-stream".to_string(),
        bytes: vec![1],
    }])
    .unwrap_err();
    assert!(error.contains("forgepack_path_invalid"), "{error}");

    let error = forgepack::build(vec![
        BuildEntry {
            path: "folder".to_string(),
            media_type: "text/plain".to_string(),
            bytes: vec![],
        },
        BuildEntry {
            path: "folder/file.txt".to_string(),
            media_type: "text/plain".to_string(),
            bytes: vec![],
        },
    ])
    .unwrap_err();
    assert!(error.contains("forgepack_path_conflict"), "{error}");
}

#[test]
fn forgepack_v1_rejects_header_manifest_and_payload_corruption() {
    let bytes = forgepack::build(golden_inputs()).unwrap();

    let mut bad_magic = bytes.clone();
    bad_magic[0] ^= 1;
    assert!(forgepack::verify(&bad_magic)
        .unwrap_err()
        .contains("forgepack_bad_magic"));

    let mut unknown_flags = bytes.clone();
    unknown_flags[10] = 1;
    assert!(forgepack::verify(&unknown_flags)
        .unwrap_err()
        .contains("forgepack_unsupported_flags"));

    let mut wrong_count = bytes.clone();
    wrong_count[12..16].copy_from_slice(&3u32.to_le_bytes());
    assert!(forgepack::verify(&wrong_count)
        .unwrap_err()
        .contains("forgepack_entry_count_mismatch"));

    let mut payload_corruption = bytes.clone();
    let last = payload_corruption.len() - 1;
    payload_corruption[last] ^= 1;
    assert!(forgepack::verify(&payload_corruption)
        .unwrap_err()
        .contains("forgepack_hash_mismatch"));
}

#[test]
fn forgepack_inspect_validates_structure_without_claiming_payload_hashes() {
    let mut bytes = forgepack::build(golden_inputs()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let root = temp_root("inspect");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let archive = root.join("corrupt-payload.fpk");
    fs::write(&archive, &bytes).unwrap();

    let report = forgepack::inspect_file(&archive).unwrap();
    assert_eq!(report.entries.len(), 2);
    assert!(report.to_json().contains("\"structure_valid\":true"));
    assert!(report.to_json().contains("\"hashes_verified\":false"));
    assert!(forgepack::verify_file(&archive)
        .unwrap_err()
        .contains("forgepack_hash_mismatch"));

    bytes[0] ^= 1;
    fs::write(&archive, bytes).unwrap();
    assert!(forgepack::inspect_file(&archive)
        .unwrap_err()
        .contains("forgepack_bad_magic"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forgepack_v1_rejects_noncanonical_or_inconsistent_manifests() {
    let bytes = forgepack::build(golden_inputs()).unwrap();
    let manifest_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let manifest = std::str::from_utf8(&bytes[24..24 + manifest_len]).unwrap();

    let noncanonical = manifest.replacen("{\"entries\"", "{ \"entries\"", 1);
    let mut altered = Vec::new();
    altered.extend_from_slice(&bytes[..16]);
    altered.extend_from_slice(&(noncanonical.len() as u32).to_le_bytes());
    altered.extend_from_slice(&bytes[20..24]);
    altered.extend_from_slice(noncanonical.as_bytes());
    altered.extend_from_slice(&bytes[24 + manifest_len..]);
    assert!(forgepack::verify(&altered)
        .unwrap_err()
        .contains("forgepack_manifest_noncanonical"));

    let bad_offset = manifest.replacen("\"offset\":23", "\"offset\":22", 1);
    let mut altered = Vec::new();
    altered.extend_from_slice(&bytes[..24]);
    altered.extend_from_slice(bad_offset.as_bytes());
    altered.extend_from_slice(&bytes[24 + manifest_len..]);
    assert!(forgepack::verify(&altered)
        .unwrap_err()
        .contains("forgepack_payload_layout"));
}

#[test]
fn forgepack_build_and_verify_files_are_reproducible_and_no_clobber() {
    let root = temp_root("files");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("input")).unwrap();
    fs::create_dir_all(root.join("output")).unwrap();
    fs::write(root.join("input/a.txt"), b"alpha\n").unwrap();
    fs::write(root.join("input/b.bin"), [0, 1, 255]).unwrap();
    fs::write(
        root.join("build.json"),
        r#"{"entries":[{"media_type":"application/octet-stream","path":"b.bin","source":"input/b.bin"},{"media_type":"text/plain","path":"a.txt","source":"input/a.txt"}],"schema":"forgentex.forgepack_build_spec.v1"}"#,
    )
    .unwrap();

    let first = forgepack::build_file(&root.join("build.json"), "output/first.fpk").unwrap();
    let second = forgepack::build_file(&root.join("build.json"), "output/second.fpk").unwrap();
    assert_eq!(
        fs::read(root.join("output/first.fpk")).unwrap(),
        fs::read(root.join("output/second.fpk")).unwrap()
    );
    assert_eq!(first.sha256, second.sha256);
    let verified = forgepack::verify_file(&root.join("output/first.fpk")).unwrap();
    assert_eq!(verified.entries, 2);
    assert_eq!(verified.sha256, first.sha256);

    let error = forgepack::build_file(&root.join("build.json"), "output/first.fpk").unwrap_err();
    assert!(error.contains("forgepack_publish"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forgepack_build_rejects_source_and_output_traversal() {
    let root = temp_root("scope");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("output")).unwrap();
    fs::write(
        root.join("build.json"),
        r#"{"entries":[{"media_type":"text/plain","path":"safe.txt","source":"../outside.txt"}],"schema":"forgentex.forgepack_build_spec.v1"}"#,
    )
    .unwrap();
    let error = forgepack::build_file(&root.join("build.json"), "output/archive.fpk").unwrap_err();
    assert!(error.contains("forgepack_source_path_invalid"), "{error}");

    fs::write(
        root.join("build.json"),
        r#"{"entries":[],"schema":"forgentex.forgepack_build_spec.v1"}"#,
    )
    .unwrap();
    let error = forgepack::build_file(&root.join("build.json"), "../archive.fpk").unwrap_err();
    assert!(error.contains("forgepack_output_path_invalid"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forgepack_extract_verifies_then_publishes_confined_files_without_clobbering() {
    let root = temp_root("extract_ok");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("output")).unwrap();
    let archive = root.join("archive.fpk");
    fs::write(&archive, forgepack::build(golden_inputs()).unwrap()).unwrap();

    let report = forgepack::extract_file(&archive, &root.join("output")).unwrap();
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.bytes, 28);
    assert_eq!(
        fs::read(root.join("output/docs/readme.txt")).unwrap(),
        b"Forgentex forgepack v1\n"
    );
    assert_eq!(
        fs::read(root.join("output/images/pixel.bin")).unwrap(),
        [0, 1, 2, 3, 255]
    );
    assert!(report.to_json().contains("\"publication\":\"create_new\""));
    assert!(report.to_json().contains("\"transactional\":false"));

    let error = forgepack::extract_file(&archive, &root.join("output")).unwrap_err();
    assert!(error.contains("forgepack_extract_exists"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forgepack_extract_preflights_all_collisions_before_writing() {
    let root = temp_root("extract_collision");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("output/images")).unwrap();
    fs::write(root.join("output/images/pixel.bin"), b"keep").unwrap();
    let archive = root.join("archive.fpk");
    fs::write(&archive, forgepack::build(golden_inputs()).unwrap()).unwrap();

    let error = forgepack::extract_file(&archive, &root.join("output")).unwrap_err();
    assert!(error.contains("forgepack_extract_exists"), "{error}");
    assert!(!root.join("output/docs/readme.txt").exists());
    assert_eq!(
        fs::read(root.join("output/images/pixel.bin")).unwrap(),
        b"keep"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forgepack_extract_rejects_corruption_and_traversal_before_writing() {
    let root = temp_root("extract_denials");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("output")).unwrap();
    let archive = root.join("archive.fpk");
    let mut bytes = forgepack::build(golden_inputs()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&archive, &bytes).unwrap();
    let error = forgepack::extract_file(&archive, &root.join("output")).unwrap_err();
    assert!(error.contains("forgepack_hash_mismatch"), "{error}");
    assert_eq!(fs::read_dir(root.join("output")).unwrap().count(), 0);

    let mut traversal = forgepack::build(golden_inputs()).unwrap();
    let manifest_len = u32::from_le_bytes(traversal[16..20].try_into().unwrap()) as usize;
    let manifest = std::str::from_utf8(&traversal[24..24 + manifest_len])
        .unwrap()
        .replace("docs/readme.txt", "../x/readme.txt");
    traversal[24..24 + manifest_len].copy_from_slice(manifest.as_bytes());
    fs::write(&archive, traversal).unwrap();
    let error = forgepack::extract_file(&archive, &root.join("output")).unwrap_err();
    assert!(error.contains("forgepack_path_invalid"), "{error}");
    assert_eq!(fs::read_dir(root.join("output")).unwrap().count(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forgepack_workflow_evidence_is_self_contained_and_complete() {
    let report = forgepack::workflow_evidence();
    assert!(report.is_success(), "{:?}", report.gaps());
    assert_eq!(report.checks.len(), 10);
    assert!(report.checks.iter().all(|check| check.status == "pass"));
    let json = report.to_json();
    assert!(json.contains("\"schema\":\"forgentex.forgepack_evidence.v1\""));
    assert!(json.contains("\"checks_pass\":10"));
    assert!(json.contains("\"checks_fail\":0"));
    assert!(json.contains("\"id\":\"reproducible_build\""));
    assert!(json.contains("\"id\":\"corruption_denial\""));
}
