# C-ABI FFI Bridge (v2.13)

Native-class capsules now **execute for real** instead of mock. The typed
capsule boundary becomes a real `extern "C"` signature, the foreign code is
compiled and linked through the host toolchain, and each fixture runs as a
real execution oracle.

## How it works

```text
typed boundary  ->  extern "C" signature  ->  boundary.h
capsule body    ->  capsule.c  ->  cc -c  ->  capsule.o
generated harness (calls per fixture, against boundary.h)  ->  runner.o
link capsule.o + runner.o  ->  run  ->  compare stdout to fixture outputs
```

Because the harness compiles against the derived `boundary.h` and links
against the separately compiled `capsule.o`, the **C compiler itself enforces
the boundary**: if the body's signature does not match the typed boundary,
the link fails. The fixtures are no longer just type checks — they are run
for real and compared.

## Usage

```text
fgx capsule-run <file> --native [--capsule <name>] [--out <dir>] [--json]
```

## Live interpreter invocation (current pre-0.1 contract)

Scalar C boundaries, C++ boundaries explicitly exported as `extern "C"`, and
Forgentex-owned scalar slices or immutable/mutable reviewed-record slices are callable
by a running Forgentex program. Rust scalar, scalar-slice, immutable
or mutable record-slice, and restricted struct boundaries are live through a
public, no-mangle `extern "C"` function. Numeric scalar, scalar-slice, and
reviewed-record-slice or restricted struct Zig 0.14 boundaries are live through an explicit
`pub export fn`:

```text
fgx run examples/capsule/live_clamp_c.fgx --entry clamp_live --json-input examples/run/live_clamp_c.json --live
fgx run examples/capsule/sum_i16_c.fgx --entry sum_live --json-input examples/run/sum_i16_c.json --live
fgx run examples/capsule/add_gain_i16_c.fgx --entry boost_then_first --json-input examples/run/add_gain_i16_c.json --live
fgx run examples/capsule/add_gain_i16_cpp.fgx --entry boost_cpp_then_first --json-input examples/run/add_gain_i16_cpp.json --live
fgx run examples/capsule/render_params_c.fgx --entry render_param_score --json-input examples/run/render_params_c.json --live
fgx run examples/capsule/clamp_rust.fgx --entry clamp_with_rust --json-input examples/run/clamp_rust.json --live
fgx run examples/capsule/sum_i16_rust.fgx --entry sum_with_rust --json-input examples/run/sum_i16_rust.json --live
fgx run examples/capsule/add_gain_i16_rust.fgx --entry boost_rust_then_first --json-input examples/run/add_gain_i16_rust.json --live
fgx run examples/capsule/render_params_rust.fgx --entry adjust_render_params --json-input examples/run/render_params_rust.json --live
fgx run examples/capsule/clamp_zig.fgx --entry clamp_with_zig --json-input examples/run/clamp_zig.json --live
fgx run examples/capsule/sum_i16_zig.fgx --entry sum_with_zig --json-input examples/run/sum_i16_zig.json --live
fgx run examples/capsule/add_gain_i16_zig.fgx --entry boost_zig_then_first --json-input examples/run/add_gain_i16_zig.json --live
fgx run examples/capsule/render_params_zig.fgx --entry adjust_render_params_with_zig --json-input examples/run/render_params_zig.json --live
fgx run examples/capsule/make_bytes_c.fgx --entry bytes_from_c --json-input examples/run/make_bytes_c.json --live
fgx run examples/capsule/make_bytes_rust.fgx --entry bytes_from_rust --json-input examples/run/make_bytes_rust.json --live
fgx run examples/capsule/make_bytes_zig.fgx --entry bytes_from_zig --json-input examples/run/make_bytes_zig.json --live
fgx run examples/capsule/make_samples_c.fgx --entry samples_from_c --json-input examples/run/make_samples_c.json --live
fgx run examples/capsule/make_samples_rust.fgx --entry samples_from_rust --json-input examples/run/make_samples_rust.json --live
fgx run examples/capsule/make_samples_zig.fgx --entry samples_from_zig --json-input examples/run/make_samples_zig.json --live
fgx run examples/capsule/make_values_c.fgx --entry values_from_c --json-input examples/run/make_values_c.json --live
fgx run examples/capsule/make_values_rust.fgx --entry values_from_rust --json-input examples/run/make_values_rust.json --live
fgx run examples/capsule/make_values_zig.fgx --entry values_from_zig --json-input examples/run/make_values_zig.json --live
fgx run examples/capsule/make_ids_u64_c.fgx --entry ids_from_c --json-input examples/run/make_ids_u64.json --live
fgx run examples/capsule/make_ids_u64_rust.fgx --entry ids_from_rust --json-input examples/run/make_ids_u64.json --live
fgx run examples/capsule/make_ids_u64_zig.fgx --entry ids_from_zig --json-input examples/run/make_ids_u64.json --live
fgx run examples/capsule/make_offsets_isize_c.fgx --entry offsets_from_c --json-input examples/run/make_offsets_isize.json --live
fgx run examples/capsule/make_offsets_isize_rust.fgx --entry offsets_from_rust --json-input examples/run/make_offsets_isize.json --live
fgx run examples/capsule/make_offsets_isize_zig.fgx --entry offsets_from_zig --json-input examples/run/make_offsets_isize.json --live
fgx run examples/capsule/make_gains_f32_c.fgx --entry gains_from_c --live
fgx run examples/capsule/make_gains_f32_rust.fgx --entry gains_from_rust --live
fgx run examples/capsule/make_gains_f32_zig.fgx --entry gains_from_zig --live
fgx run examples/capsule/make_precise_f64_c.fgx --entry precise_from_c --live
fgx run examples/capsule/make_precise_f64_rust.fgx --entry precise_from_rust --live
fgx run examples/capsule/make_precise_f64_zig.fgx --entry precise_from_zig --live
fgx run examples/capsule/make_greeting_c.fgx --entry greeting_from_c --json-input examples/run/make_greeting_c.json --live
fgx run examples/capsule/make_greeting_rust.fgx --entry greeting_from_rust --json-input examples/run/make_greeting_rust.json --live
fgx run examples/capsule/make_greeting_zig.fgx --entry greeting_from_zig --json-input examples/run/make_greeting_zig.json --live
fgx run examples/capsule/persist_bytes_c.fgx --entry persist_bytes --json-input examples/run/persist_bytes_c.json --live
fgx run examples/capsule/record_slice_c.fgx --entry last_id_c --json-input examples/run/record_slice.json --live
fgx run examples/capsule/record_slice_cpp.fgx --entry last_id_cpp --json-input examples/run/record_slice.json --live
fgx run examples/capsule/record_slice_rust.fgx --entry last_id_rust --json-input examples/run/record_slice.json --live
fgx run examples/capsule/record_slice_zig.fgx --entry last_id_zig --json-input examples/run/record_slice.json --live
fgx run examples/capsule/mutate_records_c.fgx --entry mutate_then_last_c --json-input examples/run/mutate_records.json --live
fgx run examples/capsule/mutate_records_cpp.fgx --entry mutate_then_last_cpp --json-input examples/run/mutate_records.json --live
fgx run examples/capsule/mutate_records_rust.fgx --entry mutate_then_last_rust --json-input examples/run/mutate_records.json --live
fgx run examples/capsule/mutate_records_zig.fgx --entry mutate_then_last_zig --json-input examples/run/mutate_records.json --live
```

`fgx run --live` derives the same `extern "C"` declaration, compiles the
capsule and a generated one-call runner, then launches that runner as a child
process with a private line-delimited request file. C++ uses `$CXX` (then
`c++`, `clang++`, `g++`) for compilation and linking; its exported function
must use explicit `extern "C"` linkage. An owned C++ return additionally
requires the typed `<capsule>_free` export under explicit C linkage. Preflight
tokenizes declarations, ignores comments and string literals, and rejects
either missing linkage before compiler lookup. Rust uses `$RUSTC` (then `rustc`) and
requires edition `2018`, `2021`, or `2024` plus
`#[unsafe(no_mangle)] pub extern "C" fn` (the older `#[no_mangle]` spelling is
also accepted). This avoids OS command-line limits for
bounded slices. Its JSON scalar result returns to the
interpreter through the normal capsule-call path and appears in the
`capsule_calls` audit transcript. A missing language toolchain fails explicitly
with `native_runtime_unavailable` and never substitutes a mock result.

