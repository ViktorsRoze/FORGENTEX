# I/O and Networking Runtime

Stage 17 adds a reviewable runtime surface for filesystem and networking work.
The implementation is intentionally deterministic: the interpreter uses mock
filesystem and network results, while the language and tooling prove that access
is explicit in types, effects, maps, diagnostics, and patch reviews.

## Effects

Filesystem APIs use:

- `fs.read`
- `fs.write`

Network APIs use:

- `network.connect`
- `network.read`
- `network.write`
- `network.http`
- `network.udp`
- `network.websocket`

Serialization and streaming helpers use:

- `serialize`
- `stream`

Every function that calls one of these APIs must declare the matching effect.
Missing declarations produce `undeclared_effect` diagnostics. Review reports
now separately flag filesystem and network access in addition to the generic
effect/capability flag.

## Capability Scopes

Stage 17 models capability scopes as explicit `str` parameters. A scope is a
human-reviewable label such as `workspace`, `cache`, `demo`, or `agent`.

The current runtime does not grant real OS permissions. The scope parameter is
part of the public contract so later stages can attach manifests, sandbox
policies, and least-privilege review checks without changing the API shape.

## Standard Modules

`core.fs`:

- `core_fs_scope_read(scope: str) -> bool effects [fs.read]`
- `core_fs_scope_write(scope: str) -> bool effects [fs.write]`
- `core_fs_read_text(scope: str, path: str) -> Result[str,str] effects [fs.read]`
- `core_fs_write_text(scope: str, path: str, contents: str) -> Result[usize,str] effects [fs.write]`
- `core_fs_copy_text(scope: str, source: str, dest: str) -> Result[usize,str] effects [fs.read, fs.write]`

`core.net`:

- `core_net_http_get(scope: str, url: str) -> Result[str,str] effects [network.connect, network.read, network.http]`
- `core_net_http_post(scope: str, url: str, body: str) -> Result[str,str] effects [network.connect, network.write, network.read, network.http]`
- `core_net_tcp_send(scope: str, endpoint: str, payload: str) -> Result[usize,str] effects [network.connect, network.write]`
- `core_net_udp_send(scope: str, endpoint: str, payload: str) -> Result[usize,str] effects [network.udp, network.write]`
- `core_net_websocket_send(scope: str, endpoint: str, payload: str) -> Result[usize,str] effects [network.connect, network.write, network.websocket]`

`core.serialize`:

- `core_serialize_text_len(value: str) -> usize effects [serialize]`

`core.stream`:

- `core_stream_count_bytes(chunk: str) -> usize effects [stream]`

## Interpreter Behavior

The interpreter does not read real files or contact real endpoints in v0.
Instead it returns stable mock values:

- file reads return `Ok("mock-file:<scope>:<path>")`
- file writes return `Ok(<contents byte length>)`
- file copies return `Ok(<mock source byte length>)`
- HTTP GET returns `Ok("pong:<scope>:<url>")`
- HTTP POST returns `Ok("accepted:<scope>:<url>:<body length>")`
- TCP, UDP, and WebSocket sends return `Ok(<payload byte length>)`

This keeps the first runtime useful for tests and demos while preserving a
clean boundary for real async I/O, permission manifests, and transport backends.

## Examples

```powershell
fgx check examples/io/file_copy.fgx --json
fgx effects examples/io/file_copy.fgx --json
fgx run examples/io/file_copy.fgx --entry file_copy_demo --json-input examples/run/file_copy.json

fgx check examples/network/agent_ping.fgx --json
fgx effects examples/network/agent_ping.fgx --json
fgx run examples/network/agent_ping.fgx --entry agent_ping --json-input examples/run/agent_ping.json
```

The C backend rejects functions with runtime-only effects such as `fs.*`,
`io.*`, `network.*`, `serialize`, or `stream`.
