# Stage 21: Language Server and IDE Protocol

Stage 21 adds a dependency-free language server named `forgentex-lsp`. It speaks
LSP over stdio and reuses the existing Forgentex parser, checker, formatter,
target model, and patch diagnostics. The goal is editor portability: VS Code,
Neovim, Helix, Zed, and other clients can integrate without depending on the
Stage 20 VS Code extension.

## Binary

```powershell
cargo run --bin forgentex-lsp -- --probe
cargo run --bin forgentex-lsp
```

`--probe` prints server metadata and capabilities. Without flags, the server
waits for LSP `Content-Length` framed JSON-RPC messages on stdin and writes
framed responses on stdout.

## Capabilities

The v0 server advertises:

- diagnostics through `textDocument/publishDiagnostics`
- hover through `textDocument/hover`
- go-to-definition through `textDocument/definition`
- rename through `textDocument/rename`
- formatting through `textDocument/formatting`
- document symbols through `textDocument/documentSymbol`
- semantic tokens through `textDocument/semanticTokens/full`
- semantic patch code actions through `textDocument/codeAction`

Experimental capability flags:

- `nodeIdNavigation`
- `semanticPatchCodeActions`
- `targetAwareDiagnostics`

## Document Support

The server can serve:

- `.fgx` human syntax
- `.fgai` agent syntax
- `.fgp` semantic patch files

`.fgmap` files are recognized for symbols, hover, and semantic token behavior,
but Stage 21 acceptance focuses on source and patch files.

## Target-Aware Diagnostics

Clients can pass a target triple during initialize:

```json
{
  "initializationOptions": {
    "target": "aarch64-unknown-linux-gnu"
  }
}
```

Clients can also update it with a simple notification:

```json
{
  "jsonrpc": "2.0",
  "method": "forgentex/setTarget",
  "params": {
    "target": "riscv64gc-unknown-linux-gnu"
  }
}
```

When a target is set, diagnostics include normal type checks, target-family
checks, feature-guard checks, and SIMD fallback checks.

## Node-ID Navigation

Document symbols and hover text expose stable semantic IDs such as:

- `@function.mix_samples`
- `@struct.AudioBlock`

Definition requests resolve names and semantic node IDs within the active
document. This is enough for v0 agent workflows because symbol context and
project-wide navigation remain available through `fgx context` and `.fgmap`.

## Semantic Patch Code Actions

The code-action provider returns:

- quick fixes derived from compiler diagnostic suggestions
- a refactor action to create a semantic patch for the selected node
- a source action to run target-aware diagnostics

These actions are protocol hooks. The first implementation exposes command
payloads for editor clients to bind to their own patch creation/review UI.

## Deferred

- Cross-file go-to-definition.
- Persistent background project index.
- True incremental parsing.
- Full JSON parser and schema validation.
- Rich patch-edit generation directly inside the LSP server.
- Transport modes beyond stdio.
