// rt.3 slice 1 — warm capsule workers (docs/rt3_warm_workers_plan.md).
// "Warm" is proven deterministically, not by timing: a capsule that returns
// os.getpid() sees ONE process across calls in a warm run, and different
// processes in a cold run. Timings are printed for the record, not asserted.

use crate::interp::{run_entry_with_capsules, run_entry_with_warm_capsules, Value};
use crate::parser::parse_human;

const PID_PROBE: &str = "foreign worker_pid python version \"3.13\" effects [alloc]:\n    output pid: i64\n    sandbox \"fs.none\"\n    code:\n        def worker_pid():\n            import os\n            return os.getpid()\n\nfn probe() -> bool:\n    let a: i64 = worker_pid()\n    let b: i64 = worker_pid()\n    return a == b\n";

const CELSIUS: &str = "foreign to_fahrenheit python version \"3.13\" effects [alloc]:\n    input celsius: f64\n    output fahrenheit: f64\n    sandbox \"fs.none\"\n    code:\n        def to_fahrenheit(celsius):\n            return celsius * 9.0 / 5.0 + 32.0\n\nfn convert(c: f64) -> f64:\n    return to_fahrenheit(c)\n";

const BOOM: &str = "foreign boom python version \"3.13\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        def boom(x):\n            return 10 // x\n";

#[test]
fn warm_calls_share_one_process_cold_calls_do_not() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    let program = parse_human(PID_PROBE).unwrap();

    // Warm: both capsule calls land in the same persistent worker.
    let warm_started = std::time::Instant::now();
    let (value, calls, err) = run_entry_with_warm_capsules(&program, "probe", Vec::new());
    let warm_elapsed = warm_started.elapsed();
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Bool(true)), "warm calls saw one pid");
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| call.decision == "allow"));

    // Cold: each call spawns a fresh process, so the pids differ.
    let cold_started = std::time::Instant::now();
    let (value, _calls, err) = run_entry_with_capsules(&program, "probe", Vec::new());
    let cold_elapsed = cold_started.elapsed();
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Bool(false)), "cold calls saw two pids");

    // For the record (not asserted — timing is machine-dependent):
    eprintln!(
        "rt.3 measurement: 2 capsule calls warm={warm_elapsed:?} (incl. worker spawn) vs cold={cold_elapsed:?}"
    );
}

#[test]
fn warm_worker_survives_a_capsule_exception() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    // Protocol-level proof: a raising call returns {"err": …} and the SAME
    // worker still serves the next call — a bad input must not kill warmth.
    let program = parse_human(BOOM).unwrap();
    let capsule = program.capsules().next().expect("boom capsule");
    let mut worker = crate::capsule_worker::PythonWorker::spawn(capsule).unwrap();

    let raised = worker.call("[0]").unwrap();
    assert!(
        raised.contains("\"err\""),
        "division by zero raises: {raised}"
    );

    let ok = worker.call("[2]").unwrap();
    assert!(
        ok.contains("\"ok\": 5"),
        "worker survived and computed: {ok}"
    );
}

#[test]
fn warm_results_match_cold_results() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    let program = parse_human(CELSIUS).unwrap();
    let (value, calls, err) =
        run_entry_with_warm_capsules(&program, "convert", vec![Value::Float(100.0)]);
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Float(212.0)));
    assert_eq!(calls[0].decision, "allow");
    assert_eq!(calls[0].output, "212.0");
}

#[test]
fn warm_capsule_exception_traps_but_is_audited() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    // Through the interpreter: the capsule's exception surfaces as the usual
    // capsule_call_failed trap, with the error recorded in the transcript.
    let src = format!("{BOOM}\nfn f(x: i64) -> i64:\n    return boom(x)\n");
    let program = parse_human(&src).unwrap();
    let (value, calls, err) = run_entry_with_warm_capsules(&program, "f", vec![Value::Int(0)]);
    assert!(value.is_none());
    let err = err.expect("expected a trap");
    assert!(err.contains("capsule_call_failed"), "got: {err}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].decision, "error");
}

// --- rt.3 slices 2-3: node and go workers, same protocol ---

const NODE_PID: &str = "foreign node_pid node version \"node-24\" effects [alloc]:\n    output pid: i64\n    sandbox \"fs.none\"\n    code:\n        export function node_pid() {\n            return process.pid;\n        }\n\nfn probe() -> bool:\n    let a: i64 = node_pid()\n    let b: i64 = node_pid()\n    return a == b\n";

const GO_PID: &str = "foreign go_pid go version \"1.26\" effects [alloc]:\n    output pid: i64\n    sandbox \"fs.none\"\n    code:\n        func go_pid() int64 {\n            return int64(os.Getpid())\n        }\n\nfn probe() -> bool:\n    let a: i64 = go_pid()\n    let b: i64 = go_pid()\n    return a == b\n";

