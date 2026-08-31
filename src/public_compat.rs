use std::path::Path;

use crate::diagnostic_catalog::catalog;
use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::parse_file;
use crate::stdlib::standard_library_modules;

pub const DIAGNOSTICS_SCHEMA: &str = "forgentex.diagnostics.v1";
pub const DIAGNOSE_SCHEMA: &str = "forgentex.diagnose.v1";
pub const RUN_RESULT_SCHEMA: &str = "forgentex.run_result.v1";
pub const PUBLIC_EVIDENCE_SCHEMA: &str = "forgentex.public_0_1_evidence.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicJsonSchema {
    pub id: &'static str,
    pub command: &'static str,
    pub required_fields: &'static [&'static str],
}

pub const PUBLIC_DIAGNOSTIC_CODES: &[&str] = &[
    "PAR001", "PAR002", "PAR003", "PAR004", "PAR005", "PAR006", "NAM001", "NAM002", "NAM003",
    "NAM004", "NAM005", "NAM006", "TYP001", "TYP002", "TYP003", "TYP004", "TYP005", "TYP006",
    "TYP007", "TYP008", "TYP009", "TYP010", "TYP011", "TYP012", "TYP013", "BOR001", "BOR002",
    "BOR003", "BOR004", "BOR005", "EFF001", "EFF002", "EFF003", "EFF004", "ASM001", "ASM002",
    "ASM003", "ASM004", "ABI001", "ABI002", "ABI003", "TAR001", "TAR002", "TAR003", "TAR004",
    "TAR005", "SIMD001", "SEC001", "SEC002",
];

pub const PUBLIC_JSON_SCHEMAS: &[PublicJsonSchema] = &[
    PublicJsonSchema {
        id: "forgentex.ast.v1",
        command: "fgx ast --canonical",
        required_fields: &["schema", "items"],
    },
    PublicJsonSchema {
        id: DIAGNOSE_SCHEMA,
        command: "fgx diagnose --json",
        required_fields: &[
            "schema",
            "file",
            "kind",
            "diagnostics",
            "tokens",
            "source_map",
            "canonical_ast",
        ],
    },
    PublicJsonSchema {
        id: DIAGNOSTICS_SCHEMA,
        command: "fgx check --json",
        required_fields: &["schema", "diagnostics"],
    },
    PublicJsonSchema {
        id: "forgentex.diagnostic_catalog.v1",
        command: "fgx diagnostics list --json",
        required_fields: &["schema", "stage", "diagnostics"],
    },
    PublicJsonSchema {
        id: "forgentex.diagnostic_explain.v1",
        command: "fgx explain --json",
        required_fields: &["schema", "query", "found", "diagnostic"],
    },
    PublicJsonSchema {
        id: "forgentex.effects.v1",
        command: "fgx effects --json",
        required_fields: &["schema", "functions"],
    },
    PublicJsonSchema {
        id: "forgentex.test_report.v1",
        command: "fgx test --json",
        required_fields: &[
            "schema",
            "source",
            "target",
            "status",
            "passed",
            "failed",
            "xfailed",
            "unexpected_passed",
            "tests",
        ],
    },
    PublicJsonSchema {
        id: RUN_RESULT_SCHEMA,
        command: "fgx run --json-input",
        required_fields: &["schema", "entry", "status", "return", "bindings"],
    },
    PublicJsonSchema {
        id: "forgentex.agent_context.v1",
        command: "fgx context --json",
        required_fields: &[
            "schema",
            "symbol",
            "requested_deps",
            "project",
            "source",
            "symbol_map",
            "symbol_source",
            "types",
            "effects",
            "tests",
            "diagnostics",
            "unsafe_slices",
            "token_estimate",
        ],
    },
    PublicJsonSchema {
        id: "forgentex.agent_review.v1",
        command: "fgx review --json",
        required_fields: &[
            "schema",
            "patch",
            "source",
            "negotiation",
            "explanation",
            "token_estimate",
            "patch_review",
        ],
    },
    PublicJsonSchema {
        id: "forgentex.release_check.v1",
        command: "fgx release-check --target 0.1 --json",
        required_fields: &[
            "schema",
            "target",
            "current_track",
            "decision",
            "gates_pass",
            "gates_partial",
            "gates_planned",
            "hardening_pass",
            "hardening_fail",
            "gates",
            "hardening_checks",
            "release_labels",
            "notes",
        ],
    },
];

