# Stage 20: VS Code Extension

Stage 20 makes the Forgentex human/agent workflow usable from an editor without
requiring a production language server. The extension package lives at
`tools/vscode/forgentex` and is dependency-free so it can be inspected, tested,
and launched from a clean checkout.

## Scope

- Syntax highlighting for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`, covering the
  complete parser keyword surface (declarations, capsules, pipelines, shares,
  effects, attributes, carried sections, and the dense `.fgai` tokens).
- Carried payloads highlighted in their own language: a `data` section embeds the
  grammar its shape names (JSON, NDJSON, YAML, XML, SQL, Markdown, HTML, CSS,
  TOML, INI), a blob's base64 recedes, and a note's author, status, and Markdown
  text are scoped separately.
- A Project Filesystem view over the sections a file carries, with a blob
  preview, note threads, and add/extract commands.
- Debugging through the `fgx dap` adapter.
- Automatic `fgx check` diagnostics on open, on save, and while typing.
- Side-by-side human/agent source views.
- Semantic node ID hovers and document-symbol navigation.
- CLI-backed diagnostics using `fgx check <file> --json`.
- Document formatting through `fgx fmt <file> --to fgx|fgai`.
- Context-aware completion for keywords, types, effects, and binding modes.
- Patch review WebView using `fgx review <patch> --json`.
- Patch approval preview using `fgx patch <source> <patch> --review`.
- Unsafe, assembly, ABI, x86_64, ARM/AArch64, and RISC-V slices surfaced
  through an architecture filter command.

## Commands

- `Forgentex: Show Human/Agent Views`
- `Forgentex: Open Symbol Context`
- `Forgentex: Review Semantic Patch`
- `Forgentex: Approve Patch Preview`
- `Forgentex: Run Diagnostics`
- `Forgentex: Show Unsafe Architecture Slices`
- `Forgentex: Refresh Project Filesystem`
- `Forgentex: Add File to Project…`
- `Forgentex: Extract Payloads to Folder…`
- `Forgentex: Preview Blob`
- `Forgentex: Show Notes on Section`
- `Forgentex: Reveal Section` (how the tree moves the cursor; hidden from the
  palette, since it is not something to invoke by hand)

## Configuration

- `forgentex.fgxPath`: path to the `fgx` CLI binary. Defaults to `fgx`.
- `forgentex.defaultDeps`: dependency slices for symbol context. Defaults to
  `types,effects,tests`.
- `forgentex.languageServiceMode`: currently `cli` or `disabled`.
- `forgentex.checkOnType`: re-check while typing. Default `true`.
- `forgentex.checkDelayMs`: debounce before a check-on-type run. Default `400`,
  clamped to a minimum of `200`.
- `forgentex.author`: who to record when this editor adds a section, for example
  `agent:claude-opus-5` or `human:viktor`. Empty lets the CLI record the current
  user.

## Diagnostics

`fgx check` reads a path, so a dirty or untitled buffer is mirrored into a
scratch file that keeps the original extension before being checked. Without
that mirror the editor would report diagnostics for the last saved text while
the user types. Scratch files are removed synchronously after each run.

Reports use `forgentex.diagnostics.v1`, which names the diagnostic code `error`
and carries no severity field; every entry blocks the checker and is surfaced
as an error, with `expected`/`found` appended to the message when present.

A failed check clears that file's diagnostics rather than leaving stale ones,
because neither "still clean" nor "still broken" is known after a failure.
Background checks do not raise error popups; the explicit
`Forgentex: Run Diagnostics` command does.

## Tests

`tools/vscode/forgentex/test` holds three Node suites, run from `cargo test`
via `vscode_extension_suites_pass` and skipped on hosts without Node:

- `grammar-coverage.js` asserts every parser keyword is highlighted, every
  grammar regex compiles, every manifest contribution resolves, and that
  declared commands and read configuration keys match the extension source.
- `snippet-check.js` expands every `.fgx` snippet to its placeholder defaults
  and runs `fgx check` on the result, so no snippet can insert invalid source.
- `extension-smoke.js` stubs the `vscode` module and drives the extension
  against the real `fgx` binary, covering provider registration, dirty-buffer
  diagnostics, formatting, context-sensitive completion, the disabled mode,
  scratch-file cleanup, and the project filesystem view's tree, lenses, and blob
  previews.

The suites resolve the newest of the release and debug builds rather than a fixed
path. They previously used a stale release binary that predated `fgx fs`, which
emptied the view and made section snippets fail to parse — a failure that looked
like an extension bug and was not one. `tests/cli.rs` passes
`CARGO_BIN_EXE_fgx`, so the Rust gate always tests the binary it just built.

## Project filesystem view

A file that carries its data, its bytes, and remarks about them is a lot to hold
in your head as plain text, so the view renders it the way the file is actually
organised: **Data**, **Bytes**, and **Notes** groups, with every remark hanging
under the section it is about. Agent remarks take a robot icon and human ones a
person icon; an open remark carries the warning colour, a resolved one recedes.
Hovering a row shows provenance — who created and last changed it, when — plus
size, storage, and hash. Clicking a row reveals that section in the editor.

Above each section, a CodeLens summarises the remarks on it (`3 notes · 1 open ·
from agents`) and offers a preview for blobs. A blob opens in a WebView as an
image, an audio or video player, text, or a hex dump, chosen by media type;
bytes that decode as none of those still show something rather than nothing.

Everything the view knows comes from `fgx fs ls --json` and `fgx fs notes
--json`. Nothing parses the file in JavaScript: a second parser would be a
second answer to "what is in this file", and the two would drift.

## Debugging

`package.json` contributes a `forgentex` debug type backed by `adapter.js`,
which speaks the Debug Adapter Protocol to `fgx dap`. Breakpoints, stepping,
stack frames, scopes, and variable inspection go through the same adapter the
CLI serves, so an editor session and a `fgx debug` run report the same state.

## Architecture Review

The extension highlights and indexes architecture-specific lines that contain:

- `unsafe`, `asm`, `ASM`, or `ABI`
- `x86_64`
- `aarch64`, `arm64`, or `armv7`
- `riscv32`, `riscv64`, or `risc-v`
- `cpu.*` or `target.*` effects

This gives reviewers a fast way to isolate unsafe code paths for x86_64,
ARM/AArch64, and RISC-V without reading the whole project.

## Deferred

- Editor commands for the project filesystem's writing verbs. `fgx fs set`,
  `rm`, `note`, and `resolve` exist in the CLI with `--expect` guards; the view
  reads and previews but does not yet offer "add note" or "resolve" actions.
- Persistent LSP server. `forgentex-lsp` exists and speaks hover,
  documentSymbol, definition, rename, formatting, codeAction, semanticTokens,
  and publishDiagnostics over stdio, but the extension still drives the `fgx`
  CLI instead of connecting to it; wiring it up needs a JSON-RPC client and a
  `lsp` value for `forgentex.languageServiceMode`.
- Marketplace packaging and publishing.
- Editor-side source mutation after patch approval.
- Rich control-flow-aware navigation.
- Inline AST diff visualization.
