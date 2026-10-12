# The project filesystem inside a file

A Forgentex file already carries foreign source: a `foreign` capsule holds
Python, C, or Go verbatim. This extends that to the rest of a project. One file
can hold the code, the data it reads, the bytes it ships, and a graph of
remarks about any of it — instead of a directory tree an agent has to crawl.

```fgx
data sales csv:
    created "2026-09-20T09:00:00Z" by "human:viktor"
    modified "2026-09-20T10:15:00Z" by "agent:claude-opus-5"
    content:
        region,units
        north,120

blob logo image/png:
    bytes 8412
    sha256 "9f2ad1…"
    inline:
        iVBORw0KGgoAAAANSUhEUg…

note n1 on "@data.sales":
    by "agent:claude-opus-5"
    created "2026-09-20T10:20:00Z"
    status "open"
    text:
        North looks doubled after the Q3 import.

fn total(a: i32, b: i32) -> i32:
    return a + b
```

## Why these are language items, not a container format

Each section is an ordinary item in the same AST as `fn` and `struct`. That is
the whole design decision, and everything useful follows from it:

- **One address per section.** `@data.sales`, `@blob.logo`, `@note.n1` — the
  same canonical node ids `fgx check`, the project map, and semantic patches
  already speak. An agent lists a section and edits it without translating
  between two addressing schemes.
- **Safe concurrent edits.** Every section has a node hash — the one a
  `NODE_HASH` patch precondition checks — and every writing verb takes it as
  `--expect`, so two agents editing different sections never collide and two
  editing the same one fail loudly rather than silently overwriting. (Until the
  2026-09-21 slice this bullet said "already": `NODE_HASH` resolved only
  functions and structs, so no precondition on a section could be written.)
- **Budgeted reading.** `fgx context` hands an agent one section instead of the
  file, which matters most precisely when the file holds everything.
- **Both syntaxes, byte for byte.** A payload round-trips through the readable
  `.fgx` and the dense `.fgai` unchanged. A CSV that gained a space or a base64
  line that got rewrapped would be a corrupted file, not a formatting
  difference, so the round-trip is tested as an equality on the payload.

A separate container format would have needed its own parser, ids, patch
protocol, and retrieval before it could do any of this.

## The three kinds

### `data` — text payloads

`data <name> <shape>:` with a verbatim `content:` block. The shape is free-form
(`csv`, `json`, `yaml`, `xml`, `sql`, `md`, `txt`, …): the file records what the
payload is, and validating a payload *against* its shape is a separate, opt-in
concern rather than something the format forces.

### `blob` — byte payloads

`blob <name> <media-type>:` for images, audio, video, spreadsheets, columnar
files (Avro, ORC, Parquet), and compiled artifacts. A blob declares its decoded
size and may declare a `sha256`; both are checked, because a payload whose
bytes, length, and hash disagree is the failure that silently corrupts whatever
reads it.

Bytes live in one of two places:

- **`inline:`** — base64 in the file, for payloads at or below 256 KB.
- **`store "<path>"`** — a content-addressed path beside the project, for
  anything larger. The file stays the index; a 50 MB video does not become
  67 MB of base64 in an editor. A store entry is ordinary content on disk, so a
  store-backed blob must declare its hash and is verified on every read.

### `note` — the annotation graph

`note <name> on "<node-id>":` attaches a remark to any node, including another
note via `reply-to`, which is what makes the annotations a graph rather than a
list. Every note records `by "agent:<name>"` or `by "human:<name>"` — the prefix
is what lets a reader separate the file's machine-written remarks from its human
ones — plus a `status` of `open` or `resolved`.

A note carries its own name rather than a position: agents append notes
constantly, and positional identity would renumber every later note, breaking
the very references other notes reply to.

## Reading a payload from code

A file that carries its data is only half useful if the program beside it cannot
see any of it. Two builtins read a carried payload:

```fgx
fn row_count() -> usize:
    return str_len(section_text("@data.sales"))

fn logo_checksum() -> i64:
    return checksum(section_bytes("@blob.logo"))
```

