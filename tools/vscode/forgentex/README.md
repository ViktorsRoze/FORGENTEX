# Forgentex VS Code Extension

Stage 20 provides a dependency-free editor package for the first Forgentex workflow. It is intentionally small: the extension uses the existing `fgx` CLI as its language service and does not require TypeScript compilation, npm packages, or a marketplace build.

## Features

- Syntax highlighting for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`.
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

The Stage 20 extension is a CLI-language-service bridge. A persistent LSP server, marketplace packaging, and editor-side mutation of source files remain later milestones.
