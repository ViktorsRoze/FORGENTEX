# Native containment platform verification

This matrix verifies the platform-specific enforcement behind live native
capsule receipts. Run it from the repository root at the exact commit being
evaluated. The tests require a working C compiler and intentionally launch only
bounded local child processes; they do not access the network.

Record the environment before testing:

```sh
git rev-parse HEAD
uname -a
rustc --version
cargo --version
cc --version
```

Then run:

```sh
cargo test native_runner_applies_default_cpu_memory_and_output_budgets -- --nocapture
cargo test successful_native_receipt_records_applied_resource_budgets -- --nocapture
cargo test native_cpu_budget_terminates_busy_work_before_wall_fallback -- --nocapture
cargo test native_memory_budget_prevents_unbounded_committed_allocation -- --nocapture
cargo test contained_runner_ -- --nocapture
cargo test live_c_capsule_receipt_is_rendered_in_json_audit -- --nocapture
cargo test live_c_runner_removes_ambient_environment_and_workspace_authority -- --nocapture
cargo test live_native_preflight_rejects_unisolated_os_capabilities -- --nocapture
cargo test live_native_preflight_rejects_unenforced_byte_limits -- --nocapture
cargo test tampered_cached_native_runner_is_rebuilt_before_execution -- --nocapture
cargo test live_native_request_uses_anonymous_stdin_without_a_scratch_file -- --nocapture
cargo test live_native_scratch_has_protected_access_before_runner_creation -- --nocapture
```

All commands must exit zero. The receipt tests prove:

- undeclared runner budgets are 30,000 ms CPU, 512 MiB memory, and 402,653,184
  combined output bytes, each with source `default`;
- declared CPU/memory limits replace the defaults and report source `declared`;
- the busy runner is terminated before its wall fallback;
- the allocation runner cannot commit unbounded memory;
- a completed or terminated direct child cannot leave a background descendant;
- JSON audit output uses `forgentex.native_receipt.v1` and includes provenance
  for wall, CPU, memory, process-count, and output limits;
- every receipt records `runner_execution_model: fresh_process_per_call`, even
  when `runner_reused` shows that compilation was cached;
- every executed runner records a 64-hex-character
  `runner_artifact_sha256`; the tamper test must rebuild changed cache bytes,
  report `rebuilt_after_integrity_mismatch`, and then produce a
  `verified_hit` on the next call;
- request values travel through bounded anonymous stdin rather than argv or a
  scratch file; the transport test must consume a real value, report
  `runner_input_transport: anonymous_stdin`, and prove `request.txt` is absent
  while foreign code runs;
- live runners clear the ambient home/profile environment and use a private
  scratch current directory rather than the workspace;
- scratch excludes inherited peer access before compilation: Windows must
  report and verify
  `owner_system_restricted_code_appcontainer_rx_protected_dacl`, while
  Ubuntu/macOS must report `owner_mode_0700` and observe mode `0700`;
- direct filesystem, network, subprocess, environment, chain, and GPU
  capabilities fail before compiler lookup; governed host-owned artifact
  publication is the only filesystem-capability exception.

## Platform-specific expectations

### Ubuntu/Linux

The child creates a private process group. `RLIMIT_CPU` and `RLIMIT_AS` are
installed in `pre_exec` and inherited by descendants before foreign code runs.
`peak_tree_memory_bytes` remains `null`: these rlimits apply per process and do
not provide an aggregate process-tree high-water mark. Closing that separate gap
requires cgroup v2 accounting or explicit process-group sampling.
`process_limit` and `peak_tree_processes` are also `null`, with
`process_limit_source: unavailable`; the commands above verify this truthful
receipt shape but do not prove a Unix process-count ceiling.

Remaining Ubuntu work:

1. Create a delegated cgroup v2 subtree per compiler/runner call and install
   `cpu.max`, `memory.max`, and `pids.max` before foreign code can execute.
2. Read cgroup CPU, memory-peak, and process-count evidence into receipts and
   prove descendant-heavy adversarial cases cannot exceed aggregate budgets.
3. Add user/mount/network namespaces with a scratch-only writable view,
   read-only runtime dependencies, and no network namespace connectivity;
   add seccomp denial for unsupported process/socket operations.
4. Change filesystem/network receipt fields only after absolute-path and
   loopback denial tests pass on the supported Ubuntu CI image.
5. Add an enforceable aggregate filesystem-I/O byte ceiling, then replace the
   current expected `native_capsule_limit_unsupported` result for
   `limit.fs_bytes` with descendant-heavy enforcement evidence.

### macOS

The child creates a private process group and receives inherited `RLIMIT_CPU`
and `RLIMIT_AS` before `exec`. As on Linux, `peak_tree_memory_bytes` remains
`null` because the current implementation has no aggregate tree accounting.
Record any host-specific `RLIMIT_AS` behavior or test skip rather than treating
it as equivalent to Windows aggregate committed-memory enforcement.
As on Linux, process-count limit/peak fields must remain null with source
`unavailable` until aggregate process-group enforcement lands.

Remaining macOS work:

1. Add aggregate descendant CPU/memory/process accounting around the private
   process group and verify both Intel and Apple Silicon behavior, including
   the existing `RLIMIT_AS` caveat.
