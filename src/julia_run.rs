//! Julia capsules on a host Julia (a managed runtime, like Python).
//!
//! The capsule body is ordinary Julia. A generated driver defines a small JSON
//! codec (Julia's standard library has none), reads the JSON argument array
//! from stdin, converts each argument to the declared boundary type so typed
//! methods dispatch (`f(x::Float64)` receives a `Float64` even for `100`), calls
//! the entry function, and writes the result on a nonce-tagged line. The body's
//! own printing goes to stderr and is kept as diagnostics.
//!
//! Versions follow Julia's own `[compat]` rules: `version "1.10"` admits 1.10
//! and every later 1.x, `"~1.10"` only the 1.10 series, `"=1.10.12"` exactly
//! that release, and `">=1.11"` a floor. Several Julias can be installed side
//! by side (juliaup, or official tarballs under `~/.local/opt` or `/opt`); each
//! capsule runs on the newest one its declaration admits.

use std::path::PathBuf;
use std::process::Command;

use crate::ast::{Capsule, Program, Type};
use crate::managed_protocol::ManagedCapsuleRun;
use crate::native_build::Toolchain;
use crate::toolchain_version::{Version, VersionConstraint};

/// The oldest Julia the driver is verified on: the 1.10 long-term-support
/// series. The driver also runs on 1.12 and 1.13.
pub fn julia_min_supported() -> VersionConstraint {
    VersionConstraint::AtLeast(Version::new(1, 10, 0))
}

/// Read a capsule's Julia `version` with Julia's `[compat]` semantics.
pub fn declared_julia(spec: &str) -> Result<VersionConstraint, String> {
    let spec = spec.trim();
    let bad = || {
        format!(
            "'{spec}' is not a Julia version specifier such as 1.10, ~1.10, =1.10.12, or >=1.11"
        )
    };
    if let Some(exact) = spec.strip_prefix('=') {
        return Version::parse(exact.trim())
            .map(VersionConstraint::Exact)
            .ok_or_else(bad);
    }
    if let Some(tilde) = spec.strip_prefix('~') {
        let tilde = tilde.trim();
        let low = Version::parse(tilde).ok_or_else(bad)?;
        let high = if tilde.split('.').count() == 1 {
            Version::new(low.major + 1, 0, 0)
        } else {
            Version::new(low.major, low.minor + 1, 0)
        };
        return Ok(VersionConstraint::Range { low, high });
    }
    if spec.starts_with(">=") || spec.starts_with('^') {
        return VersionConstraint::parse(spec).map_err(|_| bad());
    }
    VersionConstraint::parse(&format!("^{spec}")).map_err(|_| bad())
}

/// The Julia versions a capsule may run on: its declaration narrowed to the
/// supported floor. Callers validate the declaration first.
pub fn capsule_julia_want(capsule: &Capsule) -> VersionConstraint {
    declared_julia(&capsule.version)
        .ok()
        .and_then(|declared| declared.intersection(&julia_min_supported()))
        .unwrap_or_else(julia_min_supported)
}

fn validate_declaration(capsule: &Capsule) -> Result<VersionConstraint, String> {
    let declared = declared_julia(&capsule.version).map_err(|err| {
        format!(
            "julia capsule '{}' declares an unsupported version: {err}",
            capsule.name
        )
    })?;
    declared
        .intersection(&julia_min_supported())
        .ok_or_else(|| {
            format!(
                "julia capsule '{}' declares '{}', older than the oldest supported Julia ({})",
                capsule.name,
                capsule.version,
                julia_min_supported().describe()
            )
        })
}

fn julia_at(name: &str) -> Option<Toolchain> {
    let output = Command::new(name).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let version = Version::extract(&text)?;
    Some(Toolchain {
        name: name.to_string(),
        version: version.to_string(),
    })
}

