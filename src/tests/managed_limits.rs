// Managed-capsule limits: what a Python/Node/Go/JVM/.NET capsule's declared
// `limit.*` entries actually do at run time.
//
// Until this change they did nothing: managed runners launched an ordinary
// child and waited for it however long it took, and the declarations were
// carried as plan data. The probes below are capsules written to break each
// limit — spin forever, burn CPU, allocate, flood output, leave a child behind —
// and each one is required to be stopped.
//
// The probes run real Python and skip on a host without it.

use std::time::{Duration, Instant};

use crate::managed_exec::{plan, ManagedPolicy};
use crate::parser::parse_human;

fn python_capsule(sandbox: &[&str], body: &[&str]) -> crate::ast::Program {
    let mut source = String::from(
        "foreign probe python version \"3\" effects [cpu]:\n    input value: i32\n    output result: i32\n",
    );
    for rule in sandbox {
        source.push_str(&format!("    sandbox \"{rule}\"\n"));
    }
    source.push_str("    code:\n");
    for line in body {
        source.push_str(&format!("        {line}\n"));
    }
    parse_human(&source).expect("probe capsule should parse")
}

/// Run a probe, or `None` when this host has no Python to run it with.
fn run_probe(
    program: &crate::ast::Program,
    policy: ManagedPolicy,
) -> Option<Result<String, String>> {
    let capsule = program.capsules().next().unwrap();
    let result = crate::python_run::invoke_live_with(capsule, "[1]", policy);
    if let Err(err) = &result {
        if err.contains("no python") || err.contains("python3") && err.contains("not found") {
            eprintln!("skipping: {err}");
            return None;
        }
    }
    Some(result)
}

// --- The plan: decided before anything runs --------------------------------

#[test]
fn every_managed_call_gets_a_deadline_and_budgets_even_when_none_is_declared() {
    // The old behaviour was no deadline at all, so a capsule that never
    // returned hung its caller forever.
    let program = python_capsule(&["fs.none"], &["def probe(value):", "    return value"]);
    let planned = plan(program.capsules().next().unwrap(), ManagedPolicy::default()).unwrap();
    assert_eq!(planned.profile, "contained");
    assert!(planned.waived.is_empty());
    for key in ["wall_ms", "cpu_ms", "output_bytes"] {
        let entry = planned
            .enforced
            .iter()
            .find(|(limit, _, _)| limit == key)
            .unwrap_or_else(|| panic!("{key} must always be enforced"));
        assert_eq!(entry.2, "default", "{key}");
    }
}

#[test]
fn declared_limits_replace_the_defaults_and_say_so() {
    // `mem_mb` is only enforceable where a managed runtime can bound memory
    // (Windows job objects). On Unix it is refused at planning time, so the
    // declared-limit replacement is exercised with wall and cpu there, and with
    // memory additionally on Windows.
    let mut sandbox = vec!["fs.none", "limit.wall_ms 1500", "limit.cpu_ms 700"];
    if cfg!(windows) {
        sandbox.push("limit.mem_mb 64");
    }
    let program = python_capsule(&sandbox, &["def probe(value):", "    return value"]);
    let planned = plan(program.capsules().next().unwrap(), ManagedPolicy::default()).unwrap();
    assert_eq!(planned.wall, Duration::from_millis(1500));
    assert_eq!(planned.limits.cpu_ms, Some(700));
    if cfg!(windows) {
        assert_eq!(planned.limits.memory_bytes, Some(64 * 1024 * 1024));
        assert!(planned
            .enforced
            .iter()
            .any(|(key, value, source)| key == "mem_mb" && value == "64" && *source == "declared"));
    }
}

#[test]
fn a_memory_ceiling_is_only_enforced_where_it_means_memory() {
    // On Unix the only per-process tool caps address space, which V8, Go, and
    // the JVM reserve far beyond what they use — so there it is refused like
    // any other unenforceable limit rather than applied and breaking the run.
    let program = python_capsule(
        &["fs.none", "limit.mem_mb 256"],
        &["def probe(value):", "    return value"],
    );
    let result = plan(program.capsules().next().unwrap(), ManagedPolicy::default());
    if cfg!(windows) {
        assert!(result.is_ok(), "{result:?}");
    } else {
        let err = result.unwrap_err();
        assert!(err.contains("limit.mem_mb 256"), "{err}");
    }
}

