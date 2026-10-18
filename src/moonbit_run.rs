//! MoonBit capsules, compiled to WebAssembly and run sandboxed.
//!
//! A MoonBit capsule body is ordinary MoonBit. Forgentex wraps it in a
//! generated `main` that reads the JSON argument array from stdin, decodes
//! each argument to the declared boundary type, calls the entry function, and
//! prints the encoded result on a nonce-tagged line. The `moon` toolchain
//! builds the wrapper as a standalone file for the `wasm-gc` target, and
//! `moonrun` (MoonBit's V8-based runner) executes it.
//!
//! Unlike the other managed runtimes, a MoonBit capsule is isolated:
//!
//! - `moonrun --policy` with an empty policy denies every host surface it
//!   mediates — files, network, environment variables, process spawning.
//! - The policy does not cover WASI, so the compiled module's imports are
//!   checked before it runs: it may import only the console functions the
//!   wrapper itself uses (`spectest.read_char` / `print_char`). A body that
//!   declares any other host import is refused.
//!
//! So a MoonBit capsule is compute-only: effects that need host access
//! (`fs.*`, `net.*`, `env.*`, `proc.*`) are refused up front rather than left
//! to fail inside the sandbox. A build is cached in memory per wrapper source
//! and toolchain, so repeated calls run the cached module (a few milliseconds)
//! without rebuilding.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::ast::{Capsule, Program, Type};
use crate::managed_protocol::ManagedCapsuleRun;
use crate::toolchain_version::{Version, VersionConstraint};

/// The oldest MoonBit compiler (`moonc`) the wrapper is verified on.
pub fn moonbit_min_supported() -> VersionConstraint {
    VersionConstraint::AtLeast(Version::new(0, 10, 0))
}

/// The host imports a compiled capsule may have: the console the wrapper uses.
const ALLOWED_IMPORTS: &[(&str, &str)] = &[("spectest", "read_char"), ("spectest", "print_char")];

const BUILD_WALL: Duration = Duration::from_secs(120);

/// A resolved MoonBit toolchain: `moon` (the build tool), `moonrun` beside it,
/// the `MOON_HOME` holding its core library, and the `moonc` release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoonToolchain {
    pub moon: PathBuf,
    pub moonrun: PathBuf,
    pub moon_home: PathBuf,
    pub moonc_version: String,
}

fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

fn moon_toolchain_at(moon: &Path) -> Option<MoonToolchain> {
    let bin = moon.parent()?;
    let moonrun = bin.join("moonrun");
    if !moon.is_file() || !moonrun.is_file() {
        return None;
    }
    // An install's core library sits beside its `bin`; otherwise `moon` uses
    // `$MOON_HOME`, then `~/.moon`.
    let moon_home = bin
        .parent()
        .filter(|home| home.join("lib").join("core").is_dir())
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("MOON_HOME").map(PathBuf::from))
        .or_else(|| crate::native_build::user_home().map(|home| home.join(".moon")))?;
    let output = Command::new(moon)
        .args(["version", "--all"])
        .env("MOON_HOME", &moon_home)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let moonc = text
        .lines()
        .find(|line| line.trim_start().starts_with("moonc"))?;
    Some(MoonToolchain {
        moon: moon.to_path_buf(),
        moonrun,
        moon_home,
        moonc_version: Version::extract(moonc)?.to_string(),
    })
}

/// Every MoonBit toolchain discoverable here: `$MOON`, `moon` on `PATH`,
/// `$MOON_HOME/bin`, and `~/.moon/bin`, deduplicated by location.
pub fn discover_moonbit() -> Vec<MoonToolchain> {
    let mut candidates = Vec::new();
    if let Some(explicit) = std::env::var_os("MOON").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(explicit));
    }
    candidates.extend(on_path("moon"));
    if let Some(home) = std::env::var_os("MOON_HOME").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(home).join("bin").join("moon"));
    }
    if let Some(home) = crate::native_build::user_home() {
        candidates.push(home.join(".moon").join("bin").join("moon"));
    }
    let mut found: Vec<MoonToolchain> = Vec::new();
    for candidate in candidates {
        let canonical = candidate.canonicalize().unwrap_or(candidate);
        if found.iter().any(|toolchain| toolchain.moon == canonical) {
            continue;
        }
        if let Some(toolchain) = moon_toolchain_at(&canonical) {
            found.push(toolchain);
        }
    }
    found
}