/// Julia installs beyond `PATH`: juliaup's versioned installs, and official
/// tarballs unpacked as `julia-*` under `~/.local/opt`, `~`, and `/opt`.
fn julia_well_known_paths() -> Vec<String> {
    let mut roots = vec![PathBuf::from("/opt")];
    if let Some(home) = crate::native_build::user_home() {
        roots.push(home.join(".julia").join("juliaup"));
        roots.push(home.join(".local").join("opt"));
        roots.push(home);
    }
    let mut paths = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("julia-") {
                continue;
            }
            let candidate = entry.path().join("bin").join("julia");
            if candidate.is_file() {
                paths.push(candidate.display().to_string());
            }
        }
    }
    paths.sort();
    paths
}

/// Resolve a Julia satisfying `want`. `$JULIA` and then the `PATH` `julia` are
/// used without scanning when they satisfy it; otherwise the newest satisfying
/// install is chosen. When none does, the preferred one is returned so the
/// caller can report what it found.
pub fn resolve_julia(want: &VersionConstraint) -> Option<Toolchain> {
    let satisfies = |found: &Toolchain| {
        Version::extract(&found.version).is_some_and(|version| want.matches(&version))
    };
    let explicit = std::env::var("JULIA")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| julia_at(value.trim()));
    if let Some(found) = explicit.as_ref().filter(|found| satisfies(found)) {
        return Some(found.clone());
    }
    let path_julia = julia_at("julia");
    if let Some(found) = path_julia.as_ref().filter(|found| satisfies(found)) {
        return Some(found.clone());
    }
    let mut candidates: Vec<Toolchain> = explicit.into_iter().chain(path_julia).collect();
    candidates.extend(
        julia_well_known_paths()
            .iter()
            .filter_map(|path| julia_at(path)),
    );
    crate::native_build::choose_toolchain(candidates, want)
}

/// Every Julia discoverable on this host, deduplicated by version.
pub fn discover_julia() -> Vec<Toolchain> {
    let mut names = Vec::new();
    if let Ok(explicit) = std::env::var("JULIA") {
        if !explicit.trim().is_empty() {
            names.push(explicit.trim().to_string());
        }
    }
    names.push("julia".to_string());
    names.extend(julia_well_known_paths());
    crate::native_build::dedup_toolchains(names.iter().filter_map(|name| julia_at(name)).collect())
}

/// The Julia a capsule runs on, checked against its declaration.
pub fn detect_julia_for(capsule: &Capsule) -> Result<Toolchain, String> {
    let want = validate_declaration(capsule)?;
    let found = resolve_julia(&want).ok_or_else(|| {
        "no host julia found (looked for $JULIA, julia, juliaup, and julia-* under ~/.local/opt, ~, and /opt)"
            .to_string()
    })?;
    let version = Version::extract(&found.version).unwrap_or(Version::new(0, 0, 0));
    if !want.matches(&version) {
        return Err(format!(
            "julia capsule '{}' declares Julia {} ({}), found {}",
            capsule.name,
            capsule.version,
            want.describe(),
            found.version
        ));
    }
    Ok(found)
}

/// Every top-level function the body defines, in order: `function name(` and
/// the short form `name(args) = …`.
pub(crate) fn defined_functions(body: &str) -> Vec<String> {
    let identifier = |text: &str| -> String {
        text.chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '!')
            .collect()
    };
    let mut names = Vec::new();
    for line in body.lines() {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some(rest) = line.strip_prefix("function ") {
            let name = identifier(rest.trim_start());
            if !name.is_empty() {
                names.push(name);
            }
            continue;
        }
        let name = identifier(line);
        if name.is_empty() || !line[name.len()..].starts_with('(') {
            continue;
        }
        // `name(args) = body` or `name(args)::T = body`, but not a call.
        let mut depth = 0usize;
        let mut close = None;
        for (index, ch) in line.char_indices().skip(name.len()) {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(close) = close {
            let after = line[close + 1..].trim_start();
            let after = after
                .strip_prefix("::")
                .map(|rest| rest.trim_start_matches(|ch: char| ch != '='))
                .unwrap_or(after);
            if after.starts_with('=') && !after.starts_with("==") {
                names.push(name);
            }
        }
    }
    names
}

pub(crate) fn find_entry(capsule: &Capsule, body: &str) -> Option<String> {
    crate::managed_protocol::choose_entry(&capsule.name, &defined_functions(body))
}

