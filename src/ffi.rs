//! C-ABI FFI bridge (v2.13).
//!
//! Makes native-class capsules *execute* instead of mock. The typed capsule
//! boundary becomes a real `extern "C"` signature: the capsule body compiles
//! to an object, a generated harness compiles against the boundary header,
//! the two are linked, and the program runs each fixture for real — so the
//! fixtures become execution oracles, not just type checks. Linking/execution
//! is gated: the capsule must pass `check` (effects covered by sandbox grants,
//! dependencies pinned) or it is denied.
//!
//! Current live scope (honest): C/C++ and Rust scalar, scalar-slice, and restricted
//! `@repr(C)` struct values, plus pinned Zig 0.14 numeric scalars, scalar
//! slices, and restricted `extern struct` values. C++ and Rust must
//! expose explicit C ABI boundaries; Rust additionally requires a public
//! no-mangle export. C, Rust, and Zig support one owned byte-vector output,
//! the reviewed owned `i16` and `i32` vector layouts, or one owned UTF-8 string.
//! Broader owned outputs join only when their layout/ownership rules land.
//! Host compilers are gated gracefully when absent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ast::{Capsule, CapsuleValue, Field, Program, StructDecl, Type};
use crate::json;
use crate::native_build::{
    detect_cpp_toolchain, detect_rust_toolchain, detect_toolchain, detect_zig_toolchain,
    run_with_path, run_with_path_contained_limited, run_with_path_timeout_limited, ProcessLimits,
    Toolchain,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Marshal {
    pub name: String,
    pub fgx_type: String,
    pub c_type: String,
    pub direction: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureRun {
    pub index: usize,
    pub inputs: Vec<String>,
    pub output: String,
    pub expected: String,
    pub pass: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FfiPlan {
    pub schema: String,
    pub file: String,
    pub capsule: String,
    pub language: String,
    pub risk_class: String,
    pub extern_c_signature: String,
    pub marshaling: Vec<Marshal>,
    pub link_units: Vec<String>,
    pub effects: Vec<String>,
    pub grants: Vec<String>,
    pub deps_pinned: bool,
    pub gate_decision: String,
    pub gate_reasons: Vec<String>,
    pub execution_status: String,
    pub toolchain: Option<Toolchain>,
    pub passed: usize,
    pub total: usize,
    pub transcript: Vec<FixtureRun>,
    pub message: Option<String>,
}

/// A live value crossing the process-isolated native ABI. Fixtures retain the
/// smaller [`CapsuleValue`] surface; this runtime-only representation adds
/// Forgentex-owned scalar slices. Mutable slices are copied back only through
/// the validated child-process response.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeValue {
    Scalar(CapsuleValue),
    ReadSlice(Vec<CapsuleValue>),
    MutSlice(Vec<CapsuleValue>),
    /// Scalar fields in the declaration order of a restricted `@repr(C)`
    /// named struct. The struct name remains in the typed capsule binding.
    Struct(Vec<CapsuleValue>),
}

/// A mutable slice copied back from a completed native child process. The
/// index is the capsule input position, preserving the typed declaration order
/// without relying on foreign-language parameter names at runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMutation {
    pub input_index: usize,
    pub values: Vec<CapsuleValue>,
}

/// The validated scalar or restricted-struct return value plus every mutable
/// slice updated by a live native call. Read-only calls simply return an empty
/// `mutations` list.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeInvocation {
    pub output: String,
    pub mutations: Vec<NativeMutation>,
    pub owned_output_bytes: Option<usize>,
    pub artifact: Option<NativeArtifactReceipt>,
    pub(crate) owned_output: Option<Vec<u8>>,
    pub receipt: Option<NativeReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeArtifactReceipt {
    pub schema: String,
    pub media_type: String,
    pub bytes: usize,
    pub sha256: String,
    pub path: Option<String>,
}

/// Evidence recorded for a completed native call. It identifies the derived
/// boundary, exact compiler fingerprint, elapsed native work, applied limits,
/// containment, and compiled-runner reuse without exposing the request payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeReceipt {
    pub schema: String,
    pub boundary_hash: String,
    pub toolchain: String,
    pub duration_ms: u128,
    pub wall_limit_ms: Option<u128>,
    pub owned_output_bytes: Option<usize>,
    pub artifact: Option<NativeArtifactReceipt>,
    pub containment: String,
    pub cpu_limit_ms: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub runner_reused: bool,
}

/// Non-payload evidence for a failed native dispatch. The caller-facing error
/// remains available separately, while this receipt is safe to serialize in
/// an audit transcript: it records only a stable failure class, the phase that
/// failed, elapsed time, the declared limits, and whether they reached the
/// runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFailureReceipt {
    pub schema: String,
    pub kind: String,
    pub phase: String,
    pub duration_ms: u128,
    pub wall_limit_ms: Option<u128>,
    pub cpu_limit_ms: Option<u64>,
    pub memory_limit_bytes: Option<u64>,
    pub limits_applied: bool,
    pub containment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFailure {
    pub message: String,
    pub receipt: NativeFailureReceipt,
}

/// Compiled native runners owned by one live capsule host. Entries contain no
/// request values and are removed with the host; every call still launches a
/// fresh contained process so resource limits remain per-call.
#[derive(Default)]
pub struct NativeRunnerCache {
    entries: HashMap<String, PathBuf>,
}

impl NativeRunnerCache {
    fn acquire(&mut self, key: &str, capsule: &Capsule) -> Result<(PathBuf, bool), String> {
        if let Some(scratch) = self.entries.get(key) {
            if native_runner_exe(scratch).is_file() {
                return Ok((scratch.clone(), true));
            }
        }
        if let Some(stale) = self.entries.remove(key) {
            let _ = std::fs::remove_dir_all(stale);
        }
        live_scratch(capsule).map(|scratch| (scratch, false))
    }

    fn retain(&mut self, key: String, scratch: PathBuf) {
        self.entries.insert(key, scratch);
    }
}

impl Drop for NativeRunnerCache {
    fn drop(&mut self) {
        for scratch in self.entries.values() {
            let _ = std::fs::remove_dir_all(scratch);
        }
    }
}

struct LiveRunner<'a> {
    program: &'a Program,
    capsule: &'a Capsule,
    native_args: &'a [NativeValue],
    args: &'a [String],
    signature: &'a str,
    standard: &'a str,
    output: NativeOutput,
    toolchain: &'a Toolchain,
    compiler_dir: Option<&'a Path>,
    scratch: &'a Path,
    reuse_runner: bool,
}

/// Map a scalar Forgentex type to its C-ABI type. Non-scalar types are not
/// part of this slice's marshalable set.
fn c_type(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Bool => Some("int"),
        Type::Primitive(name) => match name.as_str() {
            "i8" => Some("int8_t"),
            "i16" => Some("int16_t"),
            "i32" => Some("int32_t"),
            "i64" => Some("int64_t"),
            "isize" => Some("ptrdiff_t"),
            "u8" => Some("uint8_t"),
            "u16" => Some("uint16_t"),
            "u32" => Some("uint32_t"),
            "u64" => Some("uint64_t"),
            "usize" => Some("size_t"),
            "f32" => Some("float"),
            "f64" => Some("double"),
            _ => None,
        },
        _ => None,
    }
}

fn c_value_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Named(name) => Some(name.clone()),
        _ => c_type(ty).map(str::to_string),
    }
}

fn native_struct_names(capsule: &Capsule) -> Vec<String> {
    capsule
        .inputs
        .iter()
        .chain(capsule.outputs.iter())
        .filter_map(|binding| match &binding.ty {
            Type::Named(name) => Some(name.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn host_abi_triple() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("aarch64-unknown-linux-gnu")
    } else {
        None
    }
}

fn native_struct_header(program: &Program, capsule: &Capsule) -> Result<String, String> {
    let names = native_struct_names(capsule);
    let owned = capsule
        .outputs
        .first()
        .and_then(|binding| owned_element(&binding.ty));
    let owned_string = capsule
        .outputs
        .first()
        .is_some_and(|binding| is_owned_string_type(&binding.ty));
    if names.is_empty() && owned.is_none() && !owned_string {
        return Ok("/* no native value structs */\n".to_string());
    }
    for name in &names {
        validate_native_struct(program, name)?;
    }
    let triple = host_abi_triple().ok_or_else(|| {
        "native_capsule_boundary_unsupported: host target has no reviewed struct ABI model"
            .to_string()
    })?;
    let mut header = crate::abi::generate_struct_header(program, triple, &names)
        .map_err(|err| format!("native_capsule_boundary_unsupported: {err}"))?;
    if let Some(element) = owned {
        header.push_str(&format!(
            "\ntypedef struct {descriptor} {{\n    {c_type} *ptr;\n    size_t len;\n    size_t capacity;\n}} {descriptor};\n",
            descriptor = element.descriptor(),
            c_type = element.c_type()
        ));
    } else if owned_string {
        header.push_str(
            "\ntypedef struct FgxOwnedStr {\n    uint8_t *ptr;\n    size_t len;\n    size_t capacity;\n} FgxOwnedStr;\n",
        );
    }
    Ok(header)
}

fn validate_native_struct<'a>(program: &'a Program, name: &str) -> Result<&'a StructDecl, String> {
    let decl = program
        .structs()
        .find(|decl| decl.name == name)
        .ok_or_else(|| format!("native struct input '{name}' is not declared"))?;
    let repr_c = decl
        .attrs
        .iter()
        .any(|attr| attr.name == "repr" && attr.args.iter().any(|arg| arg == "C"));
    if !repr_c {
        return Err(format!(
            "native struct input '{name}' requires @repr(C) for a stable ABI"
        ));
    }
    if decl.attrs.iter().any(|attr| {
        attr.name == "packed"
            || attr.name == "aligned"
            || (attr.name == "repr" && attr.args.iter().any(|arg| arg == "packed"))
    }) {
        return Err(format!(
            "native struct input '{name}' cannot use packed or custom-aligned layout in this slice"
        ));
    }
    if decl.fields.is_empty() {
        return Err(format!("native struct input '{name}' cannot be empty"));
    }
    for field in &decl.fields {
        validate_native_struct_field(name, field)?;
    }
    Ok(decl)
}

fn validate_native_struct_field(struct_name: &str, field: &Field) -> Result<(), String> {
    if !field.attrs.is_empty() {
        return Err(format!(
            "native struct input '{struct_name}.{}' cannot carry field layout annotations in this slice",
            field.name
        ));
    }
    if !matches!(field.ty, Type::Primitive(_)) || c_type(&field.ty).is_none() {
        return Err(format!(
            "native struct input '{struct_name}.{}' must be a scalar primitive, got {}",
            field.name,
            field.ty.to_agent()
        ));
    }
    Ok(())
}

fn is_cpp_language(language: &str) -> bool {
    matches!(language, "cpp" | "c++")
}

fn is_rust_language(language: &str) -> bool {
    language == "rust"
}

fn is_zig_language(language: &str) -> bool {
    language == "zig"
}

fn is_c_family_language(language: &str) -> bool {
    language == "c" || is_cpp_language(language)
}

fn is_live_native_language(language: &str) -> bool {
    is_c_family_language(language) || is_rust_language(language) || is_zig_language(language)
}

fn native_standard(capsule: &Capsule) -> Result<&'static str, String> {
    match capsule.language.as_str() {
        "c" => Ok("-std=c11"),
        "cpp" | "c++" => match capsule.version.as_str() {
            "c++11" => Ok("-std=c++11"),
            "c++14" => Ok("-std=c++14"),
            "c++17" => Ok("-std=c++17"),
            "c++20" => Ok("-std=c++20"),
            "c++23" => Ok("-std=c++23"),
            version => Err(format!(
                "C++ capsule '{}' declares unsupported standard '{}'; expected c++11, c++14, c++17, c++20, or c++23",
                capsule.name, version
            )),
        },
        "rust" => match capsule.version.as_str() {
            "2018" => Ok("--edition=2018"),
            "2021" => Ok("--edition=2021"),
            "2024" => Ok("--edition=2024"),
            version => Err(format!(
                "Rust capsule '{}' declares unsupported edition '{}'; expected 2018, 2021, or 2024",
                capsule.name, version
            )),
        },
        "zig" => match capsule.version.as_str() {
            "0.14" => Ok("0.14"),
            version => Err(format!(
                "Zig capsule '{}' declares unsupported version '{}'; this slice pins Zig 0.14",
                capsule.name, version
            )),
        },
        language => Err(format!(
            "native language '{language}' is not executable in this slice"
        )),
    }
}

fn native_source_extension(capsule: &Capsule) -> &'static str {
    if is_cpp_language(&capsule.language) {
        "cpp"
    } else {
        "c"
    }
}

fn detect_native_toolchain(capsule: &Capsule) -> Option<Toolchain> {
    if is_rust_language(&capsule.language) {
        detect_rust_toolchain()
    } else if is_zig_language(&capsule.language) {
        detect_zig_toolchain()
    } else if is_cpp_language(&capsule.language) {
        detect_cpp_toolchain()
    } else {
        detect_toolchain()
    }
}

fn compiler_unavailable_message(capsule: &Capsule) -> &'static str {
    if is_rust_language(&capsule.language) {
        "no host Rust compiler found (looked for $RUSTC, rustc)"
    } else if is_zig_language(&capsule.language) {
        "no host Zig compiler found (looked for $ZIG, zig)"
    } else if is_cpp_language(&capsule.language) {
        "no host C++ compiler found (looked for $CXX, c++, clang++, g++)"
    } else {
        "no host C compiler found (looked for $CC, cc, clang, gcc)"
    }
}

pub(crate) fn validate_native_toolchain_version(
    capsule: &Capsule,
    toolchain: &Toolchain,
) -> Result<(), String> {
    let reviewed_zig = toolchain.version == "0.14" || toolchain.version.starts_with("0.14.");
    if is_zig_language(&capsule.language) && !reviewed_zig {
        return Err(format!(
            "native_runtime_version_mismatch: capsule '{}' pins Zig 0.14, found {}",
            capsule.name, toolchain.version
        ));
    }
    Ok(())
}

fn native_wall_timeout(capsule: &Capsule) -> Option<Duration> {
    crate::sandbox::analyze_capsule(capsule)
        .limits
        .into_iter()
        .find_map(|(key, value)| {
            (key == "wall_ms")
                .then(|| value.parse::<u64>().ok())
                .flatten()
                .map(Duration::from_millis)
        })
}

fn native_process_limits(capsule: &Capsule) -> ProcessLimits {
    let mut limits = ProcessLimits::default();
    for (key, value) in crate::sandbox::analyze_capsule(capsule).limits {
        match key.as_str() {
            "cpu_ms" => limits.cpu_ms = value.parse().ok(),
            "mem_mb" => {
                limits.memory_bytes = value
                    .parse::<u64>()
                    .ok()
                    .map(|megabytes| megabytes.saturating_mul(1024 * 1024));
            }
            _ => {}
        }
    }
    limits
}

