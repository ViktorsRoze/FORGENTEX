//! Contained execution for managed capsules (Python, Node, TypeScript, Go,
//! JVM, .NET).
//!
//! Managed runners used to launch the host runtime as an ordinary child and
//! wait for it however long it took. A capsule's declared `limit.*` entries
//! were carried as plan data and never applied — so `limit.wall_ms 1000` on a
//! Python capsule meant nothing at run time, and nothing said so.
//!
//! This module runs the managed child through the same process containment the
//! native path uses, and decides — before anything starts — what happens to
//! each declared limit:
//!
//! * **Enforced**: `wall_ms`, `cpu_ms`, and `output_bytes` everywhere, and
//!   `mem_mb` on Windows. Windows applies them to the whole process tree through
//!   a Job Object; Unix installs `RLIMIT_CPU` per process and kills the process
//!   group. A Unix memory ceiling would have to be `RLIMIT_AS`, which caps
//!   address space rather than memory and breaks runtimes that reserve more
//!   than they use, so on Unix `mem_mb` is treated as not enforceable. Even
//!   without declarations, every call gets a wall deadline, a CPU budget, an
//!   output bound, and process-tree cleanup — a managed capsule can no longer
//!   hang its caller forever or leave a child behind.
//! * **Not enforceable**: `fs_bytes` and `net_bytes`. A managed runtime reads
//!   its own standard library from disk, so a job-wide I/O count would measure
//!   the interpreter rather than the capsule, and there is no per-process
//!   network byte accounting. A capsule declaring either is **refused** unless
//!   the operator runs it with the trusted profile, which runs it with every
//!   enforceable limit still applied and records exactly which declarations
//!   were waived.
//!
//! Trust is granted by whoever runs the program (`--trusted`), never by the
//! capsule: a capsule able to mark itself trusted would make the refusal
//! meaningless.
//!
//! What this does **not** add is filesystem or network *denial*. A managed
//! capsule still runs with its user's authority; see `docs/enforcement_matrix.md`.

use std::time::Duration;

use crate::ast::Capsule;
use crate::native_build::{ProcessLimits, MAX_CONTAINED_OUTPUT_BYTES};

/// Wall ceiling when a managed capsule declares none. Longer than the native
/// default because `go run` compiles before it runs and a JVM or .NET runtime
/// is slow to start; still finite, which is the point.
pub const DEFAULT_MANAGED_WALL_MS: u64 = 60_000;
/// CPU ceiling when none is declared, for the same reasons.
pub const DEFAULT_MANAGED_CPU_MS: u64 = 60_000;

/// How the operator allows managed capsules to run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ManagedPolicy {
    /// Run a capsule even when it declares limits that cannot be enforced,
    /// waiving those — and only those — and recording that they were waived.
    pub trusted: bool,
}

/// What will be applied to one call, decided before the child starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedPlan {
    pub(crate) limits: ProcessLimits,
    pub wall: Duration,
    /// `(key, value, source)` for every limit that will be enforced; `source`
    /// is `declared` or `default`.
    pub enforced: Vec<(String, String, &'static str)>,
    /// Declared limits that will not be applied, which is only ever non-empty
    /// under the trusted profile.
    pub waived: Vec<String>,
    /// `contained`, or `trusted` when a declaration was waived.
    pub profile: &'static str,
    /// The complete environment the child will see.
    pub environment: Vec<(String, String)>,
    /// Caller variables the capsule was explicitly granted, by name.
    pub granted_env: Vec<String>,
}

/// Variables every managed runtime needs merely to start.
///
/// PATH is kept because a runtime resolves its own libraries and sibling tools
/// through it, and it holds no credentials. Everything else about the caller —
/// tokens, keys, cloud and CI variables — is absent unless the capsule asked
/// for it by name.
#[cfg(windows)]
const BASE_ENVIRONMENT: &[&str] = &[
    "SystemRoot",
    "windir",
    "SystemDrive",
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "TEMP",
    "TMP",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
];

#[cfg(not(windows))]
const BASE_ENVIRONMENT: &[&str] = &["PATH", "TMPDIR", "LANG", "LC_ALL"];

/// Extra variables a particular runtime needs, beyond the base.
///
/// These are locations and toolchain configuration, not secrets. `go run`
/// compiles into a build cache, the JVM and .NET resolve their installation and
/// package caches, and all three fall back to the user profile when their
/// specific variables are unset — which is why the home locations appear only
/// for those runtimes and not for Python or Node.
fn runtime_environment(language: &str) -> &'static [&'static str] {
    match language {
        "go" => &[
            "GOROOT",
            "GOPATH",
            "GOCACHE",
            "GOMODCACHE",
            "GOTMPDIR",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
        ],
        "java" => &[
            "JAVA_HOME",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
        ],
        "csharp" => &[
            "DOTNET_ROOT",
            "DOTNET_CLI_HOME",
            "NUGET_PACKAGES",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
        ],
        // Python and Node need nothing beyond the base. `PYTHONPATH`,
        // `PYTHONHOME`, and `NODE_OPTIONS` are deliberately absent: they change
        // what the runtime loads, so inheriting them would let the caller's
        // environment redirect a capsule's execution.
        _ => &[],
    }
}

/// The environment a managed capsule will run with, and the names it was
/// explicitly granted.
///
/// A capsule reads a caller's variable only by declaring it — `effects
/// [env.read]` plus `sandbox "env.read NAME"` — which the checker already
/// reconciles. A granted name that is unset in the caller is simply absent.
pub(crate) fn environment(
    capsule: &Capsule,
    language: &str,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut granted: Vec<String> = Vec::new();
    for (capability, scope) in crate::sandbox::analyze_capsule(capsule).grants {
        if capability == "env.read" && !scope.is_empty() && !granted.contains(&scope) {
            granted.push(scope);
        }
    }
    let mut environment = Vec::new();
    let mut take = |name: &str| {
        if environment.iter().any(|(existing, _)| existing == name) {
            return;
        }
        if let Some(value) = std::env::var_os(name) {
            environment.push((name.to_string(), value.to_string_lossy().into_owned()));
        }
    };
    for name in BASE_ENVIRONMENT {
        take(name);
    }
    for name in runtime_environment(language) {
        take(name);
    }
    for name in &granted {
        take(name);
    }
    (environment, granted)
}