- `section_text("<id>") -> str` — a data section's payload, or a blob's bytes as
  text when they are valid UTF-8.
- `section_bytes("<id>") -> Vec[u8]` — any payload as bytes.

Three properties worth stating, because each is a decision:

- **The id must be a literal.** A computed id could name a section that is not
  there, and that is a failure the checker can rule out completely — but only if
  the id is written down. An unknown id is `section_ref_unknown`, a note is
  `section_ref_not_a_payload`, and a computed one is `section_ref_not_literal`.
- **Reading is not I/O.** The bytes are in the program, like a string literal, so
  this needs no effect and grants no authority. The declared size and `sha256` are
  verified on the way through — `fgx check` catches a mismatch first, and the
  interpreter checks again, because a program reached through the library API may
  never have been checked and must not read bytes that disagree with what the
  file says they are.
- **An out-of-line payload is refused.** A store-backed blob's bytes live in a
  file beside the project, and reading that *is* filesystem access; allowing it
  here would hand a function that declares no effects the authority to read a
  path. `section_payload_out_of_line` names the way out: carry the payload
  inline, or `fgx fs extract` it and read the file through a granted effect.
  Making a store read explicit and grantable is the next step here.

Native lowering refuses both builtins with an explicit blocker
(`intrinsic:section_text`, `intrinsic:section_bytes`). Emitting a payload as a
static constant is sound and simply not built yet; an unsupported construct must
block rather than lower with weaker semantics.

Together with the capsule boundary this is what connects the project filesystem
to real work: `section_bytes("@blob.logo")` hands carried bytes straight to a
Python, Node, Go, JVM, or .NET capsule.

## Moving around, as an agent

```powershell
fgx fs ls <file>                       # every section, with its id and provenance
fgx fs cat <file> --tag @data.sales    # one payload; blob bytes go to stdout raw
fgx fs stat <file> --tag @blob.logo    # size, hash, storage, who touched it when
fgx fs notes <file> --agent            # only what agents wrote
fgx fs notes <file> --on @data.sales   # everything said about one section
```

`--json` on any of them emits `forgentex.project_fs.v1` /
`forgentex.project_notes.v1`. Sections also appear in `fgx map`, the project
map, and context cards, so an agent that reads only the map still knows what
data and notes the project holds.

## What is checked

`fgx check` validates carried sections alongside everything else:

| Code | Meaning |
| --- | --- |
| `blob_payload_invalid` | the inline payload is not valid base64 |
| `blob_size_mismatch` | the payload does not decode to the declared byte count |
| `blob_hash_mismatch` | the payload does not match its declared `sha256` |
| `blob_inline_too_large` | more than 256 KB carried inline |
| `blob_store_unverifiable` | a store-backed blob declares no hash |
| `note_target_missing` | the note is on a node that does not exist |
| `note_reply_missing` | the note replies to a note that does not exist |
| `note_author_unqualified` | the author is not `agent:<name>` or `human:<name>` |
| `note_status_unknown` | the status is neither `open` nor `resolved` |
| `section_duplicate_name` | two sections share a node id |
| `section_timestamp_invalid` | a timestamp is not fixed-width UTC `YYYY-MM-DDTHH:MM:SSZ` |

## Putting payloads in, and getting them out

```powershell
fgx fs add <file> --from sales.csv                  # shape and name inferred
fgx fs add <file> --from clip.mp4 --by "agent:x"    # large bytes go to the store
fgx fs add <file> --from data.bin --as blob --tag raw
fgx fs extract <file> --out ./work                  # materialise every payload
```

`add` decides where a payload belongs by its extension: known text shapes become
`data` sections with readable content, everything else becomes a `blob`. Bytes
at or below 256 KB are carried inline as wrapped base64; larger ones are written
to `.fgstore/<first two hex>/<rest>` — content-addressed, so identical payloads
are stored once and no path has to be invented. A text file that is not valid
UTF-8 is refused with `--as blob` named as the way out, rather than being
silently mangled.

