//! Toolchain version constraints and the per-language reviewed sets.
//!
//! Phase 1 of the toolchain-versioning model (see
//! `docs/toolchain_versioning.md`). A native capsule declares a version
//! *constraint* rather than one exact string; the host toolchain is probed for
//! its version; and a call is admitted only when the host version satisfies
//! both the capsule's declared constraint and the set of versions Forgentex has
//! a reviewed codegen profile for.
//!
//! The parser is deliberately small and dependency-free: it understands bare
//! versions, a minor pin (`0.17`), an exact pin (`0.17.1`), a lower bound
//! (`>=1.82`), and a semver caret (`^0.17`). That is enough to express every
//! current native-toolchain requirement, and `Version::extract` recovers a
//! version from the noisy first line of a `--version` banner.

use std::fmt;

/// A semantic version reduced to the three fields the bridges compare on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a bare `MAJOR`, `MAJOR.MINOR`, or `MAJOR.MINOR.PATCH`. Missing
    /// trailing components default to zero. Returns `None` for anything that is
    /// not a dotted run of decimal integers.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut parts = text.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = match parts.next() {
            Some(part) => part.parse::<u64>().ok()?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(part) => part.parse::<u64>().ok()?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Version::new(major, minor, patch))
    }

    /// Recover the first `MAJOR.MINOR[.PATCH]` run from arbitrary text, so a
    /// `--version` banner can be reduced to a comparable version:
    /// `rustc 1.99.0 (abc 2026-..)` -> `1.99.0`, `0.17.0` -> `0.17.0`,
    /// `cc (Ubuntu 11.5.0-1ubuntu1) 11.5.0` -> `11.5.0`. A run must contain at
    /// least one dot (a `MAJOR.MINOR`) so stray single integers are ignored.
    pub fn extract(text: &str) -> Option<Version> {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes[i].is_ascii_digit() {
                i += 1;
                continue;
            }
            let start = i;
            let mut dots = 0usize;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                if bytes[i] == b'.' {
                    // A trailing dot ends the run rather than extending it.
                    if i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit() {
                        break;
                    }
                    dots += 1;
                }
                i += 1;
            }
            if dots >= 1 {
                if let Some(version) = Version::parse(&text[start..i]) {
                    return Some(version);
                }
            }
            // Skip past this run and keep looking.
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
        }
        None
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A constraint a host toolchain version must satisfy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionConstraint {
    /// Exactly this version (all three fields equal).
    Exact(Version),
    /// At least this version, with no upper bound (`>=1.82`). Used for the
    /// backward-compatible languages where a minimum supported version is the
    /// right contract.
    AtLeast(Version),
    /// A half-open range `[low, high)`. A bare minor pin (`0.17`) and a semver
    /// caret (`^0.17`) both lower to a range.
    Range { low: Version, high: Version },
}

impl VersionConstraint {
    /// Parse a constraint string:
    /// - `>=X.Y[.Z]` -> `AtLeast`
    /// - `^X.Y[.Z]`  -> semver caret, lowered to a `Range`
    /// - `X.Y.Z`     -> `Exact`
    /// - `X.Y`       -> minor pin, i.e. `Range [X.Y.0, X.(Y+1).0)`
    pub fn parse(text: &str) -> Result<VersionConstraint, String> {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix(">=") {
            let version = Version::parse(rest.trim())
                .ok_or_else(|| format!("'{text}' is not a valid '>=' version bound"))?;
            return Ok(VersionConstraint::AtLeast(version));
        }
        if let Some(rest) = text.strip_prefix('^') {
            let version = Version::parse(rest.trim())
                .ok_or_else(|| format!("'{text}' is not a valid caret version"))?;
            return Ok(VersionConstraint::Range {
                low: version,
                high: caret_upper_bound(version),
            });
        }
        let version =
            Version::parse(text).ok_or_else(|| format!("'{text}' is not a valid version"))?;
        // Two components (no explicit patch) means "any patch of this minor".
        let components = text.split('.').count();
        if components <= 2 {
            Ok(VersionConstraint::Range {
                low: Version::new(version.major, version.minor, 0),
                high: Version::new(version.major, version.minor + 1, 0),
            })
        } else {
            Ok(VersionConstraint::Exact(version))
        }
    }

