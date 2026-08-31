// Declared capsule environments. The contract these tests pin down is that a
// capsule naming an environment either runs in that environment or fails —
// never quietly runs somewhere else.

use crate::capsule_env::resolve;

fn scratch(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("forgentex_env_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// Build a fake environment with an interpreter where the host's own layout
/// would put one, so the test does not depend on a real Python being installed.
fn fake_env(root: &std::path::Path, binary: &str) -> std::path::PathBuf {
    let (dir, name) = if cfg!(windows) {
        (root.join("Scripts"), format!("{binary}.exe"))
    } else {
        (root.join("bin"), binary.to_string())
    };
    std::fs::create_dir_all(&dir).unwrap();
    let interpreter = dir.join(name);
    std::fs::write(&interpreter, b"#!/bin/sh\n").unwrap();
    interpreter
}

#[test]
fn no_declared_env_keeps_the_ambient_runtime() {
    // A capsule without `env` must behave exactly as before.
    assert_eq!(resolve(None, "python").unwrap(), None);
    assert_eq!(resolve(Some("   "), "python").unwrap(), None);
}

#[test]
fn descriptive_runtime_labels_remain_metadata() {
    // `env` has carried labels like these since v2.0. Adding runtime selection
    // must not change what an existing capsule does, so a label without a
    // scheme resolves to nothing and the ambient interpreter is used.
    for label in [
        "cpython-3.13",
        "cpython-3.13-linux-x86_64",
        "temurin-21",
        "cuda-12.4-sm_80",
        "evm-shanghai",
        "x86_64-pc-windows-msvc",
    ] {
        assert_eq!(
            resolve(Some(label), "python").unwrap(),
            None,
            "{label} must stay descriptive metadata"
        );
    }
}

#[test]
fn a_venv_resolves_to_the_interpreter_it_contains() {
    let root = scratch("venv");
    let interpreter = fake_env(&root, "python");
    let resolved = resolve(Some(&format!("venv:{}", root.display())), "python")
        .unwrap()
        .expect("declared env resolves");
    assert_eq!(
        std::path::Path::new(&resolved.interpreter),
        interpreter.as_path()
    );
    assert_eq!(resolved.source, "declared");
    assert_eq!(
        resolved.declared.as_deref(),
        Some(&*format!("venv:{}", root.display()))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_path_env_names_the_interpreter_outright() {
    let root = scratch("path");
    let interpreter = fake_env(&root, "python");
    let resolved = resolve(Some(&format!("path:{}", interpreter.display())), "python")
        .unwrap()
        .expect("declared env resolves");
    assert_eq!(
        std::path::Path::new(&resolved.interpreter),
        interpreter.as_path()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_missing_environment_fails_rather_than_falling_back() {
    // The whole point of declaring an environment is that the capsule does not
    // run in a different one. Every failure here must be an error, never a
    // silent fallback to the ambient interpreter.
    let missing = scratch("absent");
    let venv = resolve(Some(&format!("venv:{}", missing.display())), "python").unwrap_err();
    assert!(venv.contains("has no python interpreter"), "{venv}");

    let path = resolve(
        Some(&format!("path:{}", missing.join("python").display())),
        "python",
    )
    .unwrap_err();
    assert!(path.contains("does not name an existing file"), "{path}");

    let conda = resolve(Some("conda:forgentex-no-such-env"), "python").unwrap_err();
    assert!(conda.contains("conda:forgentex-no-such-env"), "{conda}");
}

#[test]
fn an_unsupported_spec_is_rejected_with_the_shapes_that_work() {
    let unknown = resolve(Some("npm:whatever"), "python").unwrap_err();
    assert!(
        unknown.contains("expected conda:<name>, venv:<path>, or path:<file>"),
        "{unknown}"
    );

    // A colon-shaped spec with an unknown scheme is a typo worth catching:
    // `konda:sound` must not silently do nothing.
    let typo = resolve(Some("konda:sound"), "python").unwrap_err();
    assert!(typo.contains("is not supported"), "{typo}");

    let empty = resolve(Some("conda:"), "python").unwrap_err();
    assert!(empty.contains("names no environment"), "{empty}");
}

#[test]
fn each_language_looks_for_its_own_interpreter() {
    let root = scratch("langs");
    fake_env(&root, "node");
    let spec = format!("venv:{}", root.display());

    let node = resolve(Some(&spec), "node")
        .unwrap()
        .expect("node resolves");
    assert!(node.interpreter.contains("node"), "{node:?}");
    // TypeScript capsules run on the same runtime.
    assert!(resolve(Some(&spec), "typescript").unwrap().is_some());

    // The same directory has no Python, so a Python capsule must fail here.
    let python = resolve(Some(&spec), "python").unwrap_err();
    assert!(python.contains("has no python interpreter"), "{python}");

    // A language with no managed runtime is rejected outright.
    let c = resolve(Some(&spec), "c").unwrap_err();
    assert!(c.contains("not supported for 'c' capsules"), "{c}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn conda_environments_are_found_through_the_registry_not_guessed_root_names() {
    // A machine can have several conda roots with unguessable names. Probing a
    // fixed list of directory names finds only the roots you thought of and
    // reports a plainly-existing environment as missing, which is exactly the
    // failure this test exists to prevent.
    //
    // The check is conditional on a real registry so it stays host-independent:
    // where one exists, every environment it lists must resolve by name.
    let Some(home) = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(std::path::PathBuf::from)
    else {
        return;
    };
    let registry = home.join(".conda").join("environments.txt");
    let Ok(text) = std::fs::read_to_string(&registry) else {
        eprintln!("skipping: no conda registry at {}", registry.display());
        return;
    };

    let mut checked = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let prefix = std::path::Path::new(line);
        if !prefix.is_dir() {
            continue;
        }
        // Only environments that actually contain a Python are testable.
        let interpreter = if cfg!(windows) {
            prefix.join("python.exe")
        } else {
            prefix.join("bin").join("python")
        };
        if !interpreter.is_file() {
            continue;
        }
        let Some(name) = prefix.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let resolved = resolve(Some(&format!("conda:{name}")), "python")
            .unwrap_or_else(|err| panic!("registered env '{name}' failed to resolve: {err}"))
            .unwrap_or_else(|| panic!("registered env '{name}' resolved to nothing"));
        assert!(
            std::path::Path::new(&resolved.interpreter).is_file(),
            "env '{name}' resolved to a missing interpreter: {}",
            resolved.interpreter
        );
        checked += 1;
    }
    eprintln!("resolved {checked} registered conda environments");
}