fn validate_native_language_contract(capsule: &Capsule) -> Result<(), String> {
    native_standard(capsule)?;
    if is_cpp_language(&capsule.language)
        && !capsule
            .code
            .iter()
            .any(|line| line.contains("extern \"C\""))
    {
        return Err(format!(
            "C++ capsule '{}' must define its boundary function with explicit extern \"C\" linkage",
            capsule.name
        ));
    }
    if is_rust_language(&capsule.language) {
        let source = capsule.code.join("\n");
        if !source.contains("extern \"C\"") {
            return Err(format!(
                "Rust capsule '{}' must define its boundary function with explicit extern \"C\" ABI",
                capsule.name
            ));
        }
        if !source.contains("pub ") {
            return Err(format!(
                "Rust capsule '{}' must export a public boundary function",
                capsule.name
            ));
        }
        if !source.contains("#[no_mangle]") && !source.contains("#[unsafe(no_mangle)]") {
            return Err(format!(
                "Rust capsule '{}' must mark its boundary with #[unsafe(no_mangle)] (or #[no_mangle] on older toolchains)",
                capsule.name
            ));
        }
    }
    if is_zig_language(&capsule.language) {
        let source = capsule.code.join("\n");
        if !source.contains(&format!("pub export fn {}", capsule.name)) {
            return Err(format!(
                "Zig capsule '{}' must define its boundary as explicit `pub export fn {}`",
                capsule.name, capsule.name
            ));
        }
    }
    Ok(())
}

fn boundary_header(capsule: &Capsule, signature: &str) -> String {
    let deallocator = capsule
        .outputs
        .first()
        .and_then(|binding| {
            owned_element(&binding.ty)
                .map(|element| element.c_type())
                .or_else(|| is_owned_string_type(&binding.ty).then_some("uint8_t"))
        })
        .map(|c_type| {
            format!(
                "void {}_free({c_type} *ptr, size_t len, size_t capacity);\n",
                capsule.name
            )
        })
        .unwrap_or_default();
    format!(
        "#include \"types.h\"\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n{signature};\n{deallocator}#ifdef __cplusplus\n}}\n#endif\n"
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum OutKind {
    SignedInt,
    UnsignedInt,
    Float,
    Bool,
}

#[derive(Clone, Debug, PartialEq)]
enum NativeOutput {
    Scalar(OutKind),
    Struct(String),
    Owned(OwnedElement),
    OwnedString,
}

/// Reviewed element layouts for single owned native vector returns. Length
/// and capacity are always element counts; receipts account in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnedElement {
    U8,
    I16,
    I32,
}

impl OwnedElement {
    fn fgx_name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::I16 => "i16",
            Self::I32 => "i32",
        }
    }

    fn c_type(self) -> &'static str {
        match self {
            Self::U8 => "uint8_t",
            Self::I16 => "int16_t",
            Self::I32 => "int32_t",
        }
    }

    fn descriptor(self) -> &'static str {
        match self {
            Self::U8 => "FgxOwnedU8",
            Self::I16 => "FgxOwnedI16",
            Self::I32 => "FgxOwnedI32",
        }
    }

    fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16 => 2,
            Self::I32 => 4,
        }
    }

    fn ty(self) -> Type {
        Type::Primitive(self.fgx_name().to_string())
    }
}

const MAX_NATIVE_OWNED_OUTPUT_BYTES: usize = crate::capsule::MAX_NATIVE_ARTIFACT_BYTES;

fn native_owned_output_limit(capsule: &Capsule) -> usize {
    crate::capsule::artifact_policy(capsule)
        .ok()
        .flatten()
        .map(|policy| policy.max_bytes)
        .unwrap_or(MAX_NATIVE_OWNED_OUTPUT_BYTES)
}

fn owned_element(ty: &Type) -> Option<OwnedElement> {
    let Type::Generic { name, args } = ty else {
        return None;
    };
    if name != "Vec" {
        return None;
    }
    match args.as_slice() {
        [Type::Primitive(element)] if element == "u8" => Some(OwnedElement::U8),
        [Type::Primitive(element)] if element == "i16" => Some(OwnedElement::I16),
        [Type::Primitive(element)] if element == "i32" => Some(OwnedElement::I32),
        _ => None,
    }
}

fn is_owned_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(name) if name == "str")
}

fn out_kind(ty: &Type) -> Option<OutKind> {
    match ty {
        Type::Bool => Some(OutKind::Bool),
        Type::Primitive(name) => match name.as_str() {
            "f32" | "f64" => Some(OutKind::Float),
            "u8" | "u16" | "u32" | "u64" | "usize" => Some(OutKind::UnsignedInt),
            other if crate::ast::is_integer_primitive(other) => Some(OutKind::SignedInt),
            _ => None,
        },
        _ => None,
    }
}

fn native_output_kind(program: &Program, ty: &Type) -> Result<NativeOutput, String> {
    if let Some(element) = owned_element(ty) {
        return Ok(NativeOutput::Owned(element));
    }
    if is_owned_string_type(ty) {
        return Ok(NativeOutput::OwnedString);
    }
    match ty {
        Type::Named(name) => {
            validate_native_struct(program, name)?;
            Ok(NativeOutput::Struct(name.clone()))
        }
        _ => out_kind(ty).map(NativeOutput::Scalar).ok_or_else(|| {
            format!(
                "output type '{}' is not scalar or a reviewed struct",
                ty.to_agent()
            )
        }),
    }
}

fn marshaling(capsule: &Capsule) -> Vec<Marshal> {
    let mut out = Vec::new();
    for binding in &capsule.inputs {
        out.push(Marshal {
            name: binding.name.clone(),
            fgx_type: binding.ty.to_agent(),
            c_type: c_input_type(&binding.ty).unwrap_or_else(|| "?".to_string()),
            direction: "in".to_string(),
        });
    }
    for binding in &capsule.outputs {
        out.push(Marshal {
            name: binding.name.clone(),
            fgx_type: binding.ty.to_agent(),
            c_type: if let Some(element) = owned_element(&binding.ty) {
                format!("{} + required deallocator", element.descriptor())
            } else if is_owned_string_type(&binding.ty) {
                "FgxOwnedStr + required deallocator + UTF-8 validation".to_string()
            } else {
                c_value_type(&binding.ty).unwrap_or_else(|| "?".to_string())
            },
            direction: "out".to_string(),
        });
    }
    out
}

fn c_input_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Slice {
            mutable: false,
            elem,
        } => c_type(elem).map(|elem| format!("const {elem}* + size_t")),
        Type::Slice {
            mutable: true,
            elem,
        } => c_type(elem).map(|elem| format!("{elem}* + size_t (copy-back)")),
        _ => c_value_type(ty),
    }
}

fn c_input_params(binding: &crate::ast::CapsuleBinding) -> Result<Vec<String>, String> {
    match &binding.ty {
        Type::Slice {
            mutable: false,
            elem,
        } => {
            let elem = c_type(elem).ok_or_else(|| {
                format!(
                    "slice input '{}' element type '{}' is not scalar",
                    binding.name,
                    elem.to_agent()
                )
            })?;
            Ok(vec![
                format!("const {elem} *{}", binding.name),
                format!("size_t {}_len", binding.name),
            ])
        }
        Type::Slice {
            mutable: true,
            elem,
        } => {
            let elem = c_type(elem).ok_or_else(|| {
                format!(
                    "slice input '{}' element type '{}' is not scalar",
                    binding.name,
                    elem.to_agent()
                )
            })?;
            Ok(vec![
                format!("{elem} *{}", binding.name),
                format!("size_t {}_len", binding.name),
            ])
        }
        ty => c_value_type(ty)
            .map(|c| vec![format!("{c} {}", binding.name)])
            .ok_or_else(|| {
                format!(
                    "input type '{}' is not scalar or a reviewed struct",
                    ty.to_agent()
                )
            }),
    }
}

fn c_call_arg_names(binding: &crate::ast::CapsuleBinding) -> Vec<String> {
    match &binding.ty {
        Type::Slice { .. } => vec![binding.name.clone(), format!("{}_len", binding.name)],
        _ => vec![binding.name.clone()],
    }
}

/// Derive the `extern "C"` signature from the typed boundary. Requires one
/// scalar or restricted `@repr(C)` struct output and scalar inputs, reviewed
/// struct inputs, or scalar-element slices.
fn extern_c_signature(capsule: &Capsule) -> Result<String, String> {
    if capsule.outputs.len() != 1 {
        return Err(format!(
            "boundary has {} outputs; this slice supports exactly one output",
            capsule.outputs.len()
        ));
    }
    let ret = if let Some(element) = owned_element(&capsule.outputs[0].ty) {
        Some(element.descriptor().to_string())
    } else if is_owned_string_type(&capsule.outputs[0].ty) {
        Some("FgxOwnedStr".to_string())
    } else {
        c_value_type(&capsule.outputs[0].ty)
    }
    .ok_or_else(|| {
        format!(
            "output type '{}' is not scalar or a reviewed struct",
            capsule.outputs[0].ty.to_agent()
        )
    })?;
    let params = if capsule.inputs.is_empty() {
        "void".to_string()
    } else {
        capsule
            .inputs
            .iter()
            .map(c_input_params)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ")
    };
    Ok(format!("{ret} {}({params})", capsule.name))
}

fn float_repr(value: f64) -> String {
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') || text.contains("inf") || text.contains("nan") {
        text
    } else {
        format!("{text}.0")
    }
}

/// A fixture input value as a C literal at the given boundary type.
fn c_input_literal(value: &CapsuleValue, ty: &Type) -> String {
    let is_f32 = matches!(ty, Type::Primitive(n) if n == "f32");
    let is_f64 = matches!(ty, Type::Primitive(n) if n == "f64");
    match value {
        CapsuleValue::Float(v) if is_f32 => format!("{}f", float_repr(*v)),
        CapsuleValue::Float(v) => float_repr(*v),
        CapsuleValue::Int(v) if is_f32 => format!("{v}.0f"),
        CapsuleValue::Int(v) if is_f64 => format!("{v}.0"),
        CapsuleValue::Int(v) => v.to_string(),
        CapsuleValue::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        CapsuleValue::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
    }
}

fn outputs_match(actual: &str, expected: &CapsuleValue) -> bool {
    match expected {
        CapsuleValue::Int(v) => actual
            .trim()
            .parse::<i64>()
            .map(|a| a == *v)
            .unwrap_or(false),
        CapsuleValue::Bool(b) => actual.trim() == if *b { "1" } else { "0" },
        CapsuleValue::Float(v) => actual
            .trim()
            .parse::<f64>()
            .map(|a| (a - *v).abs() <= 1e-6 * v.abs().max(1.0))
            .unwrap_or(false),
        CapsuleValue::Str(s) => actual.trim() == s,
    }
}

pub fn run(
    file: &str,
    program: &Program,
    name: Option<&str>,
    out_dir: &Path,
) -> Result<FfiPlan, String> {
    let capsule = select_capsule(program, name)?;
    if capsule.risk_class() != "native" {
        return Err(format!(
            "capsule '{}' is {} ({}); --native runs C-ABI capsules — use the managed runtime bridge for this language",
            capsule.name,
            capsule.risk_class(),
            capsule.language
        ));
    }

    let marshaling = marshaling(capsule);
    let signature = extern_c_signature(capsule);

    // Gate: the capsule must pass check (effects covered, deps pinned).
    let diagnostics = crate::typecheck::check(program);
    let deps_pinned = capsule.deps.iter().all(|d| d.is_pinned());
    let mut reasons = Vec::new();
    if !diagnostics.is_empty() {
        reasons.push(format!("check failed: {} diagnostic(s)", diagnostics.len()));
    }
    if !deps_pinned {
        reasons.push("capsule has unpinned dependencies".to_string());
    }
    let allowed = reasons.is_empty();

    let mut plan = FfiPlan {
        schema: "forgentex.ffi_plan.v1".to_string(),
        file: file.to_string(),
        capsule: capsule.id.clone(),
        language: capsule.language.clone(),
        risk_class: capsule.risk_class().to_string(),
        extern_c_signature: signature.clone().unwrap_or_default(),
        marshaling,
        link_units: Vec::new(),
        effects: capsule.effects.clone(),
        grants: capsule.sandbox.clone(),
        deps_pinned,
        gate_decision: if allowed { "allow" } else { "deny" }.to_string(),
        gate_reasons: reasons,
        execution_status: "planned".to_string(),
        toolchain: None,
        passed: 0,
        total: capsule.fixtures.len(),
        transcript: Vec::new(),
        message: None,
    };

    if !allowed {
        plan.execution_status = "denied".to_string();
        return Ok(plan);
    }
    let signature = match signature {
        Ok(signature) => signature,
        Err(message) => {
            plan.execution_status = "unsupported_boundary".to_string();
            plan.message = Some(message);
            return Ok(plan);
        }
    };
    if !is_c_family_language(&capsule.language) {
        plan.execution_status = "execution_unsupported_in_slice".to_string();
        plan.message = Some(format!(
            "fixture execution is wired for C/C++ in this slice; {} is available through fgx run --live",
            capsule.language
        ));
        return Ok(plan);
    }

    if let Err(message) = validate_native_language_contract(capsule) {
        plan.execution_status = "unsupported_boundary".to_string();
        plan.message = Some(message);
        return Ok(plan);
    }
    let types_header = match native_struct_header(program, capsule) {
        Ok(header) => header,
        Err(message) => {
            plan.execution_status = "unsupported_boundary".to_string();
            plan.message = Some(message);
            return Ok(plan);
        }
    };
    let standard = match native_standard(capsule) {
        Ok(standard) => standard,
        Err(message) => {
            plan.execution_status = "unsupported_boundary".to_string();
            plan.message = Some(message);
            return Ok(plan);
        }
    };

    let toolchain = match detect_native_toolchain(capsule) {
        Some(toolchain) => toolchain,
        None => {
            plan.execution_status = "compiler_unavailable".to_string();
            plan.message = Some(compiler_unavailable_message(capsule).to_string());
            return Ok(plan);
        }
    };
    let compiler_dir = Path::new(&toolchain.name).parent();

    if let Err(message) = std::fs::create_dir_all(out_dir) {
        return Err(format!("failed to create {}: {message}", out_dir.display()));
    }
    let capsule_c = out_dir.join(format!("capsule.{}", native_source_extension(capsule)));
    let types_h = out_dir.join("types.h");
    let boundary_h = out_dir.join("boundary.h");
    let runner_c = out_dir.join(format!("runner.{}", native_source_extension(capsule)));
    let capsule_o = out_dir.join("capsule.o");
    let runner_o = out_dir.join("runner.o");
    let exe = out_dir.join(if cfg!(windows) {
        "runner.exe"
    } else {
        "runner"
    });

    let body = capsule.code.join("\n");
    let output_kind = native_output_kind(program, &capsule.outputs[0].ty)?;
    if matches!(
        output_kind,
        NativeOutput::Owned(_) | NativeOutput::OwnedString
    ) {
        plan.execution_status = "execution_unsupported_in_slice".to_string();
        plan.message = Some(
            "owned outputs execute through fgx run --live; static fixture literals do not own native allocations"
                .to_string(),
        );
        return Ok(plan);
    }
    let harness = generate_harness(capsule, &signature, &output_kind)?;
    write_file(&capsule_c, &body)?;
    write_file(&types_h, &types_header)?;
    write_file(&boundary_h, &boundary_header(capsule, &signature))?;
    write_file(&runner_c, &harness)?;

    // Compile the capsule body and the harness, then link them.
    let steps = [
        vec![
            tc(&toolchain),
            standard.into(),
            "-O2".into(),
            "-c".into(),
            "-I".into(),
            disp(out_dir),
            disp(&capsule_c),
            "-o".into(),
            disp(&capsule_o),
        ],
        vec![
            tc(&toolchain),
            standard.into(),
            "-O2".into(),
            "-c".into(),
            "-I".into(),
            disp(out_dir),
            disp(&runner_c),
            "-o".into(),
            disp(&runner_o),
        ],
        vec![
            tc(&toolchain),
            disp(&capsule_o),
            disp(&runner_o),
            "-o".into(),
            disp(&exe),
        ],
    ];
    for argv in &steps {
        match run_with_path(argv, compiler_dir) {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                plan.execution_status = "compile_failed".to_string();
                plan.toolchain = Some(toolchain);
                plan.message = Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
                return Ok(plan);
            }
            Err(err) => {
                plan.execution_status = "compile_failed".to_string();
                plan.toolchain = Some(toolchain);
                plan.message = Some(format!("failed to launch compiler: {err}"));
                return Ok(plan);
            }
        }
    }
    plan.link_units = vec![disp(&capsule_o), disp(&runner_o)];

    // Run the linked program; it prints one line per fixture.
    let run_argv = vec![disp(&exe)];
    let output = match run_with_path(&run_argv, compiler_dir) {
        Ok(output) => output,
        Err(err) => {
            plan.execution_status = "run_failed".to_string();
            plan.toolchain = Some(toolchain);
            plan.message = Some(format!("failed to launch program: {err}"));
            return Ok(plan);
        }
    };
    plan.toolchain = Some(toolchain);
    if !output.status.success() {
        plan.execution_status = "run_failed".to_string();
        plan.message = Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
        return Ok(plan);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    for (index, fixture) in capsule.fixtures.iter().enumerate() {
        let inputs: Vec<String> = fixture
            .inputs
            .iter()
            .map(CapsuleValue::to_literal)
            .collect();
        let expected = fixture
            .outputs
            .first()
            .map(CapsuleValue::to_literal)
            .unwrap_or_default();
        let actual = lines.get(index).copied().unwrap_or("").trim().to_string();
        let pass = fixture
            .outputs
            .first()
            .map(|exp| outputs_match(&actual, exp))
            .unwrap_or(false);
        if pass {
            plan.passed += 1;
        }
        plan.transcript.push(FixtureRun {
            index,
            inputs,
            output: actual,
            expected,
            pass,
        });
    }
    plan.execution_status = if capsule.fixtures.is_empty() {
        "compiled".to_string()
    } else if plan.passed == plan.total {
        "executed".to_string()
    } else {
        "mismatch".to_string()
    };
    Ok(plan)
}