pub fn compatibility_gaps() -> Vec<String> {
    let actual = catalog()
        .into_iter()
        .map(|entry| entry.code)
        .collect::<Vec<_>>();
    if actual == PUBLIC_DIAGNOSTIC_CODES {
        Vec::new()
    } else {
        vec![format!(
            "public diagnostic catalog drifted: expected [{}], found [{}]",
            PUBLIC_DIAGNOSTIC_CODES.join(","),
            actual.join(",")
        )]
    }
}

/// A frozen module in the public 0.1 standard foundation.
///
/// The foundation is deliberately a *selection* of the full standard library:
/// the deterministic, allocation-free, interpreter-first modules that carry a
/// patch-line stability promise. Extra modules may exist in the live catalog
/// (host I/O, crypto, media, feature probes) but are not part of the frozen
/// minimal foundation, so they carry no 0.1 compatibility promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicStandardModule {
    pub name: &'static str,
    pub functions: &'static [&'static str],
}

pub const PUBLIC_STANDARD_FOUNDATION: &[PublicStandardModule] = &[
    PublicStandardModule {
        name: "core.mem",
        functions: &["core_mem_is_aligned_i16"],
    },
    PublicStandardModule {
        name: "core.slice",
        functions: &[
            "core_slice_len_i16",
            "core_slice_len_u8",
            "core_slice_len_mut_i16",
            "core_slice_require_same_len_i16",
            "core_slice_require_mut_same_len_i16",
            "core_slice_copy_i16",
        ],
    },
    PublicStandardModule {
        name: "core.math",
        functions: &[
            "core_math_clamp_i16",
            "core_math_checked_add_i32",
            "core_math_checked_sub_i32",
            "core_math_checked_mul_i32",
            "core_math_saturating_add_i16",
        ],
    },
    PublicStandardModule {
        name: "core.bits",
        functions: &[
            "core_bits_extract_u8",
            "core_bits_popcount_u32",
            "core_bits_clz_u32",
            "core_bits_ctz_u32",
            "core_bits_rotl_u32",
        ],
    },
    PublicStandardModule {
        name: "core.option",
        functions: &[
            "core_option_some_u8",
            "core_option_none_u8",
            "core_option_is_some_u8",
            "core_option_is_none_u8",
            "core_option_unwrap_or_u8",
        ],
    },
    PublicStandardModule {
        name: "core.result",
        functions: &[
            "core_result_ok_u8",
            "core_result_err_u8",
            "core_result_is_ok_u8",
            "core_result_is_err_u8",
            "core_result_unwrap_or_u8",
        ],
    },
];

/// Gaps between the frozen public 0.1 standard foundation and the live catalog.
///
/// Empty means the foundation is fully backed: every frozen module still exists
/// and still exposes every frozen function. Any removal or rename surfaces as an
/// explicit gap so `release-check --target 0.1` cannot silently drift.
pub fn standard_foundation_gaps() -> Vec<String> {
    let modules = standard_library_modules();
    let mut gaps = Vec::new();
    for frozen in PUBLIC_STANDARD_FOUNDATION {
        match modules.iter().find(|module| module.name == frozen.name) {
            None => gaps.push(format!(
                "frozen public foundation module {} is missing from the standard library",
                frozen.name
            )),
            Some(actual) => {
                for function in frozen.functions {
                    if !actual.functions.contains(function) {
                        gaps.push(format!(
                            "frozen public foundation function {}::{} is missing from the standard library",
                            frozen.name, function
                        ));
                    }
                }
            }
        }
    }
    gaps
}

