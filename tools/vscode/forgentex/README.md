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