Zig live calls use `$ZIG` and then `zig`, require the capsule and detected host
toolchain to match the reviewed 0.14 line, and compile a private one-call
`ReleaseSafe` executable. Zig accepts numeric scalar inputs, read-only or
mutable numeric scalar slices and immutable or mutable reviewed-record slices as
pointer/length pairs, and one numeric scalar, restricted struct, reviewed owned numeric vector
(`Vec[u8]`/`Vec[i8]`/`Vec[u16]`/`Vec[i16]`/`Vec[u32]`/`Vec[i32]`/`Vec[u64]`/
`Vec[i64]`/`Vec[isize]`/`Vec[usize]`/`Vec[f32]`/`Vec[f64]`), one reviewed owned
record vector, or owned UTF-8
`str` return.
Mutable slices use the same bounded temporary allocation and atomic
copy-back envelope as C and Rust. A named struct must be `@repr(C)` in Forgentex and have an exact public `extern struct`
declaration in Zig with the same field order, names, types, and layout.
Struct fields may themselves be scalar primitives or other `@repr(C)` structs
(recursively), and may include recursively nested non-empty fixed-size arrays
of numeric scalars or reviewed structs under the shared 1,024-scalar-leaf and
16-array-dimension ceilings. Booleans, arbitrary pointers, and other exotic Zig
layouts remain blocked.

Each successful native call attaches `forgentex.native_receipt.v1` inside its
`capsule_calls` audit entry. The receipt records the FNV-1a hash of the derived
boundary signature, compiler name/version, elapsed native duration, the
effective `wall_limit_ms`, and whether it came from a `declared` limit or the
mandatory 30,000 ms `default`; it deliberately excludes the request payload.
`compile_wall_limit_ms: 60000` and `compile_containment: process_tree` record
the separate first-build policy. Compilation and linking run in a contained
process tree and are terminated if that ceiling expires.
The receipt records `containment: process_tree`. Windows uses a private Job
Object with kill-on-close; Unix uses a private process group. Descendants are
terminated when the runner exits or times out, so background work cannot
outlive a native call.

CPU and memory budgets are installed before the runner begins foreign work.
Declared `limit.cpu_ms` and `limit.mem_mb` values override mandatory defaults of
30,000 ms and 512 MiB. Receipts record `cpu_limit_ms`,
`memory_limit_bytes`, and a `default` or `declared` source for each. Windows
applies aggregate job CPU and committed-memory limits. To install those limits
*before* any foreign instruction runs, the
Windows runner is created suspended, assigned to its Job Object, and only then
resumed; this closes the spawn-to-assignment window in which a fast allocator or
busy loop could otherwise escape its budget, matching the Unix guarantee. Unix
descendants inherit `RLIMIT_CPU` and `RLIMIT_AS` in `pre_exec`; CPU time is
rounded up to the platform's whole-second granularity and the limits apply per
process rather than as an aggregate tree quota.
Budget exhaustion terminates or constrains the runner and currently surfaces
as `native_capsule_failed`.

On Windows, a declared `limit.fs_bytes` is also enforced as a conservative
aggregate Job I/O ceiling. Forgentex queries class-8 Job accounting every
sampling tick and once more before accepting a completed response; the counters
cover every process ever associated with the Job. The ceiling sums read,
write, and other transfer bytes, including the generated runner's request and
response traffic, so it may deny earlier than a path-only filesystem counter
would. Crossing it terminates the Job and reports `native_capsule_io_limit`.
Receipts expose `io_limit_bytes`, source `declared_fs_bytes`, and
`job_read_transfer_bytes`, `job_write_transfer_bytes`, and
`job_other_transfer_bytes`. Without a declaration, Windows still records the
counters with source `undeclared`. Ubuntu and macOS reject live-native
`limit.fs_bytes` until equivalent enforcement exists. `limit.net_bytes` is
rejected for live native on every platform rather than silently ignored.

Where the host accounts memory across the whole tree (the Windows Job Object),
successful receipts also record `peak_tree_memory_bytes`: the maximum aggregate
memory sampled across the process tree while it ran. It is a best-effort
observation — the Job Object peak reflects only currently-associated processes,
so the runner samples it live and keeps the maximum, and a spike shorter than
the sampling interval may be under-reported. Platforms without aggregate
whole-tree accounting report `null`.