struct PublicExampleRun {
    entry: &'static str,
    input: &'static str,
}

struct PublicExample {
    path: &'static str,
    source: &'static str,
    runs: &'static [PublicExampleRun],
}

const PUBLIC_EXAMPLES: &[PublicExample] = &[
    PublicExample {
        path: "examples/public_0_1/portable_core.fgx",
        source: include_str!("../examples/public_0_1/portable_core.fgx"),
        runs: &[PublicExampleRun {
            entry: "public_score",
            input: r#"{"args":{"limit":4},"expect":{"return":19}}"#,
        }],
    },
    PublicExample {
        path: "examples/public_0_1/arrays_and_borrows.fgx",
        source: include_str!("../examples/public_0_1/arrays_and_borrows.fgx"),
        runs: &[
            PublicExampleRun {
                entry: "public_array_score",
                input: r#"{"args":{"values":[4,7,9,12]},"expect":{"return":16}}"#,
            },
            PublicExampleRun {
                entry: "public_slice_score",
                input: r#"{"args":{"values":[4,7,9,12]},"expect":{"return":16}}"#,
            },
        ],
    },
    PublicExample {
        path: "examples/public_0_1/collections.fgx",
        source: include_str!("../examples/public_0_1/collections.fgx"),
        runs: &[PublicExampleRun {
            entry: "public_collection_score",
            input: r#"{"args":{},"expect":{"return":14}}"#,
        }],
    },
];

const EXECUTION_MATRIX: &str = include_str!("../docs/public_0_1_execution_matrix.md");

