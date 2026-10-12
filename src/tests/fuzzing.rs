use std::path::PathBuf;

use crate::fuzzing::{run_backend_differential, run_fuzz_suite, BackendDifferentialOptions};

#[test]
fn fuzz_suite_covers_roundtrip_patch_no_panic_and_target_invariants() {
    let report = run_fuzz_suite(6);
    assert!(report.is_success(), "{}", report.to_text());
    assert_eq!(report.failed(), 0);
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.fuzz.v1\""));
    assert!(report
        .cases
        .iter()
        .any(|case| case.category == "roundtrip-generated"));
    assert!(report
        .cases
        .iter()
        .any(|case| case.category == "patch-format-check"));
    assert!(report
        .cases
        .iter()
        .any(|case| case.category == "target-validation"));
}

#[test]
fn backend_differential_runner_links_interpreter_c_backend_and_fgir() {
    let options = BackendDifferentialOptions {
        project: PathBuf::from("."),
        source: PathBuf::from("examples/media/audio_mix.fgx"),
        entry: "mix_samples".to_string(),
        input: PathBuf::from("examples/run/audio_mix.json"),
    };
    let report = run_backend_differential(&options).expect("differential report should run");
    assert!(report.is_success(), "{}", report.to_text());
    assert_eq!(report.interpreter_status, "pass");
    assert!(report.backend_entry_found);
    assert!(report.ir_entry_found);
    assert!(report.generated_header_bytes > 0);
    assert!(report.generated_source_bytes > 0);
    assert!(report
        .to_json()
        .contains("\"schema\": \"forgentex.backend_differential.v1\""));
}

#[test]
fn backend_differential_executes_the_generated_c_and_compares_results() {
    // The point of the command's name. Before this, the "differential" asserted
    // that the entry's *name* appeared in the generated text, which cannot tell
    // correct C from C that computes a different answer.
    let options = BackendDifferentialOptions::default();
    let report = run_backend_differential(&options).expect("differential report should run");
    if crate::native_build::detect_toolchain().is_none() {
        assert_eq!(
            report.native_execution,
            "unavailable",
            "{}",
            report.to_text()
        );
        assert!(report.is_success(), "an absent toolchain is not a failure");
        return;
    }
    assert_eq!(report.native_execution, "matched", "{}", report.to_text());
    assert!(report.is_success(), "{}", report.to_text());
    assert!(report
        .to_json()
        .contains("\"native_execution\": \"matched\""));
    assert!(report
        .notes
        .iter()
        .any(|note| note.contains("compiled, ran, and agreed")));
}

#[test]
fn the_backend_differential_notices_when_the_two_sides_disagree() {
    // A comparison that has never been shown to fail is not evidence. Feed the
    // comparison a doctored interpreter result and require it to object, so the
    // passing case above means the answers actually matched.
    if crate::native_build::detect_toolchain().is_none() {
        eprintln!("skipping: no host C toolchain");
        return;
    }
    let options = BackendDifferentialOptions::default();
    let source = std::fs::read_to_string(&options.source).unwrap();
    let input = std::fs::read_to_string(&options.input).unwrap();
    let program = crate::parser::parse_file(&options.source, &source).unwrap();
    let (header, generated) = crate::c_backend::generate_c_for_project(&options.project).unwrap();
    let (args, return_value, bindings) =
        crate::interp::run_entry_values(&program, &options.entry, &input).unwrap();
    let function = program.find_function(&options.entry).unwrap();

    let honest = crate::backend_diff::compare_generated_c(
        function,
        &header,
        &generated,
        &args,
        &return_value,
        &bindings,
    );
    assert_eq!(honest.status(), "matched", "{}", honest.note());

    // Change one sample of the expected output. Everything else is identical,
    // so only the comparison itself can catch it.
    let mut doctored = bindings.clone();
    let Some(crate::interp::Value::Array(samples)) = doctored.get_mut("dst") else {
        panic!("the entry's mutable slice binding should be an array");
    };
    let crate::interp::Value::Int(first) = &mut samples[0] else {
        panic!("samples should be integers");
    };
    *first += 1;

    let doctored = crate::backend_diff::compare_generated_c(
        function,
        &header,
        &generated,
        &args,
        &return_value,
        &doctored,
    );
    assert_eq!(doctored.status(), "mismatched", "{}", doctored.note());
    assert!(!doctored.permits_pass());
}

#[test]
fn the_generated_header_compiles_on_its_own() {
    // A public function whose signature named a non-public struct produced a
    // header that declared the prototype but never defined the type, so any
    // consumer including the header failed with `unknown type name`. The
    // header is the backend's published interface, so it has to stand alone.
    let Some(toolchain) = crate::native_build::detect_toolchain() else {
        eprintln!("skipping: no host C toolchain");
        return;
    };
    let (header, source) =
        crate::c_backend::generate_c_for_project(std::path::Path::new(".")).unwrap();
    let include = source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("#include \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .expect("the generated source includes its own header");

    let root = std::env::temp_dir().join(format!(
        "forgentex_header_only_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(include), &header).unwrap();
    let unit = root.join("include_only.c");
    std::fs::write(
        &unit,
        format!(
            "#include \"{include}\"
"
        ),
    )
    .unwrap();

    let argv = vec![
        toolchain.name.clone(),
        "-std=c11".to_string(),
        "-c".to_string(),
        unit.display().to_string(),
        "-I".to_string(),
        root.display().to_string(),
        "-o".to_string(),
        root.join("include_only.o").display().to_string(),
    ];
    let compiled =
        crate::native_build::run_with_path(&argv, std::path::Path::new(&toolchain.name).parent())
            .expect("failed to invoke the host C compiler");
    assert!(
        compiled.status.success(),
        "the generated header does not compile on its own:
{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let _ = std::fs::remove_dir_all(&root);
}