Windows compiler Job Objects retain a mandatory 64-active-process ceiling so a
compiler driver may invoke its assembler and linker without unbounded fan-out.
A live native runner instead receives `process_limit: 1`: the direct runner is
the Job Object's only permitted process, so foreign code cannot execute a
subprocess. Successful receipts record `process_limit_source: default` and
sampled `peak_tree_processes: 1`. Unix currently records a null process
limit/peak with source `unavailable`; POSIX process groups provide termination
but not a portable aggregate count. That platform gap requires cgroup v2 or
explicit process-group accounting.

Every live runner also starts with its current directory set to its private
scratch directory instead of the Forgentex workspace. Unix clears the inherited
environment; only the detected toolchain directory may be restored as `PATH`.
Windows constructs a new block containing only `SystemRoot`/`WINDIR`,
AppContainer-scoped `LOCALAPPDATA`/`TEMP`/`TMP`, and that optional toolchain
path. Receipts expose
`runner_environment_policy: controlled_appcontainer_system_and_toolchain` on
Windows, `clear_except_toolchain_path` on Unix, and
`runner_working_directory_policy: private_scratch`.

On Windows, live runners are created suspended inside the stable per-user
`Forgentex.NativeRunner.0_1` AppContainer with zero capabilities, layered over
a restricted primary token. The token uses `DISABLE_MAX_PRIVILEGE` and
`LUA_TOKEN`, plus restricting SIDs for the current user, Builtin Users, and
Restricted Code. Forgentex reopens the suspended process token, requires the
expected AppContainer SID and zero capability count, requires
`IsTokenRestricted`, verifies that exact three-SID restricting profile,
rejects more than the single privilege that `DISABLE_MAX_PRIVILEGE` may
retain, and requires a low Mandatory Integrity Control label before assigning
the Job Object and resuming the process.
Receipts record `windows_restricted_token: true`,
`windows_restricting_sid_count: 3`, `windows_integrity_level: low`, and
`windows_appcontainer: true`, `windows_appcontainer_capability_count: 0`,
`windows_appcontainer_profile: Forgentex.NativeRunner.0_1`, and
`runner_process_identity_policy:
zero_capability_appcontainer_over_restricted_low_integrity_token`; Unix records
null Windows fields and `inherited_unix_identity`. Windows treats unlabeled
objects as medium integrity, so a low-integrity runner cannot modify ordinary
medium-integrity user data even if its DACL would otherwise allow the write.
The AppContainer makes access to host resources capability/ACL-based. Tests
verify denial of workspace read/write, a user-owned low-integrity-directory
write, live loopback connect, an unrelated named pipe, and host-user registry
modification.

The scratch directory itself is created fail-closed with no permissive parent
inheritance. Windows uses a protected, object/container-inheritable DACL that
grants full access to the object owner, LocalSystem, and the Restricted Code SID
used by the restricted runner, plus read/execute for the AppContainer package
SID; the descriptor is queried after creation and rejected unless
`SE_DACL_PROTECTED` is present. Ubuntu/macOS create and verify mode `0700`.
Receipts distinguish `runner_scratch_access_policy:
owner_system_restricted_code_appcontainer_rx_protected_dacl` from
`owner_mode_0700`.

Windows Jobs additionally install the complete `0xff` Basic UI restriction
mask before the suspended runner resumes. It denies access to external USER
handles, clipboard reads/writes, desktop creation/switching, global atoms,
display/system setting changes, and window-station shutdown. Successful
receipts record `windows_ui_restrictions: 255` and
`ui_isolation_source: job_object`; other platforms record `null` and
`unavailable`.