fn validate_declaration(capsule: &Capsule) -> Result<VersionConstraint, String> {
    let declared = VersionConstraint::parse(&capsule.version).map_err(|err| {
        format!(
            "moonbit capsule '{}' declares an unsupported moonc version: {err}",
            capsule.name
        )
    })?;
    declared
        .intersection(&moonbit_min_supported())
        .ok_or_else(|| {
            format!(
                "moonbit capsule '{}' declares '{}', older than the oldest supported moonc ({})",
                capsule.name,
                capsule.version,
                moonbit_min_supported().describe()
            )
        })
}

/// The MoonBit toolchain a capsule builds with: the first discovered (so
/// `$MOON` wins) whose `moonc` satisfies the capsule's declaration, which
/// follows the native version syntax (`0.10` is the 0.10.x series).
pub fn detect_moonbit_for(capsule: &Capsule) -> Result<MoonToolchain, String> {
    let want = validate_declaration(capsule)?;
    let found = discover_moonbit();
    if found.is_empty() {
        return Err(
            "no host moonbit toolchain found (looked for $MOON, moon on PATH, $MOON_HOME/bin, ~/.moon/bin)"
                .to_string(),
        );
    }
    found
        .iter()
        .find(|toolchain| {
            Version::parse(&toolchain.moonc_version).is_some_and(|version| want.matches(&version))
        })
        .cloned()
        .ok_or_else(|| {
            format!(
                "moonbit capsule '{}' declares moonc {} ({}), found {}",
                capsule.name,
                capsule.version,
                want.describe(),
                found
                    .iter()
                    .map(|toolchain| toolchain.moonc_version.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Every top-level function the body defines (`fn name` / `pub fn name`).
pub(crate) fn defined_functions(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let rest = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("fn "))?;
            let name: String = rest
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

pub(crate) fn find_entry(capsule: &Capsule, body: &str) -> Option<String> {
    crate::managed_protocol::choose_entry(&capsule.name, &defined_functions(body))
}

/// A boundary type MoonBit capsules can take or return.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MoonType {
    Int,
    Int64,
    UInt,
    UInt64,
    Float,
    Double,
    Bool,
    String,
    Array(Box<MoonType>),
}

impl MoonType {
    fn of(ty: &Type) -> Option<MoonType> {
        let scalar = |name: &str| {
            Some(match name {
                "i8" | "i16" | "i32" | "u8" | "u16" => MoonType::Int,
                "i64" | "isize" => MoonType::Int64,
                "u32" => MoonType::UInt,
                "u64" | "usize" => MoonType::UInt64,
                "f32" => MoonType::Float,
                "f64" => MoonType::Double,
                "bool" => MoonType::Bool,
                "str" | "String" | "string" => MoonType::String,
                _ => return None,
            })
        };
        match ty {
            Type::Bool => Some(MoonType::Bool),
            Type::Primitive(name) | Type::Named(name) => scalar(name),
            Type::Slice { elem, .. } | Type::Array { elem, .. } => {
                MoonType::of(elem).map(|inner| MoonType::Array(Box::new(inner)))
            }
            Type::Generic { name, args } if name == "Vec" && args.len() == 1 => {
                MoonType::of(&args[0]).map(|inner| MoonType::Array(Box::new(inner)))
            }
            _ => None,
        }
    }

    /// MoonBit source decoding the `Json` value `expression` to this type.
    fn decode(&self, expression: &str) -> String {
        match self {
            MoonType::Int => format!("fgx_int64({expression}).to_int()"),
            MoonType::Int64 => format!("fgx_int64({expression})"),
            MoonType::UInt => format!("fgx_uint64({expression}).to_uint()"),
            MoonType::UInt64 => format!("fgx_uint64({expression})"),
            MoonType::Float => format!("fgx_double({expression}).to_float()"),
            MoonType::Double => format!("fgx_double({expression})"),
            MoonType::Bool => format!("fgx_bool({expression})"),
            MoonType::String => format!("fgx_string({expression})"),
            MoonType::Array(inner) => format!(
                "fgx_array({expression}).map(fn(fgx_item) {{ {} }})",
                inner.decode("fgx_item")
            ),
        }
    }

    /// MoonBit source encoding the value `expression` of this type as JSON.
    fn encode(&self, expression: &str) -> String {
        match self {
            MoonType::Int | MoonType::Int64 | MoonType::UInt | MoonType::UInt64 => {
                format!("{expression}.to_string()")
            }
            MoonType::Float => format!("fgx_float_text({expression}.to_double())"),
            MoonType::Double => format!("fgx_float_text({expression})"),
            MoonType::Bool => format!("(if {expression} {{ \"true\" }} else {{ \"false\" }})"),
            MoonType::String => format!("{expression}.to_json().stringify()"),
            MoonType::Array(inner) => format!(
                "fgx_join({expression}.map(fn(fgx_item) {{ {} }}))",
                inner.encode("fgx_item")
            ),
        }
    }
}

/// The wrapper's helpers: console input, typed JSON decoding (64-bit integers
/// from the number's source text, so they stay exact), and JSON encoding.
const MOONBIT_PRELUDE: &str = r#"fn fgx_read_char() -> Int = "spectest" "read_char"

fn fgx_read_line() -> String {
  let buffer = StringBuilder::new()
  while true {
    let c = fgx_read_char()
    if c < 0 || c == 10 {
      break
    }
    buffer.write_char(Int::unsafe_to_char(c))
  }
  buffer.to_string()
}

fn fgx_read_rest() -> String {
  let buffer = StringBuilder::new()
  while true {
    let c = fgx_read_char()
    if c < 0 {
      break
    }
    buffer.write_char(Int::unsafe_to_char(c))
  }
  buffer.to_string()
}

fn fgx_number_text(json : Json) -> String {
  match json {
    Number(value, repr~) =>
      match repr {
        Some(text) => text
        None => value.to_string()
      }
    _ => abort("fgx: expected a number argument")
  }
}

fn fgx_double(json : Json) -> Double {
  match json {
    Number(value, ..) => value
    _ => abort("fgx: expected a number argument")
  }
}

fn fgx_int64(json : Json) -> Int64 {
  let mut value = 0L
  let mut negative = false
  for ch in fgx_number_text(json) {
    if ch == '-' {
      negative = true
    } else if ch >= '0' && ch <= '9' {
      value = value * 10L + (ch.to_int() - 48).to_int64()
    } else {
      abort("fgx: expected an integer argument")
    }
  }
  if negative {
    -value
  } else {
    value
  }
}

fn fgx_uint64(json : Json) -> UInt64 {
  let mut value = 0UL
  for ch in fgx_number_text(json) {
    if ch >= '0' && ch <= '9' {
      value = value * 10UL + (ch.to_int() - 48).to_uint64()
    } else {
      abort("fgx: expected an unsigned integer argument")
    }
  }
  value
}

fn fgx_bool(json : Json) -> Bool {
  match json {
    True => true
    False => false
    _ => abort("fgx: expected a boolean argument")
  }
}

fn fgx_string(json : Json) -> String {
  match json {
    String(text) => text
    _ => abort("fgx: expected a string argument")
  }
}

fn fgx_array(json : Json) -> Array[Json] {
  match json {
    Array(items) => items
    _ => abort("fgx: expected an array argument")
  }
}

fn fgx_float_text(value : Double) -> String {
  if value.is_nan() || value.is_inf() {
    abort("fgx: cannot encode a non-finite result")
  }
  let text = value.to_string()
  if text.contains(".") || text.contains("e") || text.contains("E") {
    text
  } else {
    text + ".0"
  }
}

fn fgx_join(items : Array[String]) -> String {
  let buffer = StringBuilder::new()
  buffer.write_string("[")
  for index, item in items {
    if index > 0 {
      buffer.write_string(",")
    }
    buffer.write_string(item)
  }
  buffer.write_string("]")
  buffer.to_string()
}
"#;

/// The boundary types of a capsule, or why MoonBit cannot carry them yet.
fn boundary(capsule: &Capsule) -> Result<(Vec<MoonType>, MoonType), String> {
    let inputs = capsule
        .inputs
        .iter()
        .map(|binding| {
            MoonType::of(&binding.ty).ok_or_else(|| {
                format!(
                    "moonbit capsule '{}' input '{}' has type '{}'; MoonBit capsules take integers, floats, bools, strings, and arrays of them",
                    capsule.name,
                    binding.name,
                    binding.ty.to_agent()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let output = match capsule.outputs.as_slice() {
        [binding] => MoonType::of(&binding.ty).ok_or_else(|| {
            format!(
                "moonbit capsule '{}' output '{}' has type '{}'; MoonBit capsules return integers, floats, bools, strings, and arrays of them",
                capsule.name,
                binding.name,
                binding.ty.to_agent()
            )
        })?,
        _ => {
            return Err(format!(
                "moonbit capsule '{}' must declare exactly one output",
                capsule.name
            ))
        }
    };
    Ok((inputs, output))
}

/// Effects and grants a sandboxed MoonBit capsule cannot have: anything that
/// reaches the host. They are refused before building, not left to fail in
/// the sandbox.
fn validate_compute_only(capsule: &Capsule) -> Result<(), String> {
    let needs_host = |capability: &str| {
        !capability.ends_with(".none")
            && !capability.starts_with("limit.")
            && !capability.starts_with("audit.")
            && matches!(
                capability.split('.').next().unwrap_or(capability),
                "fs" | "io"
                    | "net"
                    | "network"
                    | "env"
                    | "proc"
                    | "process"
                    | "subprocess"
                    | "gpu"
                    | "chain"
            )
    };
    let mut requested: Vec<String> = capsule
        .effects
        .iter()
        .filter(|effect| needs_host(effect))
        .map(|effect| format!("effect {effect}"))
        .chain(capsule.sandbox.iter().filter_map(|rule| {
            let capability = rule.split_whitespace().next().unwrap_or(rule);
            needs_host(capability).then(|| format!("grant {capability}"))
        }))
        .collect();
    requested.sort();
    requested.dedup();
    if requested.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "moonbit capsule '{}' requests host access ({}); MoonBit capsules run in a deny-all WebAssembly sandbox",
            capsule.name,
            requested.join(", ")
        ))
    }
}

/// The complete wrapper program for a capsule. It holds no per-call data:
/// the nonce arrives as the request's first line.
pub(crate) fn wrapper_source(capsule: &Capsule, body: &str, entry: &str) -> Result<String, String> {
    let (inputs, output) = boundary(capsule)?;
    let arguments = inputs
        .iter()
        .enumerate()
        .map(|(index, ty)| format!("    {},\n", ty.decode(&format!("fgx_args[{index}]"))))
        .collect::<String>();
    Ok(format!(
        "{MOONBIT_PRELUDE}\n{body}\n\nfn main {{\n  let fgx_nonce = fgx_read_line()\n  let fgx_json = @json.parse(fgx_read_rest()) catch {{\n    _ => abort(\"fgx: the request is not JSON\")\n  }}\n  let fgx_args = fgx_array(fgx_json)\n  if fgx_args.length() != {count} {{\n    abort(\"fgx: expected {count} argument(s)\")\n  }}\n  let fgx_value = {entry}(\n{arguments}  )\n  {emit}\n}}\n",
        count = inputs.len(),
        emit = crate::managed_protocol::moonbit_emit("fgx_nonce", &output.encode("fgx_value")),
    ))
}

/// The `(module, name)` pairs a WebAssembly binary imports, read from its
/// import section. Only the binary layout is decoded.
pub(crate) fn wasm_imports(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    fn leb(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = *bytes.get(*at).ok_or("truncated LEB128 value")?;
            *at += 1;
            if shift >= 64 {
                return Err("oversized LEB128 value".to_string());
            }
            value |= u64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
    }
    fn name(bytes: &[u8], at: &mut usize) -> Result<String, String> {
        let length = usize::try_from(leb(bytes, at)?).map_err(|_| "oversized name")?;
        let end = at.checked_add(length).ok_or("oversized name")?;
        let text = bytes.get(*at..end).ok_or("truncated name")?;
        *at = end;
        String::from_utf8(text.to_vec()).map_err(|_| "import name is not UTF-8".to_string())
    }
    fn limits(bytes: &[u8], at: &mut usize) -> Result<(), String> {
        let flags = leb(bytes, at)?;
        leb(bytes, at)?;
        if flags & 1 != 0 {
            leb(bytes, at)?;
        }
        Ok(())
    }
    fn value_type(bytes: &[u8], at: &mut usize) -> Result<(), String> {
        let code = *bytes.get(*at).ok_or("truncated value type")?;
        *at += 1;
        // `(ref null ht)` / `(ref ht)` carry a heap type.
        if matches!(code, 0x63 | 0x64) {
            leb(bytes, at)?;
        }
        Ok(())
    }

    if bytes.get(..8) != Some(b"\0asm\x01\0\0\0".as_slice()) {
        return Err("not a WebAssembly 1.0 binary".to_string());
    }
    let mut at = 8;
    while at < bytes.len() {
        let id = bytes[at];
        at += 1;
        let size = usize::try_from(leb(bytes, &mut at)?).map_err(|_| "oversized section")?;
        let end = at.checked_add(size).ok_or("oversized section")?;
        if end > bytes.len() {
            return Err("truncated section".to_string());
        }
        if id == 2 {
            let mut cursor = at;
            let count = leb(bytes, &mut cursor)?;
            let mut imports = Vec::new();
            for _ in 0..count {
                let module = name(bytes, &mut cursor)?;
                let field = name(bytes, &mut cursor)?;
                let kind = *bytes.get(cursor).ok_or("truncated import")?;
                cursor += 1;
                match kind {
                    0x00 => {
                        leb(bytes, &mut cursor)?;
                    }
                    0x01 => {
                        value_type(bytes, &mut cursor)?;
                        limits(bytes, &mut cursor)?;
                    }
                    0x02 => limits(bytes, &mut cursor)?,
                    0x03 => {
                        value_type(bytes, &mut cursor)?;
                        cursor += 1;
                    }
                    0x04 => {
                        cursor += 1;
                        leb(bytes, &mut cursor)?;
                    }
                    other => return Err(format!("unknown import kind {other:#x}")),
                }
                imports.push((module, field));
            }
            return Ok(imports);
        }
        at = end;
    }
    Ok(Vec::new())
}

/// Refuse a module importing anything beyond the wrapper's console.
fn check_imports(capsule: &Capsule, wasm: &[u8]) -> Result<(), String> {
    let denied: Vec<String> = wasm_imports(wasm)
        .map_err(|err| {
            format!(
                "moonbit capsule '{}' built an unreadable module: {err}",
                capsule.name
            )
        })?
        .into_iter()
        .filter(|(module, name)| {
            !ALLOWED_IMPORTS
                .iter()
                .any(|(allowed_module, allowed_name)| {
                    module == allowed_module && name == allowed_name
                })
        })
        .map(|(module, name)| format!("{module}.{name}"))
        .collect();
    if denied.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "moonbit_capsule_import_denied: capsule '{}' imports host functions a sandboxed capsule may not use: {}",
            capsule.name,
            denied.join(", ")
        ))
    }
}

fn scratch(tag: &str) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forgentex_{}_{}_{tag}", std::process::id(), seq));
    std::fs::create_dir_all(&dir).map_err(|err| format!("failed to create scratch dir: {err}"))?;
    Ok(dir)
}

fn find_wasm(dir: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_wasm(&path) {
                return Some(found);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "wasm")
        {
            return Some(path);
        }
    }
    None
}

/// Build `source` with `toolchain` into a checked Wasm module, or return the
/// module built earlier in this process from the same source and toolchain.
fn build(
    capsule: &Capsule,
    toolchain: &MoonToolchain,
    source: &str,
) -> Result<Arc<Vec<u8>>, String> {
    static BUILT: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    let built = BUILT.get_or_init(|| Mutex::new(HashMap::new()));
    let key = crate::crypto::sha256_hex(
        format!(
            "{}\n{}\n{source}",
            toolchain.moon.display(),
            toolchain.moonc_version
        )
        .as_bytes(),
    );
    if let Some(wasm) = built.lock().ok().and_then(|map| map.get(&key).cloned()) {
        return Ok(wasm);
    }
    let dir = scratch("mbtbuild")?;
    let result: Result<Arc<Vec<u8>>, String> = (|| {
        let file = dir.join("capsule.mbt");
        std::fs::write(&file, source)
            .map_err(|err| format!("failed to write the wrapper: {err}"))?;
        let mut environment: Vec<(String, String)> = std::env::vars()
            .filter(|(name, _)| name != "MOON_HOME")
            .collect();
        environment.push((
            "MOON_HOME".to_string(),
            toolchain.moon_home.display().to_string(),
        ));
        let argv = vec![
            toolchain.moon.display().to_string(),
            "run".to_string(),
            file.display().to_string(),
            "--target".to_string(),
            "wasm-gc".to_string(),
            "--build-only".to_string(),
            "--target-dir".to_string(),
            dir.join("out").display().to_string(),
        ];
        let run = crate::native_build::run_with_environment_timeout_limited(
            &argv,
            &environment,
            BUILD_WALL,
            crate::native_build::ProcessLimits::default(),
        )
        .map_err(|err| format!("failed to launch moon: {err}"))?
        .ok_or_else(|| {
            format!(
                "moonbit capsule '{}' build exceeded {}s",
                capsule.name,
                BUILD_WALL.as_secs()
            )
        })?;
        let wasm = find_wasm(&dir.join("out"))
            .filter(|_| run.output.status.success())
            .ok_or_else(|| {
                let mut diagnostics = String::new();
                for line in String::from_utf8_lossy(&run.output.stderr)
                    .lines()
                    .chain(String::from_utf8_lossy(&run.output.stdout).lines())
                    .filter(|line| !line.trim().is_empty())
                {
                    crate::managed_protocol::push_diagnostic(&mut diagnostics, line);
                }
                format!(
                    "moonbit capsule '{}' failed to compile: {diagnostics}",
                    capsule.name
                )
            })?;
        let bytes =
            std::fs::read(&wasm).map_err(|err| format!("failed to read the module: {err}"))?;
        check_imports(capsule, &bytes)?;
        Ok(Arc::new(bytes))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    let wasm = result?;
    if let Ok(mut map) = built.lock() {
        map.insert(key, Arc::clone(&wasm));
    }
    Ok(wasm)
}

/// Everything a call needs before it runs: the toolchain and the checked,
/// built module.
fn prepare(capsule: &Capsule) -> Result<(MoonToolchain, String, Arc<Vec<u8>>), String> {
    validate_compute_only(capsule)?;
    let body = crate::python_run::dedent(&capsule.code);
    let entry = find_entry(capsule, &body)
        .ok_or_else(|| "no top-level fn found in capsule body".to_string())?;
    let source = wrapper_source(capsule, &body, &entry)?;
    let toolchain = detect_moonbit_for(capsule)?;
    let wasm = build(capsule, &toolchain, &source)?;
    Ok((toolchain, entry, wasm))
}

/// Write the module and an empty (deny-all) policy into a fresh directory,
/// returning the directory and the `moonrun` command line.
fn stage(toolchain: &MoonToolchain, wasm: &[u8]) -> Result<(PathBuf, Vec<String>), String> {
    let dir = scratch("mbtrun")?;
    let module = dir.join("capsule.wasm");
    let policy = dir.join("policy.json");
    std::fs::write(&module, wasm)
        .and_then(|()| std::fs::write(&policy, "{}\n"))
        .map_err(|err| {
            let _ = std::fs::remove_dir_all(&dir);
            format!("failed to stage the module: {err}")
        })?;
    let argv = vec![
        toolchain.moonrun.display().to_string(),
        "--policy".to_string(),
        policy.display().to_string(),
        module.display().to_string(),
    ];
    Ok((dir, argv))
}

pub fn invoke_live(capsule: &Capsule, json_args: &str) -> Result<String, String> {
    invoke_live_with(
        capsule,
        json_args,
        crate::managed_exec::ManagedPolicy::default(),
    )
}

/// Run a MoonBit capsule live under `policy`: the managed containment layer
/// (wall deadline, CPU and output budgets, scrubbed environment) around
/// `moonrun`'s deny-all sandbox.
pub fn invoke_live_with(
    capsule: &Capsule,
    json_args: &str,
    policy: crate::managed_exec::ManagedPolicy,
) -> Result<String, String> {
    let managed_plan = crate::managed_exec::plan(capsule, policy)?;
    if !crate::sandbox::is_approved(capsule) {
        return Err("capsule effects are not covered by its sandbox".to_string());
    }
    let (toolchain, _entry, wasm) = prepare(capsule)?;
    let (dir, argv) = stage(&toolchain, &wasm)?;
    let nonce = crate::managed_protocol::nonce();
    let request = format!("{nonce}\n{json_args}");
    let output =
        crate::managed_exec::execute_with_input(&argv, &managed_plan, Some(request.as_bytes()));
    let _ = std::fs::remove_dir_all(&dir);
    let output = output?;
    let framed = crate::managed_protocol::split(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        &nonce,
    );
    if !output.status.success() {
        return Err(format!(
            "moonbit capsule '{}' failed: {}",
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
        .filter(|capsule| capsule.language == "moonbit")
        .find(|capsule| name.map(|name| name == capsule.name).unwrap_or(true))
}

/// Run a MoonBit capsule's fixtures (`fgx capsule-run --moonbit`): the module
/// is built once and each fixture runs in its own sandboxed `moonrun`.
pub fn run(file: &str, program: &Program, name: Option<&str>) -> Result<ManagedCapsuleRun, String> {
    let capsule =
        select(program, name).ok_or_else(|| "no moonbit capsule found in program".to_string())?;
    let mut report = ManagedCapsuleRun::new("forgentex.moonbit_run.v1", "moonbit", file, capsule);
    report.entry = find_entry(capsule, &crate::python_run::dedent(&capsule.code));
    if report.sandbox_decision == "deny" {
        return Ok(report.stop("denied", "capsule effects are not covered by its sandbox"));
    }
    let (toolchain, wasm) = match prepare(capsule) {
        Ok((toolchain, _entry, wasm)) => (toolchain, wasm),
        Err(err) if err.starts_with("no host moonbit") => {
            return Ok(report.stop("runtime_unavailable", err));
        }
        Err(err) => return Ok(report.stop("error", err)),
    };
    report.runtime = Some(format!(
        "moonc {} ({})",
        toolchain.moonc_version,
        toolchain.moon.display()
    ));
    let (dir, argv) = match stage(&toolchain, &wasm) {
        Ok(staged) => staged,
        Err(err) => return Ok(report.stop("error", err)),
    };
    for fixture in &capsule.fixtures {
        let nonce = crate::managed_protocol::nonce();
        let request = format!(
            "{nonce}\n{}",
            crate::managed_protocol::fixture_request(&fixture.inputs)
        );
        match crate::managed_protocol::run_with_stdin(
            Command::new(&argv[0]).args(&argv[1..]).env_clear(),
            &request,
        ) {
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
                report = report.stop("error", format!("failed to launch moonrun: {err}"));
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
    fn boundary_types_map_to_moonbit() {
        assert_eq!(
            MoonType::of(&Type::Primitive("i64".to_string())),
            Some(MoonType::Int64)
        );
        let doubles = Type::Slice {
            mutable: false,
            elem: Box::new(Type::Primitive("f64".to_string())),
        };
        assert_eq!(
            MoonType::of(&doubles).unwrap().decode("a"),
            "fgx_array(a).map(fn(fgx_item) { fgx_double(fgx_item) })"
        );
        assert_eq!(MoonType::of(&Type::Named("Point".to_string())), None);
    }

    #[test]
    fn entry_and_functions_are_found_at_top_level() {
        let body = "fn helper(x : Int) -> Int {\n  x * 2\n}\n\npub fn double_all(xs : Array[Int]) -> Array[Int] {\n  xs.map(helper)\n}\n";
        assert_eq!(defined_functions(body), vec!["helper", "double_all"]);
    }

    #[test]
    fn wasm_import_sections_are_read() {
        // (module (import "spectest" "print_char" (func (param i32))))
        let module = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // header
            0x01, 0x05, 0x01, 0x60, 0x01, 0x7f, 0x00, // type section
            0x02, 0x17, 0x01, // import section, one import
            0x08, b's', b'p', b'e', b'c', b't', b'e', b's', b't', 0x0a, b'p', b'r', b'i', b'n',
            b't', b'_', b'c', b'h', b'a', b'r', 0x00, 0x00,
        ];
        assert_eq!(
            wasm_imports(&module).unwrap(),
            vec![("spectest".to_string(), "print_char".to_string())]
        );
        assert!(wasm_imports(b"not wasm").is_err());
    }
}