fn julia_scalar(name: &str) -> Option<&'static str> {
    Some(match name {
        "i8" => "Int8",
        "i16" => "Int16",
        "i32" => "Int32",
        "i64" => "Int64",
        "isize" => "Int",
        "u8" => "UInt8",
        "u16" => "UInt16",
        "u32" => "UInt32",
        "u64" => "UInt64",
        "usize" => "UInt",
        "f32" => "Float32",
        "f64" => "Float64",
        "bool" => "Bool",
        "str" | "String" | "string" => "String",
        _ => return None,
    })
}

fn element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Slice { elem, .. } | Type::Array { elem, .. } => Some(elem),
        Type::Generic { name, args } if name == "Vec" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

fn julia_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Bool => Some("Bool".to_string()),
        Type::Primitive(name) | Type::Named(name) => julia_scalar(name).map(str::to_string),
        _ => element_type(ty)
            .and_then(julia_type_name)
            .map(|inner| format!("Vector{{{inner}}}")),
    }
}

/// Julia source converting the decoded JSON value `expression` to the
/// boundary type `ty`.
fn julia_argument(ty: &Type, expression: &str, depth: usize) -> String {
    if let Some(elem) = element_type(ty) {
        let item = format!("_fgx_item{depth}");
        let converted = julia_argument(elem, &item, depth + 1);
        let prefix = julia_type_name(elem).unwrap_or_default();
        return format!("{prefix}[{converted} for {item} in {expression}]");
    }
    match julia_type_name(ty).as_deref() {
        Some("String") => format!("String({expression})"),
        Some(scalar) => format!("convert({scalar}, {expression})"),
        // A struct arrives as a JSON object; the body sees a NamedTuple.
        None if matches!(ty, Type::Named(_)) => format!("FgxWire.record({expression})"),
        None => expression.to_string(),
    }
}

fn call_arguments(capsule: &Capsule, arguments: &str) -> String {
    capsule
        .inputs
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            julia_argument(&binding.ty, &format!("{arguments}[{}]", index + 1), 0)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The JSON codec every driver carries: Julia's standard library has none.
const JULIA_CODEC: &str = r#"module FgxWire
function skipws(s, i)
    while i <= ncodeunits(s) && s[i] in (' ', '\t', '\n', '\r')
        i = nextind(s, i)
    end
    i
end
function parse_value(s, i)
    i = skipws(s, i)
    c = s[i]
    if c == '{'
        d = Dict{String,Any}(); i = skipws(s, nextind(s, i))
        s[i] == '}' && return d, nextind(s, i)
        while true
            k, i = parse_string(s, skipws(s, i)); i = skipws(s, i)
            s[i] == ':' || error("expected ':' in JSON")
            v, i = parse_value(s, nextind(s, i)); d[k] = v; i = skipws(s, i)
            s[i] == '}' && return d, nextind(s, i)
            s[i] == ',' || error("expected ',' in JSON"); i = nextind(s, i)
        end
    elseif c == '['
        a = Any[]; i = skipws(s, nextind(s, i))
        s[i] == ']' && return a, nextind(s, i)
        while true
            v, i = parse_value(s, i); push!(a, v); i = skipws(s, i)
            s[i] == ']' && return a, nextind(s, i)
            s[i] == ',' || error("expected ',' in JSON"); i = nextind(s, i)
        end
    elseif c == '"'
        return parse_string(s, i)
    elseif startswith(SubString(s, i), "true")
        return true, i + 4
    elseif startswith(SubString(s, i), "false")
        return false, i + 5
    elseif startswith(SubString(s, i), "null")
        return nothing, i + 4
    end
    j = i
    while j <= ncodeunits(s) && (isdigit(s[j]) || s[j] in ('-', '+', '.', 'e', 'E'))
        j += 1
    end
    t = s[i:j-1]
    if any(ch -> ch in ('.', 'e', 'E'), t)
        return parse(Float64, t), j
    end
    v = tryparse(Int64, t)
    return (v === nothing ? parse(Int128, t) : v), j
end
function parse_string(s, i)
    s[i] == '"' || error("expected a JSON string")
    io = IOBuffer(); i = nextind(s, i)
    while true
        c = s[i]
        if c == '"'
            return String(take!(io)), nextind(s, i)
        elseif c == '\\'
            i = nextind(s, i); e = s[i]
            if e == 'u'
                code = parse(UInt32, s[i+1:i+4], base=16); i += 4
                if 0xd800 <= code <= 0xdbff && i + 6 <= ncodeunits(s) && s[i+1] == '\\' && s[i+2] == 'u'
                    low = parse(UInt32, s[i+3:i+6], base=16)
                    print(io, Char(0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00))); i += 6
                else
                    print(io, Char(code))
                end
            else
                print(io, e == 'n' ? '\n' : e == 't' ? '\t' : e == 'r' ? '\r' : e == 'b' ? '\b' : e == 'f' ? '\f' : e)
            end
        else
            print(io, c)
        end
        i = nextind(s, i)
    end
