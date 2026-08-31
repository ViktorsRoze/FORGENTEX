// Foreign-debugger admission. The property under test: attaching a foreign
// debugger costs three guarantees the runtime otherwise holds, so it must be
// asked for explicitly, must name the environment it attaches inside, and must
// leave a record either way.

use crate::foreign_debug::{admit, debugger_for, RELAXATIONS};
use crate::parser::parse_human;

fn capsule(language: &str, env: Option<&str>) -> crate::ast::Capsule {
    let env_clause = env.map(|e| format!(" env \"{e}\"")).unwrap_or_default();
    let source = format!(
        "foreign probe {language} version \"1\"{env_clause} effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    code:\n        def probe(value):\n            return value\n"
    );
    let program = parse_human(&source).unwrap();
    let found = program.capsules().next().expect("capsule parses").clone();
    found
}

fn python_env() -> String {
    // Any real interpreter will do; the admission gate only needs the
    // environment to resolve, not to contain debugpy. Forward slashes because
    // a backslash is an escape inside a capsule's quoted `env` string.
    let exe = std::env::current_exe().unwrap();
    format!("path:{}", exe.display().to_string().replace('\\', "/"))
}

#[test]
fn debugging_is_denied_unless_it_is_explicitly_requested() {
    // The relaxations must never be applied just because a debugger exists.
    let capsule = capsule("python", Some(&python_env()));
    let admission = admit(&capsule, false);
    assert!(!admission.is_allowed());
    assert!(admission.relaxations.is_empty());
    assert!(
        admission.reason.contains("was not requested"),
        "{}",
        admission.reason
    );
}

#[test]
fn an_admitted_capsule_names_every_guarantee_it_gives_up() {
    let capsule = capsule("python", Some(&python_env()));
    let admission = admit(&capsule, true);
    assert!(admission.is_allowed(), "{}", admission.reason);
    assert_eq!(admission.debugger, Some("debugpy"));

    // Each relaxation contradicts something the runtime asserts elsewhere, so
    // none may be dropped silently from the record.
    assert_eq!(admission.relaxations, RELAXATIONS);
    for expected in [
        "loopback_listener",
        "deadlines_suspended",
        "nondeterministic",
    ] {
        assert!(
            admission.relaxations.contains(&expected),
            "missing relaxation {expected}"
        );
    }
}

#[test]
fn debugging_requires_a_resolvable_environment() {
    // Without one there is nothing definite to attach to, and guessing would
    // debug a different runtime than the one that actually runs.
    let undeclared = capsule("python", None);
    let admission = admit(&undeclared, true);
    assert!(!admission.is_allowed());
    assert!(
        admission.reason.contains("no resolvable environment"),
        "{}",
        admission.reason
    );

    // A descriptive label is not an environment either.
    let labelled = capsule("python", Some("cpython-3.13"));
    assert!(!admit(&labelled, true).is_allowed());

    // A named environment that does not exist is refused, not guessed at.
    let missing = capsule("python", Some("conda:forgentex-no-such-env"));
    let refused = admit(&missing, true);
    assert!(!refused.is_allowed());
    assert!(refused.reason.contains("conda:"), "{}", refused.reason);
}

#[test]
fn only_languages_whose_debugger_speaks_dap_are_admitted() {
    // The approach is cheap precisely because the child already speaks DAP.
    assert_eq!(debugger_for("python"), Some("debugpy"));
    assert_eq!(debugger_for("node"), Some("node --inspect-brk"));
    assert_eq!(debugger_for("typescript"), Some("node --inspect-brk"));
    assert_eq!(debugger_for("go"), Some("dlv dap"));

    // Compiled capsules need a gdb/lldb bridge and a much sharper containment
    // decision, so they are not admitted by this gate.
    for language in ["c", "cpp", "rust", "zig"] {
        assert_eq!(debugger_for(language), None);
        let admission = admit(&capsule(language, Some(&python_env())), true);
        assert!(!admission.is_allowed(), "{language} must not be admitted");
        assert!(
            admission.reason.contains("gdb/lldb"),
            "{}",
            admission.reason
        );
    }
}

