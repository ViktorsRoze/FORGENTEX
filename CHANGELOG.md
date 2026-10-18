# Changelog

Public releases of Forgentex. The repository's internal `v1`, `v2`, and `v3`
labels are development tracks, not versions; the per-slice ledger lives in
[docs/v3_progress.md](docs/v3_progress.md).

## 0.1.0 — unreleased

The first coherent public release: a bounded, documented, interpreter-first
language and toolchain, with every promise it makes machine-checked by
`fgx release-check --target public-0.1`.

**Requires a recorded release run before tagging.** The distribution gate closes
when [docs/public_0_1_release_proof.md](docs/public_0_1_release_proof.md)
records a green `release-artifacts` run; until then `--strict` fails by design.

### The language

- One canonical AST with two syntaxes: readable `.fgx` and dense `.fgai`, which
  round-trip byte for byte and describe the same program.
- Functions, records, enums, pattern matching, control flow, generics, traits,
  function values, and non-capturing closures, executable through the
  interpreter, which is the reference semantics for 0.1.
- Ownership and borrowing checked by a CFG dataflow borrow checker with
  interprocedural lifetime contracts, backed by a conformance suite that
  compares the production checker against an independent reference model.
- Effects and capabilities declared in source, checked before anything runs.
- Integer semantics specified per width and per operation. Overflow, division
  by zero, `MIN / -1`, and out-of-range shifts trap, at the declared width, in
  both the interpreter and the generated C.
- `@exact` is transitive: an exact function cannot reach floating point through
  any call path.

### For agents

- Semantic patches target AST nodes, carry `NODE_HASH` preconditions, and
  produce machine-readable review reports, so two agents editing one file
  cannot silently overwrite each other.
- Project maps, focused context packets, diagnostic explanations, and repair
  plans, all with versioned JSON schemas and frozen diagnostic codes.
- A project filesystem in one file: `data`, `blob`, and `note` items carry the
  payloads a project reads, the bytes it ships, and an annotation graph that
  records whether an agent or a person wrote each remark. `fgx fs` navigates and
  edits it; edits are guarded by content hashes, stamped with provenance, and
  reportable as a change feed.

### Execution and interop

- A C11 backend over a bounded subset that refuses what it cannot lower with
  the same meaning, verified by compiling and running the output and comparing
  results against the interpreter.
- A Zig source backend for safe experiments.
- Polyglot capsules for Python, Node, TypeScript, Go, JVM, .NET, C, C++, Rust,
  and Zig, with typed boundaries, audit receipts, and real limits: deadlines,
  CPU, memory, output bounds, and process-tree cleanup. A limit the platform
  cannot enforce is refused rather than ignored.
- Native capsules on Windows run in a zero-capability AppContainer, with
  network and IPC denial demonstrated by adversarial tests.

### Tooling

- `fgx` CLI and a dependency-free language server, plus a VS Code extension
  with diagnostics, formatting, symbols, and a Project Filesystem view.
- Inline tests, fuzzing, backend differential testing, and a strict release gate.

### Known limits

Stated in full in
[the profile](docs/public_0_1_profile.md#known-limits). In short: one verified
host (`x86_64-pc-windows-msvc`); managed capsules get limits but not filesystem
or network denial; the dense agent syntax measures about 1% *larger* than the
readable one under a real BPE tokenizer, not smaller; semantic patching is
0.94–1.83× a read-once text patch, not the larger multiples measured against
whole-file resend. A Windows native output-loss defect that long looked like a
flake is fixed, by measurement rather than by a proven mechanism.