Live native dispatch is intentionally compute-only until broader OS isolation
lands. Declared filesystem reads/direct writes, network, subprocess,
environment, chain, and GPU capabilities fail preflight with
`native_capsule_capability_unsupported`, even when their sandbox grant is
otherwise well formed. The sole filesystem exception is `fs.write` attached to
governed `artifact.path` publication: Forgentex validates and publishes the
returned bytes after the foreign process exits, and the destination path never
crosses the native boundary. Receipts freeze this as
`runner_capability_profile: appcontainer_compute_or_host_artifact_only` on
Windows, with
`filesystem_isolation: appcontainer_default_deny_host_files_explicit_runner_rx`
and `network_isolation: appcontainer_zero_network_capabilities`. Unix retains
the narrower `compute_or_host_artifact_only`,
`private_working_directory_only`, and `unavailable` values until its OS
containment work lands.

Every contained native call also drains stdout and stderr concurrently under
one combined capture ceiling. The default ceiling is 402,653,184 bytes, large
enough for the worst-case textual expansion of the reviewed 64-MiB owned-value
wire formats; `sandbox "limit.output_bytes <bytes>"` may lower it to any
non-zero value. Concurrent draining prevents a child from deadlocking after an
OS pipe fills. Crossing the ceiling terminates the complete process tree,
discards the partial response, and reports `native_capsule_output_limit`.
Successful receipts expose `output_limit_bytes`, its `default` or `declared`
source, `stdout_bytes`, and `stderr_bytes`, never their contents.

Failed live native dispatches attach a `forgentex.native_failure.v1` receipt to
the attempted capsule-call audit entry. It contains a stable failure `kind`,
the `preflight`, `compile`, `launch`, `execute`, `response`, or `persist`
phase, elapsed time, effective wall policy, compiler ceiling,
effective CPU/memory/process-count/output limits plus their provenance, and
whether those limits and process-tree containment were active. The structured
receipt also records the environment/current-directory policy for runner-phase
failures and `null` before a runner starts. It deliberately
omits arguments, foreign output, and stderr; the caller-facing error remains a
separate diagnostic. A preflight failure cannot claim containment; compile
failures record compiler-tree containment, while runner timeouts and crashes
record runner-tree containment.

These canonical v1 shapes include the mandatory default limits and per-limit
provenance fields. An earlier in-repository draft used the same v1 identifier
without those fields, but it had no external consumers and was never part of
the public compatibility catalog. It is retired rather than supported through
a legacy branch; receipts generated before this consolidation are disposable
development artifacts.

An input declared with `sandbox "audit.redact <input-name>"` still crosses the
checked native request boundary unchanged, but its capsule-call transcript
slot becomes `{"redacted":true}` and the audit entry names the binding in
`redacted_inputs`. Redaction happens before both effect denial and dispatch, so
denied, failed, and successful attempts follow one rule. Invalid or unknown
input names fail `fgx check`. Pipeline reports also mask a redacted stage input
and its shared source value instead of reintroducing the value through the
composition audit. Failed redacted calls retain their stable failure kind but
replace foreign stderr in both the audit entry and caller diagnostic.

Warm live hosts now cache the compiled C/C++/Rust/Zig runner by a SHA-256 key
covering the checked program, capsule contract/body, toolchain, ABI signature,
language standard, and platform. The cache also records the SHA-256 of the
compiled executable itself. Every cache lookup hashes the current executable,
and every C/C++/Rust/Zig runner is hashed again immediately before launch. A
missing, unreadable, or changed cache entry is removed and rebuilt instead of
being executed. After the call, the runner remains cacheable only when its
bytes still match the pre-launch digest. Receipts expose the exact
`runner_artifact_sha256` plus `runner_cache_status` (`disabled`, `miss`,
`verified_hit`, or `rebuilt_after_integrity_mismatch`); a verified hit also
records `runner_reused: true`.

Runtime inputs are never part of the key or retained cache. The bounded wire
request crosses through an inherited anonymous stdin pipe for C/C++/Rust/Zig;
it is absent from both process arguments and the scratch filesystem. The host
closes the pipe after writing and removes its runner directories when it is
dropped. Receipts record `runner_input_transport: anonymous_stdin`. Each call
still starts a fresh process under the normal containment and effective
default-or-declared wall/CPU/memory/output limits; this is compile reuse, not a
persistent foreign process. Receipts freeze this policy as
`runner_execution_model: fresh_process_per_call`; persistent native workers are
explicitly rejected because they cannot yet preserve these per-call Job
Object/process-group guarantees. `--cold` keeps the original compile-per-call
behavior, with compiler containment and its hard ceiling.