    pub fn matches(&self, version: &Version) -> bool {
        match self {
            VersionConstraint::Exact(exact) => version == exact,
            VersionConstraint::AtLeast(low) => version >= low,
            VersionConstraint::Range { low, high } => version >= low && version < high,
        }
    }

    /// Whether every version this constraint admits is also admitted by
    /// `outer`. Used to check that a capsule's declared constraint stays inside
    /// a language's reviewed set.
    pub fn is_within(&self, outer: &VersionConstraint) -> bool {
        match self {
            VersionConstraint::Exact(v) => outer.matches(v),
            // An unbounded-above constraint can only sit inside another
            // unbounded-above one with a low bound at or below ours.
            VersionConstraint::AtLeast(low) => match outer {
                VersionConstraint::AtLeast(outer_low) => low >= outer_low,
                _ => false,
            },
            VersionConstraint::Range { low, high } => match outer {
                VersionConstraint::AtLeast(outer_low) => low >= outer_low,
                VersionConstraint::Range {
                    low: olow,
                    high: ohigh,
                } => low >= olow && high <= ohigh,
                VersionConstraint::Exact(_) => false,
            },
        }
    }

    pub fn describe(&self) -> String {
        match self {
            VersionConstraint::Exact(v) => v.to_string(),
            VersionConstraint::AtLeast(v) => format!(">={v}"),
            VersionConstraint::Range { low, high }
                if high.major == low.major && high.minor == low.minor + 1 && low.patch == 0 =>
            {
                // A whole-minor range reads best as the minor itself.
                format!("{}.{}.x", low.major, low.minor)
            }
            VersionConstraint::Range { low, high }
                if high.major == low.major
                    && high.minor > low.minor + 1
                    && low.patch == 0
                    && high.patch == 0 =>
            {
                // Several whole minors read as the first and last series.
                format!(
                    "{}.{}.x–{}.{}.x",
                    low.major,
                    low.minor,
                    high.major,
                    high.minor - 1
                )
            }
            VersionConstraint::Range { low, high }
                if low.major > 0
                    && high.major == low.major + 1
                    && low.minor == 0
                    && low.patch == 0
                    && high.minor == 0
                    && high.patch == 0 =>
            {
                // A whole major release (a CUDA `cuda-13` declaration).
                format!("{}.x", low.major)
            }
            VersionConstraint::Range { low, high } => format!(">={low}, <{high}"),
        }
    }

    /// The versions admitted by both constraints, or `None` when they share
    /// none. A capsule's declaration intersected with a language's reviewed set
    /// is what host resolution searches for.
    pub fn intersection(&self, other: &VersionConstraint) -> Option<VersionConstraint> {
        let bounds = |c: &VersionConstraint| match c {
            VersionConstraint::Exact(v) => (*v, Some(Version::new(v.major, v.minor, v.patch + 1))),
            VersionConstraint::AtLeast(v) => (*v, None),
            VersionConstraint::Range { low, high } => (*low, Some(*high)),
        };
        let (a_low, a_high) = bounds(self);
        let (b_low, b_high) = bounds(other);
        let low = a_low.max(b_low);
        let high = match (a_high, b_high) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (one, None) | (None, one) => one,
        };
        match high {
            Some(high) if low >= high => None,
            Some(high) if high == Version::new(low.major, low.minor, low.patch + 1) => {
                Some(VersionConstraint::Exact(low))
            }
            Some(high) => Some(VersionConstraint::Range { low, high }),
            None => Some(VersionConstraint::AtLeast(low)),
        }
    }

    /// Whether any version satisfies both constraints.
    pub fn intersects(&self, other: &VersionConstraint) -> bool {
        self.intersection(other).is_some()
    }
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.describe())
    }
}

/// Semver caret upper bound: pin the most significant non-zero component.
fn caret_upper_bound(v: Version) -> Version {
    if v.major > 0 {
        Version::new(v.major + 1, 0, 0)
    } else if v.minor > 0 {
        Version::new(0, v.minor + 1, 0)
    } else {
        Version::new(0, 0, v.patch + 1)
    }
}

