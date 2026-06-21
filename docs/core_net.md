# core.socket — Real, Effect-Gated Networking (v2.15)

The agent-to-agent transport substrate. `core.socket` opens **real TCP
sockets**, but only to endpoints on an allowlist, and records a deterministic
transcript of every connection. Same gate-and-transcript model as `core.io`
(v2.14), now for the network.

## Builtins

| Builtin | Effects | Returns |
| --- | --- | --- |
| `core_socket_tcp_request(endpoint, payload)` | `network.connect`, `network.write`, `network.read` | response (`str`) |
| `core_socket_tcp_send(endpoint, payload)` | `network.connect`, `network.write` | bytes sent (`usize`) |

`tcp_request` connects, sends the payload, half-closes the write side, and
reads the response to completion. These are the real sibling of the mock
`core.net` family (mirroring `core.io` vs `core.fs`).

## Usage

```text
fgx net-run <file> --entry <fn> [--allow <host:port>]... [--arg <v>]... [--json]
```

Real round-trip to an allowlisted endpoint:

```text
$ fgx net-run examples/network/tcp_ping.fgx --entry ping \
    --allow 127.0.0.1:9777 --arg 127.0.0.1:9777 --arg ping-forgentex
Forgentex effect-gated network run
schema=forgentex.net_run.v1 file=examples/network/tcp_ping.fgx entry=ping status=ok
  allow: [127.0.0.1:9777]
  request  allow  127.0.0.1:9777 (out 14 / in 14)
  bytes_out=14 bytes_in=14 transcript_hash=...
  return: ping-forgentex
```

An endpoint **not** on the allowlist is denied and the socket is never
opened:

```text
$ fgx net-run examples/network/tcp_ping.fgx --entry ping --arg 127.0.0.1:9777 --arg ping
  request  deny   127.0.0.1:9777 (out 0 / in 0)
  note: trap: net_denied: connect to 127.0.0.1:9777 is not in the allowlist
```

## Fail closed

The allowlist check happens **before** `connect`. A connection to an unlisted
host:port is recorded `decision=deny`, the run errors, and no socket is
opened — runtime-enforced reach, not just review. `core_socket_*` builtins
are only available under `fgx net-run`; a plain `fgx run` has no network host.

## The report: `forgentex.net_run.v1`

| Field | Meaning |
| --- | --- |
| `allow` | the host:port allowlist supplied to the run |
| `status` | `ok`, `denied`, or `error` |
| `ops` | the transcript: per op `{op, endpoint, bytes_out, bytes_in, decision}` |
| `bytes_out` / `bytes_in` | totals over allowed operations |
| `transcript_hash` | FNV-1a of the transcript (no wall-clock fields), for replay/diff |
| `return_value` | the entry's return value |

The transcript deliberately excludes timing, so it is stable and replayable;
tests run against an in-process loopback echo server, so CI needs no network.

## Scope and honesty

- Real TCP request/response and fire-and-forget send, gated by a host:port
  allowlist. UDP, a minimal HTTP client/server, and TLS are follow-ons.
- The allowlist is the reach gate (`network.connect <host:port>` from the
  sandbox plan); per-direction `network.read`/`network.write` effects are
  carried for typing and review.
- This is the transport the v2.19 agent trust fabric (signed messages,
  capability tokens, receipts) will run over.