/// Invoke a native C/C++/Rust/Zig capsule through an isolated live child.
///
/// This is the runtime half of the v2.13 FFI bridge. The existing bridge
/// proves a derived C ABI by compiling a capsule plus a fixture harness. This
/// function uses the same derived ABI, but generates a one-call runner and
/// launches it as a child process. Native code therefore never enters the
/// interpreter process. This slice deliberately supports only checked C,
/// C++ and Rust capsules with scalar/slice/restricted-struct boundaries, plus
/// pinned Zig 0.14 capsules with numeric scalar, scalar-slice, and restricted
/// struct boundaries.
pub fn invoke_live(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
) -> Result<NativeInvocation, String> {
    invoke_live_impl(program, capsule, args, None)
}

/// Reuse a compiled runner owned by the current live host while retaining a
/// fresh contained process for every invocation.
pub fn invoke_live_cached(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
    cache: &mut NativeRunnerCache,
) -> Result<NativeInvocation, String> {
    invoke_live_impl(program, capsule, args, Some(cache))
}

fn invoke_live_impl(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
    mut cache: Option<&mut NativeRunnerCache>,
) -> Result<NativeInvocation, String> {
    if capsule.risk_class() != "native" || !is_live_native_language(&capsule.language) {
        return Err(format!(
            "native_runtime_unsupported: live native dispatch currently supports C/C++/Rust/Zig capsules, got '{}' ({})",
            capsule.name, capsule.language
        ));
    }
    if !crate::typecheck::check(program).is_empty() {
        return Err("native_capsule_check_failed: program has check diagnostics".to_string());
    }
    if !crate::sandbox::is_approved(capsule) {
        return Err(format!(
            "native_capsule_sandbox_denied: capsule '{}' is denied by its sandbox",
            capsule.name
        ));
    }
    if !capsule.deps.iter().all(|dep| dep.is_pinned()) {
        return Err(format!(
            "native_capsule_dependency_unpinned: capsule '{}' has an unpinned dependency",
            capsule.name
        ));
    }
    validate_native_language_contract(capsule)
        .map_err(|err| format!("native_capsule_boundary_unsupported: {err}"))?;
    let artifact_policy = crate::capsule::artifact_policy(capsule)
        .map_err(|err| format!("native_capsule_artifact_policy_invalid: {err}"))?;
    native_struct_header(program, capsule)
        .map_err(|err| format!("native_capsule_boundary_unsupported: {err}"))?;
    if capsule.inputs.len() != args.len() {
        return Err(format!(
            "native_capsule_arity: capsule '{}' expects {} argument(s), got {}",
            capsule.name,
            capsule.inputs.len(),
            args.len()
        ));
    }
    for (arg, binding) in args.iter().zip(&capsule.inputs) {
        validate_native_arg(program, arg, &binding.ty, &binding.name)?;
    }
    if is_rust_language(&capsule.language) {
        validate_rust_boundary(program, capsule, args)?;
    } else if is_zig_language(&capsule.language) {
        validate_zig_boundary(program, capsule, args)?;
    }

    let signature = extern_c_signature(capsule)
        .map_err(|err| format!("native_capsule_boundary_unsupported: {err}"))?;
    let standard = native_standard(capsule)
        .map_err(|err| format!("native_capsule_boundary_unsupported: {err}"))?;
    let output = native_output_kind(program, &capsule.outputs[0].ty)
        .map_err(|err| format!("native_capsule_boundary_unsupported: {err}"))?;
    // Validate and render arguments before looking for a host compiler. This
    // keeps type/range errors deterministic on every machine and prevents the
    // generated C runner from silently narrowing an out-of-range value.
    let rendered_args = args
        .iter()
        .zip(&capsule.inputs)
        .map(|(arg, binding)| native_arg_wire(program, arg, &binding.ty))
        .collect::<Result<Vec<_>, _>>()?;
    let toolchain = detect_native_toolchain(capsule).ok_or_else(|| {
        format!(
            "native_runtime_unavailable: {}",
            compiler_unavailable_message(capsule)
        )
    })?;
    validate_native_toolchain_version(capsule, &toolchain)?;
    let started = Instant::now();
    let wall_limit_ms = native_wall_timeout(capsule).map(|limit| limit.as_millis());
    let process_limits = native_process_limits(capsule);
    let compiler_dir = Path::new(&toolchain.name).parent();
    let cache_key = native_runner_cache_key(program, capsule, &toolchain, &signature, standard);
    let (scratch, runner_reused) = match cache.as_deref_mut() {
        Some(cache) => cache.acquire(&cache_key, capsule)?,
        None => (live_scratch(capsule)?, false),
    };
    let runner = LiveRunner {
        program,
        capsule,
        native_args: args,
        args: &rendered_args,
        signature: &signature,
        standard,
        output,
        toolchain: &toolchain,
        compiler_dir,
        scratch: &scratch,
        reuse_runner: runner_reused,
    };
    let result = if is_rust_language(&capsule.language) {
        invoke_rust_runner(runner)
    } else if is_zig_language(&capsule.language) {
        invoke_zig_runner(runner)
    } else {
        invoke_c_runner(runner)
    };
    let _ = std::fs::remove_file(scratch.join("request.txt"));
    if let Some(cache) = cache {
        if native_runner_exe(&scratch).is_file() {
            cache.retain(cache_key, scratch.clone());
        } else {
            let _ = std::fs::remove_dir_all(&scratch);
        }
    } else {
        let _ = std::fs::remove_dir_all(&scratch);
    }
    let mut invocation = result?;
    if let Some(policy) = &artifact_policy {
        if let Some(path) = &policy.path {
            let bytes = invocation.owned_output.as_deref().ok_or_else(|| {
                "native_capsule_artifact_persist_failed: owned output bytes are missing".to_string()
            })?;
            persist_native_artifact(capsule, path, bytes)?;
            if let Some(artifact) = &mut invocation.artifact {
                artifact.path = Some(path.clone());
            }
        }
    }
    invocation.receipt = Some(NativeReceipt {
        schema: "forgentex.native_receipt.v1".to_string(),
        boundary_hash: fnv1a(&format!(
            "{}{}",
            boundary_header(capsule, &signature),
            artifact_policy
                .as_ref()
                .map(|policy| format!(
                    "artifact.media_type={};artifact.max_bytes={};artifact.path={};",
                    policy.media_type,
                    policy.max_bytes,
                    policy.path.as_deref().unwrap_or("")
                ))
                .unwrap_or_default()
        )),
        toolchain: format!("{} ({})", toolchain.name, toolchain.version),
        duration_ms: started.elapsed().as_millis(),
        wall_limit_ms,
        owned_output_bytes: invocation.owned_output_bytes,
        artifact: invocation.artifact.clone(),
        containment: "process_tree".to_string(),
        cpu_limit_ms: process_limits.cpu_ms,
        memory_limit_bytes: process_limits.memory_bytes,
        runner_reused,
    });
    Ok(invocation)
}

/// Invoke a native capsule and preserve structured audit evidence when the
/// dispatch fails. The existing [`invoke_live`] API remains the convenient
/// string-error surface for direct callers; live program execution uses this
/// audited form so failures are not represented by an unstructured message
/// alone.
pub fn invoke_live_audited(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
) -> Result<NativeInvocation, Box<NativeFailure>> {
    let started = Instant::now();
    invoke_live(program, capsule, args).map_err(|message| native_failure(capsule, started, message))
}

pub fn invoke_live_cached_audited(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
    cache: &mut NativeRunnerCache,
) -> Result<NativeInvocation, Box<NativeFailure>> {
    let started = Instant::now();
    invoke_live_cached(program, capsule, args, cache)
        .map_err(|message| native_failure(capsule, started, message))
}

fn native_failure(capsule: &Capsule, started: Instant, message: String) -> Box<NativeFailure> {
    let kind = native_failure_kind(&message);
    let phase = native_failure_phase(&kind);
    let limits_applied = matches!(phase, "execute" | "response" | "persist");
    let limits = native_process_limits(capsule);
    Box::new(NativeFailure {
        message,
        receipt: NativeFailureReceipt {
            schema: "forgentex.native_failure.v1".to_string(),
            kind,
            phase: phase.to_string(),
            duration_ms: started.elapsed().as_millis(),
            wall_limit_ms: native_wall_timeout(capsule).map(|limit| limit.as_millis()),
            cpu_limit_ms: limits.cpu_ms,
            memory_limit_bytes: limits.memory_bytes,
            limits_applied,
            containment: limits_applied.then(|| "process_tree".to_string()),
        },
    })
}

fn native_failure_kind(message: &str) -> String {
    let candidate = message
        .split_once(':')
        .map(|(kind, _)| kind)
        .unwrap_or(message)
        .trim();
    if candidate.starts_with("native_")
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch == '_')
    {
        candidate.to_string()
    } else {
        "native_capsule_error".to_string()
    }
}

fn native_failure_phase(kind: &str) -> &'static str {
    match kind {
        "native_capsule_compile_failed" => "compile",
        "native_capsule_launch_failed" => "launch",
        "native_capsule_timeout" | "native_capsule_failed" => "execute",
        "native_capsule_bad_output" | "native_capsule_artifact_too_large" => "response",
        "native_capsule_artifact_exists"
        | "native_capsule_artifact_persist_denied"
        | "native_capsule_artifact_persist_failed" => "persist",
        _ => "preflight",
    }
}

fn persist_native_artifact(capsule: &Capsule, path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let relative = Path::new(path);
    let scope = capsule
        .sandbox
        .iter()
        .filter_map(|rule| rule.trim().split_once(char::is_whitespace))
        .filter(|(capability, _)| *capability == "fs.write")
        .map(|(_, scope)| Path::new(scope.trim()))
        .filter(|scope| relative.starts_with(scope))
        .max_by_key(|scope| scope.components().count())
        .ok_or_else(|| {
            "native_capsule_artifact_persist_failed: no covering fs.write scope".to_string()
        })?;
    let cwd = std::env::current_dir().map_err(|err| {
        format!("native_capsule_artifact_persist_failed: current directory unavailable: {err}")
    })?;
    let canonical_cwd = cwd.canonicalize().map_err(|err| {
        format!("native_capsule_artifact_persist_failed: cannot resolve workspace: {err}")
    })?;
    let canonical_scope = resolve_or_create_scoped_directory(&canonical_cwd, scope)?;
    let relative_parent = relative.parent().ok_or_else(|| {
        "native_capsule_artifact_persist_failed: artifact path has no parent".to_string()
    })?;
    let beneath_scope = relative_parent.strip_prefix(scope).map_err(|_| {
        "native_capsule_artifact_persist_denied: artifact parent escapes fs.write scope".to_string()
    })?;
    let canonical_parent = resolve_or_create_scoped_directory(&canonical_scope, beneath_scope)?;
    let target = canonical_parent.join(relative.file_name().ok_or_else(|| {
        "native_capsule_artifact_persist_failed: artifact filename is invalid".to_string()
    })?);
    if target.exists() {
        let existing = std::fs::read(&target).map_err(|err| {
            format!(
                "native_capsule_artifact_persist_failed: cannot verify existing artifact: {err}"
            )
        })?;
        if existing == bytes {
            return Ok(());
        }
        return Err(format!(
            "native_capsule_artifact_exists: '{}' already contains different bytes",
            path
        ));
    }
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            "native_capsule_artifact_persist_failed: artifact filename is invalid".to_string()
        })?;
    let temp = canonical_parent.join(format!(".{file_name}.fgx-tmp-{}", std::process::id()));
    let write_result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|err| {
                format!("native_capsule_artifact_persist_failed: cannot create temporary artifact: {err}")
            })?;
        file.write_all(bytes).map_err(|err| {
            format!("native_capsule_artifact_persist_failed: cannot write artifact: {err}")
        })?;
        file.sync_all().map_err(|err| {
            format!("native_capsule_artifact_persist_failed: cannot sync artifact: {err}")
        })?;
        // A hard link publishes the fully synced temporary file atomically
        // and, unlike rename on Unix, fails if a concurrent writer created the
        // destination. Removing the temporary name leaves the published link.
        std::fs::hard_link(&temp, &target).map_err(|err| {
            format!("native_capsule_artifact_persist_failed: cannot publish artifact: {err}")
        })
    })();
    let _ = std::fs::remove_file(&temp);
    write_result
}

