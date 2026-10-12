//! Behavioural comparison between the interpreter and the C backend.
//!
//! `fgx diff-test` used to assert that the entry's *name* appeared in the
//! generated header and source. That is a generation check wearing a
//! differential test's name: it cannot distinguish correct C from C that
//! computes the wrong answer. This module closes the gap by compiling the
//! generated translation unit together with a harness that calls the entry on
//! the same arguments the interpreter received, then comparing what both sides
//! produced.
//!
//! The comparison is exact rather than textual. Floating-point results are
//! compared as bit patterns at their declared width, so a value that prints the
//! same but is not the same still fails, and a `f32` parameter is compared as a
//! `f32` rather than against the interpreter's wider carrier.
//!
//! Signatures the harness cannot construct are reported as `unsupported`, never
//! silently skipped — the point of the exercise is that the report says exactly
//! how much was checked.

use std::path::Path;

use crate::ast::{FunctionDecl, Type};
use crate::interp::Value;

/// What the differential managed to establish about the generated C.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeExecution {
    /// The C was compiled, run, and compared against the interpreter.
    Compared {
        matched: bool,
        interpreter: String,
        native: String,
    },
    /// The entry's signature is outside what the harness can build a call for.
    Unsupported(String),
    /// No host C toolchain, so nothing could be compiled.
    Unavailable(String),
    /// Compilation or execution failed, which is itself a differential result.
    Failed(String),
}

impl NativeExecution {
    pub(crate) fn status(&self) -> &'static str {
        match self {
            NativeExecution::Compared { matched: true, .. } => "matched",
            NativeExecution::Compared { matched: false, .. } => "mismatched",
            NativeExecution::Unsupported(_) => "unsupported",
            NativeExecution::Unavailable(_) => "unavailable",
            NativeExecution::Failed(_) => "failed",
        }
    }

    /// Whether the run may be called a pass. An absent toolchain or an
    /// unsupported signature is not a failure — but the report says which of
    /// the two it was, so nobody reads a skipped comparison as a passed one.
    pub(crate) fn permits_pass(&self) -> bool {
        matches!(
            self,
            NativeExecution::Compared { matched: true, .. }
                | NativeExecution::Unsupported(_)
                | NativeExecution::Unavailable(_)
        )
    }

    pub(crate) fn note(&self) -> String {
        match self {
            NativeExecution::Compared { matched: true, .. } => {
                "generated C compiled, ran, and agreed with the interpreter".to_string()
            }
            NativeExecution::Compared {
                matched: false,
                interpreter,
                native,
            } => format!(
                "generated C disagreed with the interpreter: interpreter={interpreter:?} native={native:?}"
            ),
            NativeExecution::Unsupported(why) => {
                format!("generated C was not executed: {why}")
            }
            NativeExecution::Unavailable(why) => {
                format!("generated C was not executed: {why}")
            }
            NativeExecution::Failed(why) => format!("generated C failed: {why}"),
        }
    }
}

/// A scalar the harness knows how to declare, print, and compare.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Scalar {
    /// The C spelling used in the harness.
    c_type: &'static str,
    /// The suffix the backend uses in `fgx_slice_<suffix>`.
    slice_suffix: &'static str,
    kind: ScalarKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScalarKind {
    Signed,
    Unsigned,
    Bool,
    F32,
    F64,
}

fn scalar(ty: &Type) -> Option<Scalar> {
    let (c_type, slice_suffix, kind) = match ty {
        Type::Bool => ("bool", "bool", ScalarKind::Bool),
        Type::Primitive(name) => match name.as_str() {
            "i8" => ("int8_t", "i8", ScalarKind::Signed),
            "i16" => ("int16_t", "i16", ScalarKind::Signed),
            "i32" => ("int32_t", "i32", ScalarKind::Signed),
            "i64" => ("int64_t", "i64", ScalarKind::Signed),
            "u8" => ("uint8_t", "u8", ScalarKind::Unsigned),
            "u16" => ("uint16_t", "u16", ScalarKind::Unsigned),
            "u32" => ("uint32_t", "u32", ScalarKind::Unsigned),
            "u64" => ("uint64_t", "u64", ScalarKind::Unsigned),
            "usize" => ("size_t", "usize", ScalarKind::Unsigned),
            "bool" => ("bool", "bool", ScalarKind::Bool),
            "f32" => ("float", "f32", ScalarKind::F32),
            "f64" => ("double", "f64", ScalarKind::F64),
            _ => return None,
        },
        _ => return None,
    };
    Some(Scalar {
        c_type,
        slice_suffix,
        kind,
    })
}

