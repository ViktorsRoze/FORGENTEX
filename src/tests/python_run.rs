use crate::python_run::run;

use super::parse_fixture;

#[test]
fn celsius_capsule_runs_on_real_python_or_degrades() {
    let program = parse_fixture("examples/capsule/celsius_python.fgx");
    let report = run("celsius_python.fgx", &program, None).unwrap();

    assert_eq!(report.schema, "forgentex.python_run.v1");
    assert_eq!(report.capsule, "to_fahrenheit");
    assert_eq!(report.entry.as_deref(), Some("to_fahrenheit"));
    assert_eq!(report.total, 3);
    assert!(report.boundary_tokens > 0 && report.body_tokens > 0);

    // With a host python the fixtures are real oracles; without one the run
    // degrades gracefully. Both are a success for Forgentex.
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "ok" {
        assert_eq!(report.passed, 3, "all fixtures should pass on real python");
        assert!(report.runtime.is_some());
        for case in &report.cases {
            assert!(case.pass, "fixture failed: {case:?}");
        }
    } else {
        assert_eq!(report.status, "runtime_unavailable");
    }
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/capsule/celsius_python.fgx");
    let json = run("celsius_python.fgx", &program, None).unwrap().to_json();
    assert!(json.contains("\"schema\": \"forgentex.python_run.v1\""));
    assert!(json.contains("\"boundary_tokens\""));
    assert!(json.contains("\"entry\""));
}

#[test]
fn boundary_is_cheaper_than_body() {
    // The agent-visible surface (boundary card) should never cost more than
    // the verbatim body — the whole point of the special relationship.
    let program = parse_fixture("examples/capsule/normalize_python.fgx");
    let report = run("normalize_python.fgx", &program, None).unwrap();
    assert!(report.boundary_tokens <= report.body_tokens);
}

#[test]
fn a_managed_python_capsule_is_checked_but_not_confined_on_the_filesystem() {
    // Pins a row of docs/enforcement_matrix.md. A managed capsule's declared
    // effects are checked against its sandbox before it runs, but nothing at
    // the OS level stops a body that declares no filesystem access from reading
    // a file — it runs with its user's authority. When managed runners gain
    // filesystem confinement this test is expected to fail; update the matrix
    // in the same change.
    //
    // The caller's *environment* is a different matter and no longer leaks: the
    // capsule is given an explicit set of variables, so an ungranted one reads
    // as absent. That half is asserted here too, so the two cannot drift.
    let secret_dir = std::env::temp_dir().join(format!(
        "forgentex_unconfined_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&secret_dir).unwrap();
    let secret = secret_dir.join("outside_the_capsule.txt");
    std::fs::write(&secret, "not declared anywhere").unwrap();
    let path = secret.display().to_string().replace('\\', "/");

    let source = format!(
        "foreign peek python version \"3\" effects [cpu]:\n    input value: i32\n    output result: str\n    sandbox \"fs.none\"\n    code:\n        def peek(value):\n            import os\n            with open(\"{path}\") as handle:\n                text = handle.read()\n            return text + \"|\" + os.environ.get(\"FORGENTEX_MATRIX_PROBE\", \"absent\")\n"
    );
    let program = crate::parser::parse_human(&source).unwrap();
    let capsule = program.capsules().next().unwrap();
    assert!(
        crate::sandbox::is_approved(capsule),
        "the declaration itself must pass the checker, or this proves nothing"
    );

    std::env::set_var("FORGENTEX_MATRIX_PROBE", "inherited");
    let result = crate::python_run::invoke_live(capsule, "[1]");
    std::env::remove_var("FORGENTEX_MATRIX_PROBE");
    let _ = std::fs::remove_dir_all(&secret_dir);

    let output = match result {
        Ok(output) => output,
        Err(err) if err.contains("python") && err.contains("not") => {
            eprintln!("skipping: no Python interpreter ({err})");
            return;
        }
        Err(err) => panic!("unexpected failure: {err}"),
    };
    assert!(
        output.contains("not declared anywhere"),
        "the fs.none capsule read a file outside its declaration: {output}"
    );
    assert!(
        output.contains("absent") && !output.contains("inherited"),
        "an ungranted caller variable must not reach the capsule: {output}"
    );
}