fn resolve_or_create_scoped_directory(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        let candidate = current.join(name);
        match std::fs::create_dir(&candidate) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(format!(
                    "native_capsule_artifact_persist_failed: cannot create artifact directory '{}': {err}",
                    candidate.display()
                ));
            }
        }
        let resolved = candidate.canonicalize().map_err(|err| {
            format!(
                "native_capsule_artifact_persist_failed: cannot resolve artifact directory '{}': {err}",
                candidate.display()
            )
        })?;
        if !resolved.starts_with(root) {
            return Err(
                "native_capsule_artifact_persist_denied: resolved path escapes its approved root"
                    .to_string(),
            );
        }
        current = resolved;
    }
    Ok(current)
}

fn invoke_c_runner(request: LiveRunner<'_>) -> Result<NativeInvocation, String> {
    let LiveRunner {
        program,
        capsule,
        native_args,
        args,
        signature,
        standard,
        output,
        toolchain,
        compiler_dir,
        scratch,
        reuse_runner,
    } = request;
    let capsule_c = scratch.join(format!("capsule.{}", native_source_extension(capsule)));
    let types_h = scratch.join("types.h");
    let boundary_h = scratch.join("boundary.h");
    let runner_c = scratch.join(format!("live_runner.{}", native_source_extension(capsule)));
    let request_file = scratch.join("request.txt");
    let capsule_o = scratch.join("capsule.o");
    let runner_o = scratch.join("live_runner.o");
    let exe = native_runner_exe(scratch);

    if !reuse_runner {
        write_file(&capsule_c, &capsule.code.join("\n"))?;
        write_file(&types_h, &native_struct_header(program, capsule)?)?;
        write_file(&boundary_h, &boundary_header(capsule, signature))?;
        write_file(&runner_c, &generate_live_runner(program, capsule, &output)?)?;
    }
    let request = if args.is_empty() {
        String::new()
    } else {
        format!("{}\n", args.join("\n"))
    };
    write_file(&request_file, &request)?;

    let steps = [
        vec![
            tc(toolchain),
            standard.into(),
            "-O2".into(),
            "-c".into(),
            "-I".into(),
            disp(scratch),
            disp(&capsule_c),
            "-o".into(),
            disp(&capsule_o),
        ],
        vec![
            tc(toolchain),
            standard.into(),
            "-O2".into(),
            "-c".into(),
            "-I".into(),
            disp(scratch),
            disp(&runner_c),
            "-o".into(),
            disp(&runner_o),
        ],
        vec![
            tc(toolchain),
            disp(&capsule_o),
            disp(&runner_o),
            "-o".into(),
            disp(&exe),
        ],
    ];
    if !reuse_runner {
        for argv in &steps {
            let output = run_with_path(argv, compiler_dir).map_err(|err| {
                format!("native_capsule_compile_failed: failed to launch compiler: {err}")
            })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(format!("native_capsule_compile_failed: {stderr}"));
            }
        }
    }

    // Keep live values out of the process command line. Windows command lines
    // are commonly limited to roughly 32 KiB, far below the bounded scalar
    // slice payload supported here. The one-call child reads one typed value
    // per line from this private scratch request instead.
    let argv = vec![disp(&exe), disp(&request_file)];
    let output = match native_wall_timeout(capsule) {
        Some(timeout) => match run_with_path_timeout_limited(
            &argv,
            compiler_dir,
            timeout,
            native_process_limits(capsule),
        )
        .map_err(|err| format!("native_capsule_launch_failed: {err}"))?
        {
            Some(output) => output,
            None => {
                return Err(format!(
                    "native_capsule_timeout: capsule '{}' exceeded limit.wall_ms {}",
                    capsule.name,
                    timeout.as_millis()
                ))
            }
        },
        None => {
            run_with_path_contained_limited(&argv, compiler_dir, native_process_limits(capsule))
                .map_err(|err| format!("native_capsule_launch_failed: {err}"))?
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let status = output
            .status
            .code()
            .map(|code| format!("exit code {code}"))
            .unwrap_or_else(|| "terminated by signal".to_string());
        let detail = if stderr.is_empty() { status } else { stderr };
        return Err(format!("native_capsule_failed: {detail}"));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("native_capsule_bad_output: output was not UTF-8: {err}"))?;
    parse_native_invocation(&stdout, program, capsule, native_args)
}

fn validate_rust_boundary(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
) -> Result<(), String> {
    for (binding, value) in capsule.inputs.iter().zip(args) {
        match (&binding.ty, value) {
            (ty, NativeValue::Scalar(_)) if rust_scalar_type(ty).is_some() => {}
            (Type::Slice { elem, .. }, NativeValue::ReadSlice(_) | NativeValue::MutSlice(_))
                if rust_scalar_type(elem).is_some() => {}
            (Type::Named(name), NativeValue::Struct(_)) => {
                let decl = validate_native_struct(program, name)?;
                validate_rust_struct_source(capsule, decl)?;
            }
            _ => {
                return Err(format!(
                    "native_capsule_boundary_unsupported: Rust live input '{}' must be a scalar, matching scalar slice, or reviewed @repr(C) struct; got {}",
                    binding.name,
                    binding.ty.to_agent()
                ))
            }
        }
    }
    let output = capsule.outputs.first().ok_or_else(|| {
        "native_capsule_boundary_unsupported: Rust capsule requires one output".to_string()
    })?;
    match &output.ty {
        ty if rust_scalar_type(ty).is_some()
            || owned_element(ty).is_some()
            || is_owned_string_type(ty) => {}
        Type::Named(name) => {
            let decl = validate_native_struct(program, name)?;
            validate_rust_struct_source(capsule, decl)?;
        }
        ty => {
            return Err(format!(
                "native_capsule_boundary_unsupported: Rust live output must be a scalar, reviewed owned vector, or reviewed @repr(C) struct; got {}",
                ty.to_agent()
            ));
        }
    }
    Ok(())
}

fn validate_rust_struct_source(capsule: &Capsule, decl: &StructDecl) -> Result<(), String> {
    let name = &decl.name;
    let source = capsule.code.join("\n");
    if !source.contains("#[repr(C)]") {
        return Err(format!(
            "native_capsule_boundary_unsupported: Rust struct '{name}' requires a matching #[repr(C)] declaration in capsule code"
        ));
    }
    if !source.contains(&format!("pub struct {name}")) {
        return Err(format!(
            "native_capsule_boundary_unsupported: Rust struct '{name}' must be public with public scalar fields so the isolated runner can validate it"
        ));
    }
    let declaration = source
        .split_once(&format!("pub struct {name}"))
        .and_then(|(_, tail)| tail.split_once('{'))
        .and_then(|(_, body)| body.split_once('}'))
        .map(|(body, _)| body)
        .ok_or_else(|| {
            format!(
                "native_capsule_boundary_unsupported: Rust struct '{name}' declaration is malformed"
            )
        })?;
    let actual = declaration
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let expected = decl
        .fields
        .iter()
        .map(|field| {
            let ty = rust_scalar_type(&field.ty).expect("validated native struct scalar");
            format!("pub{}:{ty},", field.name)
        })
        .collect::<String>();
    if actual != expected {
        return Err(format!(
            "native_capsule_boundary_unsupported: Rust struct '{name}' fields must exactly match Forgentex declaration order, names, scalar types, and public visibility"
        ));
    }
    Ok(())
}

fn rust_scalar_type(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Bool => Some("bool"),
        Type::Primitive(name) => match name.as_str() {
            "i8" => Some("i8"),
            "i16" => Some("i16"),
            "i32" => Some("i32"),
            "i64" => Some("i64"),
            "isize" => Some("isize"),
            "u8" => Some("u8"),
            "u16" => Some("u16"),
            "u32" => Some("u32"),
            "u64" => Some("u64"),
            "usize" => Some("usize"),
            "f32" => Some("f32"),
            "f64" => Some("f64"),
            _ => None,
        },
        _ => None,
    }
}

fn zig_scalar_type(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Primitive(name) => match name.as_str() {
            "i8" => Some("i8"),
            "i16" => Some("i16"),
            "i32" => Some("i32"),
            "i64" => Some("i64"),
            "isize" => Some("isize"),
            "u8" => Some("u8"),
            "u16" => Some("u16"),
            "u32" => Some("u32"),
            "u64" => Some("u64"),
            "usize" => Some("usize"),
            "f32" => Some("f32"),
            "f64" => Some("f64"),
            _ => None,
        },
        _ => None,
    }
}

fn validate_zig_boundary(
    program: &Program,
    capsule: &Capsule,
    args: &[NativeValue],
) -> Result<(), String> {
    for (binding, value) in capsule.inputs.iter().zip(args) {
        match (&binding.ty, value) {
            (ty, NativeValue::Scalar(_)) if zig_scalar_type(ty).is_some() => {}
            (Type::Slice { elem, .. }, NativeValue::ReadSlice(_) | NativeValue::MutSlice(_))
                if zig_scalar_type(elem).is_some() => {}
            (Type::Named(name), NativeValue::Struct(_)) => {
                let decl = validate_native_struct(program, name)?;
                validate_zig_struct_source(capsule, decl)?;
            }
            _ => {
                return Err(format!(
                    "native_capsule_boundary_unsupported: Zig live input '{}' must be a numeric scalar or matching numeric scalar slice in this slice; got {}",
                    binding.name,
                    binding.ty.to_agent()
                ));
            }
        }
    }
    let output = capsule.outputs.first().ok_or_else(|| {
        "native_capsule_boundary_unsupported: Zig capsule requires one output".to_string()
    })?;
    if let Some(element) = owned_element(&output.ty) {
        validate_zig_owned_source(capsule, element)?;
    } else if is_owned_string_type(&output.ty) {
        validate_zig_owned_string_source(capsule)?;
    } else {
        match &output.ty {
            ty if zig_scalar_type(ty).is_some() => {}
            Type::Named(name) => {
                let decl = validate_native_struct(program, name)?;
                validate_zig_struct_source(capsule, decl)?;
            }
            ty => {
                return Err(format!(
                    "native_capsule_boundary_unsupported: Zig live output must be a numeric scalar, reviewed owned vector, or reviewed @repr(C) struct in this slice; got {}",
                    ty.to_agent()
                ));
            }
        }
    }
    Ok(())
}

fn validate_zig_owned_source(capsule: &Capsule, element: OwnedElement) -> Result<(), String> {
    let compact = capsule
        .code
        .join("\n")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let descriptor = format!(
        "pubconst{name}=externstruct{{ptr:?[*]{ty},len:usize,capacity:usize,}};",
        name = element.descriptor(),
        ty = element.fgx_name()
    );
    if !compact.contains(&descriptor) {
        return Err(format!(
            "native_capsule_boundary_unsupported: Zig Vec[{}] output requires the exact public `{}` extern struct descriptor",
            element.fgx_name(),
            element.descriptor()
        ));
    }
    if !compact.contains(&format!("pubexportfn{}_free(", capsule.name)) {
        return Err(format!(
            "native_capsule_boundary_unsupported: Zig Vec[{}] output requires `pub export fn {}_free`",
            element.fgx_name(), capsule.name
        ));
    }
    Ok(())
}

fn validate_zig_owned_string_source(capsule: &Capsule) -> Result<(), String> {
    let compact = capsule
        .code
        .join("\n")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let descriptor = "pubconstFgxOwnedStr=externstruct{ptr:?[*]u8,len:usize,capacity:usize,};";
    if !compact.contains(descriptor) {
        return Err(
            "native_capsule_boundary_unsupported: Zig str output requires the exact public `FgxOwnedStr` extern struct descriptor"
                .to_string(),
        );
    }
    if !compact.contains(&format!("pubexportfn{}_free(", capsule.name)) {
        return Err(format!(
            "native_capsule_boundary_unsupported: Zig str output requires `pub export fn {}_free`",
            capsule.name
        ));
    }
    Ok(())
}

fn validate_zig_struct_source(capsule: &Capsule, decl: &StructDecl) -> Result<(), String> {
    let name = &decl.name;
    let source = capsule.code.join("\n");
    let marker = format!("pub const {name} = extern struct");
    let declaration = source
        .split_once(&marker)
        .and_then(|(_, tail)| tail.split_once('{'))
        .and_then(|(_, body)| body.split_once('}'))
        .map(|(body, _)| body)
        .ok_or_else(|| {
            format!(
                "native_capsule_boundary_unsupported: Zig struct '{name}' requires a matching public `extern struct` declaration in capsule code"
            )
        })?;
    let actual = declaration
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let expected = decl
        .fields
        .iter()
        .map(|field| {
            let ty = zig_scalar_type(&field.ty).expect("validated native struct scalar");
            format!("{}:{ty},", field.name)
        })
        .collect::<String>();
    if actual != expected {
        return Err(format!(
            "native_capsule_boundary_unsupported: Zig struct '{name}' fields must exactly match Forgentex declaration order, names, and scalar types"
        ));
    }
    Ok(())
}