const GO_BOOM: &str = "foreign go_boom go version \"1.26\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        func go_boom(x int64) int64 {\n            return 10 / x\n        }\n";

#[test]
fn warm_node_calls_share_one_process() {
    if crate::node_run::detect_node().is_none() {
        return;
    }
    let program = parse_human(NODE_PID).unwrap();
    let (value, calls, err) = run_entry_with_warm_capsules(&program, "probe", Vec::new());
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(
        value,
        Some(Value::Bool(true)),
        "warm node calls saw one pid"
    );
    assert_eq!(calls.len(), 2);

    // Cold comparison: separate processes, different pids.
    let (value, _calls, err) = run_entry_with_capsules(&program, "probe", Vec::new());
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(
        value,
        Some(Value::Bool(false)),
        "cold node calls saw two pids"
    );
}

#[test]
fn warm_go_calls_share_one_compiled_process() {
    if crate::go_run::detect_go().is_none() {
        return;
    }
    // For Go the warm win is double: one process AND one compile. (No cold
    // comparison here: the cold driver only imports what its generated main
    // needs, so an os.Getpid body cannot compile cold — the warm driver always
    // provides os/fmt/json/bufio.)
    let started = std::time::Instant::now();
    let program = parse_human(GO_PID).unwrap();
    let (value, calls, err) = run_entry_with_warm_capsules(&program, "probe", Vec::new());
    let elapsed = started.elapsed();
    assert!(err.is_none(), "unexpected error: {err:?}");
    assert_eq!(value, Some(Value::Bool(true)), "warm go calls saw one pid");
    assert_eq!(calls.len(), 2);
    eprintln!("rt.3 go: 2 warm calls (incl. one compile) took {elapsed:?}");
}

#[test]
fn warm_go_worker_survives_a_panic() {
    if crate::go_run::detect_go().is_none() {
        return;
    }
    // Integer division by zero panics in Go; recover() must turn it into an
    // {"err": …} response and the SAME worker must serve the next call.
    let program = parse_human(GO_BOOM).unwrap();
    let capsule = program.capsules().next().expect("go_boom capsule");
    let mut worker = crate::capsule_worker::CapsuleWorker::spawn(capsule).unwrap();

    let raised = worker.call("[0]").unwrap();
    assert!(
        raised.contains("\"err\""),
        "panic became an error: {raised}"
    );

    let ok = worker.call("[2]").unwrap();
    assert!(ok.contains("\"ok\":5"), "worker survived the panic: {ok}");
}

// --- rt.3 slice 4: auto-respawn ---

// Exits the whole worker process on x == 0 (a transport death, not a capsule
// error); returns x * 2 otherwise.
const DIE_ON_ZERO: &str = "foreign die_on_zero python version \"3.13\" effects [alloc]:\n    input x: i64\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        def die_on_zero(x):\n            import sys\n            if x == 0:\n                sys.exit(1)\n            return x * 2\n";

#[test]
fn dead_worker_is_respawned_for_the_next_call() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    // A deterministic death: the call kills the worker, the retry (same args)
    // kills the replacement too, so the CALL fails — but the pool now holds a
    // fresh worker, and the next call with good args succeeds warm.
    let program = parse_human(DIE_ON_ZERO).unwrap();
    let capsule = program.capsules().next().expect("capsule");
    let host = crate::interp::CapsuleHost::new().with_warm(true);

    let err = host.warm_call(capsule, "[0]").unwrap_err();
    assert!(
        err.contains("after respawn"),
        "the retry was attempted: {err}"
    );

    let ok = host.warm_call(capsule, "[21]").unwrap();
    assert!(ok.contains("\"ok\": 42"), "fresh worker serves: {ok}");
}

#[test]
fn transient_death_is_healed_by_the_single_retry() {
    if crate::python_run::detect_python().is_none() {
        return;
    }
    // Die-once semantics via a marker file: the first attempt writes the
    // marker and exits (transport death); the auto-respawned retry sees the
    // marker and answers. The CALLER observes one successful call.
    let marker = std::env::temp_dir().join(format!(
        "forgentex_{}_respawn_marker.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let capsule_src = "foreign die_once python version \"3.13\" effects [alloc]:\n    input p: str\n    output y: i64\n    sandbox \"fs.none\"\n    code:\n        def die_once(p):\n            import os, sys\n            if not os.path.exists(p):\n                open(p, 'w').write('x')\n                sys.exit(1)\n            return 7\n";
    let program = parse_human(capsule_src).unwrap();
    let capsule = program.capsules().next().expect("capsule");
    let host = crate::interp::CapsuleHost::new().with_warm(true);

    let path_json = format!("[{:?}]", marker.to_string_lossy().replace('\\', "/"));
    let ok = host.warm_call(capsule, &path_json).unwrap();
    assert!(ok.contains("\"ok\": 7"), "retry healed the call: {ok}");
    let _ = std::fs::remove_file(&marker);
}