/// From a list of candidate versions (in preference order), pick the index of
/// the highest version that satisfies `want`. Ties keep the earlier candidate,
/// so a caller that lists its preferred source first wins an exact draw.
/// Returns `None` when nothing matches — the caller then reports what it found.
pub fn select_highest(versions: &[Version], want: &VersionConstraint) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (index, version) in versions.iter().enumerate() {
        if !want.matches(version) {
            continue;
        }
        match best {
            Some(current) if versions[current] >= *version => {}
            _ => best = Some(index),
        }
    }
    best
}

/// The Zig versions Forgentex has a reviewed runner-codegen profile for:
/// the 0.14, 0.15, 0.16, and 0.17 series. Zig makes no cross-version source
/// compatibility promise, and its standard-library I/O changed twice in that
/// span, so each series maps to a profile (`ffi::ZigRunnerProfile`); admitting
/// a new series means reviewing its profile first.
pub fn zig_reviewed() -> VersionConstraint {
    VersionConstraint::Range {
        low: Version::new(0, 14, 0),
        high: Version::new(0, 18, 0),
    }
}

/// The minimum Rust compiler Forgentex's generated capsule code requires. The
/// emitted runners use `unsafe extern` blocks and the v4 `Cargo.lock` format,
/// both of which land in 1.82. Rust is backward compatible, so this is a floor
/// rather than a pinned series.
pub fn rust_min_supported() -> VersionConstraint {
    VersionConstraint::AtLeast(Version::new(1, 82, 0))
}

/// The minimum Node for TypeScript capsules. They run through Node's built-in
/// type stripping (`--experimental-strip-types`), which Node gained in 22.6;
/// older Node rejects the flag outright. Plain JavaScript capsules have no
/// such floor.
pub fn node_typescript_min() -> VersionConstraint {
    VersionConstraint::AtLeast(Version::new(22, 6, 0))
}