fn invoke_zig_runner(request: LiveRunner<'_>) -> Result<NativeInvocation, String> {
    let LiveRunner {
        program,
        capsule,
        native_args,
        args,
        toolchain,
        compiler_dir,
        scratch,
        reuse_runner,
        ..
    } = request;
    let capsule_zig = scratch.join("capsule.zig");
    let runner_zig = scratch.join("live_runner.zig");
    let request_file = scratch.join("request.txt");
    let cache_dir = scratch.join("zig-cache");
    let global_cache_dir = scratch.join("zig-global-cache");
    let exe = native_runner_exe(scratch);
    if !reuse_runner {
        write_file(&capsule_zig, &capsule.code.join("\n"))?;
        write_file(&runner_zig, &generate_zig_live_runner(program, capsule)?)?;
    }
    let request = if args.is_empty() {
        String::new()
    } else {
        format!("{}\n", args.join("\n"))
    };
    write_file(&request_file, &request)?;

    let argv = vec![
        tc(toolchain),
        "build-exe".to_string(),
        disp(&runner_zig),
        "-O".to_string(),
        "ReleaseSafe".to_string(),
        format!("-femit-bin={}", exe.display()),
        "--cache-dir".to_string(),
        disp(&cache_dir),
        "--global-cache-dir".to_string(),
        disp(&global_cache_dir),
    ];
    if !reuse_runner {
        let output = run_with_path(&argv, compiler_dir)
            .map_err(|err| format!("native_capsule_compile_failed: failed to launch zig: {err}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("native_capsule_compile_failed: {stderr}"));
        }
    }

    let argv = vec![disp(&exe), disp(&request_file)];
    let output = match native_wall_timeout(capsule) {
        Some(timeout) => match run_with_path_timeout_limited(
            &argv,
            compiler_dir,
            timeout,
            native_process_limits(capsule),
        )
        .map_err(|err| format!("native_capsule_launch_failed: {err}"))?
        {
            Some(output) => output,
            None => {
                return Err(format!(
                    "native_capsule_timeout: capsule '{}' exceeded limit.wall_ms {}",
                    capsule.name,
                    timeout.as_millis()
                ));
            }
        },
        None => {
            run_with_path_contained_limited(&argv, compiler_dir, native_process_limits(capsule))
                .map_err(|err| format!("native_capsule_launch_failed: {err}"))?
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let status = output
            .status
            .code()
            .map(|code| format!("exit code {code}"))
            .unwrap_or_else(|| "terminated by signal".to_string());
        return Err(format!(
            "native_capsule_failed: {}",
            if stderr.is_empty() { status } else { stderr }
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("native_capsule_bad_output: output was not UTF-8: {err}"))?;
    parse_native_invocation(&stdout, program, capsule, native_args)
}

pub(crate) fn generate_zig_live_runner(
    program: &Program,
    capsule: &Capsule,
) -> Result<String, String> {
    let mut parse = String::new();
    let mut call_args = Vec::new();
    let mut mutations = String::new();
    for (index, binding) in capsule.inputs.iter().enumerate() {
        match &binding.ty {
            Type::Slice { mutable, elem } => {
                let zig_ty = zig_scalar_type(elem).ok_or_else(|| {
                    format!(
                        "native_capsule_boundary_unsupported: Zig slice input '{}' has non-scalar elements",
                        binding.name
                    )
                })?;
                parse.push_str(&format!(
                    "    const {name}: []{zig_ty} = try parseSlice({zig_ty}, allocator, lines.next(), {position});\n    defer allocator.free({name});\n",
                    name = binding.name,
                    position = index + 1
                ));
                call_args.push(format!("{}.ptr", binding.name));
                call_args.push(format!("{}.len", binding.name));
                if *mutable {
                    mutations.push_str(&format!(
                        "    try stdout.print(\"mut:{index}=\", .{{}});\n    for ({name}, 0..) |item, item_index| {{\n        if (item_index > 0) try stdout.print(\",\", .{{}});\n        try stdout.print(\"{{}}\", .{{item}});\n    }}\n    try stdout.print(\"\\n\", .{{}});\n",
                        name = binding.name
                    ));
                }
            }
            Type::Named(struct_name) => {
                let decl = validate_native_struct(program, struct_name)?;
                let mut fields = Vec::new();
                for field in &decl.fields {
                    let zig_ty = zig_scalar_type(&field.ty).ok_or_else(|| {
                        format!(
                            "native_capsule_boundary_unsupported: Zig struct field '{}.{}' is not scalar",
                            struct_name, field.name
                        )
                    })?;
                    fields.push(format!(
                        "            .{name} = try parseArg({zig_ty}, tokens.next(), {position})",
                        name = field.name,
                        position = index + 1
                    ));
                }
                parse.push_str(&format!(
                    "    const {name}: capsule.{struct_name} = value: {{\n        const text = lines.next() orelse return error.StructArgumentMissing;\n        var tokens = std.mem.splitScalar(u8, text, ',');\n        const result = capsule.{struct_name} {{\n{fields}\n        }};\n        if (tokens.next() != null) return error.InvalidStructArgument;\n        break :value result;\n    }};\n",
                    name = binding.name,
                    fields = fields.join(",\n")
                ));
                call_args.push(binding.name.clone());
            }
            ty => {
                let zig_ty = zig_scalar_type(ty).ok_or_else(|| {
                    format!(
                        "native_capsule_boundary_unsupported: Zig input '{}' is not a numeric scalar",
                        binding.name
                    )
                })?;
                parse.push_str(&format!(
                    "    const {name}: {zig_ty} = try parseArg({zig_ty}, lines.next(), {position});\n",
                    name = binding.name,
                    position = index + 1
                ));
                call_args.push(binding.name.clone());
            }
        }
    }
    let output = capsule.outputs.first().ok_or_else(|| {
        "native_capsule_boundary_unsupported: Zig capsule has no output".to_string()
    })?;
    let (output_ty, result_print) = if let Some(element) = owned_element(&output.ty) {
        let max_elements = native_owned_output_limit(capsule) / element.bytes();
        let print = format!(
            "    if ((result.ptr == null and result.len != 0) or result.capacity < result.len or result.len > {max_elements}) {{\n        capsule.{name}_free(result.ptr, result.len, result.capacity);\n        return error.InvalidOwnedBufferDescriptor;\n    }}\n    defer capsule.{name}_free(result.ptr, result.len, result.capacity);\n    const owned_copy = try allocator.alloc({element_ty}, result.len);\n    defer allocator.free(owned_copy);\n    if (result.len > 0) @memcpy(owned_copy, result.ptr.?[0..result.len]);\n    try stdout.print(\"result=\", .{{}});\n    for (owned_copy, 0..) |item, item_index| {{\n        if (item_index > 0) try stdout.print(\",\", .{{}});\n        try stdout.print(\"{{}}\", .{{item}});\n    }}\n    try stdout.print(\"\\n\", .{{}});\n",
            name = capsule.name,
            element_ty = element.fgx_name()
        );
        (format!("capsule.{}", element.descriptor()), print)
    } else if is_owned_string_type(&output.ty) {
        let print = format!(
            "    if ((result.ptr == null and result.len != 0) or result.capacity < result.len or result.len > {MAX_NATIVE_OWNED_OUTPUT_BYTES}) {{\n        capsule.{name}_free(result.ptr, result.len, result.capacity);\n        return error.InvalidOwnedStringDescriptor;\n    }}\n    defer capsule.{name}_free(result.ptr, result.len, result.capacity);\n    const owned_copy = try allocator.alloc(u8, result.len);\n    defer allocator.free(owned_copy);\n    if (result.len > 0) @memcpy(owned_copy, result.ptr.?[0..result.len]);\n    try stdout.print(\"result=\", .{{}});\n    for (owned_copy, 0..) |byte, byte_index| {{\n        if (byte_index > 0) try stdout.print(\",\", .{{}});\n        try stdout.print(\"{{}}\", .{{byte}});\n    }}\n    try stdout.print(\"\\n\", .{{}});\n",
            name = capsule.name
        );
        ("capsule.FgxOwnedStr".to_string(), print)
    } else if let Type::Named(name) = &output.ty {
        let decl = validate_native_struct(program, name)?;
        let mut prints = String::from("    try stdout.print(\"result=\", .{});\n");
        for (index, field) in decl.fields.iter().enumerate() {
            if index > 0 {
                prints.push_str("    try stdout.print(\",\", .{});\n");
            }
            prints.push_str(&format!(
                "    try stdout.print(\"{{}}\", .{{result.{}}});\n",
                field.name
            ));
        }
        prints.push_str("    try stdout.print(\"\\n\", .{});\n");
        (format!("capsule.{name}"), prints)
    } else {
        let ty = zig_scalar_type(&output.ty).ok_or_else(|| {
            "native_capsule_boundary_unsupported: Zig output is not a numeric scalar, reviewed owned vector, or reviewed struct"
                .to_string()
        })?;
        (
            ty.to_string(),
            "    try stdout.print(\"result={}\\n\", .{result});\n".to_string(),
        )
    };
    let count_check = if capsule.inputs.is_empty() {
        "    if (wire.len != 0) return error.ArgumentCountMismatch;\n"
    } else {
        "    if (lines.next() != null) return error.ArgumentCountMismatch;\n"
    };
    Ok(format!(
        "const std = @import(\"std\");\nconst capsule = @import(\"capsule.zig\");\n\nfn parseArg(comptime T: type, value: ?[]const u8, position: usize) !T {{\n    _ = position;\n    const text = value orelse return error.ArgumentMissing;\n    return switch (@typeInfo(T)) {{\n        .int => std.fmt.parseInt(T, text, 10),\n        .float => std.fmt.parseFloat(T, text),\n        else => error.UnsupportedArgumentType,\n    }};\n}}\n\nfn parseSlice(comptime T: type, allocator: std.mem.Allocator, value: ?[]const u8, position: usize) ![]T {{\n    const text = value orelse return error.ArgumentMissing;\n    if (text.len == 0) return allocator.alloc(T, 0);\n    const count = std.mem.count(u8, text, \",\") + 1;\n    if (count > {MAX_NATIVE_SLICE_ELEMENTS}) return error.SliceTooLarge;\n    const result = try allocator.alloc(T, count);\n    errdefer allocator.free(result);\n    var tokens = std.mem.splitScalar(u8, text, ',');\n    for (result) |*item| item.* = try parseArg(T, tokens.next(), position);\n    return result;\n}}\n\npub fn main() !void {{\n    const allocator = std.heap.page_allocator;\n    const argv = try std.process.argsAlloc(allocator);\n    defer std.process.argsFree(allocator, argv);\n    if (argv.len != 2) return error.RequestPathMissing;\n    const request = try std.fs.cwd().readFileAlloc(allocator, argv[1], 64 * 1024 * 1024);\n    defer allocator.free(request);\n    const wire = std.mem.trimRight(u8, request, \"\\r\\n\");\n    var lines = std.mem.splitScalar(u8, wire, '\\n');\n{parse}{count_check}    const result: {output_ty} = capsule.{name}({call_args});\n    const stdout = std.io.getStdOut().writer();\n{result_print}{mutations}}}\n",
        name = capsule.name,
        call_args = call_args.join(", ")
    ))
}

fn invoke_rust_runner(request: LiveRunner<'_>) -> Result<NativeInvocation, String> {
    let LiveRunner {
        program,
        capsule,
        native_args,
        args,
        standard,
        toolchain,
        compiler_dir,
        scratch,
        reuse_runner,
        ..
    } = request;
    let runner_rs = scratch.join("live_runner.rs");
    let request_file = scratch.join("request.txt");
    let exe = native_runner_exe(scratch);
    if !reuse_runner {
        write_file(&runner_rs, &generate_rust_live_runner(program, capsule)?)?;
    }
    let request = if args.is_empty() {
        String::new()
    } else {
        format!("{}\n", args.join("\n"))
    };
    write_file(&request_file, &request)?;

    let argv = vec![
        tc(toolchain),
        standard.to_string(),
        "-O".to_string(),
        disp(&runner_rs),
        "-o".to_string(),
        disp(&exe),
    ];
    if !reuse_runner {
        let output = run_with_path(&argv, compiler_dir).map_err(|err| {
            format!("native_capsule_compile_failed: failed to launch rustc: {err}")
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("native_capsule_compile_failed: {stderr}"));
        }
    }

    let argv = vec![disp(&exe), disp(&request_file)];
    let output = match native_wall_timeout(capsule) {
        Some(timeout) => match run_with_path_timeout_limited(
            &argv,
            compiler_dir,
            timeout,
            native_process_limits(capsule),
        )
        .map_err(|err| format!("native_capsule_launch_failed: {err}"))?
        {
            Some(output) => output,
            None => {
                return Err(format!(
                    "native_capsule_timeout: capsule '{}' exceeded limit.wall_ms {}",
                    capsule.name,
                    timeout.as_millis()
                ))
            }
        },
        None => {
            run_with_path_contained_limited(&argv, compiler_dir, native_process_limits(capsule))
                .map_err(|err| format!("native_capsule_launch_failed: {err}"))?
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let status = output
            .status
            .code()
            .map(|code| format!("exit code {code}"))
            .unwrap_or_else(|| "terminated by signal".to_string());
        return Err(format!(
            "native_capsule_failed: {}",
            if stderr.is_empty() { status } else { stderr }
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("native_capsule_bad_output: output was not UTF-8: {err}"))?;
    parse_native_invocation(&stdout, program, capsule, native_args)
}

fn generate_rust_live_runner(program: &Program, capsule: &Capsule) -> Result<String, String> {
    let mut parse = String::new();
    let mut call_args = Vec::new();
    let mut mutations = String::new();
    for (index, binding) in capsule.inputs.iter().enumerate() {
        match &binding.ty {
            Type::Slice { mutable, elem } => {
                let rust_ty = rust_scalar_type(elem).ok_or_else(|| {
                    format!(
                        "native_capsule_boundary_unsupported: Rust slice input '{}' has non-scalar elements",
                        binding.name
                    )
                })?;
                parse.push_str(&format!(
                    "    let mut {name}: Vec<{rust_ty}> = parse_slice(lines.next(), {position})?;\n",
                    name = binding.name,
                    position = index + 1
                ));
                call_args.push(if *mutable {
                    format!("{}.as_mut_ptr()", binding.name)
                } else {
                    format!("{}.as_ptr()", binding.name)
                });
                call_args.push(format!("{}.len()", binding.name));
                if *mutable {
                    mutations.push_str(&format!(
                        "    print!(\"mut:{index}=\");\n    for (item_index, item) in {name}.iter().enumerate() {{\n        if item_index > 0 {{ print!(\",\"); }}\n        print!(\"{{}}\", item);\n    }}\n    println!();\n",
                        name = binding.name
                    ));
                }
            }
            Type::Named(struct_name) => {
                let decl = validate_native_struct(program, struct_name)?;
                let mut fields = Vec::new();
                for field in &decl.fields {
                    rust_scalar_type(&field.ty).ok_or_else(|| {
                        format!(
                            "native_capsule_boundary_unsupported: Rust struct field '{}.{}' is not scalar",
                            struct_name, field.name
                        )
                    })?;
                    fields.push(format!(
                        "        {name}: parse_arg(tokens.next(), {position})?",
                        name = field.name,
                        position = index + 1
                    ));
                }
                parse.push_str(&format!(
                    "    let {name}: capsule::{struct_name} = {{\n        let wire = lines.next().ok_or_else(|| \"native capsule struct argument {position} missing\".to_string())?;\n        let mut tokens = wire.split(',');\n        let value = capsule::{struct_name} {{\n{fields}\n        }};\n        if tokens.next().is_some() {{ return Err(\"native capsule invalid struct argument {position}\".to_string()); }}\n        value\n    }};\n",
                    name = binding.name,
                    position = index + 1,
                    fields = fields.join(",\n")
                ));
                call_args.push(binding.name.clone());
            }
            ty => {
                let rust_ty = rust_scalar_type(ty).ok_or_else(|| {
                    format!(
                        "native_capsule_boundary_unsupported: Rust input '{}' is not scalar",
                        binding.name
                    )
                })?;
                parse.push_str(&format!(
                    "    let {name}: {rust_ty} = parse_arg(lines.next(), {position})?;\n",
                    name = binding.name,
                    position = index + 1
                ));
                call_args.push(binding.name.clone());
            }
        }
    }
    let output = capsule.outputs.first().ok_or_else(|| {
        "native_capsule_boundary_unsupported: Rust capsule has no output".to_string()
    })?;
    let (output_ty, result_output) = if let Some(element) = owned_element(&output.ty) {
        let max_elements = native_owned_output_limit(capsule) / element.bytes();
        let descriptor = element.descriptor();
        let rust_ty = element.fgx_name();
        (
            format!("capsule::{descriptor}"),
            format!(
                "    if (result.ptr.is_null() && result.len != 0) || result.capacity < result.len || result.len > {max_elements} {{\n        unsafe {{ capsule::{name}_free(result.ptr, result.len, result.capacity); }}\n        return Err(\"native capsule invalid owned buffer descriptor\".to_string());\n    }}\n    let owned_copy: Vec<{rust_ty}> = if result.len == 0 {{ Vec::new() }} else {{ unsafe {{ std::slice::from_raw_parts(result.ptr, result.len) }}.to_vec() }};\n    unsafe {{ capsule::{name}_free(result.ptr, result.len, result.capacity); }}\n    print!(\"result=\");\n    for (index, item) in owned_copy.iter().enumerate() {{\n        if index > 0 {{ print!(\",\"); }}\n        print!(\"{{}}\", item);\n    }}\n    println!();\n",
                name = capsule.name
            ),
        )
    } else if is_owned_string_type(&output.ty) {
        (
            "capsule::FgxOwnedStr".to_string(),
            format!(
                "    if (result.ptr.is_null() && result.len != 0) || result.capacity < result.len || result.len > {MAX_NATIVE_OWNED_OUTPUT_BYTES} {{\n        unsafe {{ capsule::{name}_free(result.ptr, result.len, result.capacity); }}\n        return Err(\"native capsule invalid owned string descriptor\".to_string());\n    }}\n    let owned_copy: Vec<u8> = if result.len == 0 {{ Vec::new() }} else {{ unsafe {{ std::slice::from_raw_parts(result.ptr, result.len) }}.to_vec() }};\n    unsafe {{ capsule::{name}_free(result.ptr, result.len, result.capacity); }}\n    print!(\"result=\");\n    for (index, byte) in owned_copy.iter().enumerate() {{\n        if index > 0 {{ print!(\",\"); }}\n        print!(\"{{}}\", byte);\n    }}\n    println!();\n",
                name = capsule.name
            ),
        )
    } else if let Type::Named(name) = &output.ty {
        let decl = validate_native_struct(program, name)?;
        let mut prints = String::new();
        for (index, field) in decl.fields.iter().enumerate() {
            if index > 0 {
                prints.push_str("    print!(\",\");\n");
            }
            prints.push_str(&format!("    print!(\"{{}}\", result.{});\n", field.name));
        }
        (
            format!("capsule::{name}"),
            format!("    print!(\"result=\");\n{prints}    println!();\n"),
        )
    } else {
        let ty = rust_scalar_type(&output.ty).ok_or_else(|| {
            "native_capsule_boundary_unsupported: Rust output is not scalar or a reviewed owned vector".to_string()
        })?;
        (
            ty.to_string(),
            "    println!(\"result={}\", result);\n".to_string(),
        )
    };

    let mut runner = format!(
        "#![allow(unused_unsafe, unused_mut)]\nmod capsule {{\n{}\n}}\n\n",
        capsule.code.join("\n")
    );
    runner.push_str("fn parse_arg<T: std::str::FromStr>(value: Option<&str>, position: usize) -> Result<T, String> {\n    value.ok_or_else(|| format!(\"native capsule argument {position} missing\"))?.parse::<T>().map_err(|_| format!(\"native capsule invalid argument {position}\"))\n}\n\n");
    runner.push_str(&format!(
        "fn parse_slice<T: std::str::FromStr>(value: Option<&str>, position: usize) -> Result<Vec<T>, String> {{\n    let wire = value.ok_or_else(|| format!(\"native capsule argument {{position}} missing\"))?;\n    if wire.is_empty() {{ return Ok(Vec::new()); }}\n    let values = wire.split(',').map(|item| item.parse::<T>().map_err(|_| format!(\"native capsule invalid slice element for argument {{position}}\"))).collect::<Result<Vec<_>, _>>()?;\n    if values.len() > {MAX_NATIVE_SLICE_ELEMENTS} {{ return Err(\"native capsule slice exceeds element limit\".to_string()); }}\n    Ok(values)\n}}\n\n"
    ));
    runner.push_str("fn run() -> Result<(), String> {\n    let request_path = std::env::args().nth(1).ok_or_else(|| \"native capsule request path missing\".to_string())?;\n    let request = std::fs::read_to_string(request_path).map_err(|_| \"native capsule request unavailable\".to_string())?;\n    let mut lines = request.lines();\n");
    runner.push_str(&parse);
    runner.push_str("    if lines.next().is_some() { return Err(\"native capsule argument count mismatch\".to_string()); }\n");
    runner.push_str(&format!(
        "    let result: {output_ty} = unsafe {{ capsule::{}({}) }};\n",
        capsule.name,
        call_args.join(", ")
    ));
    runner.push_str(&result_output);
    runner.push_str(&mutations);
    runner.push_str("    Ok(())\n}\n\nfn main() {\n    if let Err(error) = run() {\n        eprintln!(\"{error}\");\n        std::process::exit(2);\n    }\n}\n");
    Ok(runner)
}

pub(crate) fn parse_native_invocation(
    stdout: &str,
    program: &Program,
    capsule: &Capsule,
    native_args: &[NativeValue],
) -> Result<NativeInvocation, String> {
    let mut lines = stdout.lines();
    let result = lines
        .next()
        .and_then(|line| line.strip_prefix("result="))
        .ok_or_else(|| "native_capsule_bad_output: missing result envelope".to_string())?;
    let output = capsule
        .outputs
        .first()
        .ok_or_else(|| "native_capsule_bad_output: capsule has no declared output".to_string())?;
    let owned_element = owned_element(&output.ty);
    let owned_string = is_owned_string_type(&output.ty);
    if result.is_empty() && owned_element.is_none() && !owned_string {
        return Err("native_capsule_bad_output: native runner returned no value".to_string());
    }
    // Validate the result envelope before exposing any mutable copy-back to
    // the interpreter. This rejects NaN/infinity, range violations, and type
    // mismatches atomically with the rest of the child response.
    let owned_values = if let Some(element) = owned_element {
        Some(parse_owned_wire(result, element)?)
    } else {
        None
    };
    let owned_string_bytes = owned_string
        .then(|| parse_owned_bytes_wire(result))
        .transpose()?;
    let owned_output_bytes = owned_values
        .as_ref()
        .zip(owned_element)
        .map(|(values, element)| values.len() * element.bytes())
        .or_else(|| owned_string_bytes.as_ref().map(Vec::len));
    let owned_output = (owned_element == Some(OwnedElement::U8))
        .then_some(owned_values.as_ref())
        .flatten()
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|value| match value {
                    CapsuleValue::Int(value) => Ok(*value as u8),
                    _ => {
                        Err("native_capsule_bad_output: owned byte was not an integer".to_string())
                    }
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let artifact = match (crate::capsule::artifact_policy(capsule)?, &owned_output) {
        (Some(policy), Some(bytes)) => {
            if bytes.len() > policy.max_bytes {
                return Err(format!(
                    "native_capsule_artifact_too_large: artifact has {} bytes; declared limit is {}",
                    bytes.len(), policy.max_bytes
                ));
            }
            Some(NativeArtifactReceipt {
                schema: "forgentex.native_artifact.v1".to_string(),
                media_type: policy.media_type,
                bytes: bytes.len(),
                sha256: crate::crypto::sha256_hex(bytes),
                path: None,
            })
        }
        _ => None,
    };
    let output = parse_native_result(program, result, &output.ty)?;
    let mut mutations = Vec::new();
    for (index, binding) in capsule.inputs.iter().enumerate() {
        let Type::Slice {
            mutable: true,
            elem,
        } = &binding.ty
        else {
            continue;
        };
        let prefix = format!("mut:{index}=");
        let wire = lines
            .next()
            .and_then(|line| line.strip_prefix(&prefix))
            .ok_or_else(|| {
                format!(
                    "native_capsule_bad_output: missing mutable slice envelope for '{}'",
                    binding.name
                )
            })?;
        let values = parse_scalar_slice_wire(wire, elem)?;
        let expected_len = match native_args.get(index) {
            Some(NativeValue::MutSlice(values)) => values.len(),
            _ => {
                return Err(format!(
                    "native_capsule_bad_output: missing mutable input state for '{}'",
                    binding.name
                ))
            }
        };
        if values.len() != expected_len {
            return Err(format!(
                "native_capsule_bad_output: mutable slice '{}' changed length from {expected_len} to {}",
                binding.name,
                values.len()
            ));
        }
        mutations.push(NativeMutation {
            input_index: index,
            values,
        });
    }
    if lines.any(|line| !line.trim().is_empty()) {
        return Err("native_capsule_bad_output: unexpected extra output".to_string());
    }
    Ok(NativeInvocation {
        output,
        mutations,
        owned_output_bytes,
        artifact,
        owned_output,
        receipt: None,
    })
}

fn parse_native_result(program: &Program, wire: &str, ty: &Type) -> Result<String, String> {
    if let Some(element) = owned_element(ty) {
        let values = parse_owned_wire(wire, element)?;
        return Ok(format!(
            "[{}]",
            values
                .iter()
                .map(CapsuleValue::to_literal)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if is_owned_string_type(ty) {
        let bytes = parse_owned_bytes_wire(wire)?;
        let value = String::from_utf8(bytes).map_err(|_| {
            "native_capsule_bad_output: owned string is not valid UTF-8".to_string()
        })?;
        return Ok(json::string(&value));
    }
    match ty {
        Type::Named(name) => {
            let decl = validate_native_struct(program, name)?;
            let values = wire
                .split(',')
                .map(|token| token.trim())
                .collect::<Vec<_>>();
            if values.len() != decl.fields.len() || values.iter().any(|value| value.is_empty()) {
                return Err(format!(
                    "native_capsule_bad_output: struct '{}' has {} field value(s); expected {}",
                    name,
                    values.len(),
                    decl.fields.len()
                ));
            }
            let fields = values
                .iter()
                .zip(&decl.fields)
                .map(|(value, field)| {
                    parse_wire_scalar(value, &field.ty).map(|parsed| {
                        format!("{}:{}", json::string(&field.name), parsed.to_literal())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{{{}}}", fields.join(",")))
        }
        _ => parse_wire_scalar(wire, ty).map(|value| value.to_literal()),
    }
}

fn parse_owned_wire(wire: &str, element: OwnedElement) -> Result<Vec<CapsuleValue>, String> {
    if wire.is_empty() {
        return Ok(Vec::new());
    }
    let values = wire
        .split(',')
        .map(|token| parse_wire_scalar(token, &element.ty()))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = values.len().saturating_mul(element.bytes());
    if bytes > MAX_NATIVE_OWNED_OUTPUT_BYTES {
        return Err(format!(
            "native_capsule_bad_output: owned buffer has {bytes} bytes; limit is {MAX_NATIVE_OWNED_OUTPUT_BYTES}"
        ));
    }
    Ok(values)
}

fn parse_owned_bytes_wire(wire: &str) -> Result<Vec<u8>, String> {
    parse_owned_wire(wire, OwnedElement::U8)?
        .into_iter()
        .map(|value| match value {
            CapsuleValue::Int(value) => Ok(value as u8),
            _ => Err("native_capsule_bad_output: owned byte was not an integer".to_string()),
        })
        .collect()
}

fn fnv1a(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn parse_scalar_slice_wire(wire: &str, elem: &Type) -> Result<Vec<CapsuleValue>, String> {
    if wire.is_empty() {
        return Ok(Vec::new());
    }
    let values = wire
        .split(',')
        .map(|token| parse_wire_scalar(token, elem))
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() > MAX_NATIVE_SLICE_ELEMENTS {
        return Err(format!(
            "native_capsule_bad_output: slice has {} elements; limit is {MAX_NATIVE_SLICE_ELEMENTS}",
            values.len()
        ));
    }
    Ok(values)
}

fn parse_wire_scalar(token: &str, ty: &Type) -> Result<CapsuleValue, String> {
    let value = match ty {
        Type::Bool => match token {
            "true" => CapsuleValue::Bool(true),
            "false" => CapsuleValue::Bool(false),
            _ => {
                return Err(format!(
                    "native_capsule_bad_output: '{token}' is not a bool slice element"
                ))
            }
        },
        Type::Primitive(name) if matches!(name.as_str(), "f32" | "f64") => {
            let value = token.parse::<f64>().map_err(|_| {
                format!("native_capsule_bad_output: '{token}' is not a float slice element")
            })?;
            if !value.is_finite() {
                return Err("native_capsule_bad_output: non-finite float slice element".to_string());
            }
            CapsuleValue::Float(value)
        }
        Type::Primitive(name) if crate::ast::is_integer_primitive(name) => {
            let value = token.parse::<i64>().map_err(|_| {
                format!("native_capsule_bad_output: '{token}' is not an integer slice element")
            })?;
            CapsuleValue::Int(value)
        }
        _ => {
            return Err(format!(
                "native_capsule_bad_output: unsupported slice element type {}",
                ty.to_agent()
            ))
        }
    };
    scalar_arg(&value, ty)?;
    Ok(value)
}

fn validate_native_arg(
    program: &Program,
    value: &NativeValue,
    ty: &Type,
    name: &str,
) -> Result<(), String> {
    match (value, ty) {
        (NativeValue::Scalar(_), Type::Slice { .. }) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects {}, got scalar",
            ty.to_agent()
        )),
        (
            NativeValue::ReadSlice(values),
            Type::Slice {
                mutable: false,
                elem,
            },
        ) => validate_scalar_slice(values, elem, name),
        (
            NativeValue::MutSlice(values),
            Type::Slice {
                mutable: true,
                elem,
            },
        ) => validate_scalar_slice(values, elem, name),
        (NativeValue::ReadSlice(_), Type::Slice { mutable: true, .. }) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects a mutable slice"
        )),
        (NativeValue::MutSlice(_), Type::Slice { mutable: false, .. }) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects a read-only slice"
        )),
        (NativeValue::ReadSlice(_), _) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects {}, got read-only slice",
            ty.to_agent()
        )),
        (NativeValue::MutSlice(_), _) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects {}, got mutable slice",
            ty.to_agent()
        )),
        (NativeValue::Struct(values), Type::Named(struct_name)) => {
            let decl = validate_native_struct(program, struct_name)?;
            if values.len() != decl.fields.len() {
                return Err(format!(
                    "native_capsule_argument_type: struct input '{name}' expects {} field(s), got {}",
                    decl.fields.len(),
                    values.len()
                ));
            }
            for (value, field) in values.iter().zip(&decl.fields) {
                scalar_arg(value, &field.ty)?;
            }
            Ok(())
        }
        (NativeValue::Struct(_), _) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects {}, got struct",
            ty.to_agent()
        )),
        (NativeValue::Scalar(value), _) if value.matches_type(ty) => {
            scalar_arg(value, ty).map(|_| ())
        }
        (NativeValue::Scalar(value), _) => Err(format!(
            "native_capsule_argument_type: input '{name}' expects {}, got {}",
            ty.to_agent(),
            value.type_name()
        )),
    }
}

fn validate_scalar_slice(values: &[CapsuleValue], elem: &Type, name: &str) -> Result<(), String> {
    if values.len() > MAX_NATIVE_SLICE_ELEMENTS {
        return Err(format!(
            "native_capsule_slice_too_large: input '{name}' has {} elements; limit is {MAX_NATIVE_SLICE_ELEMENTS}",
            values.len()
        ));
    }
    for value in values {
        if !value.matches_type(elem) {
            return Err(format!(
                "native_capsule_argument_type: slice input '{name}' expects {}, got {}",
                elem.to_agent(),
                value.type_name()
            ));
        }
        scalar_arg(value, elem)?;
    }
    Ok(())
}

const MAX_NATIVE_SLICE_ELEMENTS: usize = 1_000_000;

fn native_arg_wire(program: &Program, value: &NativeValue, ty: &Type) -> Result<String, String> {
    match (value, ty) {
        (NativeValue::Scalar(value), _) => scalar_arg(value, ty),
        (
            NativeValue::ReadSlice(values),
            Type::Slice {
                mutable: false,
                elem,
            },
        )
        | (
            NativeValue::MutSlice(values),
            Type::Slice {
                mutable: true,
                elem,
            },
        ) => values
            .iter()
            .map(|value| scalar_arg(value, elem))
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.join(",")),
        (NativeValue::Struct(values), Type::Named(struct_name)) => {
            let decl = validate_native_struct(program, struct_name)?;
            values
                .iter()
                .zip(&decl.fields)
                .map(|(value, field)| scalar_arg(value, &field.ty))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| values.join(","))
        }
        _ => Err("native_capsule_argument_type: invalid native argument shape".to_string()),
    }
}

fn scalar_arg(value: &CapsuleValue, ty: &Type) -> Result<String, String> {
    if !value.matches_type(ty) {
        return Err(format!(
            "native_capsule_argument_type: expected {}, got {}",
            ty.to_agent(),
            value.type_name()
        ));
    }
    match value {
        CapsuleValue::Int(value) => {
            if let Some((min, max)) = integer_range(ty) {
                if *value < min || *value > max {
                    return Err(format!(
                        "native_capsule_argument_range: {} is outside {min}..{max} for {}",
                        value,
                        ty.to_agent()
                    ));
                }
            }
            Ok(value.to_string())
        }
        CapsuleValue::Float(value) if value.is_finite() => {
            if matches!(ty, Type::Primitive(name) if name == "f32") && value.abs() > f32::MAX as f64
            {
                return Err(format!(
                    "native_capsule_argument_range: {} is outside the f32 range",
                    value
                ));
            }
            Ok(float_repr(*value))
        }
        CapsuleValue::Float(_) => {
            Err("native_capsule_argument_invalid: non-finite float".to_string())
        }
        CapsuleValue::Bool(value) => Ok(value.to_string()),
        CapsuleValue::Str(_) => Err(format!(
            "native_capsule_boundary_unsupported: {} is not a scalar C ABI type",
            ty.to_agent()
        )),
    }
}

/// The range representable by the interpreter's signed `i64` integer carrier
/// at a concrete scalar boundary. `u64` is limited to `i64::MAX` for now
/// because `Value::Int` does not yet carry an unsigned 64-bit value.
fn integer_range(ty: &Type) -> Option<(i64, i64)> {
    let pointer_bits = std::mem::size_of::<usize>() * 8;
    match ty {
        Type::Primitive(name) => match name.as_str() {
            "i8" => Some((i8::MIN as i64, i8::MAX as i64)),
            "i16" => Some((i16::MIN as i64, i16::MAX as i64)),
            "i32" => Some((i32::MIN as i64, i32::MAX as i64)),
            "i64" => Some((i64::MIN, i64::MAX)),
            "isize" if pointer_bits == 32 => Some((i32::MIN as i64, i32::MAX as i64)),
            "isize" => Some((i64::MIN, i64::MAX)),
            "u8" => Some((0, u8::MAX as i64)),
            "u16" => Some((0, u16::MAX as i64)),
            "u32" => Some((0, u32::MAX as i64)),
            "u64" => Some((0, i64::MAX)),
            "usize" if pointer_bits == 32 => Some((0, u32::MAX as i64)),
            "usize" => Some((0, i64::MAX)),
            _ => None,
        },
        _ => None,
    }
}

fn generate_live_runner(
    program: &Program,
    capsule: &Capsule,
    output: &NativeOutput,
) -> Result<String, String> {
    let mut parse = String::new();
    let mut cleanup = String::new();
    for (index, binding) in capsule.inputs.iter().enumerate() {
        let argv_index = index + 1;
        parse.push_str(&format!(
            "    char *arg_{argv_index} = read_line(request);\n    if (arg_{argv_index} == NULL) {{\n        fputs(\"native capsule argument count mismatch\\n\", stderr);\n        return 2;\n    }}\n"
        ));
        cleanup.push_str(&format!("    free(arg_{argv_index});\n"));
        let (setup, release) = parse_live_arg(program, &binding.name, &binding.ty, argv_index)?;
        parse.push_str(&setup);
        cleanup.push_str(&release);
    }
    let call_args = capsule
        .inputs
        .iter()
        .flat_map(c_call_arg_names)
        .collect::<Vec<_>>()
        .join(", ");
    let call = format!("{}({call_args})", capsule.name);
    let result_print = match output {
        NativeOutput::Scalar(kind) => print_scalar_expr(&call, *kind),
        NativeOutput::Struct(name) => print_struct_result(program, name, &call)?,
        NativeOutput::Owned(element) => print_owned_result(capsule, &call, *element),
        NativeOutput::OwnedString => {
            print_owned_result_with_descriptor(capsule, &call, OwnedElement::U8, "FgxOwnedStr")
        }
    };
    let mutations = generate_mutation_output(capsule)?;
    Ok(format!(
        "#include <errno.h>\n#include <stdint.h>\n#include <stddef.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n#include \"boundary.h\"\n\nstatic char *read_line(FILE *input) {{\n    size_t length = 0;\n    size_t capacity = 128;\n    char *line = (char *)malloc(capacity);\n    if (line == NULL) return NULL;\n    int ch = 0;\n    while ((ch = fgetc(input)) != EOF && ch != '\\n') {{\n        if (length + 1 >= capacity) {{\n            if (capacity > SIZE_MAX / 2) {{ free(line); return NULL; }}\n            capacity *= 2;\n            char *grown = (char *)realloc(line, capacity);\n            if (grown == NULL) {{ free(line); return NULL; }}\n            line = grown;\n        }}\n        line[length++] = (char)ch;\n    }}\n    if (ch == EOF && length == 0) {{ free(line); return NULL; }}\n    line[length] = '\\0';\n    return line;\n}}\n\nint main(int argc, char **argv) {{\n    if (argc != 2) {{\n        fputs(\"native capsule request path missing\\n\", stderr);\n        return 2;\n    }}\n    FILE *request = fopen(argv[1], \"rb\");\n    if (request == NULL) {{\n        fputs(\"native capsule request unavailable\\n\", stderr);\n        return 2;\n    }}\n{parse}    if (fgetc(request) != EOF) {{\n        fputs(\"native capsule argument count mismatch\\n\", stderr);\n        return 2;\n    }}\n    fclose(request);\n    fputs(\"result=\", stdout);\n{result_print}    fputc('\\n', stdout);\n{mutations}{cleanup}    return 0;\n}}\n"
    ))
}

fn print_owned_result(capsule: &Capsule, call: &str, element: OwnedElement) -> String {
    print_owned_result_with_descriptor(capsule, call, element, element.descriptor())
}

fn print_owned_result_with_descriptor(
    capsule: &Capsule,
    call: &str,
    element: OwnedElement,
    descriptor: &str,
) -> String {
    let max_elements = native_owned_output_limit(capsule) / element.bytes();
    format!(
        "    {descriptor} native_result = {call};\n    if ((native_result.ptr == NULL && native_result.len != 0) || native_result.capacity < native_result.len || native_result.len > {max_elements}) {{\n        {name}_free(native_result.ptr, native_result.len, native_result.capacity);\n        fputs(\"native capsule invalid owned buffer descriptor\\n\", stderr);\n        return 5;\n    }}\n    {c_type} *owned_copy = NULL;\n    if (native_result.len > 0) {{\n        owned_copy = ({c_type} *)malloc(native_result.len * sizeof({c_type}));\n        if (owned_copy == NULL) {{\n            {name}_free(native_result.ptr, native_result.len, native_result.capacity);\n            fputs(\"native capsule owned buffer copy failed\\n\", stderr);\n            return 4;\n        }}\n        memcpy(owned_copy, native_result.ptr, native_result.len * sizeof({c_type}));\n    }}\n    {name}_free(native_result.ptr, native_result.len, native_result.capacity);\n    for (size_t owned_index = 0; owned_index < native_result.len; ++owned_index) {{\n        if (owned_index > 0) {{ fputc(',', stdout); }}\n{print}    }}\n    free(owned_copy);\n",
        name = capsule.name,
        descriptor = descriptor,
        c_type = element.c_type(),
        print = print_scalar_expr("owned_copy[owned_index]", out_kind(&element.ty()).unwrap())
    )
}

fn print_struct_result(program: &Program, name: &str, call: &str) -> Result<String, String> {
    let decl = validate_native_struct(program, name)?;
    let mut out = format!("    {name} native_result = {call};\n");
    for (index, field) in decl.fields.iter().enumerate() {
        if index > 0 {
            out.push_str("    fputc(',', stdout);\n");
        }
        let kind = out_kind(&field.ty).ok_or_else(|| {
            format!(
                "native_capsule_boundary_unsupported: struct output field '{}.{}' is not scalar",
                name, field.name
            )
        })?;
        out.push_str(&print_scalar_expr(
            &format!("native_result.{}", field.name),
            kind,
        ));
    }
    Ok(out)
}

fn print_scalar_expr(expression: &str, kind: OutKind) -> String {
    match kind {
        OutKind::SignedInt => format!("    printf(\"%lld\", (long long)({expression}));\n"),
        OutKind::UnsignedInt => {
            format!("    printf(\"%llu\", (unsigned long long)({expression}));\n")
        }
        OutKind::Float => format!("    printf(\"%.17g\", (double)({expression}));\n"),
        OutKind::Bool => format!("    fputs(({expression}) ? \"true\" : \"false\", stdout);\n"),
    }
}

fn generate_mutation_output(capsule: &Capsule) -> Result<String, String> {
    let mut out = String::new();
    for (index, binding) in capsule.inputs.iter().enumerate() {
        let Type::Slice {
            mutable: true,
            elem,
        } = &binding.ty
        else {
            continue;
        };
        let kind = out_kind(elem).ok_or_else(|| {
            format!(
                "native_capsule_boundary_unsupported: mutable slice '{}' element type '{}' is not scalar",
                binding.name,
                elem.to_agent()
            )
        })?;
        out.push_str(&format!("    fputs(\"mut:{index}=\", stdout);\n"));
        out.push_str(&format!(
            "    for (size_t output_index_{} = 0; output_index_{} < {}_len; ++output_index_{}) {{\n        if (output_index_{} > 0) {{ fputc(',', stdout); }}\n{}    }}\n    fputc('\\n', stdout);\n",
            binding.name,
            binding.name,
            binding.name,
            binding.name,
            binding.name,
            indent_print_scalar_expr(
                &format!("{}[output_index_{}]", binding.name, binding.name),
                kind,
                8
            )
        ));
    }
    Ok(out)
}

fn indent_print_scalar_expr(expression: &str, kind: OutKind, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    print_scalar_expr(expression, kind)
        .lines()
        .map(|line| format!("{indent}{line}\n"))
        .collect()
}

fn parse_live_arg(
    program: &Program,
    name: &str,
    ty: &Type,
    argv_index: usize,
) -> Result<(String, String), String> {
    if let Type::Slice { elem, .. } = ty {
        return parse_live_slice(name, elem, argv_index);
    }
    if let Type::Named(struct_name) = ty {
        return parse_live_struct(program, name, struct_name, argv_index);
    }
    let c = c_type(ty).ok_or_else(|| {
        format!(
            "native_capsule_boundary_unsupported: input '{name}' type '{}' is not scalar",
            ty.to_agent()
        )
    })?;
    let bad = format!(
        "        fputs(\"native capsule invalid argument {argv_index}\\n\", stderr);\n        return 3;\n"
    );
    match ty {
        Type::Bool => Ok((format!(
            "    int {name};\n    if (strcmp(arg_{argv_index}, \"true\") == 0) {{ {name} = 1; }}\n    else if (strcmp(arg_{argv_index}, \"false\") == 0) {{ {name} = 0; }}\n    else {{\n{bad}    }}\n"
        ), String::new())),
        Type::Primitive(kind) if matches!(kind.as_str(), "f32" | "f64") => Ok((format!(
            "    errno = 0;\n    char *end_{name} = NULL;\n    double raw_{name} = strtod(arg_{argv_index}, &end_{name});\n    if (errno != 0 || end_{name} == arg_{argv_index} || *end_{name} != '\\0') {{\n{bad}    }}\n    {c} {name} = ({c})raw_{name};\n"
        ), String::new())),
        Type::Primitive(kind)
            if matches!(kind.as_str(), "u8" | "u16" | "u32" | "u64" | "usize") =>
        {
            Ok((format!(
                "    errno = 0;\n    char *end_{name} = NULL;\n    unsigned long long raw_{name} = strtoull(arg_{argv_index}, &end_{name}, 10);\n    if (errno != 0 || end_{name} == arg_{argv_index} || *end_{name} != '\\0') {{\n{bad}    }}\n    {c} {name} = ({c})raw_{name};\n"
            ), String::new()))
        }
        Type::Primitive(_) => Ok((format!(
            "    errno = 0;\n    char *end_{name} = NULL;\n    long long raw_{name} = strtoll(arg_{argv_index}, &end_{name}, 10);\n    if (errno != 0 || end_{name} == arg_{argv_index} || *end_{name} != '\\0') {{\n{bad}    }}\n    {c} {name} = ({c})raw_{name};\n"
        ), String::new())),
        _ => Err(format!(
            "native_capsule_boundary_unsupported: input '{name}' type '{}' is not scalar",
            ty.to_agent()
        )),
    }
}

fn parse_live_struct(
    program: &Program,
    name: &str,
    struct_name: &str,
    argv_index: usize,
) -> Result<(String, String), String> {
    let decl = validate_native_struct(program, struct_name)?;
    let bad = format!(
        "        fputs(\"native capsule invalid struct argument {argv_index}\\n\", stderr);\n        return 3;\n"
    );
    let mut fields = String::new();
    for (index, field) in decl.fields.iter().enumerate() {
        let next = format!("next_{name}_{index}");
        fields.push_str(&format!(
            "    char *{next} = strchr(token_{name}, ',');\n    if ({next} != NULL) {{ *{next} = '\\0'; }}\n"
        ));
        if index + 1 == decl.fields.len() {
            fields.push_str(&format!("    if ({next} != NULL) {{\n{bad}    }}\n"));
        } else {
            fields.push_str(&format!("    if ({next} == NULL) {{\n{bad}    }}\n"));
        }
        fields.push_str(&parse_struct_scalar(
            &format!("{name}.{}", field.name),
            &field.ty,
            &format!("token_{name}"),
            &format!("{name}_{index}"),
            &bad,
        )?);
        if index + 1 != decl.fields.len() {
            fields.push_str(&format!("    token_{name} = {next} + 1;\n"));
        }
    }
    let setup = format!(
        "    {struct_name} {name};\n    char *copy_{name} = (char *)malloc(strlen(arg_{argv_index}) + 1);\n    if (copy_{name} == NULL) {{\n        fputs(\"native capsule struct allocation failed\\n\", stderr);\n        return 4;\n    }}\n    strcpy(copy_{name}, arg_{argv_index});\n    char *token_{name} = copy_{name};\n{fields}"
    );
    Ok((setup, format!("    free(copy_{name});\n")))
}

fn parse_struct_scalar(
    destination: &str,
    ty: &Type,
    token: &str,
    suffix: &str,
    bad: &str,
) -> Result<String, String> {
    let c = c_type(ty).ok_or_else(|| {
        format!(
            "native_capsule_boundary_unsupported: struct field '{destination}' type '{}' is not scalar",
            ty.to_agent()
        )
    })?;
    match ty {
        Type::Primitive(kind) if matches!(kind.as_str(), "f32" | "f64") => Ok(format!(
            "    errno = 0;\n    char *end_{suffix} = NULL;\n    double raw_{suffix} = strtod({token}, &end_{suffix});\n    if (errno != 0 || end_{suffix} == {token} || *end_{suffix} != '\\0') {{\n{bad}    }}\n    {destination} = ({c})raw_{suffix};\n"
        )),
        Type::Primitive(kind)
            if matches!(kind.as_str(), "u8" | "u16" | "u32" | "u64" | "usize") =>
        {
            Ok(format!(
                "    errno = 0;\n    char *end_{suffix} = NULL;\n    unsigned long long raw_{suffix} = strtoull({token}, &end_{suffix}, 10);\n    if (errno != 0 || end_{suffix} == {token} || *end_{suffix} != '\\0') {{\n{bad}    }}\n    {destination} = ({c})raw_{suffix};\n"
            ))
        }
        Type::Primitive(_) => Ok(format!(
            "    errno = 0;\n    char *end_{suffix} = NULL;\n    long long raw_{suffix} = strtoll({token}, &end_{suffix}, 10);\n    if (errno != 0 || end_{suffix} == {token} || *end_{suffix} != '\\0') {{\n{bad}    }}\n    {destination} = ({c})raw_{suffix};\n"
        )),
        _ => Err(format!(
            "native_capsule_boundary_unsupported: struct field '{destination}' type '{}' is not scalar",
            ty.to_agent()
        )),
    }
}

fn parse_live_slice(
    name: &str,
    elem: &Type,
    argv_index: usize,
) -> Result<(String, String), String> {
    let c = c_type(elem).ok_or_else(|| {
        format!(
            "native_capsule_boundary_unsupported: slice input '{name}' element type '{}' is not scalar",
            elem.to_agent()
        )
    })?;
    let bad = format!(
        "            fputs(\"native capsule invalid slice element for {name}\\n\", stderr);\n            return 3;\n"
    );
    let element_parse = parse_slice_element(name, elem, &bad)?;
    let setup = format!(
        "    const char *wire_{name} = arg_{argv_index};\n    size_t {name}_len = 0;\n    if (*wire_{name} != '\\0') {{\n        {name}_len = 1;\n        for (const char *scan_{name} = wire_{name}; *scan_{name} != '\\0'; ++scan_{name}) {{\n            if (*scan_{name} == ',') {{ ++{name}_len; }}\n        }}\n    }}\n    if ({name}_len > {MAX_NATIVE_SLICE_ELEMENTS}) {{\n        fputs(\"native capsule slice exceeds element limit\\n\", stderr);\n        return 3;\n    }}\n    {c} *{name} = NULL;\n    char *copy_{name} = NULL;\n    if ({name}_len > 0) {{\n        {name} = ({c} *)calloc({name}_len, sizeof(*{name}));\n        copy_{name} = (char *)malloc(strlen(wire_{name}) + 1);\n        if ({name} == NULL || copy_{name} == NULL) {{\n            free(copy_{name});\n            free({name});\n            fputs(\"native capsule slice allocation failed\\n\", stderr);\n            return 4;\n        }}\n        strcpy(copy_{name}, wire_{name});\n        char *token_{name} = copy_{name};\n        for (size_t index_{name} = 0; index_{name} < {name}_len; ++index_{name}) {{\n            char *next_{name} = strchr(token_{name}, ',');\n            if (next_{name} != NULL) {{ *next_{name} = '\\0'; }}\n{element_parse}            token_{name} = next_{name} == NULL ? token_{name} : next_{name} + 1;\n        }}\n    }}\n"
    );
    let cleanup = format!("    free(copy_{name});\n    free({name});\n");
    Ok((setup, cleanup))
}

fn parse_slice_element(name: &str, elem: &Type, bad: &str) -> Result<String, String> {
    let c = c_type(elem).ok_or_else(|| {
        format!(
            "native_capsule_boundary_unsupported: slice input '{name}' element type '{}' is not scalar",
            elem.to_agent()
        )
    })?;
    let destination = format!("{name}[index_{name}]");
    let suffix = format!("{name}_element");
    match elem {
        Type::Bool => Ok(format!(
            "            if (strcmp(token_{name}, \"true\") == 0) {{ {destination} = 1; }}\n            else if (strcmp(token_{name}, \"false\") == 0) {{ {destination} = 0; }}\n            else {{\n{bad}            }}\n"
        )),
        Type::Primitive(kind) if matches!(kind.as_str(), "f32" | "f64") => Ok(format!(
            "            errno = 0;\n            char *end_{suffix} = NULL;\n            double raw_{suffix} = strtod(token_{name}, &end_{suffix});\n            if (errno != 0 || end_{suffix} == token_{name} || *end_{suffix} != '\\0') {{\n{bad}            }}\n            {destination} = ({c})raw_{suffix};\n"
        )),
        Type::Primitive(kind)
            if matches!(kind.as_str(), "u8" | "u16" | "u32" | "u64" | "usize") =>
        {
            Ok(format!(
                "            errno = 0;\n            char *end_{suffix} = NULL;\n            unsigned long long raw_{suffix} = strtoull(token_{name}, &end_{suffix}, 10);\n            if (errno != 0 || end_{suffix} == token_{name} || *end_{suffix} != '\\0') {{\n{bad}            }}\n            {destination} = ({c})raw_{suffix};\n"
            ))
        }
        Type::Primitive(_) => Ok(format!(
            "            errno = 0;\n            char *end_{suffix} = NULL;\n            long long raw_{suffix} = strtoll(token_{name}, &end_{suffix}, 10);\n            if (errno != 0 || end_{suffix} == token_{name} || *end_{suffix} != '\\0') {{\n{bad}            }}\n            {destination} = ({c})raw_{suffix};\n"
        )),
        _ => Err(format!(
            "native_capsule_boundary_unsupported: slice input '{name}' element type '{}' is not scalar",
            elem.to_agent()
        )),
    }
}

fn live_scratch(capsule: &Capsule) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LIVE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = LIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!(
        "forgentex_{}_{}_{}_native_live",
        std::process::id(),
        seq,
        capsule.name
    ));
    std::fs::create_dir_all(&scratch).map_err(|err| {
        format!(
            "native_runtime_unavailable: failed to create {}: {err}",
            scratch.display()
        )
    })?;
    Ok(scratch)
}

