# Julia and MoonBit capsules

Forgentex runs capsules written in Julia and MoonBit for real, behind the same
typed boundary as the other managed runtimes: a JSON argument array goes in on
stdin, the result comes back on a nonce-tagged line, and anything the body
prints is kept as diagnostics (`src/managed_protocol.rs`). Both run under the
managed containment layer (`src/managed_exec.rs`): wall deadline, CPU and
output budgets, process-tree cleanup, and an explicit environment.

Each one answers a different need:

- **Julia** gives numerical code with the host's Julia and its standard
  library (`LinearAlgebra`, `Statistics`, …), in several Julia versions side
  by side.
- **MoonBit** gives compute capsules that are actually **sandboxed**:
  WebAssembly, run in `moonrun` with every host surface denied. Native and
  other managed capsules are not isolated on Linux; MoonBit capsules are.

| | Julia | MoonBit |
| --- | --- | --- |
| Language name | `julia` | `moonbit` |
| Runs on | the host `julia` | `moon` builds `wasm-gc`; `moonrun` runs it |
| `version` | Julia `[compat]` rules: `1.10` = 1.10 or later 1.x, `~1.10`, `=1.10.12`, `>=1.11` | moonc version: `0.10` = the 0.10.x series, `>=0.10`, `0.10.15` |
| Oldest supported | Julia 1.10 (the LTS) | moonc 0.10 |
| Boundary types | integers, floats, bools, strings, arrays of them (nested), and structs (as a `NamedTuple`) | integers, floats, bools, strings, and arrays of them (nested) |
| Host access | the user's authority, like Python | none: files, network, environment, and processes are denied |
| Cold call | ≈ 0.25–0.3 s (Julia start-up and compilation) | built once per process, then ≈ 10 ms per call (a three-fixture run took 0.16 s including the build) |
| Warm worker | yes — methods stay compiled across calls | not needed |
| Fixtures | `fgx capsule-run <file> --julia` | `fgx capsule-run <file> --moonbit` |

## Julia

```text
foreign rms_norm julia version "1.10" effects [alloc]:
    input x: &[f64]
    input eps: f64
    output norm: f64
    sandbox "fs.none"
    code:
        using LinearAlgebra

        square(v) = v * v

        function rms_norm(x::Vector{Float64}, eps::Float64)::Float64
            sqrt(sum(square, x) / length(x) + eps)
        end
```

- **Entry point.** The function named like the capsule, otherwise the body's
  first top-level function (`function f(…)` or `f(…) = …`). Helpers may come
  first.
- **Typed arguments.** The driver converts each argument to its declared type
  before the call — `f64` to `Float64`, `i32` to `Int32`, `&[f64]` to
  `Vector{Float64}` — so methods with typed signatures dispatch even when JSON
  wrote `100` for a float. A struct input arrives as a `NamedTuple` (`p.x`).
- **Results.** Numbers, strings, `Bool`, `nothing`, arrays, tuples, `Dict`s,
  `NamedTuple`s, and plain structs are encoded as JSON. A non-finite float is
  an error, not invalid JSON.
- **JSON.** Julia's standard library has no JSON module, so every driver
  carries a small codec (`FgxWire` in `src/julia_run.rs`). Capsules need no
  packages to cross the boundary.
- **Output and errors.** The body's `print`/`println` goes to stderr and is
  kept as diagnostics. An uncaught exception fails the call with
  `julia capsule '<name>' raised: <error>`.
- **Start-up.** Julia runs with `--startup-file=no --history-file=no`, so a
  user's `startup.jl` cannot change what a capsule runs. The environment keeps
  only `JULIA_DEPOT_PATH` and the home-directory variables, so installed
  packages remain loadable. `JULIA_LOAD_PATH` and `JULIA_PROJECT` are
  dropped, as `PYTHONPATH` is for Python.

### Several Julia versions

A capsule runs on the newest installed Julia its `version` admits. `$JULIA`
and then `julia` on `PATH` are used when they satisfy it; otherwise Forgentex
searches juliaup's installs (`~/.julia/juliaup/julia-*`) and official
tarballs unpacked as `julia-*` under `~/.local/opt`, `~`, or `/opt`. So
`version "~1.10"` runs on 1.10.x while another capsule in the same program
uses 1.13. `fgx doctor` lists every Julia it finds. A Julia that does not
satisfy the declaration is reported as
`declares Julia ~1.10 (…), found 1.13.2`.

