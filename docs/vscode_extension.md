# Stage 20: VS Code Extension

Stage 20 makes the Forgentex human/agent workflow usable from an editor without
requiring a production language server. The extension package lives at
`tools/vscode/forgentex` and is dependency-free so it can be inspected, tested,
and launched from a clean checkout.

## Scope

- Syntax highlighting for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`, covering the
  complete parser keyword surface (declarations, capsules, pipelines, shares,
  effects, attributes, and the dense `.fgai` tokens).
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

## Configuration

- `forgentex.fgxPath`: path to the `fgx` CLI binary. Defaults to `fgx`.
- `forgentex.defaultDeps`: dependency slices for symbol context. Defaults to
  `types,effects,tests`.
- `forgentex.languageServiceMode`: currently `cli` or `disabled`.
- `forgentex.checkOnType`: re-check while typing. Default `true`.
- `forgentex.checkDelayMs`: debounce before a check-on-type run. Default `400`,
  clamped to a minimum of `200`.

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
  diagnostics, formatting, context-sensitive completion, the disabled mode, and
  scratch-file cleanup.

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

- Persistent LSP server. `forgentex-lsp` exists and speaks hover,
  documentSymbol, definition, rename, formatting, codeAction, semanticTokens,
  and publishDiagnostics over stdio, but the extension still drives the `fgx`
  CLI instead of connecting to it; wiring it up needs a JSON-RPC client and a
  `lsp` value for `forgentex.languageServiceMode`.
- Marketplace packaging and publishing.
- Editor-side source mutation after patch approval.
- Rich control-flow-aware navigation.
- Inline AST diff visualization.