Two properties this holds to:

- **The store write happens first.** If it fails, the project file is untouched
  rather than left pointing at bytes that never arrived.
- **Nothing is written that cannot be read back.** Before replacing the file,
  the rendered text is parsed again and its canonical form compared to the
  program in memory. A render that would not round-trip is refused.

`extract` writes each section to `<name>.<shape>` or `<name>.<extension>`,
verifying every store-backed blob against its hash on the way out — so a store
entry that was edited or corrupted is caught rather than passed on to whatever
reads it.

Timestamps and authorship come from `--at` and `--by`, defaulting to the current
time and `human:<user>`. The clock is read in one place, at the command line:
everything beneath it takes the timestamp as an argument, so parsing,
formatting, and checking stay deterministic.

## Editing, when more than one agent is at work

```powershell
fgx fs ls <file> --json                                    # each section carries its "hash"
fgx fs set <file> --tag @data.sales --from new.csv --expect <hash>
fgx fs rm <file> --tag @blob.logo --expect <hash> [--with-notes]
fgx fs note <file> --on @data.sales --text "North looks doubled." [--reply-to @note.n1]
fgx fs resolve <file> --tag @note.n1 [--text "Halved at source."]
```

Every verb that writes follows the same rules:

- **`--expect <hash>` refuses a stale edit.** The hash is the one `fs ls --json`
  reports and a `NODE_HASH` patch precondition checks. If the section moved
  after it was read, the edit is refused with both hashes named, and the file
  is untouched.
- **The edit is stamped.** `set` keeps a section's name, shape, and creation
  record and writes `modified` / `modified_by`; nothing relies on a caller
  remembering to.
- **Nothing is left dangling.** `rm` refuses to remove a section that still has
  remarks on it, naming them; `--with-notes` removes the whole thread,
  replies included. A new note is refused if its target, its parent, or its
  author would fail `fgx check`.
- **A resolution is recorded, not just flipped.** A note stores only when it
  was written, so `resolve` adds a resolved reply carrying who closed the thread,
  when, and why (`Resolved.` when no reason is given). A bare status change
  would be unattributed and invisible to the change feed.
- **Names are not reused.** An unnamed note takes one past the highest `n<k>`
  in use, so removing a note never makes a later one inherit a name that a log,
  a commit message, or another agent may still refer to.

## What changed since I last looked

```powershell
fgx fs changes <file> --rev HEAD                 # against a git revision of the file
fgx fs changes <file> --against old.fgx          # against any earlier copy
fgx fs changes <file> --since 2026-09-21T00:00:00Z
```

The first two compare node hashes, so they cover code as well as sections — an
agent resuming work needs to know that a function it depends on moved, not only
that a payload did — and they report `added`, `removed`, or `changed` with both
hashes. `--since` needs no earlier copy because it reads the provenance the file
records, which is also its limit: it trusts what the file says about itself,
and code carries no timestamps. That is why timestamps are held to fixed-width
UTC, which sorts correctly as text, and why a local offset or fractional second
is rejected by `fgx check` rather than silently landing on the wrong side of a
cut-off. `--json` emits `forgentex.project_changes.v1`.

## What is not built yet

Shipped: the three item kinds, both syntaxes with byte-exact round-trip,
canonical AST participation, node ids and source positions, validation, the
`fgx fs` read verbs, `add`/`extract` including the content-addressed store, the
guarded writing verbs `set`/`rm`/`note`/`resolve`, and the change feed.

Not yet built, in the order they matter:

1. **`fs import`** — pull a whole directory in at once, rather than a file at a
   time.
2. **`fs mv`, and store garbage collection.** Renaming a section has to repoint
   every note on it; a store entry nothing references is never collected.
3. **Section operations in semantic patches.** Sections can be hashed as patch
   preconditions, but no patch operation edits one: the `fs` verbs are the
   writing path for now.
4. **Shape-aware validation.** Checking that a `json` payload parses, a `csv`
   has consistent columns, or an Avro file matches a declared schema.