end
parse_json(s::AbstractString) = parse_value(s, 1)[1]
record(d::AbstractDict) = (; (Symbol(k) => record(v) for (k, v) in d if k != "\$struct")...)
record(v::AbstractVector) = map(record, v)
record(v) = v
function write_json(io, v)
    if v === nothing
        print(io, "null")
    elseif v isa Bool
        print(io, v ? "true" : "false")
    elseif v isa Integer
        print(io, v)
    elseif v isa AbstractFloat
        isfinite(v) || error("cannot encode the non-finite result $v")
        print(io, Float64(v))
    elseif v isa AbstractString || v isa Symbol || v isa AbstractChar
        print(io, '"')
        for c in string(v)
            if c == '"'
                print(io, "\\\"")
            elseif c == '\\'
                print(io, "\\\\")
            elseif c == '\n'
                print(io, "\\n")
            elseif c == '\t'
                print(io, "\\t")
            elseif c == '\r'
                print(io, "\\r")
            elseif c < ' '
                print(io, "\\u", string(UInt16(c), base=16, pad=4))
            else
                print(io, c)
            end
        end
        print(io, '"')
    elseif v isa AbstractDict || v isa NamedTuple
        print(io, '{'); first = true
        for (k, x) in pairs(v)
            first || print(io, ','); first = false
            write_json(io, string(k)); print(io, ':'); write_json(io, x)
        end
        print(io, '}')
    elseif v isa Union{AbstractArray,Tuple}
        print(io, '[')
        for (n, x) in enumerate(v)
            n > 1 && print(io, ',')
            write_json(io, x)
        end
        print(io, ']')
    elseif isstructtype(typeof(v))
        print(io, '{')
        for (n, f) in enumerate(fieldnames(typeof(v)))
            n > 1 && print(io, ',')
            write_json(io, string(f)); print(io, ':'); write_json(io, getfield(v, f))
        end
        print(io, '}')
    else
        error("cannot encode a $(typeof(v)) result as JSON")
    end
end
json(v) = sprint(write_json, v)
end
"#;

/// The cold-call driver: one request on stdin, one tagged result line.
pub(crate) fn driver_source(capsule: &Capsule, body: &str, entry: &str, nonce: &str) -> String {
    format!(
        "{JULIA_CODEC}{prelude}{body}\nlet _fgx_args = FgxWire.parse_json(read(stdin, String))\n    _fgx_value = {entry}({arguments})\n    {emit}\nend\n",
        prelude = crate::managed_protocol::julia_prelude(),
        arguments = call_arguments(capsule, "_fgx_args"),
        emit = crate::managed_protocol::julia_emit(nonce, "_fgx_value"),
    )
}