/// The C `printf` call that emits one value in the canonical form, and the
/// matching Rust rendering, must agree exactly — so both live here.
impl Scalar {
    fn print_expr(&self, expr: &str) -> String {
        match self.kind {
            ScalarKind::Signed => format!("printf(\"%lld\", (long long){expr});"),
            ScalarKind::Unsigned => format!("printf(\"%llu\", (unsigned long long){expr});"),
            ScalarKind::Bool => format!("fputs(({expr}) ? \"true\" : \"false\", stdout);"),
            // Bit patterns, not decimals: two different doubles can print the
            // same at any fixed precision, and a comparison that cannot tell
            // them apart is not a comparison.
            ScalarKind::F32 => format!(
                "{{ float fv = (float)({expr}); uint32_t bits; memcpy(&bits, &fv, 4); printf(\"f32:%08lx\", (unsigned long)bits); }}"
            ),
            ScalarKind::F64 => format!(
                "{{ double dv = (double)({expr}); uint64_t bits; memcpy(&bits, &dv, 8); printf(\"f64:%016llx\", (unsigned long long)bits); }}"
            ),
        }
    }

    fn render(&self, value: &Value) -> Result<String, String> {
        Ok(match (self.kind, value) {
            (ScalarKind::Signed, Value::Int(v)) => v.to_string(),
            (ScalarKind::Unsigned, Value::Int(v)) if *v >= 0 => v.to_string(),
            (ScalarKind::Unsigned, Value::UInt(v)) => v.to_string(),
            (ScalarKind::Bool, Value::Bool(v)) => v.to_string(),
            (ScalarKind::F32, Value::Float(v)) => format!("f32:{:08x}", (*v as f32).to_bits()),
            (ScalarKind::F64, Value::Float(v)) => format!("f64:{:016x}", v.to_bits()),
            // An integer literal standing in for a float is how a golden input
            // spells `1` for an `f64` parameter.
            (ScalarKind::F32, Value::Int(v)) => format!("f32:{:08x}", (*v as f32).to_bits()),
            (ScalarKind::F64, Value::Int(v)) => format!("f64:{:016x}", (*v as f64).to_bits()),
            _ => return Err(format!("value {value:?} does not fit the declared type")),
        })
    }

    fn literal(&self, value: &Value) -> Result<String, String> {
        Ok(match (self.kind, value) {
            (ScalarKind::Signed, Value::Int(v)) => format!("INT64_C({v})"),
            (ScalarKind::Unsigned, Value::Int(v)) if *v >= 0 => format!("UINT64_C({v})"),
            (ScalarKind::Unsigned, Value::UInt(v)) => format!("UINT64_C({v})"),
            (ScalarKind::Bool, Value::Bool(v)) => v.to_string(),
            // Hex float literals are exact, so the C compiler parses the same
            // number the interpreter holds rather than a decimal rounding of it.
            (ScalarKind::F32, Value::Float(v)) => hex_float(*v as f32 as f64),
            (ScalarKind::F64, Value::Float(v)) => hex_float(*v),
            (ScalarKind::F32, Value::Int(v)) => hex_float(*v as f32 as f64),
            (ScalarKind::F64, Value::Int(v)) => hex_float(*v as f64),
            _ => return Err(format!("value {value:?} does not fit the declared type")),
        })
    }
}

/// A C99 hexadecimal floating literal, which round-trips exactly.
fn hex_float(value: f64) -> String {
    if value.is_nan() {
        return "(0.0/0.0)".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "(1.0/0.0)".to_string()
        } else {
            "(-1.0/0.0)".to_string()
        };
    }
    let bits = value.to_bits();
    let negative = bits >> 63 == 1;
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let mantissa = bits & 0x000f_ffff_ffff_ffff;
    let sign = if negative { "-" } else { "" };
    if exponent == 0 && mantissa == 0 {
        return format!("{sign}0x0p+0");
    }
    let (lead, unbiased) = if exponent == 0 {
        (0, -1022)
    } else {
        (1, exponent - 1023)
    };
    format!("{sign}0x{lead}.{mantissa:013x}p{unbiased:+}")
}

/// One argument the harness has to materialise.
enum Argument {
    Scalar(Scalar),
    Slice { elem: Scalar, mutable: bool },
}

fn argument(ty: &Type) -> Option<Argument> {
    match ty {
        Type::Slice { mutable, elem } => scalar(elem).map(|elem| Argument::Slice {
            elem,
            mutable: *mutable,
        }),
        other => scalar(other).map(Argument::Scalar),
    }
}

