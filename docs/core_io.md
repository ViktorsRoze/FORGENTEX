# core.io — Real, Effect-Gated I/O (v2.14)

Agentic work is I/O-heavy. `core.io` gives Forgentex real filesystem I/O that
is **safe by construction**: every operation runs only under a granted
capability and is recorded in a deterministic transcript.

## Builtins

| Builtin | Effect | Returns |
| --- | --- | --- |
| `core_io_read_file(path)` | `fs.read` | file contents (`str`) |
| `core_io_write_file(path, content)` | `fs.write` | bytes written (`usize`) |
| `core_io_copy_file(src, dst)` | `fs.read` + `fs.write` | bytes copied (`usize`) |

These differ from the mock `core.fs` family: `core.io` performs **real disk
operations**, but only through `fgx io-run`, which supplies the grant set and
records the transcript.

## Usage

```text
fgx io-run <file> --entry <fn> [--grant fs.read] [--grant fs.write] [--arg <v>]... [--json]
```

Copy a file under an explicit grant:

```text
$ fgx io-run examples/io/real_copy.fgx --entry copy_file \
    --grant fs.read --grant fs.write --arg src.txt --arg dst.txt
Forgentex effect-gated I/O run
schema=forgentex.io_run.v1 file=examples/io/real_copy.fgx entry=copy_file status=ok
  grants: [fs.read, fs.write]
  copy   allow  src.txt -> dst.txt (19 bytes)
  bytes_total=19 transcript_hash=...
  return: 19
```

Without the grants, the operation is **denied, not silently skipped**, and
nothing is written:

```text
$ fgx io-run examples/io/real_copy.fgx --entry copy_file --arg src.txt --arg dst.txt
  copy   deny   src.txt -> dst.txt (0 bytes)
  ...
  note: trap: io_denied: copy src.txt -> dst.txt requires fs.read and fs.write
```

## Fail closed

The grant check happens **before** the operation. A denied op is recorded
with `decision=deny`, the run errors, and the filesystem is untouched — the
destination file is never created. This is the effects/sandbox model enforced
at runtime, not just reviewed.

`core_io_*` builtins are only available under `fgx io-run`; the plain
interpreter has no I/O host, so a normal `fgx run` cannot perform disk I/O.

## The report: `forgentex.io_run.v1`

| Field | Meaning |
| --- | --- |
| `grants` | the capabilities supplied to the run |
| `status` | `ok`, `denied`, or `error` |
| `ops` | the transcript: per op `{op, path, bytes, decision}` |
| `bytes_total` | total bytes of allowed operations |
| `transcript_hash` | FNV-1a of the transcript, for replay/diff |
| `return_value` | the entry's return value |

## Scope and honesty

- Synchronous file read/write/copy with `fs.read`/`fs.write` grants. Path
  scoping (restricting to a root), append/seek, directories, and async I/O
  are follow-ons.
- The transcript is the deterministic oracle; the bytes on disk depend on the
  host, but the recorded operation sequence is reproducible.
- `core.net` is the sibling for networking (v2.15), built the same way.