/// The warm-worker driver: one JSON request per line, one enveloped response
/// per line. An exception becomes an `{"err": …}` response, so the worker —
/// and the methods Julia has already compiled — survive a failing call.
pub(crate) fn worker_source(capsule: &Capsule, body: &str, entry: &str, nonce: &str) -> String {
    format!(
        "{JULIA_CODEC}{prelude}{body}\nfor _fgx_line in eachline(stdin)\n    isempty(strip(_fgx_line)) && continue\n    _fgx_envelope = try\n        _fgx_args = FgxWire.parse_json(_fgx_line)\n        Dict{{String,Any}}(\"ok\" => {entry}({arguments}))\n    catch _fgx_error\n        Dict{{String,Any}}(\"err\" => sprint(showerror, _fgx_error))\n    end\n    {emit}\nend\n",
        prelude = crate::managed_protocol::julia_prelude(),
        arguments = call_arguments(capsule, "_fgx_args"),
        emit = crate::managed_protocol::julia_emit(nonce, "_fgx_envelope"),
    )
}

/// The Julia command line for a driver: no startup file or history, so the
/// user's `~/.julia/config/startup.jl` cannot change what the capsule runs.
pub(crate) fn julia_command(julia: &str, driver: &std::path::Path) -> Vec<String> {
    vec![
        julia.to_string(),
        "--startup-file=no".to_string(),
        "--history-file=no".to_string(),
        "--color=no".to_string(),
        driver.display().to_string(),
    ]
}

pub fn invoke_live(capsule: &Capsule, json_args: &str) -> Result<String, String> {
    invoke_live_with(
        capsule,
        json_args,
        crate::managed_exec::ManagedPolicy::default(),
    )
}

/// Run a Julia capsule live under `policy`, inside the managed containment
/// layer (wall deadline, CPU and output budgets, scrubbed environment).
pub fn invoke_live_with(
    capsule: &Capsule,
    json_args: &str,
    policy: crate::managed_exec::ManagedPolicy,
) -> Result<String, String> {
    let managed_plan = crate::managed_exec::plan(capsule, policy)?;
    if !crate::sandbox::is_approved(capsule) {
        return Err("capsule effects are not covered by its sandbox".to_string());
    }
    let body = crate::python_run::dedent(&capsule.code);
    let entry = find_entry(capsule, &body)
        .ok_or_else(|| "no top-level function found in capsule body".to_string())?;
    let julia = detect_julia_for(capsule)?;

    use std::sync::atomic::{AtomicU64, Ordering};
    static LIVE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_jllive", std::process::id(), seq));
    std::fs::create_dir_all(&dir).map_err(|err| format!("failed to create scratch dir: {err}"))?;
    let nonce = crate::managed_protocol::nonce();
    let driver_path = dir.join("driver.jl");
    std::fs::write(&driver_path, driver_source(capsule, &body, &entry, &nonce))
        .map_err(|err| format!("failed to write driver: {err}"))?;
    let argv = julia_command(&julia.name, &driver_path);
    let output =
        crate::managed_exec::execute_with_input(&argv, &managed_plan, Some(json_args.as_bytes()));
    let _ = std::fs::remove_dir_all(&dir);
    let output = output?;
    let framed = crate::managed_protocol::split(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        &nonce,
    );
    if !output.status.success() {
        return Err(format!(
            "julia capsule '{}' raised: {}",
            capsule.name,
            if framed.diagnostics.is_empty() {
                "no output".to_string()
            } else {
                framed.diagnostics
            }
        ));
    }
    framed.result_or_error(&capsule.name)
}

fn select<'a>(program: &'a Program, name: Option<&str>) -> Option<&'a Capsule> {
    program
        .capsules()
        .filter(|capsule| capsule.language == "julia")
        .find(|capsule| name.map(|name| name == capsule.name).unwrap_or(true))
}

