use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use crate::net_run::run;

use super::parse_fixture;

/// A one-shot in-process echo server on an ephemeral loopback port.
/// Returns its address and a join handle. No external network needed.
fn echo_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            let _ = stream.write_all(&buf);
            let _ = stream.flush();
        }
    });
    (addr, handle)
}

#[test]
fn allowed_request_does_a_real_round_trip() {
    let (addr, handle) = echo_server();
    let program = parse_fixture("examples/network/tcp_ping.fgx");
    let report = run(
        "tcp_ping.fgx",
        &program,
        "ping",
        vec![addr.clone(), "ping".to_string()],
        vec![addr.clone()],
    );
    handle.join().ok();

    assert_eq!(report.status, "ok");
    assert_eq!(report.return_value.as_deref(), Some("ping"));
    assert_eq!(report.ops.len(), 1);
    assert_eq!(report.ops[0].op, "request");
    assert_eq!(report.ops[0].decision, "allow");
    assert_eq!(report.bytes_out, 4);
    assert_eq!(report.bytes_in, 4);
}

#[test]
fn endpoint_not_on_allowlist_is_denied_without_connecting() {
    let program = parse_fixture("examples/network/tcp_ping.fgx");
    let report = run(
        "tcp_ping.fgx",
        &program,
        "ping",
        vec!["127.0.0.1:9".to_string(), "x".to_string()],
        Vec::new(),
    );
    assert_eq!(report.status, "denied");
    assert_eq!(report.ops[0].decision, "deny");
    assert_eq!(report.bytes_out, 0);
}

#[test]
fn report_json_is_well_formed() {
    let program = parse_fixture("examples/network/tcp_ping.fgx");
    let json = run(
        "tcp_ping.fgx",
        &program,
        "ping",
        vec!["127.0.0.1:9".to_string(), "x".to_string()],
        Vec::new(),
    )
    .to_json();
    assert!(json.contains("\"schema\": \"forgentex.net_run.v1\""));
    assert!(json.contains("\"transcript_hash\""));
    assert!(json.contains("\"endpoint\""));
}

#[test]
fn core_socket_outside_net_run_is_unavailable() {
    use crate::ast::Expr;
    let program = parse_fixture("examples/network/tcp_ping.fgx");
    let call = Expr::Call {
        name: "core_socket_tcp_request".to_string(),
        args: vec![
            Expr::String("127.0.0.1:9".to_string()),
            Expr::String("x".to_string()),
        ],
    };
    let err = crate::interp::eval_expr_for_test(&program, &call)
        .err()
        .unwrap_or_default();
    assert!(
        err.contains("net"),
        "expected net unavailable error, got: {err}"
    );
}