#[test]
fn the_decision_is_recorded_whether_or_not_it_was_admitted() {
    // A debug run has to be identifiable as one after the fact.
    let capsule = capsule("python", Some(&python_env()));

    let allowed = admit(&capsule, true).to_json();
    assert!(allowed.contains("\"schema\": \"forgentex.foreign_debug.v1\""));
    assert!(allowed.contains("\"decision\": \"allow\""));
    assert!(allowed.contains("loopback_listener"));
    assert!(allowed.contains("\"debugger\": \"debugpy\""));

    let denied = admit(&capsule, false).to_json();
    assert!(denied.contains("\"decision\": \"deny\""));
    assert!(denied.contains("\"relaxations\": []"));
    assert!(denied.contains("\"debugger\": null"));
}

// --- Attaching for real -----------------------------------------------------

/// A conda environment with debugpy, or `None`. Keeps the attach test
/// host-independent: it verifies real delegation where possible and skips
/// cleanly where debugpy is absent.
fn env_with_debugpy() -> Option<(String, std::path::PathBuf)> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(std::path::PathBuf::from)?;
    let registry = std::fs::read_to_string(home.join(".conda").join("environments.txt")).ok()?;
    for line in registry.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let prefix = std::path::Path::new(line);
        let interpreter = if cfg!(windows) {
            prefix.join("python.exe")
        } else {
            prefix.join("bin").join("python")
        };
        if !interpreter.is_file() {
            continue;
        }
        let has = std::process::Command::new(&interpreter)
            .args(["-c", "import debugpy"])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if has {
            let name = prefix.file_name()?.to_str()?.to_string();
            return Some((name, interpreter));
        }
    }
    None
}

#[test]
fn a_python_capsule_runs_under_its_own_debugger_and_stops_at_a_breakpoint() {
    let Some((env_name, interpreter)) = env_with_debugpy() else {
        eprintln!("skipping: no conda environment with debugpy on this host");
        return;
    };

    let capsule = capsule("python", Some(&format!("conda:{env_name}")));
    let admission = admit(&capsule, true);
    assert!(admission.is_allowed(), "{}", admission.reason);

    // Two statements so a breakpoint on the second proves the debugger stopped
    // where asked rather than merely at entry.
    let body = "def probe(value):\n    doubled = value * 2\n    return doubled + 1\n";
    let mut attached = crate::foreign_debug::attach_python(
        &admission,
        &interpreter,
        body,
        "probe",
        "[21]",
    )
    .expect("debugpy should attach");

    attached
        .request("initialize", "{\"adapterID\":\"forgentex\"}")
        .unwrap();
    attached
        .read_until(|m| m.contains("\"event\":\"initialized\""))
        .unwrap()
        .expect("initialized event");

    // Line 3 is `return doubled + 1`, after `doubled` is bound.
    // JSON-escape the path: a Windows script path is full of backslashes.
    let script = attached.script.display().to_string().replace('\\', "\\\\");
    attached
        .request(
            "setBreakpoints",
            &format!(
                "{{\"source\":{{\"path\":\"{script}\"}},\"breakpoints\":[{{\"line\":3}}]}}"
            ),
        )
        .unwrap();
    attached
        .read_until(|m| m.contains("\"command\":\"setBreakpoints\""))
        .unwrap()
        .expect("setBreakpoints response");

    attached.request("configurationDone", "{}").unwrap();
    let stopped = attached
        .read_until(|m| m.contains("\"event\":\"stopped\""))
        .unwrap()
        .expect("the capsule should stop at the breakpoint");
    assert!(stopped.contains("breakpoint"), "{stopped}");

    // The stack must be inside the capsule body, and the local bound by the
    // preceding line must be visible — which is what stepping into foreign
    // code is for.
    attached.request("stackTrace", "{\"threadId\":1}").unwrap();
    let stack = attached
        .read_until(|m| m.contains("\"command\":\"stackTrace\""))
        .unwrap()
        .expect("stackTrace response");
    assert!(stack.contains("probe"), "stack should be inside the capsule: {stack}");

    attached.request("scopes", "{\"frameId\":1}").unwrap();
    let scopes = attached
        .read_until(|m| m.contains("\"command\":\"scopes\""))
        .unwrap()
        .expect("scopes response");
    let reference = crate::dap::number_field(&scopes, "variablesReference")
        .expect("a variables reference");
    attached
        .request(
            "variables",
            &format!("{{\"variablesReference\":{reference}}}"),
        )
        .unwrap();
    let variables = attached
        .read_until(|m| m.contains("\"command\":\"variables\""))
        .unwrap()
        .expect("variables response");
    assert!(
        variables.contains("doubled") && variables.contains("42"),
        "the capsule's own local should be readable: {variables}"
    );
}
