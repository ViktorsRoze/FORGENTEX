//! Public 0.1 distribution manifest.
//!
//! This module freezes the shape of a Forgentex 0.1 toolchain distribution:
//! which binaries ship, where they install, which host triples are supported,
//! which backends and prerequisites each host needs, and the checksum algorithm
//! that release artifacts are published under. `distribution_gaps()` computes
//! the release-readiness of that contract from source-verifiable facts (the
//! manifest itself, the release CI workflow, and the documented install path),
//! and reports the single item that source alone cannot prove: a green release
//! build that checksums and clean-installs the toolchain across the supported
//! host matrix in CI.

pub const DISTRIBUTION_SCHEMA: &str = "forgentex.distribution.v1";
pub const CHECKSUM_ALGORITHM: &str = "sha256";

/// The CI workflow that must build, checksum, and clean-install-verify release
/// artifacts. Embedding it ties the distribution gate to the real workflow.
const CI_WORKFLOW: &str = include_str!("../.github/workflows/ci.yml");
const README: &str = include_str!("../README.md");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistArtifact {
    pub name: &'static str,
    pub binary: &'static str,
    pub install_path_unix: &'static str,
    pub install_path_windows: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistHost {
    pub triple: &'static str,
    pub os: &'static str,
    pub primary: bool,
    pub ci_verified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistBackend {
    pub name: &'static str,
    pub requirement: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistPrerequisite {
    pub name: &'static str,
    pub purpose: &'static str,
    pub required: bool,
}

pub const DIST_ARTIFACTS: &[DistArtifact] = &[
    DistArtifact {
        name: "fgx",
        binary: "fgx",
        install_path_unix: "$CARGO_HOME/bin/fgx",
        install_path_windows: "%USERPROFILE%\\.cargo\\bin\\fgx.exe",
    },
    DistArtifact {
        name: "forgentex-lsp",
        binary: "forgentex-lsp",
        install_path_unix: "$CARGO_HOME/bin/forgentex-lsp",
        install_path_windows: "%USERPROFILE%\\.cargo\\bin\\forgentex-lsp.exe",
    },
];

pub const DIST_HOSTS: &[DistHost] = &[
    DistHost {
        triple: "x86_64-pc-windows-msvc",
        os: "windows",
        primary: true,
        ci_verified: true,
    },
    DistHost {
        triple: "x86_64-unknown-linux-gnu",
        os: "linux",
        primary: false,
        ci_verified: false,
    },
    DistHost {
        triple: "aarch64-apple-darwin",
        os: "macos",
        primary: false,
        ci_verified: false,
    },
];

pub const DIST_BACKENDS: &[DistBackend] = &[
    DistBackend {
        name: "interp",
        requirement: "always available; no host toolchain required",
    },
    DistBackend {
        name: "c",
        requirement: "optional host C11 compiler (cc/clang/msvc) for native builds",
    },
    DistBackend {
        name: "zig",
        requirement: "optional Zig 0.14 toolchain for native builds",
    },
];

pub const DIST_PREREQUISITES: &[DistPrerequisite] = &[
    DistPrerequisite {
        name: "rust-stable",
        purpose: "build the fgx and forgentex-lsp binaries from source",
        required: true,
    },
    DistPrerequisite {
        name: "host-c-compiler",
        purpose: "run the C backend and native/capsule demonstrations",
        required: false,
    },
    DistPrerequisite {
        name: "zig-0.14",
        purpose: "run the Zig backend",
        required: false,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistributionManifest {
    pub schema: String,
    pub version: String,
    pub checksum_algorithm: String,
    pub artifacts: Vec<DistArtifact>,
    pub hosts: Vec<DistHost>,
    pub backends: Vec<DistBackend>,
    pub prerequisites: Vec<DistPrerequisite>,
    pub gaps: Vec<String>,
}

pub fn distribution_manifest() -> DistributionManifest {
    DistributionManifest {
        schema: DISTRIBUTION_SCHEMA.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        checksum_algorithm: CHECKSUM_ALGORITHM.to_string(),
        artifacts: DIST_ARTIFACTS.to_vec(),
        hosts: DIST_HOSTS.to_vec(),
        backends: DIST_BACKENDS.to_vec(),
        prerequisites: DIST_PREREQUISITES.to_vec(),
        gaps: distribution_gaps(),
    }
}

/// Gaps that keep the public 0.1 distribution gate from passing.
///
/// The manifest, the release CI workflow, and the documented install path are
/// checked from source. The final residual is intentional and honest: a green
/// release build that checksums and clean-installs across the supported host
/// matrix is a CI-runtime fact that cannot be asserted from a source checkout.
pub fn distribution_gaps() -> Vec<String> {
    let mut gaps = Vec::new();

    // 1. The manifest must be internally well-formed.
    if DIST_ARTIFACTS.is_empty() {
        gaps.push("distribution manifest declares no release artifacts".to_string());
    }
    for artifact in DIST_ARTIFACTS {
        if artifact.name.is_empty()
            || artifact.binary.is_empty()
            || artifact.install_path_unix.is_empty()
            || artifact.install_path_windows.is_empty()
        {
            gaps.push(format!(
                "release artifact {} is missing a name, binary, or install path",
                artifact.name
            ));
        }
    }
    if !DIST_HOSTS.iter().any(|host| host.primary) {
        gaps.push("distribution manifest declares no primary supported host".to_string());
    }
    if !DIST_HOSTS.iter().any(|host| host.ci_verified) {
        gaps.push("distribution manifest declares no CI-verified host".to_string());
    }
    if CHECKSUM_ALGORITHM.is_empty() {
        gaps.push("distribution manifest declares no checksum algorithm".to_string());
    }

    // 2. The release CI workflow must build, checksum, and clean-install-verify.
    let workflow_markers = [
        ("release-artifacts", "a release-artifacts job"),
        ("cargo build --release", "a release build step"),
        ("CHECKSUMS.txt", "a checksum step"),
        ("clean install", "a clean-install verification step"),
        (
            "forgepack evidence --strict",
            "a packaged forgepack workflow evidence step",
        ),
    ];
    for (needle, description) in workflow_markers {
        if !CI_WORKFLOW.contains(needle) {
            gaps.push(format!(
                "release CI workflow does not declare {description} (missing \"{needle}\")"
            ));
        }
    }

    // 3. The install path and prerequisites must be documented for users.
    if !README.contains("## Installation") {
        gaps.push("README does not document an installation path".to_string());
    }
    for prerequisite in DIST_PREREQUISITES {
        if !README.contains(prerequisite.name) {
            gaps.push(format!(
                "installation docs do not mention the {} prerequisite",
                prerequisite.name
            ));
        }
    }

    // Residual: a real green CI release run is the operational proof. Only the
    // release CI, on the supported host matrix, can produce and publish the
    // per-host checksummed artifacts and prove a clean install; a source
    // checkout cannot assert that this has happened.
    gaps.push(
        "exercise the release-artifacts CI job on the supported host matrix and publish its checksummed clean-install proof"
            .to_string(),
    );

    gaps
}

fn json_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

impl DistributionManifest {
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", json_string(&self.schema)));
        out.push_str(&format!("  \"version\": {},\n", json_string(&self.version)));
        out.push_str(&format!(
            "  \"checksum_algorithm\": {},\n",
            json_string(&self.checksum_algorithm)
        ));

        out.push_str("  \"artifacts\": [\n");
        for (idx, artifact) in self.artifacts.iter().enumerate() {
            out.push_str("    {");
            out.push_str(&format!("\"name\": {}, ", json_string(artifact.name)));
            out.push_str(&format!("\"binary\": {}, ", json_string(artifact.binary)));
            out.push_str(&format!(
                "\"install_path_unix\": {}, ",
                json_string(artifact.install_path_unix)
            ));
            out.push_str(&format!(
                "\"install_path_windows\": {}",
                json_string(artifact.install_path_windows)
            ));
            out.push('}');
            if idx + 1 < self.artifacts.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");

        out.push_str("  \"hosts\": [\n");
        for (idx, host) in self.hosts.iter().enumerate() {
            out.push_str("    {");
            out.push_str(&format!("\"triple\": {}, ", json_string(host.triple)));
            out.push_str(&format!("\"os\": {}, ", json_string(host.os)));
            out.push_str(&format!("\"primary\": {}, ", host.primary));
            out.push_str(&format!("\"ci_verified\": {}", host.ci_verified));
            out.push('}');
            if idx + 1 < self.hosts.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");

        out.push_str("  \"backends\": [\n");
        for (idx, backend) in self.backends.iter().enumerate() {
            out.push_str("    {");
            out.push_str(&format!("\"name\": {}, ", json_string(backend.name)));
            out.push_str(&format!(
                "\"requirement\": {}",
                json_string(backend.requirement)
            ));
            out.push('}');
            if idx + 1 < self.backends.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");

        out.push_str("  \"prerequisites\": [\n");
        for (idx, prerequisite) in self.prerequisites.iter().enumerate() {
            out.push_str("    {");
            out.push_str(&format!("\"name\": {}, ", json_string(prerequisite.name)));
            out.push_str(&format!(
                "\"purpose\": {}, ",
                json_string(prerequisite.purpose)
            ));
            out.push_str(&format!("\"required\": {}", prerequisite.required));
            out.push('}');
            if idx + 1 < self.prerequisites.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");

        out.push_str(&format!(
            "  \"status\": {},\n",
            json_string(if self.gaps.is_empty() {
                "pass"
            } else {
                "partial"
            })
        ));
        out.push_str("  \"gaps\": [");
        for (idx, gap) in self.gaps.iter().enumerate() {
            if idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&json_string(gap));
        }
        out.push_str("]\n");
        out.push_str("}\n");
        out
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex distribution manifest\n");
        out.push_str(&format!(
            "schema={} version={} checksum={} status={}\n",
            self.schema,
            self.version,
            self.checksum_algorithm,
            if self.gaps.is_empty() {
                "pass"
            } else {
                "partial"
            }
        ));
        out.push_str("artifacts:\n");
        for artifact in &self.artifacts {
            out.push_str(&format!(
                "  - {} unix={} windows={}\n",
                artifact.name, artifact.install_path_unix, artifact.install_path_windows
            ));
        }
        out.push_str("hosts:\n");
        for host in &self.hosts {
            out.push_str(&format!(
                "  - {} os={} primary={} ci_verified={}\n",
                host.triple, host.os, host.primary, host.ci_verified
            ));
        }
        out.push_str("backends:\n");
        for backend in &self.backends {
            out.push_str(&format!("  - {}: {}\n", backend.name, backend.requirement));
        }
        out.push_str("prerequisites:\n");
        for prerequisite in &self.prerequisites {
            out.push_str(&format!(
                "  - {} required={} ({})\n",
                prerequisite.name, prerequisite.required, prerequisite.purpose
            ));
        }
        out.push_str(&format!("gaps=[{}]\n", self.gaps.join(",")));
        out
    }
}