A read-only `&[T]` input with a scalar element type lowers to two C parameters:
`const T *name, size_t name_len`. A mutable `&mut [T]` lowers to
`T *name, size_t name_len`. The child runner validates every element, rebuilds
a bounded temporary buffer, calls the capsule, emits a typed result envelope,
then emits one validated copy-back envelope per mutable slice before freeing
the buffer. The scalar result and every copy-back envelope validate as one
response; copy-back must have the original length and scalar element type.

A `str` input lowers to `const uint8_t *name, size_t name_len` in C/C++,
`*const u8, usize` in Rust, and `[*]const u8, usize` in Zig. It is a read-only
UTF-8 borrow valid only for the call; the capsule must not retain or free it.
The value is bounded to 16 MiB, hex-framed through the private request file so
NULs/newlines cannot alter the envelope, and reconstructed in runner-owned
memory. The complete encoded request is capped at 64 MiB before compiler
lookup. Successful receipts record only the aggregate `borrowed_input_bytes`.
See `docs/native_string_inputs.md`.

Rust uses the same ownership contract with an explicit export signature:
read-only slices receive `*const T, usize`, mutable slices receive
`*mut T, usize`, and the capsule may construct a Rust slice only for the call's
duration. The generated child owns the temporary `Vec<T>`. A mutable vector is
serialized only after the call, then Forgentex validates the complete result
and every fixed-length mutation before replacing interpreter state. Retaining
either pointer is forbidden.