2. Select a supported, distributable macOS sandbox mechanism for scratch-only
   filesystem access and network denial. Do not treat deprecated
   `sandbox-exec` experiments as production proof.
3. Add absolute-path, loopback-socket, descendant-budget, timeout, and cleanup
   adversarial tests before changing the truthful `unavailable` receipt fields.
4. Add aggregate filesystem-I/O byte accounting and enforcement before
   accepting live-native `limit.fs_bytes`.

### Windows

The runner starts suspended, is assigned to a private Job Object with aggregate
CPU and committed-memory limits, then resumes. Job close/termination kills the
complete tree. Successful sustained-memory tests should report a non-null
`peak_tree_memory_bytes` no greater than the effective memory budget.
Before Job assignment, the suspended process is created inside the stable
per-user `Forgentex.NativeRunner.0_1` AppContainer with no capabilities,
layered over a restricted primary token using `DISABLE_MAX_PRIVILEGE`,
`LUA_TOKEN`, and current-user, Builtin-Users, and Restricted-Code restricting
SIDs. Forgentex queries that process token before resume and fails closed
unless the kernel reports the expected AppContainer SID, zero capabilities, a
restricted token carrying exactly those three restricting SIDs, at most one
enabled privilege, and a low Mandatory Integrity Control label. Microsoft
documents AppContainer profile/capability isolation and that low-integrity
processes cannot write medium-integrity data even when a DACL would permit the
write; unlabeled objects are treated as medium. This follows the documented
[`AppContainer` implementation](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer),
[`CreateAppContainerProfile`](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-createappcontainerprofile),
[`CreateRestrictedToken`](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken)
and
[`CreateProcessAsUserW`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw)
path plus
[Mandatory Integrity Control](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control).
[Microsoft documents](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_and_io_accounting_information)
that class-8 Job I/O accounting includes current processes and every process
previously associated with the Job; Forgentex uses those cumulative counters.
Compiler Job Objects have a mandatory 64-active-process ceiling. Live runner
Job Objects have a stricter one-process ceiling, so a foreign child may be
created only to be terminated before it executes. Native scratch creation
installs and verifies a protected
owner/LocalSystem/Restricted-Code/AppContainer inheritable DACL before the
compiler or runner receives the path. The AppContainer receives read/execute,
not write, access to the cached runner. Run these additional Windows-only
adversarial checks:

```powershell
cargo test contained_runner_enforces_the_windows_active_process_limit -- --nocapture
cargo test contained_runner_reports_windows_peak_process_count -- --nocapture
cargo test contained_runner_installs_complete_windows_ui_restrictions -- --nocapture
cargo test windows_native_fs_byte_limit_bounds_aggregate_job_io -- --nocapture
cargo test windows_live_native_runner_uses_a_kernel_verified_restricted_token -- --nocapture
cargo test windows_low_integrity_runner_cannot_modify_a_medium_integrity_workspace_file -- --nocapture
cargo test windows_appcontainer_runner_cannot_read_a_workspace_file -- --nocapture
cargo test windows_appcontainer_cannot_write_a_user_owned_low_integrity_directory -- --nocapture
cargo test windows_zero_capability_appcontainer_cannot_connect_to_loopback -- --nocapture
cargo test windows_appcontainer_cannot_open_an_unrelated_named_pipe -- --nocapture
cargo test windows_appcontainer_cannot_modify_the_host_user_registry -- --nocapture
```

All eleven must pass. Successful native receipts must record `process_limit: 1`,
source `default`, and `peak_tree_processes: 1`. They must also record
`windows_ui_restrictions: 255` with source `job_object`. The portable
ambient-authority test above additionally attempts a Windows child side effect
and must prove that the child never executes. The I/O test must terminate a
foreign writer above its declared conservative aggregate ceiling and record
`native_capsule_io_limit`. The token test must record
`windows_restricted_token: true` and
`windows_restricting_sid_count: 3`, `windows_integrity_level: low`, and
`runner_process_identity_policy:
zero_capability_appcontainer_over_restricted_low_integrity_token`. They must
also record `windows_appcontainer: true`,
`windows_appcontainer_capability_count: 0`, and
`windows_appcontainer_profile: Forgentex.NativeRunner.0_1`. The integrity
test must prove that the foreign runner cannot modify an existing
medium-integrity workspace file and that its original bytes remain unchanged.
The AppContainer tests additionally prove ordinary workspace-read,
user-owned/low-integrity-directory write, live loopback-connect, unrelated
named-pipe, and host-HKCU write denial.

Remaining Windows work:

1. Repeat the eleven-test matrix on each supported Windows 10/11 build and record
   the exact build/compiler evidence.
2. Add a hermetic external-network attempt that does not depend on public
   network availability.
3. Expand undeclared IPC coverage beyond named pipes (for example RPC/COM
   activation) without granting a general capability SID.
4. Decide and document profile cleanup/migration policy before renaming the
   stable AppContainer profile. Deleting it must never occur while runners are
   active.

## Result report

For Ubuntu and macOS, retain the twelve command results plus the environment
block.
Report failures with the full test name, exit status, and assertion/error text.
Do not attach generated runner binaries or request files.