#[test]
fn a_limit_that_cannot_be_enforced_is_refused_rather_than_ignored() {
    // This is the case that used to pass silently: `limit.net_bytes` on a
    // managed capsule was carried as plan data and never applied.
    let program = python_capsule(
        &["fs.none", "limit.wall_ms 1000", "limit.net_bytes 4096"],
        &["def probe(value):", "    return value"],
    );
    let err = plan(program.capsules().next().unwrap(), ManagedPolicy::default()).unwrap_err();
    assert!(err.contains("managed_limit_unenforceable"), "{err}");
    assert!(err.contains("limit.net_bytes 4096"), "{err}");
    assert!(
        err.contains("--trusted"),
        "the way out has to be named: {err}"
    );
}

#[test]
fn the_trusted_profile_waives_only_what_cannot_be_enforced_and_records_it() {
    // Trust is not "turn containment off": the enforceable limits still apply.
    let program = python_capsule(
        &[
            "fs.none",
            "limit.wall_ms 1000",
            "limit.fs_bytes 10",
            "limit.net_bytes 4096",
        ],
        &["def probe(value):", "    return value"],
    );
    let planned = plan(
        program.capsules().next().unwrap(),
        ManagedPolicy { trusted: true },
    )
    .unwrap();
    assert_eq!(planned.profile, "trusted");
    assert_eq!(
        planned.waived,
        vec![
            "limit.fs_bytes 10".to_string(),
            "limit.net_bytes 4096".to_string()
        ]
    );
    assert_eq!(
        planned.wall,
        Duration::from_millis(1000),
        "wall still enforced"
    );

    // Granting trust to a capsule with nothing to waive changes nothing.
    let plain = python_capsule(&["fs.none"], &["def probe(value):", "    return value"]);
    let planned = plan(
        plain.capsules().next().unwrap(),
        ManagedPolicy { trusted: true },
    )
    .unwrap();
    assert_eq!(planned.profile, "contained");
    assert!(planned.waived.is_empty());
}

#[test]
fn a_malformed_limit_is_rejected() {
    let program = python_capsule(
        &["fs.none", "limit.wall_ms 0"],
        &["def probe(value):", "    return value"],
    );
    let err = plan(program.capsules().next().unwrap(), ManagedPolicy::default()).unwrap_err();
    assert!(err.contains("managed_limit_invalid"), "{err}");
}

// --- Probes: capsules written to break a limit ------------------------------

#[test]
fn probe_a_capsule_that_never_returns_is_stopped_at_its_deadline() {
    let program = python_capsule(
        &["fs.none", "limit.wall_ms 1500"],
        &[
            "def probe(value):",
            "    import time",
            "    while True:",
            "        time.sleep(0.05)",
        ],
    );
    let started = Instant::now();
    let Some(result) = run_probe(&program, ManagedPolicy::default()) else {
        return;
    };
    let err = result.expect_err("an endless capsule must not return");
    assert!(err.contains("managed_capsule_timeout"), "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "stopped after {:?}, far past its 1.5 s deadline",
        started.elapsed()
    );
}

#[test]
fn probe_a_capsule_that_burns_cpu_is_stopped_by_its_cpu_budget() {
    // A generous wall, so only the CPU budget can be what stops it.
    let program = python_capsule(
        &["fs.none", "limit.cpu_ms 1000", "limit.wall_ms 30000"],
        &[
            "def probe(value):",
            "    total = 0",
            "    while True:",
            "        total += 1",
        ],
    );
    let started = Instant::now();
    let Some(result) = run_probe(&program, ManagedPolicy::default()) else {
        return;
    };
    assert!(result.is_err(), "a CPU-bound capsule must not return");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the CPU budget should fire long before the 30 s wall; took {:?}",
        started.elapsed()
    );
}

#[test]
fn probe_a_capsule_that_floods_its_output_is_stopped_at_the_bound() {
    let program = python_capsule(
        &["fs.none", "limit.output_bytes 4096"],
        &[
            "def probe(value):",
            "    import sys",
            "    sys.stdout.write('x' * 5000000)",
            "    sys.stdout.flush()",
            "    return value",
        ],
    );
    let Some(result) = run_probe(&program, ManagedPolicy::default()) else {
        return;
    };
    let err = result.expect_err("output past the bound must not be accepted");
    assert!(err.contains("output limit"), "{err}");
}

