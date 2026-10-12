// The wire protocol between Forgentex and a managed capsule.
//
// Two defects these lock down, both found by probing a running capsule rather
// than by reading the code:
//
// 1. A capsule whose body printed anything returned that print as its result,
//    so one `print()` — a deprecation warning, a progress bar, a debug line a
//    library ships — turned a working capsule into "invalid JSON".
// 2. Arguments travelled on argv, which caps a payload at the platform's
//    command-line length. Because a capsule that declares any `limit.*` is not
//    eligible for a warm worker, declaring a limit silently capped its input at
//    about 32 KB, and the failure surfaced as an opaque OS message. Asking for
//    containment should not cost capacity.

use crate::interp::Value;
use crate::interp::{run_entry_with_capsules, run_entry_with_warm_capsules};
use crate::managed_protocol::{nonce, split, MAX_DIAGNOSTIC_BYTES};
use crate::parser::parse_human;

/// A capsule that prints before returning, which is what real libraries do.
const NOISY_PYTHON: &str = "\
foreign noisy python version \"3.13\" env \"cpython-3.13\" effects [alloc]:
    input celsius: f64
    output fahrenheit: f64
    sandbox \"fs.none\"
    code:
        import sys
        print(\"a banner on stdout, as libraries print\")
        print(\"and a warning on stderr\", file=sys.stderr)

        def to_fahrenheit(celsius):
            print(\"converting %s\" % celsius)
            return celsius * 9.0 / 5.0 + 32.0

fn convert(celsius: f64) -> f64:
    return noisy(celsius)
";

/// The same capsule with a declared limit, which forces the cold path.
const MEASURE_PYTHON_WITH_LIMIT: &str = "\
foreign measure python version \"3.13\" env \"cpython-3.13\" effects [alloc]:
    input payload: str
    output size: i64
    sandbox \"fs.none\"
    sandbox \"limit.wall_ms 30000\"
    code:
        def measure(payload):
            return len(payload)

fn size_of(payload: str) -> i64:
    return measure(payload)
";

fn python_available() -> bool {
    crate::python_run::detect_python().is_some()
}

#[test]
fn a_capsule_that_prints_still_returns_its_result() {
    if !python_available() {
        return;
    }
    let program = parse_human(NOISY_PYTHON).unwrap();

    // Warm: the worker survives the printing and answers the call.
    let (value, calls, err) =
        run_entry_with_warm_capsules(&program, "convert", vec![Value::Float(100.0)]);
    assert!(err.is_none(), "warm call failed: {err:?}");
    assert_eq!(value, Some(Value::Float(212.0)));
    assert_eq!(calls.len(), 1);

    // Cold: the same, through a fresh process per call.
    let (value, _calls, err) =
        run_entry_with_capsules(&program, "convert", vec![Value::Float(100.0)]);
    assert!(err.is_none(), "cold call failed: {err:?}");
    assert_eq!(value, Some(Value::Float(212.0)));
}

#[test]
fn a_declared_limit_does_not_cap_a_capsule_payload() {
    if !python_available() {
        return;
    }
    // Above any platform command-line limit: this is the case that used to fail
    // with "The filename or extension is too long".
    let payload = "x".repeat(100_000);
    let program = parse_human(MEASURE_PYTHON_WITH_LIMIT).unwrap();
    let (value, calls, err) =
        run_entry_with_capsules(&program, "size_of", vec![Value::String(payload)]);
    assert!(err.is_none(), "large payload failed: {err:?}");
    assert_eq!(value, Some(Value::Int(100_000)));
    assert_eq!(calls.len(), 1);
}

#[test]
fn the_result_is_the_tagged_line_and_everything_else_is_diagnostics() {
    let tag = "fgxdeadbeefdeadbeef";
    let framed = split(
        &format!(
            "a banner\n{tag} D printed by the capsule\n{tag} R {{\"ok\": 42}}\ntrailing chatter\n"
        ),
        "a warning on the error stream\n",
        tag,
    );
    assert_eq!(framed.result.as_deref(), Some("{\"ok\": 42}"));
    assert!(framed.diagnostics.contains("a banner"));
    assert!(
        framed.diagnostics.contains("printed by the capsule"),
        "a tagged diagnostic line is the capsule's own output: {framed:?}"
    );
    assert!(framed.diagnostics.contains("trailing chatter"));
    assert!(framed.diagnostics.contains("a warning on the error stream"));
}

#[test]
fn guest_output_cannot_pose_as_a_result() {
    // Redirection covers a language's own print facility; it cannot cover a C
    // extension or a subprocess writing to the descriptor. The tag is what makes
    // that output harmless, so output carrying someone else's tag — or echoing
    // the shape of a result — must stay diagnostics.
    let tag = nonce();
    let other = nonce();
    let framed = split(
        &format!("{other} R {{\"ok\": \"not the answer\"}}\n{{\"ok\": \"nor this\"}}\n"),
        "",
        &tag,
    );
    assert_eq!(framed.result, None, "{framed:?}");
    assert!(framed.diagnostics.contains("not the answer"));

    for shape in [
        format!("{tag}no space, so no kind\n"),
        // The nonce alone is not enough: a line must also say what it carries,
        // so output that echoes the tag cannot become an answer.
        format!("{tag} but no kind marker\n"),
        format!("{tag} X an unknown kind\n"),
    ] {
        assert_eq!(split(&shape, "", &tag).result, None, "{shape}");
    }
}

#[test]
fn a_result_line_is_found_on_either_stream() {
    // A runtime may merge its streams, or buffer them in an order the host does
    // not control; losing the answer to that would be a worse failure than the
    // one this protocol replaced.
    let tag = nonce();
    let on_stderr = split("noise\n", &format!("{tag} R 7\n"), &tag);
    assert_eq!(on_stderr.result.as_deref(), Some("7"));
}

#[test]
fn diagnostics_are_bounded_and_stay_printable() {
    // This text ends up in a receipt, and a capsule can print without end.
    let tag = nonce();
    let chatty = "line of guest output\n".repeat(2_000);
    let framed = split(&chatty, "", &tag);
    assert!(
        framed.diagnostics.len() <= MAX_DIAGNOSTIC_BYTES + 8,
        "unbounded diagnostics: {} bytes",
        framed.diagnostics.len()
    );

    // A multi-byte character cut in half would make the receipt unprintable.
    let wide = "π".repeat(MAX_DIAGNOSTIC_BYTES);
    let framed = split(&wide, "", &tag);
    assert!(framed
        .diagnostics
        .is_char_boundary(framed.diagnostics.len()));
    assert!(std::str::from_utf8(framed.diagnostics.as_bytes()).is_ok());
}

#[test]
fn every_call_gets_its_own_tag() {
    let tags: std::collections::BTreeSet<String> = (0..64).map(|_| nonce()).collect();
    assert_eq!(tags.len(), 64, "tags must not repeat within a run");
    for tag in &tags {
        assert!(tag.starts_with("fgx"));
        assert!(tag.len() >= 16, "{tag}");
        assert!(!tag.contains(' '), "a tag with a space could not be parsed");
    }
}
