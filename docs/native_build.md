# Native Build Core (v2.12)

The first real backend. Forgentex's own pure/numeric core becomes a real
native artifact — not interpreted, not transpiled-and-left-as-source, but a
genuine object file produced by a real compiler.

## How it works

We do **not** write a code generator. We reuse the existing C transpiler to
emit C, then drive the **host C compiler** to produce the artifact:

```text
.fgx  ->  (existing C transpiler)  ->  C source  ->  host cc -c  ->  .o
```

The compiler is detected at use time — `$CC` first, then `cc`, `clang`,
`gcc` — so the Rust toolchain stays dependency-free. If none is found, the
build degrades gracefully to status `compiler_unavailable` (still emitting
the C and the deterministic facts) and never crashes; CI does not need a C
compiler.

If the compiler is given by an absolute path (for example
`$CC=C:\w64devkit\bin\gcc.exe`), Forgentex adds that directory to the child
process's `PATH` so the compiler can find its sibling tools (the assembler
and linker).

## Usage

```text
fgx build <project> --native [--emit obj|shared] [--out <dir>] [--json]
```

- `--emit obj` (default) — a relocatable object file (`cc -c`). Always valid
  for a library of functions, and exactly the linkable unit the v2.13 FFI
  bridge will link against.
- `--emit shared` — a shared library (`cc -shared -fPIC`).

Example:

```text
$ fgx build examples/native --native
Forgentex native build
schema=forgentex.native_build.v1 project=examples/native status=built kind=object
  toolchain: cc (cc (GCC) 15.2.0)
  functions=4 effects=[] exact=[clamp_i32, mix_avg_i32, scale_q15]
  c_source_hash=b2e11ec614bde52f c_header_hash=1bfb63fb3c184ac4
  command: cc -std=c11 -O2 -c -I build/native build/native/native_demo.c -o build/native/native_demo.o
  artifact: build/native/native_demo.o (1100 bytes, hash 96384b6f65a322c2)
```

The `export`ed functions keep external linkage, so they are real, visible
symbols in the object:

```text
$ nm build/native/native_demo.o
T clamp_i32
T gain_fast
T mix_avg_i32
T scale_q15
```

## The report: `forgentex.native_build.v1`

| Field | Meaning |
| --- | --- |
| `status` | `built`, `compiler_unavailable`, or `failed` |
| `toolchain` | detected compiler name + version line |
| `command` | the exact compiler invocation |
| `c_source_hash` / `c_header_hash` | FNV-1a of the generated C — the **deterministic, testable** surface |
| `artifact_path` / `artifact_bytes` / `artifact_hash` | the produced object (informational; bytes depend on the host toolchain) |
| `functions` | function count compiled in |
| `effects` | union of declared effects compiled in |
| `exact_functions` | `@exact` functions honored (bit-exact integer/fixed-point) |

## What is and isn't deterministic

The generated **C source hash is deterministic** — same program, same C,
same hash, on any machine. The **object bytes are not** reproducible across
machines (they depend on the host compiler and flags), so the report records
the toolchain identity instead of pretending otherwise. Tests assert on the
C-source hash and the program facts, and accept either `built` or
`compiler_unavailable` for the artifact, so they pass with or without a
compiler installed.

## Scope and honesty

- No custom codegen and no optimizer — we inherit the host compiler's.
- `@exact` is honored: those functions use the exact integer path and are
  bit-exact; the fast float path stays the default for everything else.
- Functions that the C backend cannot lower (assembly, `unsafe`, runtime
  effects like `io`/`fs`/`network`) are not part of the native core; they
  belong to the capsule/FFI track (v2.13+).
- This is the keystone: the object file produced here is the unit the v2.13
  C-ABI FFI bridge links foreign C/C++/Rust/Zig capsules into.