/// Decide how a managed capsule will run, or refuse it.
pub(crate) fn plan(capsule: &Capsule, policy: ManagedPolicy) -> Result<ManagedPlan, String> {
    let mut wall_ms = (DEFAULT_MANAGED_WALL_MS, "default");
    let mut cpu_ms = (DEFAULT_MANAGED_CPU_MS, "default");
    let mut memory_mb: Option<(u64, &'static str)> = None;
    let mut output_bytes = (MAX_CONTAINED_OUTPUT_BYTES, "default");
    let mut unenforceable = Vec::new();

    for (key, value) in crate::sandbox::analyze_capsule(capsule).limits {
        let parse = || {
            value
                .parse::<u64>()
                .ok()
                .filter(|parsed| *parsed > 0)
                .ok_or_else(|| {
                    format!(
                        "managed_limit_invalid: capsule '{}' declares limit.{key} {value}, which is not a positive integer",
                        capsule.name
                    )
                })
        };
        match key.as_str() {
            "wall_ms" => wall_ms = (parse()?, "declared"),
            "cpu_ms" => cpu_ms = (parse()?, "declared"),
            // Windows caps the committed memory of the whole process tree. The
            // only Unix tool is `RLIMIT_AS`, which caps *address space*: V8, the
            // Go runtime, and the JVM reserve gigabytes they never touch, so it
            // would break the runtime rather than bound what it uses. On Unix a
            // declared memory ceiling is therefore not enforceable here.
            "mem_mb" if cfg!(windows) => memory_mb = Some((parse()?, "declared")),
            "mem_mb" => {
                parse()?;
                unenforceable.push(format!("limit.mem_mb {value}"));
            }
            "output_bytes" => {
                output_bytes = (
                    usize::try_from(parse()?)
                        .unwrap_or(usize::MAX)
                        .min(MAX_CONTAINED_OUTPUT_BYTES),
                    "declared",
                )
            }
            "fs_bytes" | "net_bytes" => unenforceable.push(format!("limit.{key} {value}")),
            other => unenforceable.push(format!("limit.{other} {value}")),
        }
    }

    if !unenforceable.is_empty() && !policy.trusted {
        return Err(format!(
            "managed_limit_unenforceable: capsule '{}' ({}) declares {}, which a managed runtime cannot enforce; \
             it was not run. Run with --trusted to run it with every enforceable limit applied and these waived, \
             or remove the declaration",
            capsule.name,
            capsule.language,
            unenforceable.join(", ")
        ));
    }

    let (environment, granted_env) = environment(capsule, &capsule.language);
    let mut enforced = vec![
        ("wall_ms".to_string(), wall_ms.0.to_string(), wall_ms.1),
        ("cpu_ms".to_string(), cpu_ms.0.to_string(), cpu_ms.1),
        (
            "output_bytes".to_string(),
            output_bytes.0.to_string(),
            output_bytes.1,
        ),
    ];
    if let Some((mb, source)) = memory_mb {
        enforced.push(("mem_mb".to_string(), mb.to_string(), source));
    }

    Ok(ManagedPlan {
        limits: ProcessLimits {
            cpu_ms: Some(cpu_ms.0),
            // No default memory ceiling: on Unix the only per-process tool is
            // `RLIMIT_AS`, and V8, the Go runtime, and the JVM all reserve far
            // more address space than they use, so a default would break them.
            // A declared ceiling is the author's explicit choice and is applied.
            memory_bytes: memory_mb.map(|(mb, _)| mb.saturating_mul(1024 * 1024)),
            output_bytes: output_bytes.0,
            ..ProcessLimits::default()
        },
        wall: Duration::from_millis(wall_ms.0),
        enforced,
        profile: if unenforceable.is_empty() {
            "contained"
        } else {
            "trusted"
        },
        waived: unenforceable,
        environment,
        granted_env,
    })
}

/// Run a managed capsule's command line under its plan.
///
/// Returns the child's output exactly as `Command::output` would, so the
/// runners' own result handling is unchanged; a limit that fires becomes an
/// error naming the limit.
pub(crate) fn execute(argv: &[String], plan: &ManagedPlan) -> Result<std::process::Output, String> {
    execute_with_input(argv, plan, None)
}

/// Run a managed capsule, optionally writing its request to stdin.
///
/// The request goes on a pipe rather than argv so a payload is bounded by the
/// capsule's declared limits instead of by the platform's command-line length.
pub(crate) fn execute_with_input(
    argv: &[String],
    plan: &ManagedPlan,
    input: Option<&[u8]>,
) -> Result<std::process::Output, String> {
    let run = match input {
        Some(input) => crate::native_build::run_with_environment_timeout_limited_input(
            argv,
            &plan.environment,
            input,
            plan.wall,
            plan.limits,
        ),
        None => crate::native_build::run_with_environment_timeout_limited(
            argv,
            &plan.environment,
            plan.wall,
            plan.limits,
        ),
    }
    .map_err(|err| format!("managed_capsule_contained_failure: {err}"))?;
    let Some(run) = run else {
        return Err(format!(
            "managed_capsule_timeout: exceeded limit.wall_ms {}",
            plan.wall.as_millis()
        ));
    };
    Ok(run.output)
}