fn native_runner_exe(scratch: &Path) -> PathBuf {
    scratch.join(if cfg!(windows) {
        "live_runner.exe"
    } else {
        "live_runner"
    })
}

fn native_runner_cache_key(
    program: &Program,
    capsule: &Capsule,
    toolchain: &Toolchain,
    signature: &str,
    standard: &str,
) -> String {
    let material = format!(
        "program={program:?}\ncapsule={capsule:?}\ntoolchain={toolchain:?}\nsignature={signature}\nstandard={standard}\nplatform={}",
        std::env::consts::OS
    );
    crate::crypto::sha256_hex(material.as_bytes())
}

fn generate_harness(
    capsule: &Capsule,
    _signature: &str,
    output: &NativeOutput,
) -> Result<String, String> {
    if matches!(output, NativeOutput::Struct(_)) && !capsule.fixtures.is_empty() {
        return Err(
            "struct-output fixture literals are not part of the static FFI runner; use fgx run --live for the reviewed runtime boundary"
                .to_string(),
        );
    }
    let mut out = String::new();
    out.push_str("#include <stdio.h>\n#include <stdint.h>\n#include <stddef.h>\n");
    out.push_str("#include \"boundary.h\"\n\nint main(void) {\n");
    for fixture in &capsule.fixtures {
        let args = fixture
            .inputs
            .iter()
            .zip(capsule.inputs.iter())
            .map(|(value, binding)| c_input_literal(value, &binding.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let call = format!("{}({args})", capsule.name);
        let NativeOutput::Scalar(kind) = output else {
            unreachable!("struct output fixtures were rejected above");
        };
        let line = match kind {
            OutKind::SignedInt => format!("    printf(\"%lld\\n\", (long long)({call}));\n"),
            OutKind::UnsignedInt => {
                format!("    printf(\"%llu\\n\", (unsigned long long)({call}));\n")
            }
            OutKind::Float => format!("    printf(\"%.9g\\n\", (double)({call}));\n"),
            OutKind::Bool => format!("    printf(\"%d\\n\", ({call}) ? 1 : 0);\n"),
        };
        out.push_str(&line);
    }
    out.push_str("    return 0;\n}\n");
    Ok(out)
}

fn tc(toolchain: &Toolchain) -> String {
    toolchain.name.clone()
}

fn disp(path: &Path) -> String {
    path.display().to_string()
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn select_capsule<'a>(program: &'a Program, name: Option<&str>) -> Result<&'a Capsule, String> {
    match name {
        Some(name) => program
            .capsules()
            .find(|capsule| capsule.name == name)
            .ok_or_else(|| format!("no capsule named '{name}'")),
        None => {
            let mut capsules = program.capsules();
            let first = capsules
                .next()
                .ok_or_else(|| "no capsule in file".to_string())?;
            if capsules.next().is_some() {
                return Err(
                    "file has multiple capsules; pass --capsule <name> to choose one".to_string(),
                );
            }
            Ok(first)
        }
    }
}

impl FfiPlan {
    pub fn is_success(&self) -> bool {
        // A missing host compiler or a not-yet-wired language is not a failure
        // of Forgentex itself; a real mismatch or denial is.
        matches!(
            self.execution_status.as_str(),
            "executed"
                | "compiled"
                | "compiler_unavailable"
                | "execution_unsupported_in_slice"
                | "planned"
        )
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Forgentex C-ABI FFI bridge\n");
        out.push_str(&format!(
            "schema={} file={} capsule={} lang={} class={}\n",
            self.schema, self.file, self.capsule, self.language, self.risk_class
        ));
        out.push_str(&format!("  extern \"C\": {}\n", self.extern_c_signature));
        for m in &self.marshaling {
            out.push_str(&format!(
                "    {:<4} {:<12} -> {:<10} {}\n",
                m.direction, m.fgx_type, m.c_type, m.name
            ));
        }
        out.push_str(&format!(
            "  gate: {} effects=[{}] grants=[{}] deps_pinned={}{}\n",
            self.gate_decision,
            self.effects.join(", "),
            self.grants.join(", "),
            self.deps_pinned,
            if self.gate_reasons.is_empty() {
                String::new()
            } else {
                format!(" reasons=[{}]", self.gate_reasons.join("; "))
            }
        ));
        if let Some(toolchain) = &self.toolchain {
            out.push_str(&format!(
                "  toolchain: {} ({})\n",
                toolchain.name, toolchain.version
            ));
        }
        if !self.link_units.is_empty() {
            out.push_str(&format!("  link: {}\n", self.link_units.join(" + ")));
        }
        out.push_str(&format!(
            "  execution: {} ({}/{} fixtures)\n",
            self.execution_status, self.passed, self.total
        ));
        for run in &self.transcript {
            out.push_str(&format!(
                "    [{}] ({}) -> {} expected {} {}\n",
                run.index,
                run.inputs.join(", "),
                run.output,
                run.expected,
                if run.pass { "ok" } else { "MISMATCH" }
            ));
        }
        if let Some(message) = &self.message {
            out.push_str(&format!("  note: {message}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let marshaling = self
            .marshaling
            .iter()
            .map(|m| {
                format!(
                    "{{ \"name\": {}, \"fgx_type\": {}, \"c_type\": {}, \"direction\": {} }}",
                    json::string(&m.name),
                    json::string(&m.fgx_type),
                    json::string(&m.c_type),
                    json::string(&m.direction)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        let transcript = self
            .transcript
            .iter()
            .map(|r| {
                format!(
                    "{{ \"index\": {}, \"inputs\": {}, \"output\": {}, \"expected\": {}, \"pass\": {} }}",
                    r.index,
                    json::string_array(&r.inputs),
                    json::string(&r.output),
                    json::string(&r.expected),
                    r.pass
                )
            })
            .collect::<Vec<_>>()
            .join(",\n    ");
        let toolchain = match &self.toolchain {
            Some(t) => format!(
                "{{ \"name\": {}, \"version\": {} }}",
                json::string(&t.name),
                json::string(&t.version)
            ),
            None => "null".to_string(),
        };
        format!(
            "{{\n  \"schema\": {},\n  \"file\": {},\n  \"capsule\": {},\n  \"language\": {},\n  \"risk_class\": {},\n  \"extern_c_signature\": {},\n  \"marshaling\": [\n    {}\n  ],\n  \"link_units\": {},\n  \"effects\": {},\n  \"grants\": {},\n  \"deps_pinned\": {},\n  \"gate_decision\": {},\n  \"gate_reasons\": {},\n  \"execution_status\": {},\n  \"toolchain\": {},\n  \"passed\": {},\n  \"total\": {},\n  \"transcript\": [\n    {}\n  ],\n  \"message\": {}\n}}",
            json::string(&self.schema),
            json::string(&self.file),
            json::string(&self.capsule),
            json::string(&self.language),
            json::string(&self.risk_class),
            json::string(&self.extern_c_signature),
            marshaling,
            json::string_array(&self.link_units),
            json::string_array(&self.effects),
            json::string_array(&self.grants),
            self.deps_pinned,
            json::string(&self.gate_decision),
            json::string_array(&self.gate_reasons),
            json::string(&self.execution_status),
            toolchain,
            self.passed,
            self.total,
            transcript,
            json::opt_string(self.message.as_deref()),
        )
    }
}
