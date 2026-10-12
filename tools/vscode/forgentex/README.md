# Forgentex VS Code Extension

Stage 20 provides a dependency-free editor package for the first Forgentex workflow. It is intentionally small: the extension uses the existing `fgx` CLI as its language service and does not require TypeScript compilation, npm packages, or a marketplace build.

## Features

- Syntax highlighting for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`, covering the
  complete parser keyword surface: declarations, `foreign` capsules, pipelines,
  shares, effects, attributes, and the dense `.fgai` tokens (`EN`, `LET`, `WH`,
  `MT`, `CAP`, …).
- Automatic diagnostics on open, on save, and while typing. Dirty buffers are
  mirrored to a scratch file first, so problems track the editor rather than
  the last save.
- Document formatting through `fgx fmt`, in both the human and agent syntaxes.
- Context-aware completion: effect names inside `effects [...]`, binding modes
  after `bind <name>`, and keywords plus types elsewhere.
- Side-by-side human/agent views using either sibling files or `fgx fmt`.
- Document symbols and hovers for semantic node IDs such as `@function.mix_samples`.
- CLI-backed diagnostics through `fgx check <file> --json`.
- Patch review WebView backed by `fgx review <patch> --json`.
- Patch approval preview backed by `fgx patch <source> <patch> --review`.
- architecture filters for unsafe, assembly, ABI, x86_64, ARM/AArch64, and RISC-V slices.

### The project filesystem

A Forgentex file can carry its data, its bytes, and a graph of remarks beside
its code (`docs/project_filesystem.md`). The extension shows that as what it is:

- **A Project Filesystem view** in the activity bar, grouping a file's sections
  into Data and Bytes, with every remark hanging under the thing it is about.
  Each row carries its provenance — shape or media type, size, storage, and who
  touched it when — and clicking one puts the cursor on its header.
- **Payloads highlighted in their own language.** A `json`, `yaml`, `xml`,
  `sql`, or `md` payload is syntax-highlighted as that language inside the
  section; base64 recedes into a comment scope, because it is transport rather
  than something to read.
- **Remarks tell you who is talking.** An agent's note carries a different icon
  and scope from a person's, and an open one is coloured as still needing
  attention.
- **Code lenses on a section header** show how many notes it has, how many are
  open, and how many came from agents; a blob header offers a preview.
- **Blob preview** renders a payload for what it is: an image as an image, audio
  and video in a player, anything else as a hex dump of its first bytes.
- **Add and extract** from the view's title bar: bring a file on disk into the
  project, or materialise every payload back to real files.

The view reads through the `fgx` CLI rather than parsing files in JavaScript. A
second parser would be a second answer to "what is in this file", and the two
would drift.

## Local Use

1. Build or install the CLI so `fgx` is on your `PATH`, or set `forgentex.fgxPath` to the binary path.
2. Open this folder in VS Code:

   ```powershell
   code tools/vscode/forgentex
   ```

3. Press `F5` in VS Code to launch an Extension Development Host.
4. Open the Forgentex repository in that host and use the `Forgentex:*` commands.

## Settings

| Setting | Default | Purpose |
|---|---|---|
| `forgentex.fgxPath` | `fgx` | Path to the CLI binary. |
| `forgentex.languageServiceMode` | `cli` | `cli` or `disabled`. Disabling stops all CLI work. |
| `forgentex.checkOnType` | `true` | Re-check while typing. |
| `forgentex.checkDelayMs` | `400` | Debounce before a check-on-type run, minimum `200`. |
| `forgentex.defaultDeps` | `types,effects,tests` | Dependency slices for symbol context. |
| `forgentex.author` | *(empty)* | Author recorded when the editor adds a section, e.g. `agent:claude-opus-5`. Empty lets the CLI record the current user. |

## Tests

No npm install is needed; the suites use only the Node standard library.

```powershell
cargo build --release --bin fgx
node test/grammar-coverage.js
node test/snippet-check.js
node test/extension-smoke.js
```

`grammar-coverage.js` checks the grammars against the parser's keyword surface
and validates the manifest. `snippet-check.js` expands every snippet and runs
`fgx check` on it. `extension-smoke.js` stubs the `vscode` module and drives the
extension against the real binary. All three also run from `cargo test` as
`vscode_extension_suites_pass`, and skip on hosts without Node.

The extension is a CLI-language-service bridge. `forgentex-lsp` already speaks
hover, symbols, definition, rename, formatting, code actions, semantic tokens,
and diagnostics over stdio, but the extension does not connect to it yet;
marketplace packaging and editor-side source mutation also remain later
milestones.