An owned numeric vector output lowers to the matching
`FgxOwned* { ptr, len, capacity }` descriptor in C, C++, Rust, or Zig. The reviewed
set is `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `isize`, `usize`,
`f32`, and `f64`. `u64` and 64-bit `usize` preserve their complete unsigned
ranges through an exact runtime carrier. `isize`
and `usize` use the host ABI pointer width (`ptrdiff_t` and `size_t` in C) and
are admitted only on the reviewed 64-bit native host matrix. Every layout requires a typed
`<capsule>_free(ptr, len, capacity)` export in the same allocator domain. The
descriptor length and capacity count elements; the child rejects null non-empty
descriptors, capacities smaller than length, and allocations whose byte size
exceeds 64 MiB. It copies the typed elements before calling the capsule
deallocator exactly once. Integers are range-checked;
floats use language-specific round-trip-safe decimal formatting, narrow to the
declared width, and reject NaN/infinity. The receipt records the actual byte
footprint in `owned_output_bytes`; its boundary hash covers both exports.
Unsigned values remain exact through decimal wire parsing, interpreter
arithmetic/comparison, JSON, and canonical little-endian pipeline receipts. See
`docs/native_owned_outputs.md`.

One owned vector of a named, pointer-free scalar-leaf `@repr(C)` record is also
reviewed. `Vec[NativeRecord]` lowers to
`FgxOwnedNativeRecord { NativeRecord *ptr, size_t len, size_t capacity }` in C/C++
and the exact corresponding public descriptor in Rust/Zig. The same recursively
composed struct and fixed-array validation used for by-value records applies to
each element. The 64 MiB limit uses the reviewed host ABI record size. The child
copies complete records before exactly-once same-domain deallocation; the parent
requires complete leaf groups, validates every scalar, rebuilds nested values
atomically, and restores record tags. Record vectors are typed values, never byte
artifacts. See `make_records_{c,cpp,rust,zig}.fgx`.

An owned `str` output uses `FgxOwnedStr { ptr, len, capacity }`, where length
and capacity count bytes. The isolated child copies and frees the allocation,
then the parent validates UTF-8 before constructing the language string. Bytes
cross the child envelope numerically, so embedded commas, newlines, NULs, and
multibyte characters cannot change its structure. Owned strings record their
byte footprint but are not artifacts and cannot opt into artifact policy.

An owned byte output may additionally declare `artifact.media_type` and
`artifact.max_bytes` sandbox rules. They are validated before compiler lookup,
the declared ceiling is enforced before the value reaches the interpreter,
and successful calls attach `forgentex.native_artifact.v1` metadata containing
the media type, byte length, and SHA-256 content hash. The boundary hash also
covers this policy. This is in-memory artifact identity and policy, not file
persistence.

Adding `artifact.path` turns that identity into governed persistence. The path
must be relative and contained by a declared relative `fs.write` scope, and the
capsule/caller effect chain must include `fs.write`. Forgentex publishes the
validated bytes atomically from the parent; the foreign child never sees the
path. Identical content is idempotent, while an existing different artifact
fails closed. The receipt records the persisted path.

A `@repr(C)` named struct can cross **by value** as an input or the one return
value when every leaf field is a supported scalar primitive. Fields may
recursively reference other reviewed `@repr(C)` structs and may contain
recursively nested non-empty fixed-size arrays of either scalar leaves or
reviewed structs.
Forgentex generates dependency-ordered `types.h` declarations for the private
C/C++ child build; that capsule body must include it before defining the
matching function. Rust and Zig capsule code must declare the same exact public
layout, including each array length and element type. The contract is rejected
before compiler lookup when declarations differ. Inputs flatten in declaration
and array-index order. The child serializes every returned leaf and validates
the complete value before the interpreter rebuilds arrays and recursively
restores every declared struct tag. Repeated elements count independently
toward the 1,024-leaf ceiling, and array nesting deeper than 16 dimensions fails
closed. A result struct can therefore carry several logical scalar results atomically; see
`examples/capsule/dual_output_c.fgx` and
`examples/capsule/struct_array_c.fgx` and
`examples/capsule/nested_array_c.fgx`. Pointers, strings,
packed/custom-aligned layouts, field endian annotations, nested ownership, and
several independent outputs remain deliberately blocked.

An immutable `&[Record]` input uses that same reviewed struct layout through a
call-scoped pointer and element count: `const Record *` in C/C++, `*const Record`
in Rust, and `[*]const Record` in Zig. The interpreter validates every record
tag, field, nested fixed array, and scalar leaf before flattening the private
wire. The child requires complete leaf groups, reconstructs a runner-owned
typed array, and frees it after the call; foreign code must not retain the
pointer. A one-million-record ceiling is further reduced by
`64 MiB / sizeof(Record)`, and multiple aggregate inputs share the 64 MiB ABI
reservation. Successful receipts add their ABI footprint to
`borrowed_input_bytes` without recording payloads.

A mutable `&mut [Record]` uses a non-const `Record *` in C/C++, `*mut Record`
in Rust, and `[*]Record` in Zig. It shares the immutable record input's layout,
record-count, ABI-byte, and complete-leaf-group checks. After the call, the
runner serializes every record leaf in declaration/index order; the parent
requires the original record count, validates every scalar range and finite
float, and recursively rebuilds fixed arrays, nested records, and record tags.
The record slice participates in the same complete non-aliased mutation
transaction as scalar slices and mutable strings, so malformed later copy-back
withholds all earlier staged mutations. The runner owns the only native record
array, and foreign code must not retain its pointer.

For mutable inputs, including fixed-byte-length `&mut str` and bounded
resizable `&mut String`, the Forgentex call argument must be a direct variable.
The runtime replaces that variable only
after the complete child response validates, so an array literal, indexed
expression, invalid UTF-8 mutation, out-of-capacity length, or retained foreign
pointer cannot become a hidden write path. The capsule never receives an
interpreter-owned pointer and must not retain the temporary pointer after the
call. `&mut String` receives a zero-initialized 16 MiB runner buffer through a
pointer, mutable byte length, and immutable capacity; aggregate reserved
capacity is capped at 64 MiB before compiler lookup. Calls with several mutable
inputs stage and validate the complete copy-back set before the first state
write: every declared input must appear exactly once, targets may not alias,
types, fixed lengths, and resizable capacities must still match, and any failure
withholds the entire set.

This is intentionally C/C++/Rust/Zig scalar values, borrowed, fixed-length
mutable, and bounded resizable UTF-8 strings, scalar slices, recursively
composed scalar-leaf structs with bounded arrays of scalars or reviewed
structs, plus one owned reviewed
numeric vector, reviewed pointer-free record vector, or UTF-8 `str` output.
Persistent native workers,
retained strings, record-vector pipeline handoff,
multiple owned outputs, GPU
dispatch, aggregate Unix tree budgets, filesystem/network byte budgets, and
broader OS-level sandbox enforcement remain later slices. A declared `limit.wall_ms`,
or the mandatory 30,000 ms default when absent, terminates the native process
tree and reports `native_capsule_timeout`. The child process
provides interpreter-process isolation; it is not a substitute for an OS
sandbox. A non-zero child exit (including an abort/crash) is contained and
reported as `native_capsule_failed`; its failure receipt records the stable
class and containment evidence without copying the raw diagnostic.

Cross-platform verification commands and expected differences are maintained in
[`native_containment_platform_verification.md`](native_containment_platform_verification.md).

```text
$ fgx capsule-run examples/capsule/clamp_c.fgx --native
Forgentex C-ABI FFI bridge
schema=forgentex.ffi_plan.v1 file=examples/capsule/clamp_c.fgx capsule=@capsule.clamp_sample lang=c class=native
  extern "C": int16_t clamp_sample(float x)
    in   f32          -> float      x
    out  i16          -> int16_t    y
  gate: allow effects=[cpu.avx2] grants=[fs.none] deps_pinned=true
  toolchain: cc (cc (GCC) 15.2.0)
  link: build/ffi/capsule.o + build/ffi/runner.o
  execution: executed (3/3 fixtures)
    [0] (40000.0) -> 32767 expected 32767 ok
    [1] (-40000.0) -> -32768 expected -32768 ok
    [2] (100.0) -> 100 expected 100 ok