/// Build a C `main` that calls `entry` with `args` and prints the canonical
/// form of everything the call can be observed to produce.
fn harness_source(
    function: &FunctionDecl,
    header_include: &str,
    args: &[Value],
) -> Result<String, String> {
    if function.params.len() != args.len() {
        return Err(format!(
            "entry takes {} parameters but the golden input supplied {}",
            function.params.len(),
            args.len()
        ));
    }
    let returns = match &function.ret {
        Type::Void => None,
        other => Some(scalar(other).ok_or_else(|| {
            format!(
                "return type {} is not a scalar the harness can print",
                other.to_agent()
            )
        })?),
    };

    let mut body = String::new();
    let mut call_args = Vec::new();
    let mut observed: Vec<(String, Scalar, String)> = Vec::new();
    for (index, (param, value)) in function.params.iter().zip(args).enumerate() {
        let shape = argument(&param.ty).ok_or_else(|| {
            format!(
                "parameter '{}' has type {} which the harness cannot construct",
                param.name,
                param.ty.to_agent()
            )
        })?;
        match shape {
            Argument::Scalar(scalar) => {
                let literal = scalar.literal(value).map_err(|err| {
                    format!(
                        "parameter '{}' ({}): {err}",
                        param.name,
                        param.ty.to_agent()
                    )
                })?;
                body.push_str(&format!(
                    "    {} arg{index} = ({}){literal};\n",
                    scalar.c_type, scalar.c_type
                ));
                call_args.push(format!("arg{index}"));
            }
            Argument::Slice { elem, mutable } => {
                let Value::Array(items) = value else {
                    return Err(format!(
                        "parameter '{}' is a slice but the golden input supplied {value:?}",
                        param.name
                    ));
                };
                let literals = items
                    .iter()
                    .map(|item| elem.literal(item))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|err| format!("parameter '{}': {err}", param.name))?;
                // A zero-length array is not valid C, so an empty slice becomes
                // a null pointer with length zero — which is what the backend's
                // own empty slices look like.
                if literals.is_empty() {
                    body.push_str(&format!("    {} *arg{index}_data = NULL;\n", elem.c_type));
                } else {
                    body.push_str(&format!(
                        "    {} arg{index}_data[] = {{ {} }};\n",
                        elem.c_type,
                        literals.join(", ")
                    ));
                }
                let slice_type = if mutable {
                    format!("fgx_slice_{}", elem.slice_suffix)
                } else {
                    format!("fgx_const_slice_{}", elem.slice_suffix)
                };
                body.push_str(&format!(
                    "    {slice_type} arg{index} = {{ arg{index}_data, {} }};\n",
                    literals.len()
                ));
                call_args.push(format!("arg{index}"));
                if mutable {
                    // A `&mut` slice is the only way a void entry can be
                    // observed at all, so it must be part of the comparison.
                    observed.push((
                        param.name.clone(),
                        elem,
                        format!("arg{index}_data, arg{index}.len"),
                    ));
                }
            }
        }
    }

    let call = format!("{}({})", function.name, call_args.join(", "));
    let mut out = String::new();
    out.push_str("#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n");
    out.push_str(&format!(
        "#include \"{header_include}\"\n\nint main(void) {{\n"
    ));
    out.push_str(&body);
    match &returns {
        Some(scalar) => {
            out.push_str(&format!("    {} ret = {call};\n", scalar.c_type));
            out.push_str("    fputs(\"return=\", stdout);\n    ");
            out.push_str(&scalar.print_expr("ret"));
            out.push_str("\n    fputc('\\n', stdout);\n");
        }
        None => {
            out.push_str(&format!("    {call};\n"));
            out.push_str("    fputs(\"return=void\\n\", stdout);\n");
        }
    }
    for (name, elem, access) in &observed {
        let (data, len) = access
            .split_once(", ")
            .expect("access is built as \"data, len\"");
        out.push_str(&format!("    fputs(\"{name}=[\", stdout);\n"));
        out.push_str(&format!(
            "    for (size_t i = 0; i < {len}; ++i) {{\n        if (i) fputc(',', stdout);\n        "
        ));
        out.push_str(&elem.print_expr(&format!("{data}[i]")));
        out.push_str("\n    }\n");
        out.push_str("    fputs(\"]\\n\", stdout);\n");
    }
    out.push_str("    return 0;\n}\n");
    Ok(out)
}