/// The oldest CUDA toolkit for CUDA capsules. The generated runner is built as
/// C++17 (`nvcc -std=c++17`), which nvcc accepts from CUDA 11. A capsule's own
/// `cuda-X.Y` declaration narrows this further (see `crate::cuda`).
pub fn cuda_min_supported() -> VersionConstraint {
    VersionConstraint::AtLeast(Version::new(11, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_versions_with_defaulted_components() {
        assert_eq!(Version::parse("0.17"), Some(Version::new(0, 17, 0)));
        assert_eq!(Version::parse("0.17.1"), Some(Version::new(0, 17, 1)));
        assert_eq!(Version::parse("1"), Some(Version::new(1, 0, 0)));
        assert_eq!(Version::parse("0.17.1.2"), None);
        assert_eq!(Version::parse("zig"), None);
    }

    #[test]
    fn extracts_version_from_noisy_banners() {
        assert_eq!(
            Version::extract("rustc 1.99.0 (b940084d7 2026-09-28)"),
            Some(Version::new(1, 99, 0))
        );
        assert_eq!(Version::extract("0.17.0"), Some(Version::new(0, 17, 0)));
        assert_eq!(
            Version::extract("cc (Ubuntu 11.5.0-1ubuntu1~24.04.1) 11.5.0"),
            Some(Version::new(11, 5, 0))
        );
        assert_eq!(
            Version::extract("Ubuntu clang version 18.1.3"),
            Some(Version::new(18, 1, 3))
        );
        assert_eq!(Version::extract("no version here"), None);
    }

    #[test]
    fn minor_pin_matches_any_patch_but_not_the_next_minor() {
        let c = VersionConstraint::parse("0.17").unwrap();
        assert!(c.matches(&Version::new(0, 17, 0)));
        assert!(c.matches(&Version::new(0, 17, 9)));
        assert!(!c.matches(&Version::new(0, 18, 0)));
        assert!(!c.matches(&Version::new(0, 16, 9)));
    }

    #[test]
    fn exact_pin_matches_only_itself() {
        let c = VersionConstraint::parse("0.17.1").unwrap();
        assert!(c.matches(&Version::new(0, 17, 1)));
        assert!(!c.matches(&Version::new(0, 17, 0)));
        assert!(!c.matches(&Version::new(0, 17, 2)));
    }

    #[test]
    fn at_least_is_an_open_floor() {
        let c = VersionConstraint::parse(">=1.82").unwrap();
        assert!(c.matches(&Version::new(1, 82, 0)));
        assert!(c.matches(&Version::new(1, 99, 0)));
        assert!(!c.matches(&Version::new(1, 75, 0)));
    }

    #[test]
    fn caret_pins_the_most_significant_nonzero_component() {
        let zerox = VersionConstraint::parse("^0.17").unwrap();
        assert!(zerox.matches(&Version::new(0, 17, 5)));
        assert!(!zerox.matches(&Version::new(0, 18, 0)));
        let onex = VersionConstraint::parse("^1.2").unwrap();
        assert!(onex.matches(&Version::new(1, 9, 0)));
        assert!(!onex.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn reviewed_sets_admit_the_right_hosts() {
        // Every series 0.14 through 0.17 is reviewed; 0.13 and 0.18 are not.
        for (minor, patch) in [(14, 0), (14, 1), (15, 2), (16, 0), (17, 0), (17, 1)] {
            assert!(zig_reviewed().matches(&Version::new(0, minor, patch)));
        }
        assert!(!zig_reviewed().matches(&Version::new(0, 13, 0)));
        assert!(!zig_reviewed().matches(&Version::new(0, 18, 0)));
        assert_eq!(zig_reviewed().describe(), "0.14.x–0.17.x");

        assert!(rust_min_supported().matches(&Version::new(1, 99, 0)));
        assert!(!rust_min_supported().matches(&Version::new(1, 75, 0)));

        // Node banners carry a leading `v`; type stripping lands in 22.6.
        let node = |banner: &str| Version::extract(banner).unwrap();
        assert!(node_typescript_min().matches(&node("node v22.23.3")));
        assert!(node_typescript_min().matches(&node("v22.6.0")));
        assert!(!node_typescript_min().matches(&node("node v22.5.9")));
        assert!(!node_typescript_min().matches(&node("node v18.19.1")));
    }

    #[test]
    fn select_highest_picks_the_newest_match_and_skips_the_rest() {
        let reviewed = zig_reviewed();
        // Several Zigs side by side: the newest reviewed one wins overall, and
        // a declared series picks its own member even when newer ones exist.
        let versions = [
            Version::new(0, 14, 1),
            Version::new(0, 17, 0),
            Version::new(0, 17, 3),
            Version::new(0, 18, 0),
        ];
        assert_eq!(select_highest(&versions, &reviewed), Some(2));
        let series_014 = VersionConstraint::parse("0.14").unwrap();
        assert_eq!(select_highest(&versions, &series_014), Some(0));
        // Nothing in the reviewed series -> no pick.
        let none = [Version::new(0, 13, 0), Version::new(0, 18, 0)];
        assert_eq!(select_highest(&none, &reviewed), None);
        // An exact declaration selects only that patch.
        let exact = VersionConstraint::parse("0.17.0").unwrap();
        assert_eq!(select_highest(&versions, &exact), Some(1));
    }

    #[test]
    fn declared_constraints_can_be_checked_against_the_reviewed_set() {
        let reviewed = zig_reviewed();
        for declared in ["0.14", "0.15", "0.16", "0.17", "0.17.3", "^0.16", ">=0.14"] {
            assert!(
                VersionConstraint::parse(declared)
                    .unwrap()
                    .intersects(&reviewed),
                "{declared}"
            );
        }
        for declared in ["0.13", "0.18", "0.18.0", ">=0.18"] {
            assert!(
                !VersionConstraint::parse(declared)
                    .unwrap()
                    .intersects(&reviewed),
                "{declared}"
            );
        }
        assert!(VersionConstraint::parse("0.15")
            .unwrap()
            .is_within(&reviewed));
    }

    #[test]
    fn intersection_narrows_a_declaration_to_the_reviewed_set() {
        let reviewed = zig_reviewed();
        let open = VersionConstraint::parse(">=0.16").unwrap();
        assert_eq!(
            open.intersection(&reviewed),
            Some(VersionConstraint::Range {
                low: Version::new(0, 16, 0),
                high: Version::new(0, 18, 0),
            })
        );
        let series = VersionConstraint::parse("0.15").unwrap();
        assert_eq!(series.intersection(&reviewed).unwrap().describe(), "0.15.x");
        let exact = VersionConstraint::parse("0.14.1").unwrap();
        assert_eq!(exact.intersection(&reviewed), Some(exact.clone()));
        assert_eq!(
            VersionConstraint::parse("0.18")
                .unwrap()
                .intersection(&reviewed),
            None
        );
    }
}
