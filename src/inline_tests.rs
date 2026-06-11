use std::path::{Path, PathBuf};

use crate::ast::{Program, TestDecl, TestExpectation};
use crate::diag::Diagnostic;
use crate::diagnostic_catalog::canonical_code_for;
use crate::interp::{eval_expr_for_test, value_to_text, Value};
use crate::parser::parse_file;
use crate::project::{check_project, load_project};
use crate::simd::check_simd_fallbacks;
use crate::targets::check_program_target;
use crate::typecheck::check;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineTestOptions {
    pub input: PathBuf,
    pub target: Option<String>,
    pub filter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineTestReport {
    pub schema: String,
    pub source: String,
    pub target: Option<String>,
    pub status: String,
    pub passed: usize,
    pub failed: usize,
    pub xfailed: usize,
    pub unexpected_passed: usize,
    pub tests: Vec<InlineTestCase>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineTestCase {
    pub id: String,
    pub name: String,
    pub xfail: bool,
    pub status: String,
    pub expectations: Vec<InlineExpectationResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineExpectationResult {
    pub kind: String,
    pub status: String,
    pub expected: String,
    pub actual: String,
    pub detail: String,
}

pub fn run_inline_tests(options: &InlineTestOptions) -> Result<InlineTestReport, String> {
    let (program, tests, diagnostics) = if options.input.is_dir() {
        project_inputs(&options.input, options.target.as_deref())?
    } else {
        file_inputs(&options.input, options.target.as_deref())?
    };
    let tests = tests
        .into_iter()
        .filter(|test| {
            options
                .filter
                .as_ref()
                .map(|filter| test.name.contains(filter) || test.id.contains(filter))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let mut cases = Vec::new();
    for test in tests {
        cases.push(run_test_case(&program, &diagnostics, &test));
    }
    let passed = cases.iter().filter(|case| case.status == "pass").count();
    let failed = cases.iter().filter(|case| case.status == "fail").count();
    let xfailed = cases.iter().filter(|case| case.status == "xfail").count();
    let unexpected_passed = cases
        .iter()
        .filter(|case| case.status == "unexpected-pass")
        .count();
    let status = if failed == 0 && unexpected_passed == 0 {
        "pass"
    } else {
        "fail"
    };
    Ok(InlineTestReport {
        schema: "forgentex.test_report.v1".to_string(),
        source: options.input.display().to_string(),
        target: options.target.clone(),
        status: status.to_string(),
        passed,
        failed,
        xfailed,
        unexpected_passed,
        tests: cases,
    })
}

impl InlineTestReport {
    pub fn is_success(&self) -> bool {
        self.status == "pass"
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex inline test report\n");
        out.push_str(&format!(
            "schema={} source={} status={} passed={} failed={} xfailed={} unexpected_passed={}\n",
            self.schema,
            self.source,
            self.status,
            self.passed,
            self.failed,
            self.xfailed,
            self.unexpected_passed
        ));
        for case in &self.tests {
            out.push_str(&format!(
                "  - {} {} status={} expectations={}\n",
                case.id,
                quoted_text(&case.name),
                case.status,
                case.expectations.len()
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        push_prop(&mut out, 1, "schema", &quoted(&self.schema), true);
        push_prop(&mut out, 1, "source", &quoted(&self.source), true);
        push_prop(
            &mut out,
            1,
            "target",
            &self
                .target
                .as_ref()
                .map(|target| quoted(target))
                .unwrap_or_else(|| "null".to_string()),
            true,
        );
        push_prop(&mut out, 1, "status", &quoted(&self.status), true);
        push_prop(&mut out, 1, "passed", &self.passed.to_string(), true);
        push_prop(&mut out, 1, "failed", &self.failed.to_string(), true);
        push_prop(&mut out, 1, "xfailed", &self.xfailed.to_string(), true);
        push_prop(
            &mut out,
            1,
            "unexpected_passed",
            &self.unexpected_passed.to_string(),
            true,
        );
        push_line(&mut out, 1, "\"tests\": [");
        for (idx, case) in self.tests.iter().enumerate() {
            if idx > 0 {
                out.push_str(",\n");
            }
            push_case_json(&mut out, case, 2);
        }
        if !self.tests.is_empty() {
            out.push('\n');
        }
        push_line(&mut out, 1, "]");
        out.push('}');
        out
    }
}

fn file_inputs(
    path: &Path,
    target: Option<&str>,
) -> Result<(Program, Vec<TestDecl>, Vec<Diagnostic>), String> {
    let source = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let program = parse_file(path, &source)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    let tests = program.tests().cloned().collect::<Vec<_>>();
    let mut diagnostics = check(&program);
    if let Some(triple) = target {
        diagnostics.extend(check_program_target(&program, triple));
        diagnostics.extend(check_simd_fallbacks(&program, triple));
    }
    Ok((program, tests, diagnostics))
}

fn project_inputs(
    path: &Path,
    target: Option<&str>,
) -> Result<(Program, Vec<TestDecl>, Vec<Diagnostic>), String> {
    let project = load_project(path)?;
    let mut items = Vec::new();
    let mut tests = Vec::new();
    for module in &project.modules {
        items.extend(module.program.items.clone());
        if !module.standard && !module.dependency {
            tests.extend(module.program.tests().cloned());
        }
    }
    let program = Program { items };
    let mut diagnostics = check_project(path)?;
    if let Some(triple) = target {
        diagnostics.extend(check_program_target(&program, triple));
        diagnostics.extend(check_simd_fallbacks(&program, triple));
    }
    Ok((program, tests, diagnostics))
}

fn run_test_case(program: &Program, diagnostics: &[Diagnostic], test: &TestDecl) -> InlineTestCase {
    let mut expectations = Vec::new();
    for expectation in &test.expectations {
        expectations.push(run_expectation(
            program,
            diagnostics,
            expectation,
            test.xfail,
        ));
    }
    let all_pass = expectations
        .iter()
        .all(|expectation| expectation.status == "pass");
    let any_unexpected_pass = expectations
        .iter()
        .any(|expectation| expectation.status == "unexpected-pass");
    let any_fail = expectations
        .iter()
        .any(|expectation| expectation.status == "fail");
    let status = if test.xfail {
        if any_unexpected_pass || all_pass {
            "unexpected-pass"
        } else {
            "xfail"
        }
    } else if any_fail {
        "fail"
    } else {
        "pass"
    };
    InlineTestCase {
        id: test.id.clone(),
        name: test.name.clone(),
        xfail: test.xfail,
        status: status.to_string(),
        expectations,
    }
}

fn run_expectation(
    program: &Program,
    diagnostics: &[Diagnostic],
    expectation: &TestExpectation,
    xfail: bool,
) -> InlineExpectationResult {
    match expectation {
        TestExpectation::Expr { expr } => match eval_expr_for_test(program, expr) {
            Ok(Value::Bool(true)) if xfail => InlineExpectationResult {
                kind: "expect".to_string(),
                status: "unexpected-pass".to_string(),
                expected: "false-or-error".to_string(),
                actual: "true".to_string(),
                detail: "xfail expression unexpectedly evaluated to true".to_string(),
            },
            Ok(Value::Bool(true)) => InlineExpectationResult {
                kind: "expect".to_string(),
                status: "pass".to_string(),
                expected: "true".to_string(),
                actual: "true".to_string(),
                detail: "expression evaluated to true".to_string(),
            },
            Ok(value) if xfail => InlineExpectationResult {
                kind: "expect".to_string(),
                status: "xfail".to_string(),
                expected: "false-or-error".to_string(),
                actual: value_to_text(&value),
                detail: "xfail expression did not pass".to_string(),
            },
            Ok(value) => InlineExpectationResult {
                kind: "expect".to_string(),
                status: "fail".to_string(),
                expected: "true".to_string(),
                actual: value_to_text(&value),
                detail: "expression did not evaluate to true".to_string(),
            },
            Err(err) if xfail => InlineExpectationResult {
                kind: "expect".to_string(),
                status: "xfail".to_string(),
                expected: "false-or-error".to_string(),
                actual: "error".to_string(),
                detail: err,
            },
            Err(err) => InlineExpectationResult {
                kind: "expect".to_string(),
                status: "fail".to_string(),
                expected: "true".to_string(),
                actual: "error".to_string(),
                detail: err,
            },
        },
        TestExpectation::Diagnostic { code } => {
            let canonical = canonical_code_for(code).unwrap_or_else(|| code.clone());
            let found = diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == canonical || diagnostic.code == *code);
            match (found, xfail) {
                (true, true) => InlineExpectationResult {
                    kind: "expect_diagnostic".to_string(),
                    status: "xfail".to_string(),
                    expected: code.clone(),
                    actual: canonical,
                    detail: "expected diagnostic was present".to_string(),
                },
                (true, false) => InlineExpectationResult {
                    kind: "expect_diagnostic".to_string(),
                    status: "pass".to_string(),
                    expected: code.clone(),
                    actual: canonical,
                    detail: "diagnostic was present".to_string(),
                },
                (false, true) => InlineExpectationResult {
                    kind: "expect_diagnostic".to_string(),
                    status: "unexpected-pass".to_string(),
                    expected: code.clone(),
                    actual: "none".to_string(),
                    detail: "expected diagnostic was absent".to_string(),
                },
                (false, false) => InlineExpectationResult {
                    kind: "expect_diagnostic".to_string(),
                    status: "fail".to_string(),
                    expected: code.clone(),
                    actual: "none".to_string(),
                    detail: "diagnostic was absent".to_string(),
                },
            }
        }
    }
}

fn push_case_json(out: &mut String, case: &InlineTestCase, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "id", &quoted(&case.id), true);
    push_prop(out, indent + 1, "name", &quoted(&case.name), true);
    push_prop(
        out,
        indent + 1,
        "xfail",
        if case.xfail { "true" } else { "false" },
        true,
    );
    push_prop(out, indent + 1, "status", &quoted(&case.status), true);
    push_line(out, indent + 1, "\"expectations\": [");
    for (idx, expectation) in case.expectations.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        push_expectation_json(out, expectation, indent + 2);
    }
    if !case.expectations.is_empty() {
        out.push('\n');
    }
    push_line(out, indent + 1, "]");
    push_indent(out, indent);
    out.push('}');
}

fn push_expectation_json(out: &mut String, expectation: &InlineExpectationResult, indent: usize) {
    push_line(out, indent, "{");
    push_prop(out, indent + 1, "kind", &quoted(&expectation.kind), true);
    push_prop(
        out,
        indent + 1,
        "status",
        &quoted(&expectation.status),
        true,
    );
    push_prop(
        out,
        indent + 1,
        "expected",
        &quoted(&expectation.expected),
        true,
    );
    push_prop(
        out,
        indent + 1,
        "actual",
        &quoted(&expectation.actual),
        true,
    );
    push_prop(
        out,
        indent + 1,
        "detail",
        &quoted(&expectation.detail),
        false,
    );
    push_indent(out, indent);
    out.push('}');
}

fn push_prop(out: &mut String, indent: usize, key: &str, value: &str, trailing: bool) {
    push_indent(out, indent);
    out.push_str(&format!("\"{key}\": {value}"));
    if trailing {
        out.push(',');
    }
    out.push('\n');
}

fn push_line(out: &mut String, indent: usize, line: &str) {
    push_indent(out, indent);
    out.push_str(line);
    out.push('\n');
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape(value))
}

fn quoted_text(value: &str) -> String {
    format!("\"{value}\"")
}

use crate::json::escape;
