//! CUDA capsules: toolkit requirements, GPU targets, and device discovery.
//!
//! A CUDA capsule is CUDA C++ that `nvcc` compiles into a process-isolated
//! runner, exactly like a C++ capsule, with the same `extern "C"` boundary
//! function. Three declarations make it reviewable and runnable:
//!
//! - `version "cuda-13.0"` names the toolkit the source was written for. It
//!   admits 13.0 and every later 13.x release: CUDA keeps source compatibility
//!   within a major release but not across one (13.0 dropped pre-Turing targets
//!   and several deprecated runtime APIs).
//! - `env "cuda-13-sm_120"` names the GPU target (`sm_NN`, `compute_NN`, or an
//!   architecture-specific `sm_NNa` / family `sm_NNf`), passed to nvcc as
//!   `-arch`.
//! - `sandbox "gpu.compute device:0"` grants the device the runner may see;
//!   it becomes `CUDA_VISIBLE_DEVICES`.
//!
//! `ffi` drives the build and the call; this module holds the CUDA-specific
//! pieces so they can be tested without a GPU.

use crate::ast::Capsule;
use crate::toolchain_version::{cuda_min_supported, Version, VersionConstraint};
use std::process::Command;
use std::sync::OnceLock;

/// The toolkit releases a capsule's `version` admits: `cuda-12.4` means 12.4
/// or any later 12.x, and `cuda-13` any 13.x.
pub fn declared_toolkit(version: &str) -> Result<VersionConstraint, String> {
    let release = version
        .strip_prefix("cuda-")
        .ok_or_else(|| format!("expected a toolkit such as 'cuda-13.0', got '{version}'"))?;
    let low = Version::parse(release)
        .filter(|_| release.split('.').count() <= 2)
        .ok_or_else(|| format!("expected 'cuda-MAJOR' or 'cuda-MAJOR.MINOR', got '{version}'"))?;
    Ok(VersionConstraint::Range {
        low,
        high: Version::new(low.major + 1, 0, 0),
    })
}

/// The toolkits a capsule may compile with: its declaration narrowed to the
/// oldest supported toolkit. Callers validate the declaration first.
pub fn capsule_toolkit_want(capsule: &Capsule) -> VersionConstraint {
    declared_toolkit(&capsule.version)
        .ok()
        .and_then(|declared| declared.intersection(&cuda_min_supported()))
        .unwrap_or_else(cuda_min_supported)
}

/// A GPU code target: the `sm_NN` / `compute_NN` token from a capsule's `env`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuTarget {
    /// The token as declared, passed to nvcc as `-arch=<token>`.
    pub token: String,
    /// The architecture number: compute capability 12.0 is `120`.
    pub arch: u32,
    /// `sm_NNa`: architecture-specific code, which runs only on exactly this
    /// compute capability.
    pub specific: bool,
    /// `sm_NNf`: family code, which runs on this compute capability and later
    /// minors of the same major.
    pub family: bool,
}

impl GpuTarget {
    pub fn parse(token: &str) -> Option<GpuTarget> {
        let rest = token
            .strip_prefix("sm_")
            .or_else(|| token.strip_prefix("compute_"))?;
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits < 2 {
            return None;
        }
        let arch = rest[..digits].parse().ok()?;
        let (specific, family) = match &rest[digits..] {
            "" => (false, false),
            "a" => (true, false),
            "f" => (false, true),
            _ => return None,
        };
        Some(GpuTarget {
            token: token.to_string(),
            arch,
            specific,
            family,
        })
    }

    /// The target declared in a capsule's `env` (`cuda-13-sm_120` -> `sm_120`).
    pub fn of(capsule: &Capsule) -> Option<GpuTarget> {
        crate::native_capsule::gpu_arch(capsule.env.as_deref())
            .and_then(|token| GpuTarget::parse(&token))
    }

    /// Whether code built for this target runs on a device whose compute
    /// capability is `device_arch` (12.0 -> `120`). Plain targets embed PTX
    /// that the driver compiles for any later architecture; `a` and `f`
    /// targets do not carry forward.
    pub fn runs_on(&self, device_arch: u32) -> bool {
        if self.specific {
            device_arch == self.arch
        } else if self.family {
            device_arch / 10 == self.arch / 10 && device_arch >= self.arch
        } else {
            device_arch >= self.arch
        }
    }
}