## MoonBit

```text
foreign fletcher16 moonbit version "0.10" effects [alloc]:
    input data: &[i32]
    output checksum: i32
    sandbox "fs.none"
    code:
        fn fletcher16(data : Array[Int]) -> Int {
          let mut sum1 = 0
          let mut sum2 = 0
          for byte in data {
            sum1 = (sum1 + (byte & 255)) % 255
            sum2 = (sum2 + sum1) % 255
          }
          (sum2 << 8) | sum1
        }
```

- **Entry point and types.** The `fn` named like the capsule, otherwise the
  first top-level `fn`. Boundary types map as follows. 64-bit integers are
  decoded from the number's source text, so they stay exact.

  | Forgentex | MoonBit |
  | --- | --- |
  | `i8`–`i32`, `u8`, `u16` | `Int` |
  | `i64`, `isize` | `Int64` |
  | `u32` | `UInt` |
  | `u64`, `usize` | `UInt64` |
  | `f32` | `Float` |
  | `f64` | `Double` |
  | `bool` | `Bool` |
  | `str` | `String` |
  | `&[T]`, `Vec[T]`, `[T; N]` | `Array[T]` |

  Struct boundaries are refused, naming the supported types.
- **Build.** Forgentex generates a `main` around the body and builds it as a
  standalone file with `moon run <file> --target wasm-gc --build-only`. No
  project files are needed, so `moon`'s changing manifest format does not
  matter. The body must not define its own `main`. A module is cached in memory
  per wrapper source and toolchain, so a program calling a capsule repeatedly
  builds it once.
- **Output and errors.** The body's `println` lines stay untagged and become
  diagnostics. A runtime fault (an out-of-bounds index, `abort`) fails the
  call with `moonbit capsule '<name>' failed: <trace>`.

### The sandbox

1. **Effects.** A capsule declaring host effects or grants (`fs.*`, `net.*`,
   `env.*`, `proc.*`, `gpu.*`, `chain.*`) is refused before anything is
   built. They could never work in the sandbox, so failing up front is
   clearer than failing inside it.
2. **Imports.** The policy below does not cover WASI, and `moonrun`
   implements WASI calls such as `path_open`, so the compiled module's import
   section is read before it runs (`moonbit_run::wasm_imports`). It may
   import only `spectest.read_char` and `spectest.print_char`, the console the
   wrapper uses. Anything else is refused with
   `moonbit_capsule_import_denied`: a body using `@env`, or one declaring its
   own `wasi_snapshot_preview1` import. Ordinary compute — arithmetic, math,
   strings, arrays, maps, sorting, JSON — imports nothing else.
3. **Policy.** `moonrun --policy` with an empty policy file denies files,
   network, environment variables, and process spawning. With it, a program
   sees no environment variables, cannot read its working directory, and
   cannot open files.

The managed containment layer still applies around `moonrun`: wall deadline,
CPU and output budgets, and a scrubbed environment.

### MoonBit toolchains

Forgentex looks for `$MOON`, `moon` on `PATH`, `$MOON_HOME/bin`, and
`~/.moon/bin`, and uses the first whose `moonc` satisfies the capsule's
`version`. An install's core library is found beside its `bin`, so a toolchain
outside `~/.moon` works without setting `MOON_HOME`. `moonrun` must sit
beside `moon`.

## Verification

Both bridges were verified on Ubuntu 26.04 with Julia 1.10.12, 1.12.7, and
1.13.2, and MoonBit (moon 0.1.20261012, moonc 0.10.15). The Julia tests passed
with each Julia as the `PATH` Julia, and with all three installed side by side
— each `~X.Y` capsule ran on its own series. Tests live in
`src/tests/julia_run.rs` and `src/tests/moonbit_run.rs`. They return early on
a host without the toolchain, as the other managed-runtime tests do. The
experiments behind the design are recorded in
[linux_and_zig_0_17_experiments.md](linux_and_zig_0_17_experiments.md) § 9.
