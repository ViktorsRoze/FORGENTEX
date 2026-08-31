use std::fs;
use std::path::Path;

use crate::c_backend::build_c_project;
use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::parse_file;

const PORTABLE_CORE: &str = include_str!("../../examples/public_0_1/portable_core.fgx");
const ARRAYS_AND_BORROWS: &str = include_str!("../../examples/public_0_1/arrays_and_borrows.fgx");
const COLLECTIONS: &str = include_str!("../../examples/public_0_1/collections.fgx");
const EXECUTION_MATRIX: &str = include_str!("../../docs/public_0_1_execution_matrix.md");

struct PublicRun<'a> {
    entry: &'a str,
    input: &'a str,
}

struct PublicFixture<'a> {
    path: &'a str,
    source: &'a str,
    runs: &'a [PublicRun<'a>],
}

#[test]
fn public_0_1_fixtures_roundtrip_check_and_execute() {
    let fixtures = [
        PublicFixture {
            path: "examples/public_0_1/portable_core.fgx",
            source: PORTABLE_CORE,
            runs: &[PublicRun {
                entry: "public_score",
                input: r#"{"args":{"limit":4},"expect":{"return":19}}"#,
            }],
        },
        PublicFixture {
            path: "examples/public_0_1/arrays_and_borrows.fgx",
            source: ARRAYS_AND_BORROWS,
            runs: &[
                PublicRun {
                    entry: "public_array_score",
                    input: r#"{"args":{"values":[4,7,9,12]},"expect":{"return":16}}"#,
                },
                PublicRun {
                    entry: "public_slice_score",
                    input: r#"{"args":{"values":[4,7,9,12]},"expect":{"return":16}}"#,
                },
            ],
        },
        PublicFixture {
            path: "examples/public_0_1/collections.fgx",
            source: COLLECTIONS,
            runs: &[PublicRun {
                entry: "public_collection_score",
                input: r#"{"args":{},"expect":{"return":14}}"#,
            }],
        },
    ];

    for fixture in fixtures {
        assert!(
            EXECUTION_MATRIX.contains(fixture.path),
            "{} is missing from the frozen execution matrix",
            fixture.path
        );
        let program = parse_file(Path::new(fixture.path), fixture.source).unwrap();
        let human = parse_file(Path::new(fixture.path), &format_human(&program)).unwrap();
        let agent_path = fixture.path.replace(".fgx", ".fgai");
        let agent = parse_file(Path::new(&agent_path), &format_agent(&program)).unwrap();
        assert!(program.canonical_eq(&human), "{} human drift", fixture.path);
        assert!(program.canonical_eq(&agent), "{} agent drift", fixture.path);

        let diagnostics = crate::typecheck::check(&program);
        assert!(
            diagnostics.is_empty(),
            "{} diagnostics: {diagnostics:?}",
            fixture.path
        );
        for run in fixture.runs {
            let report = run_entry_json(&program, run.entry, run.input).unwrap();
            assert!(
                report.contains("\"status\": \"pass\""),
                "{}::{}:\n{report}",
                fixture.path,
                run.entry
            );
        }
    }
}

#[test]
fn public_0_1_portable_core_matches_the_c11_subset() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_public_0_1_portable",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"public-core\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(root.join("src/main.fgx"), PORTABLE_CORE).unwrap();

    let artifacts = build_c_project(&root, &root.join("out-c")).unwrap();
    let generated = fs::read_to_string(&artifacts.source).unwrap();
    assert!(generated.contains("public_score"), "{generated}");
    assert!(generated.contains("while ("), "{generated}");
    assert!(generated.contains("switch ("), "{generated}");

    if let Some(toolchain) = crate::native_build::detect_toolchain() {
        let header_name = artifacts.header.file_name().unwrap().to_string_lossy();
        let harness = root.join("public_core_harness.c");
        fs::write(
            &harness,
            format!(
                "#include \"{header_name}\"\nint main(void) {{ return public_score(4) == 19 ? 0 : 1; }}\n"
            ),
        )
        .unwrap();
        let executable = root.join(if cfg!(windows) {
            "public_core_harness.exe"
        } else {
            "public_core_harness"
        });
        let argv = vec![
            toolchain.name.clone(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            artifacts.source.display().to_string(),
            harness.display().to_string(),
            "-I".to_string(),
            root.join("out-c").display().to_string(),
            "-o".to_string(),
            executable.display().to_string(),
        ];
        let compile =
            crate::native_build::run_with_path(&argv, Path::new(&toolchain.name).parent())
                .expect("failed to invoke the host C compiler");
        assert!(
            compile.status.success(),
            "public 0.1 C fixture failed to compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = std::process::Command::new(&executable)
            .output()
            .expect("failed to execute the public 0.1 C harness");
        assert!(run.status.success(), "public C result diverged");
    }

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn public_0_1_interpreter_only_collections_have_an_explicit_c_blocker() {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_public_0_1_collections",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"public-collections\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(root.join("src/main.fgx"), COLLECTIONS).unwrap();

    let error = build_c_project(&root, &root.join("out-c")).unwrap_err();
    assert!(error.contains("FIR contract blocks backend 'c'"), "{error}");
    assert!(error.contains("generic_type:Map"), "{error}");
    assert!(!root.join("out-c").exists());
    let _ = fs::remove_dir_all(&root);
}
