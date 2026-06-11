# Stage 20: VS Code Extension

Stage 20 makes the Forgentex human/agent workflow usable from an editor without
requiring a production language server. The extension package lives at
`tools/vscode/forgentex` and is dependency-free so it can be inspected, tested,
and launched from a clean checkout.

## Scope

- Syntax highlighting for `.fgx`, `.fgai`, `.fgp`, and `.fgmap`.
- Side-by-side human/agent source views.
- Semantic node ID hovers and document-symbol navigation.
- CLI-backed diagnostics using `fgx check <file> --json`.
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

- Persistent LSP server.
- Marketplace packaging and publishing.
- Editor-side source mutation after patch approval.
- Rich control-flow-aware navigation.
- Inline AST diff visualization.