#[test]
fn probe_a_capsule_that_allocates_past_its_memory_ceiling_fails() {
    if !cfg!(windows) {
        // Refused before running on Unix; covered by the test above.
        return;
    }
    let program = python_capsule(
        &["fs.none", "limit.mem_mb 64"],
        &[
            "def probe(value):",
            "    block = bytearray(512 * 1024 * 1024)",
            "    return len(block)",
        ],
    );
    let Some(result) = run_probe(&program, ManagedPolicy::default()) else {
        return;
    };
    assert!(
        result.is_err(),
        "512 MiB under a 64 MiB ceiling must not succeed: {result:?}"
    );
}

#[test]
fn probe_a_child_started_by_the_capsule_does_not_outlive_the_call() {
    let marker_dir = std::env::temp_dir().join(format!(
        "forgentex_managed_tree_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&marker_dir);
    std::fs::create_dir_all(&marker_dir).unwrap();
    let marker = marker_dir.join("escaped.txt");
    let marker_path = marker.display().to_string().replace('\\', "/");

    // The capsule starts a grandchild that writes the marker two seconds later,
    // then returns at once. If the grandchild survives the call, the marker
    // appears.
    let spawn = format!(
        "    subprocess.Popen([sys.executable, '-c', \"import time; time.sleep(2); open('{marker_path}', 'w').write('escaped')\"])"
    );
    let program = python_capsule(
        &["fs.none"],
        &[
            "def probe(value):",
            "    import subprocess, sys",
            &spawn,
            "    return value",
        ],
    );
    let Some(result) = run_probe(&program, ManagedPolicy::default()) else {
        return;
    };
    result.expect("the capsule itself returns normally");
    std::thread::sleep(Duration::from_secs(4));
    let escaped = marker.exists();
    let _ = std::fs::remove_dir_all(&marker_dir);
    assert!(
        !escaped,
        "a process started by the capsule outlived the call and wrote its marker"
    );
}

#[test]
fn probe_the_refusal_happens_before_anything_runs() {
    // A capsule refused for an unenforceable limit must not have started: the
    // body writes a marker as its first act.
    let marker_dir = std::env::temp_dir().join(format!(
        "forgentex_managed_refused_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&marker_dir);
    std::fs::create_dir_all(&marker_dir).unwrap();
    let marker = marker_dir.join("ran.txt");
    let write = format!(
        "    open('{}', 'w').write('ran')",
        marker.display().to_string().replace('\\', "/")
    );
    let program = python_capsule(
        &["fs.none", "limit.net_bytes 1"],
        &["def probe(value):", &write, "    return value"],
    );
    let Some(result) = run_probe(&program, ManagedPolicy::default()) else {
        return;
    };
    let ran = marker.exists();
    let _ = std::fs::remove_dir_all(&marker_dir);
    assert!(result.unwrap_err().contains("managed_limit_unenforceable"));
    assert!(!ran, "a refused capsule must not run at all");
}

// --- End to end through a hosted run ----------------------------------------

#[test]
fn a_hosted_run_refuses_then_runs_trusted_and_records_what_it_waived() {
    let source = "foreign probe python version \"3\" effects [cpu]:\n    input value: i32\n    output result: i32\n    sandbox \"fs.none\"\n    sandbox \"limit.wall_ms 5000\"\n    sandbox \"limit.net_bytes 4096\"\n    code:\n        def probe(value):\n            return value + 1\n\nfn call(value: i32) -> i32:\n    return probe(value)\n";
    let program = parse_human(source).unwrap();
    let ir = crate::backend::lower_program_to_run_ir(&program);
    let input = "{ \"args\": { \"value\": 41 } }";
    let run = |trusted: bool| {
        crate::interp::run_entry_json_hosted_with_ir(
            &program,
            &ir,
            "call",
            input,
            None,
            Vec::new(),
            Vec::new(),
            crate::interp::CapsulePolicy {
                warm: true,
                managed: ManagedPolicy { trusted },
            },
        )
    };

    let refused = run(false).expect_err("net_bytes cannot be enforced, so the call is refused");
    assert!(refused.contains("capsule_limit_unenforceable"), "{refused}");

    let trusted = match run(true) {
        Ok(json) => json,
        Err(err) if err.contains("no python") => {
            eprintln!("skipping: {err}");
            return;
        }
        Err(err) => panic!("the trusted run should succeed: {err}"),
    };
    assert!(trusted.contains("\"return\": 42"), "{trusted}");
    assert!(trusted.contains("\"profile\": \"trusted\""), "{trusted}");
    assert!(
        trusted.contains("\"waived\": [\"limit.net_bytes 4096\"]"),
        "{trusted}"
    );
    // Declaring limits keeps the capsule off the warm worker, even though warm
    // workers were requested, because a shared worker cannot apply them.
    assert!(trusted.contains("\"warm\": false"), "{trusted}");
    assert!(
        trusted.contains("\"limit\": \"wall_ms\", \"value\": \"5000\", \"source\": \"declared\""),
        "{trusted}"
    );
}

#[test]
#[ignore = "stress probe; run explicitly"]
fn stress_concurrent_managed_calls_each_receive_their_reply() {
    let workers = 16;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));
    let handles = (0..workers)
        .map(|worker| {
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let program = python_capsule(
                    &["fs.none"],
                    &["def probe(value):", &format!("    return value + {worker}")],
                );
                barrier.wait();
                (0..4)
                    .map(|_| run_probe(&program, ManagedPolicy::default()))
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let mut failures = Vec::new();
    for (worker, handle) in handles.into_iter().enumerate() {
        for result in handle.join().unwrap().into_iter().flatten() {
            match result {
                Ok(out) if out.trim() == (1 + worker).to_string() => {}
                other => failures.push(format!("worker {worker}: {other:?}")),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// --- The environment a managed capsule sees --------------------------------

/// A capsule whose body reports one environment variable, in `language`.
fn env_probe_capsule(language: &str, grants: &[&str], var: &str) -> crate::ast::Program {
    let effects = if grants.is_empty() {
        "[cpu]"
    } else {
        "[cpu, env.read]"
    };
    let mut source = format!(
        "foreign probe {language} version \"1\" effects {effects}:\n    input value: i64\n    output result: str\n    sandbox \"fs.none\"\n"
    );
    for grant in grants {
        source.push_str(&format!("    sandbox \"{grant}\"\n"));
    }
    source.push_str("    code:\n");
    let body = match language {
        "python" => format!(
            "        def probe(value):\n            import os\n            return os.environ.get(\"{var}\", \"absent\")\n"
        ),
        "node" => format!(
            "        function probe(value) {{\n          return process.env.{var} || \"absent\";\n        }}\n"
        ),
        "go" => format!(
            "        func probe(value int64) string {{\n            v := os.Getenv(\"{var}\")\n            if v == \"\" {{\n                return \"absent\"\n            }}\n            return v\n        }}\n"
        ),
        "java" => format!(
            "        public final class Probe {{\n            public static String probe(long value) {{\n                String v = System.getenv(\"{var}\");\n                return v == null ? \"absent\" : v;\n            }}\n        }}\n"
        ),
        other => panic!("no probe body for {other}"),
    };
    source.push_str(&body);
    parse_human(&source).expect("env probe should parse")
}

fn invoke(language: &str, program: &crate::ast::Program) -> Result<String, String> {
    let capsule = program.capsules().next().unwrap();
    let policy = ManagedPolicy::default();
    match language {
        "python" => crate::python_run::invoke_live_with(capsule, "[1]", policy),
        "node" => crate::node_run::invoke_live_with(capsule, "[1]", policy),
        "go" => crate::go_run::invoke_live_with(capsule, "[1]", policy),
        "java" => crate::jvm_run::invoke_live_with(capsule, &["1".to_string()], policy),
        other => panic!("no runner for {other}"),
    }
}

#[test]
fn a_capsule_sees_only_the_caller_variables_it_was_granted() {
    // The caller's environment is where an API key, a cloud credential, or a CI
    // token lives. A capsule used to inherit all of it; now it sees a variable
    // only by naming it, which the checker reconciles against its `env` effect.
    std::env::set_var("FORGENTEX_SECRET_PROBE", "leaked");
    std::env::set_var("FORGENTEX_GRANTED_PROBE", "allowed");

    let ungranted = env_probe_capsule("python", &[], "FORGENTEX_SECRET_PROBE");
    let granted = env_probe_capsule(
        "python",
        &["env.read FORGENTEX_GRANTED_PROBE"],
        "FORGENTEX_GRANTED_PROBE",
    );
    let ungranted_result = invoke("python", &ungranted);
    let granted_result = invoke("python", &granted);

    std::env::remove_var("FORGENTEX_SECRET_PROBE");
    std::env::remove_var("FORGENTEX_GRANTED_PROBE");

    match ungranted_result {
        Err(err) if err.contains("no python") => {
            eprintln!("skipping: {err}");
            return;
        }
        Err(err) => panic!("the probe should run: {err}"),
        Ok(output) => assert!(
            output.contains("absent"),
            "an ungranted caller variable must not reach the capsule: {output}"
        ),
    }
    assert!(
        granted_result.unwrap().contains("allowed"),
        "a granted variable must reach the capsule"
    );
}

#[test]
fn the_plan_records_the_environment_and_what_was_granted() {
    std::env::set_var("FORGENTEX_PLAN_PROBE", "value");
    let program = env_probe_capsule("python", &["env.read FORGENTEX_PLAN_PROBE"], "X");
    let planned = plan(program.capsules().next().unwrap(), ManagedPolicy::default()).unwrap();
    std::env::remove_var("FORGENTEX_PLAN_PROBE");

    assert_eq!(
        planned.granted_env,
        vec!["FORGENTEX_PLAN_PROBE".to_string()]
    );
    assert!(planned
        .environment
        .iter()
        .any(|(name, value)| name == "FORGENTEX_PLAN_PROBE" && value == "value"));
    // PATH stays: a runtime resolves its own libraries through it, and it holds
    // no credentials.
    assert!(planned.environment.iter().any(|(name, _)| name == "PATH"));
    // Variables that redirect what a runtime loads are never inherited.
    for hijack in ["PYTHONPATH", "PYTHONHOME", "NODE_OPTIONS"] {
        assert!(
            !planned.environment.iter().any(|(name, _)| name == hijack),
            "{hijack} must not be inherited"
        );
    }
}

#[test]
fn every_managed_runtime_still_starts_with_the_scrubbed_environment() {
    // Scrubbing is only safe if each runtime keeps what it needs to run: a Go
    // build cache, the JVM's installation, and so on. Runtimes absent from this
    // host are skipped rather than assumed.
    for language in ["python", "node", "go", "java"] {
        let program = env_probe_capsule(language, &[], "FORGENTEX_NOT_SET_ANYWHERE");
        match invoke(language, &program) {
            Ok(output) => assert!(
                output.contains("absent"),
                "{language}: unexpected output {output}"
            ),
            Err(err) if err.contains("not found") || err.contains("no ") || err.contains("No ") => {
                eprintln!("skipping {language}: {err}");
            }
            Err(err) => panic!("{language} capsule failed under the scrubbed environment: {err}"),
        }
    }
}

#[test]
fn a_warm_worker_gets_the_same_explicit_environment() {
    // Warm workers are the default for live runs and outlive a single call, so
    // one that inherited the caller's environment would hold its secrets for
    // the whole run. The worker's environment is fixed at spawn from the same
    // allowlist a cold call uses.
    std::env::set_var("FORGENTEX_WARM_PROBE", "leaked");
    let program = env_probe_capsule("python", &[], "FORGENTEX_WARM_PROBE");
    let capsule = program.capsules().next().unwrap();
    let worker = crate::capsule_worker::CapsuleWorker::spawn(capsule);
    std::env::remove_var("FORGENTEX_WARM_PROBE");

    let mut worker = match worker {
        Ok(worker) => worker,
        Err(err) if err.contains("no python") => {
            eprintln!("skipping: {err}");
            return;
        }
        Err(err) => panic!("worker should spawn: {err}"),
    };
    let response = worker.call("[1]").expect("worker should answer");
    assert!(
        response.contains("absent"),
        "a warm worker must not inherit the caller's environment: {response}"
    );
}