```

## The gate (real, not review)

Linking and execution are refused unless:

- the capsule passes `fgx check` — its effects are covered by sandbox grants
  and its contract is valid, and
- its dependencies are pinned (the v2.3 supply-chain rule).

A denied capsule reports `gate_decision=deny` with reasons and never compiles
or runs. This moves the effects/sandbox/lock spine from review-time to
**runtime enforcement**.

## The report: `forgentex.ffi_plan.v1`

| Field | Meaning |
| --- | --- |
| `extern_c_signature` | the C-ABI signature derived from the typed boundary |
| `marshaling` | per binding: Forgentex type → C type, in/out |
| `gate_decision` / `gate_reasons` | allow/deny + why (effects, grants, deps) |
| `link_units` | the objects linked (`capsule.o`, `runner.o`) |
| `execution_status` | `executed`, `mismatch`, `denied`, `compiler_unavailable`, `compile_failed`, `unsupported_boundary`, `execution_unsupported_in_slice` |
| `transcript` | per fixture: inputs → output vs expected, pass |

## Scope and honesty (first slice)

- **Language:** C directly, C++ with an explicit `extern "C"` export, Rust
  scalar/slice/struct live calls with an explicit public no-mangle C ABI export,
  and pinned Zig 0.14 numeric scalar/slice/struct/owned-vector calls through `pub export fn`.
  Another native capsule still produces the plan and reports
  `execution_unsupported_in_slice`.
- **Boundary:** C/C++/Rust/Zig scalar inputs/outputs, borrowed UTF-8 `str`,
  fixed-byte-length mutable UTF-8 `&mut str`, bounded resizable UTF-8
  `&mut String`, scalar-element `&[T]`/`&mut [T]` inputs, immutable
  `&[Record]`, mutable `&mut [Record]`, and reviewed scalar-leaf
  `@repr(C)` structs by value as inputs or the one return value. Structs may
  recursively contain reviewed structs; one result struct may carry several
  logical scalar values. Each language permits one scalar, struct, reviewed
  owned numeric vector, owned reviewed record vector, or owned UTF-8 string return. Mutable slices and
  `&mut str` use fixed-length copy-in/copy-back; `&mut String` may change its
  byte length inside the runner-owned capacity. Mutable strings require
  complete UTF-8 validation before state replacement;
  owned vectors use the explicit typed descriptor/free contract. Record vectors
  retain the by-value struct layout restrictions and are byte-bounded by ABI size. Governed
  artifact metadata and scoped immutable persistence remain byte-vector-only.
- **No host compiler** → `compiler_unavailable`, gracefully (CI needs no
  compiler; tests assert the deterministic plan and accept either outcome).
- This builds directly on v2.12: the same host-toolchain detection, and the
  `capsule.o` is the kind of unit the native build links.
