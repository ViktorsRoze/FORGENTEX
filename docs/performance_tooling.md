# Forgentex Performance Tooling

Stage 25 makes optimization reviewable before Forgentex has a native optimizer.
The benchmark harness is deterministic: it runs interpreter correctness once,
then estimates operation cost from the AST so CI and agents get stable,
repeatable evidence.

## Commands

Run a single benchmark:

```powershell
fgx bench examples/media/audio_mix_simd.fgx --entry mix_samples_simd --json-input examples/bench/audio_mix.json --target aarch64-unknown-linux-gnu --json
```

Generate a scalar-vs-optimized performance report:

```powershell
fgx perf-report --scalar examples/media/audio_mix.fgx --optimized examples/media/audio_mix_simd.fgx --entry mix_samples --optimized-entry mix_samples_simd --json-input examples/bench/audio_mix.json --json
```

## Benchmark Reports

`fgx bench` emits `forgentex.bench.v1` with:

- interpreter correctness status
- selected target triple
- deterministic iteration count
- estimated cycles
- scalar reference cycles
- speedup against scalar reference
- operation counts for loops, scalar ops, calls, SIMD calls, indexes, stores,
  and vector lanes
- allocation report
- bounds-check report
- review evidence and optimization suggestions

The report is intentionally an estimate, not wall-clock timing. It exists to
make performance patches explainable and stable.

## Token Efficiency Benchmarks

Runtime performance and token efficiency are related but separate claims.
Runtime benchmarks estimate execution cost, while token-efficiency benchmarks
measure how much checked, reviewable behavior fits in source, AST, project-map,
and agent-context tokens.

The planned token-efficiency corpus is documented in
[docs/token_efficiency_benchmarks.md](token_efficiency_benchmarks.md). Its main
project is a butterfly-effect ray-grid simulation where a tiny initial angle
change sends a ray through a grid of balls and out through a different exit
cell. The same plan also lists smaller demos for parsers, packets, borrow
safety, effects, patches, and inline specs.

## Performance Reports

`fgx perf-report` emits `forgentex.perf_report.v1`. It compares scalar and
optimized sources across:

- `x86_64-unknown-linux-gnu` using `x86_64.avx2-review`
- `aarch64-unknown-linux-gnu` using `aarch64.neon-review`
- `riscv64gc-unknown-linux-gnu` using `riscv.vector-review`

The report includes a `patch_gate`. The gate passes only when correctness
passes and target-specific benchmark evidence exists for all three target
families.

## Media/DSP Benchmark Example

The first benchmark input is:

```text
examples/bench/audio_mix.json
```

It exercises both:

- `examples/media/audio_mix.fgx`
- `examples/media/audio_mix_simd.fgx`

The SIMD example still runs through the interpreter using its scalar fallback,
while the performance report models the target-specific SIMD path for review.

## Current Limits

Stage 25 does not collect real elapsed time or CPU counters. It also does not
perform profile-guided optimization. Those belong after native backend work.
The current purpose is to require correctness tests, target labels,
allocation/bounds visibility, and architecture-specific evidence for proposed
performance patches. Once generated Zig artifacts can be compiled and run,
performance reports can add backend-specific evidence, but those measurements
must still be gated by interpreter and reference-backend correctness.
