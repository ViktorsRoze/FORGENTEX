# Stage 22: Testing, Fuzzing, and Formal Invariants

Stage 22 adds a deterministic trust layer around the v0 compiler pipeline. The
goal is not random throughput yet; it is repeatable fuzzing and property checks
that can run on every machine without new dependencies.

## Commands

```powershell
fgx fuzz --iterations 64 --json
fgx diff-test --project . --source examples/media/audio_mix.fgx --entry mix_samples --json-input examples/run/audio_mix.json --json
```

`fgx fuzz` emits `forgentex.fuzz.v1`. `fgx diff-test` emits
`forgentex.backend_differential.v1`.

## Fuzz and Property Coverage

The Stage 22 fuzz suite covers:

- corpus roundtrips for `.fgx` and `.fgai`
- generated safe programs
- `.fgx -> AST -> .fgai -> AST` canonical equality
- `.fgai -> AST -> .fgx -> AST` canonical equality
- human and agent formatter idempotency
- canonical AST serialization stability
- parser no-panic checks for mutated and invalid inputs
- patch diagnostic no-panic checks for malformed `.fgp`
- semantic patch property: patch, check, format, reparse, check
- target validation invariants for ABI and structured assembly

The harness is deterministic: the same iteration count produces the same case
set and the same report.

## Backend Differential Runner

The v0 differential runner compares the safe reference semantics available
today:

1. parse and type-check a safe source file
2. execute the chosen entrypoint through the interpreter with golden JSON input
3. generate C backend header/source for the project
4. generate FGIR for the project
5. verify the same entry symbol appears in C output and FGIR

This does not compile and execute generated C yet. It establishes the first
backend agreement gate before Stage 23+ build-system work adds native compiler
integration.

## CI

The normal CI path runs formatting, Clippy, and tests. The workflow also includes
a scheduled Stage 22 fuzz job that runs:

- `cargo run --bin fgx -- fuzz --iterations 128 --json`
- `cargo run --bin fgx -- diff-test --json`

Local smoke coverage is available through:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\fuzz.ps1
```

## Deferred

- Coverage-guided fuzzing.
- `cargo fuzz`/libFuzzer integration.
- Property-testing crate integration.
- Generated C compilation and execution.
- Differential tests against Zig, Cranelift, LLVM, and MLIR.
- Scheduled corpus minimization and crash artifact upload.