/// One NVIDIA GPU, as `nvidia-smi` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuDevice {
    /// The device index, in PCI bus order (`nvidia-smi`'s numbering, which the
    /// runner environment makes CUDA's numbering too).
    pub index: u32,
    pub name: String,
    /// Compute capability as reported, for example `12.0`.
    pub compute_capability: String,
    /// The compute capability as an architecture number (`12.0` -> `120`).
    pub arch: u32,
    pub memory_mib: Option<u64>,
    pub driver_version: String,
}

/// Parse `nvidia-smi --query-gpu=index,name,compute_cap,memory.total,driver_version
/// --format=csv,noheader,nounits` output.
pub fn parse_nvidia_smi(text: &str) -> Vec<GpuDevice> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            let [index, name, capability, memory, driver] = fields.as_slice() else {
                return None;
            };
            let (major, minor) = capability.split_once('.')?;
            let arch = major.parse::<u32>().ok()? * 10 + minor.parse::<u32>().ok()?;
            Some(GpuDevice {
                index: index.parse().ok()?,
                name: name.to_string(),
                compute_capability: capability.to_string(),
                arch,
                memory_mib: memory.parse().ok(),
                driver_version: driver.to_string(),
            })
        })
        .collect()
}

/// The NVIDIA GPUs on this host, probed once per process through
/// `nvidia-smi`, which ships with the driver. Empty when there is no driver,
/// no GPU, or a driver too old to report compute capability.
pub fn gpu_devices() -> &'static [GpuDevice] {
    static DEVICES: OnceLock<Vec<GpuDevice>> = OnceLock::new();
    DEVICES.get_or_init(|| {
        Command::new("nvidia-smi")
            .args([
                "--query-gpu=index,name,compute_cap,memory.total,driver_version",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or_default()
    })
}

/// The devices a capsule's `gpu.compute` grants scope it to (`device:0`), in
/// declaration order. Empty when the grants name no device, which leaves every
/// GPU visible.
pub fn granted_devices(capsule: &Capsule) -> Result<Vec<u32>, String> {
    let mut devices = Vec::new();
    for rule in &capsule.sandbox {
        let (capability, scope) = rule
            .trim()
            .split_once(char::is_whitespace)
            .map(|(capability, scope)| (capability, scope.trim()))
            .unwrap_or((rule.trim(), ""));
        if capability != "gpu.compute" || scope.is_empty() {
            continue;
        }
        for part in scope.split_whitespace() {
            let index = part
                .strip_prefix("device:")
                .and_then(|index| index.parse::<u32>().ok())
                .ok_or_else(|| {
                    format!(
                        "capsule '{}' grants gpu.compute with scope '{part}'; expected device:N",
                        capsule.name
                    )
                })?;
            if !devices.contains(&index) {
                devices.push(index);
            }
        }
    }
    Ok(devices)
}

/// The environment a CUDA runner starts with, on top of the cleared native
/// runner environment. CUDA numbers devices fastest-first by default, so the
/// PCI bus order makes a `device:N` grant name the same GPU `nvidia-smi` and
/// `fgx doctor` show as N. The driver's JIT cache would otherwise be written
/// under the user's home directory, outside the runner's private scratch.
pub fn runner_environment(devices: &[u32]) -> Vec<(String, String)> {
    let mut environment = vec![
        ("CUDA_DEVICE_ORDER".to_string(), "PCI_BUS_ID".to_string()),
        ("CUDA_CACHE_DISABLE".to_string(), "1".to_string()),
    ];
    if !devices.is_empty() {
        environment.push((
            "CUDA_VISIBLE_DEVICES".to_string(),
            devices
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    environment
}

/// A next step for the nvcc failures a toolkit/host mismatch causes, so a
/// compile error says what to change rather than only quoting a system header.
pub fn compile_hint(stderr: &str) -> Option<&'static str> {
    if stderr.contains("unsupported GNU version") || stderr.contains("unsupported clang version") {
        Some("this CUDA toolkit does not support the host C++ compiler; point NVCC_CCBIN at a supported one (for example g++-13) or use a newer toolkit")
    } else if stderr.contains("exception specification is incompatible")
        && stderr.contains("mathcalls.h")
    {
        Some(
            "this CUDA toolkit's math headers predate the host C library; use a newer CUDA toolkit",
        )
    } else if stderr.contains("Unsupported gpu architecture")
        || stderr.contains("is not defined for option 'gpu-architecture'")
    {
        Some("this CUDA toolkit cannot target the declared GPU architecture; pick a target it lists in `nvcc --list-gpu-arch`")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolkit_declarations_admit_later_releases_of_the_same_major() {
        let want = declared_toolkit("cuda-12.4").unwrap();
        assert!(want.matches(&Version::new(12, 4, 0)));
        assert!(want.matches(&Version::new(12, 8, 93)));
        assert!(!want.matches(&Version::new(12, 3, 0)));
        assert!(!want.matches(&Version::new(13, 0, 0)));
        let any_13 = declared_toolkit("cuda-13").unwrap();
        assert!(any_13.matches(&Version::new(13, 2, 51)));
        assert!(!any_13.matches(&Version::new(12, 8, 0)));
        assert!(declared_toolkit("12.4").is_err());
        assert!(declared_toolkit("cuda-13.2.51").is_err());
        assert!(declared_toolkit("cuda-latest").is_err());
    }

    #[test]
    fn gpu_targets_parse_and_check_device_compatibility() {
        let plain = GpuTarget::parse("sm_80").unwrap();
        assert_eq!(plain.arch, 80);
        assert!(plain.runs_on(80) && plain.runs_on(120));
        assert!(!plain.runs_on(75));
        let ptx = GpuTarget::parse("compute_75").unwrap();
        assert!(ptx.runs_on(120));
        let specific = GpuTarget::parse("sm_90a").unwrap();
        assert!(specific.runs_on(90) && !specific.runs_on(100) && !specific.runs_on(120));
        let family = GpuTarget::parse("sm_120f").unwrap();
        assert!(family.runs_on(120) && family.runs_on(121) && !family.runs_on(130));
        assert_eq!(GpuTarget::parse("sm_8"), None);
        assert_eq!(GpuTarget::parse("sm_90x"), None);
        assert_eq!(GpuTarget::parse("gfx90a"), None);
    }

    #[test]
    fn nvidia_smi_rows_parse_into_devices() {
        let devices = parse_nvidia_smi(
            "0, NVIDIA GeForce RTX 5090, 12.0, 32607, 595.99.02\n1, NVIDIA A100-SXM4-40GB, 8.0, 40960, 595.99.02\nnot a row\n",
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "NVIDIA GeForce RTX 5090");
        assert_eq!(devices[0].arch, 120);
        assert_eq!(devices[0].memory_mib, Some(32607));
        assert_eq!(devices[1].index, 1);
        assert_eq!(devices[1].arch, 80);
    }

    #[test]
    fn device_grants_become_the_visible_device_list() {
        let program = crate::parser::parse_human(
            "foreign k cuda version \"cuda-13.0\" env \"cuda-13-sm_120\" effects [gpu.compute]:\n    input x: i32\n    output y: i32\n    sandbox \"gpu.compute device:1 device:0\"\n    code:\n        extern \"C\" int k(int x) { return x; }\n",
        )
        .unwrap();
        let capsule = program.capsules().next().unwrap();
        assert_eq!(granted_devices(capsule).unwrap(), vec![1, 0]);
        let environment = runner_environment(&[1, 0]);
        assert!(environment.contains(&("CUDA_VISIBLE_DEVICES".to_string(), "1,0".to_string())));
        assert!(environment.contains(&("CUDA_DEVICE_ORDER".to_string(), "PCI_BUS_ID".to_string())));
        assert!(!runner_environment(&[])
            .iter()
            .any(|(name, _)| name == "CUDA_VISIBLE_DEVICES"));
    }

    #[test]
    fn compile_hints_name_the_toolkit_host_mismatch() {
        assert!(
            compile_hint("#error -- unsupported GNU version! gcc versions later than 14")
                .unwrap()
                .contains("NVCC_CCBIN")
        );
        assert!(compile_hint(
            "/usr/include/x86_64-linux-gnu/bits/mathcalls.h(83): error: exception specification is incompatible"
        )
        .unwrap()
        .contains("newer CUDA toolkit"));
        assert!(compile_hint(
            "nvcc fatal   : Value 'sm_120' is not defined for option 'gpu-architecture'"
        )
        .unwrap()
        .contains("--list-gpu-arch"));
        assert_eq!(compile_hint("error: identifier \"x\" is undefined"), None);
    }
}