/// Documented, supported limits every public 0.1 evidence run must reproduce.
///
/// Each entry is a literal substring of the frozen execution matrix, so the
/// evidence report can never claim a limit the published contract does not
/// state.
pub const PUBLIC_0_1_LIMITS: &[&str] = &[
    "portable C11 source generation",
    "explicit FIR blocker",
    "stable interpreter contract",
    "Compiler absence is reported",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicEvidenceRun {
    pub entry: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicEvidenceFixture {
    pub path: String,
    pub documented: bool,
    pub roundtrip: String,
    pub check: String,
    pub runs: Vec<PublicEvidenceRun>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicEvidenceReport {
    pub schema: String,
    pub status: String,
    pub fixtures: Vec<PublicEvidenceFixture>,
    pub limits: Vec<String>,
    pub gaps: Vec<String>,
}

/// Run the frozen public 0.1 example suite and collect a reproducible evidence
/// report: dual-view round-trip, checker cleanliness, recorded interpreter
/// results, and the documented supported limits.
pub fn public_0_1_evidence() -> PublicEvidenceReport {
    let mut fixtures = Vec::new();
    let mut gaps = Vec::new();

    for example in PUBLIC_EXAMPLES {
        let documented = EXECUTION_MATRIX.contains(example.path);
        if !documented {
            gaps.push(format!(
                "{} is missing from the frozen execution matrix",
                example.path
            ));
        }

        let mut roundtrip = "pass".to_string();
        let mut check = "pass".to_string();
        let mut runs = Vec::new();

        match parse_file(Path::new(example.path), example.source) {
            Err(error) => {
                roundtrip = "fail".to_string();
                check = "fail".to_string();
                gaps.push(format!("{} failed to parse: {error}", example.path));
            }
            Ok(program) => {
                let human = parse_file(Path::new(example.path), &format_human(&program));
                let agent_path = example.path.replace(".fgx", ".fgai");
                let agent = parse_file(Path::new(&agent_path), &format_agent(&program));
                let human_ok = human
                    .as_ref()
                    .map(|reparsed| program.canonical_eq(reparsed))
                    .unwrap_or(false);
                let agent_ok = agent
                    .as_ref()
                    .map(|reparsed| program.canonical_eq(reparsed))
                    .unwrap_or(false);
                if !human_ok || !agent_ok {
                    roundtrip = "fail".to_string();
                    gaps.push(format!(
                        "{} does not round-trip through both views",
                        example.path
                    ));
                }

                let diagnostics = crate::typecheck::check(&program);
                if !diagnostics.is_empty() {
                    check = "fail".to_string();
                    gaps.push(format!(
                        "{} reports {} checker diagnostic(s)",
                        example.path,
                        diagnostics.len()
                    ));
                }

                for run in example.runs {
                    let status = match run_entry_json(&program, run.entry, run.input) {
                        Ok(report) if report.contains("\"status\": \"pass\"") => "pass".to_string(),
                        Ok(_) => {
                            gaps.push(format!(
                                "{}::{} did not reproduce its recorded result",
                                example.path, run.entry
                            ));
                            "fail".to_string()
                        }
                        Err(error) => {
                            gaps.push(format!(
                                "{}::{} failed to execute: {error}",
                                example.path, run.entry
                            ));
                            "fail".to_string()
                        }
                    };
                    runs.push(PublicEvidenceRun {
                        entry: run.entry.to_string(),
                        status,
                    });
                }
            }
        }

        fixtures.push(PublicEvidenceFixture {
            path: example.path.to_string(),
            documented,
            roundtrip,
            check,
            runs,
        });
    }

    let mut limits = Vec::new();
    for limit in PUBLIC_0_1_LIMITS {
        if EXECUTION_MATRIX.contains(limit) {
            limits.push((*limit).to_string());
        } else {
            gaps.push(format!(
                "supported limit \"{limit}\" is not documented in the execution matrix"
            ));
        }
    }

    let status = if gaps.is_empty() { "pass" } else { "partial" };
    PublicEvidenceReport {
        schema: PUBLIC_EVIDENCE_SCHEMA.to_string(),
        status: status.to_string(),
        fixtures,
        limits,
        gaps,
    }
}

/// Gaps that keep the public 0.1 examples/evidence gate from passing.
pub fn examples_evidence_gaps() -> Vec<String> {
    public_0_1_evidence().gaps
}

impl PublicEvidenceReport {
    pub fn to_json(&self) -> String {
        fn esc(value: &str) -> String {
            format!(
                "\"{}\"",
                value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
            )
        }
        fn array(values: &[String]) -> String {
            let items = values.iter().map(|value| esc(value)).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }

        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": {},\n", esc(&self.schema)));
        out.push_str(&format!("  \"status\": {},\n", esc(&self.status)));
        out.push_str("  \"fixtures\": [\n");
        for (idx, fixture) in self.fixtures.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"path\": {},\n", esc(&fixture.path)));
            out.push_str(&format!("      \"documented\": {},\n", fixture.documented));
            out.push_str(&format!(
                "      \"roundtrip\": {},\n",
                esc(&fixture.roundtrip)
            ));
            out.push_str(&format!("      \"check\": {},\n", esc(&fixture.check)));
            out.push_str("      \"runs\": [");
            for (run_idx, run) in fixture.runs.iter().enumerate() {
                if run_idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!(
                    "{{\"entry\": {}, \"status\": {}}}",
                    esc(&run.entry),
                    esc(&run.status)
                ));
            }
            out.push_str("]\n");
            out.push_str("    }");
            if idx + 1 < self.fixtures.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n");
        out.push_str(&format!("  \"limits\": {},\n", array(&self.limits)));
        out.push_str(&format!("  \"gaps\": {}\n", array(&self.gaps)));
        out.push_str("}\n");
        out
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex public 0.1 evidence\n");
        out.push_str(&format!("schema={} status={}\n", self.schema, self.status));
        for fixture in &self.fixtures {
            out.push_str(&format!(
                "  - {} documented={} roundtrip={} check={} runs=[{}]\n",
                fixture.path,
                fixture.documented,
                fixture.roundtrip,
                fixture.check,
                fixture
                    .runs
                    .iter()
                    .map(|run| format!("{}:{}", run.entry, run.status))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        out.push_str(&format!("limits=[{}]\n", self.limits.join(",")));
        out.push_str(&format!("gaps=[{}]\n", self.gaps.join(",")));
        out
    }
}