/// The same canonical form, built from what the interpreter produced.
fn interpreter_canonical(
    function: &FunctionDecl,
    return_value: &Value,
    bindings: &std::collections::HashMap<String, Value>,
) -> Result<String, String> {
    let mut out = String::new();
    match &function.ret {
        Type::Void => out.push_str("return=void\n"),
        other => {
            let scalar = scalar(other)
                .ok_or_else(|| format!("return type {} is not scalar", other.to_agent()))?;
            out.push_str(&format!("return={}\n", scalar.render(return_value)?));
        }
    }
    for param in &function.params {
        let Type::Slice {
            mutable: true,
            elem,
        } = &param.ty
        else {
            continue;
        };
        let elem =
            scalar(elem).ok_or_else(|| format!("parameter '{}' is not scalar", param.name))?;
        let value = bindings.get(&param.name).ok_or_else(|| {
            format!(
                "the interpreter reported no binding for mutable parameter '{}'",
                param.name
            )
        })?;
        let Value::Array(items) = value else {
            return Err(format!("binding '{}' is not an array", param.name));
        };
        let rendered = items
            .iter()
            .map(|item| elem.render(item))
            .collect::<Result<Vec<_>, _>>()?;
        out.push_str(&format!("{}=[{}]\n", param.name, rendered.join(",")));
    }
    Ok(out)
}

/// Compile the generated translation unit with a harness and compare.
pub(crate) fn compare_generated_c(
    function: &FunctionDecl,
    header: &str,
    source: &str,
    args: &[Value],
    return_value: &Value,
    bindings: &std::collections::HashMap<String, Value>,
) -> NativeExecution {
    let expected = match interpreter_canonical(function, return_value, bindings) {
        Ok(expected) => expected,
        Err(why) => return NativeExecution::Unsupported(why),
    };
    // The generated source includes its header by name, so the header has to
    // be written under exactly that name rather than one of our choosing.
    let Some(header_include) = include_name(source) else {
        return NativeExecution::Failed(
            "generated source has no #include of its own header".to_string(),
        );
    };
    let harness = match harness_source(function, &header_include, args) {
        Ok(harness) => harness,
        Err(why) => return NativeExecution::Unsupported(why),
    };
    let Some(toolchain) = crate::native_build::detect_toolchain() else {
        return NativeExecution::Unavailable("no host C toolchain was found".to_string());
    };

    // A per-call sequence number, because two comparisons of the same entry can
    // run at once and would otherwise compile into — and delete — one directory.
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "forgentex_difftest_{}_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        function.name
    ));
    let _ = std::fs::remove_dir_all(&root);
    if let Err(err) = std::fs::create_dir_all(&root) {
        return NativeExecution::Failed(format!("could not create a scratch directory: {err}"));
    }
    let outcome = compile_and_run(&root, &toolchain, &header_include, header, source, &harness);
    let _ = std::fs::remove_dir_all(&root);

    match outcome {
        Err(why) => NativeExecution::Failed(why),
        Ok(actual) => {
            // The harness writes stdout in text mode, so Windows turns every
            // newline into CRLF. That is a property of the console, not of the
            // computation being compared.
            let actual = actual.replace("\r\n", "\n");
            NativeExecution::Compared {
                matched: actual.trim() == expected.trim(),
                interpreter: expected.trim().to_string(),
                native: actual.trim().to_string(),
            }
        }
    }
}

fn compile_and_run(
    root: &Path,
    toolchain: &crate::native_build::Toolchain,
    header_include: &str,
    header: &str,
    source: &str,
    harness: &str,
) -> Result<String, String> {
    let write = |name: &str, contents: &str| -> Result<std::path::PathBuf, String> {
        let path = root.join(name);
        std::fs::write(&path, contents)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
        Ok(path)
    };
    write(header_include, header)?;
    let source_path = write("generated.c", source)?;
    let harness_path = write("harness.c", harness)?;
    let executable = root.join(if cfg!(windows) {
        "difftest.exe"
    } else {
        "difftest"
    });

    let argv = vec![
        toolchain.name.clone(),
        "-std=c11".to_string(),
        "-O1".to_string(),
        harness_path.display().to_string(),
        source_path.display().to_string(),
        "-I".to_string(),
        root.display().to_string(),
        "-o".to_string(),
        executable.display().to_string(),
    ];
    let compiled = crate::native_build::run_with_path(&argv, Path::new(&toolchain.name).parent())
        .map_err(|err| format!("could not invoke the host C compiler: {err}"))?;
    if !compiled.status.success() {
        return Err(format!(
            "generated C did not compile: {}",
            String::from_utf8_lossy(&compiled.stderr).trim()
        ));
    }
    let run = std::process::Command::new(&executable)
        .output()
        .map_err(|err| format!("could not run the compiled entry: {err}"))?;
    if !run.status.success() {
        // A trap aborts, so a non-zero exit is a real behavioural difference
        // and is reported as one rather than as a harness problem.
        return Err(format!(
            "the compiled entry exited with {} (stderr: {})",
            run.status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "a signal".to_string()),
            String::from_utf8_lossy(&run.stderr).trim()
        ));
    }
    String::from_utf8(run.stdout).map_err(|err| format!("the entry's output was not UTF-8: {err}"))
}

fn include_name(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("#include \"")?;
        rest.strip_suffix('"').map(str::to_string)
    })
}