/// Run a Julia capsule's fixtures on the host Julia (`fgx capsule-run
/// --julia`): each fixture's inputs are a request, and its output the oracle.
pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<ManagedCapsuleRun, String> {
    let capsule =
        select(program, name).ok_or_else(|| "no julia capsule found in program".to_string())?;
    let mut report = ManagedCapsuleRun::new("forgentex.julia_run.v1", "julia", file, capsule);
    let body = crate::python_run::dedent(&capsule.code);
    report.entry = find_entry(capsule, &body);
    if report.sandbox_decision == "deny" {
        return Ok(report.stop("denied", "capsule effects are not covered by its sandbox"));
    }
    let Some(entry) = report.entry.clone() else {
        return Ok(report.stop("error", "no top-level function found in capsule body"));
    };
    let julia = match detect_julia_for(capsule) {
        Ok(julia) => julia,
        Err(err) if err.starts_with("no host julia") => {
            return Ok(report.stop("runtime_unavailable", err));
        }
        Err(err) => return Ok(report.stop("error", err)),
    };
    report.runtime = Some(format!("julia {} ({})", julia.version, julia.name));

    use std::sync::atomic::{AtomicU64, Ordering};
    static RUN_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_jlrun", std::process::id(), seq));
    let nonce = crate::managed_protocol::nonce();
    let driver_path = dir.join("driver.jl");
    if std::fs::create_dir_all(&dir).is_err()
        || std::fs::write(&driver_path, driver_source(capsule, &body, &entry, &nonce)).is_err()
    {
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(report.stop("error", "failed to write the driver"));
    }
    let argv = julia_command(&julia.name, &driver_path);
    for fixture in &capsule.fixtures {
        let request = crate::managed_protocol::fixture_request(&fixture.inputs);
        let output = crate::managed_protocol::run_with_stdin(
            Command::new(&argv[0]).args(&argv[1..]),
            &request,
        );
        match output {
            Ok(output) => {
                let framed = crate::managed_protocol::split(
                    &String::from_utf8_lossy(&output.stdout),
                    &String::from_utf8_lossy(&output.stderr),
                    &nonce,
                );
                if !output.status.success() {
                    report = report.stop("error", framed.diagnostics);
                    break;
                }
                report.record(fixture, framed.result.unwrap_or_default());
            }
            Err(err) => {
                report = report.stop("error", format!("failed to launch julia: {err}"));
                break;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(report.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_specifiers_follow_julia_semantics() {
        let caret = declared_julia("1.10").unwrap();
        assert!(caret.matches(&Version::new(1, 10, 0)));
        assert!(caret.matches(&Version::new(1, 13, 2)));
        assert!(!caret.matches(&Version::new(2, 0, 0)));
        assert!(!caret.matches(&Version::new(1, 9, 4)));
        let tilde = declared_julia("~1.10").unwrap();
        assert!(tilde.matches(&Version::new(1, 10, 12)));
        assert!(!tilde.matches(&Version::new(1, 11, 0)));
        let exact = declared_julia("=1.12.7").unwrap();
        assert!(exact.matches(&Version::new(1, 12, 7)));
        assert!(!exact.matches(&Version::new(1, 12, 6)));
        assert!(declared_julia(">=1.11")
            .unwrap()
            .matches(&Version::new(1, 13, 0)));
        assert!(declared_julia("latest").is_err());
    }

    #[test]
    fn entry_prefers_the_function_named_like_the_capsule() {
        let body = "helper(x) = 2x\nconst K = 3\nfunction scale(x::Float64)::Float64\n    helper(x) * K\nend\nif helper(1) == 2\nend\n";
        assert_eq!(defined_functions(body), vec!["helper", "scale"]);
        assert_eq!(
            crate::managed_protocol::choose_entry("scale", &defined_functions(body)),
            Some("scale".to_string())
        );
        assert_eq!(
            crate::managed_protocol::choose_entry("other", &defined_functions(body)),
            Some("helper".to_string())
        );
        assert_eq!(
            defined_functions("area(w, h)::Float64 = w * h\nprintln(area(1, 2))\n"),
            vec!["area"]
        );
    }

    #[test]
    fn arguments_are_converted_to_the_boundary_types() {
        assert_eq!(
            julia_argument(&Type::Primitive("f64".to_string()), "a[1]", 0),
            "convert(Float64, a[1])"
        );
        assert_eq!(
            julia_argument(
                &Type::Slice {
                    mutable: false,
                    elem: Box::new(Type::Primitive("i32".to_string()))
                },
                "a[2]",
                0
            ),
            "Int32[convert(Int32, _fgx_item0) for _fgx_item0 in a[2]]"
        );
        assert_eq!(
            julia_argument(&Type::Named("Point".to_string()), "a[3]", 0),
            "FgxWire.record(a[3])"
        );
    }
}
