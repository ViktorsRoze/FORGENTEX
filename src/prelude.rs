//! Prelude: short two-letter agent aliases for high-frequency builtins.
//!
//! Twelve high-frequency standard builtins get a two-letter alias used in
//! the `.fgai` agent rendering. The AST always stores the canonical name,
//! so `.fgx` stays readable, type-checking/effects/interpretation are
//! unchanged, and the dual syntax does the work: humans read
//! `core_math_clamp_i16`, agents read `cl`. Round-trips are lossless
//! because the alias is reserved — a user function may not take an alias
//! name, so an alias in agent source always means the builtin.

use crate::ast::Program;
use crate::diag::Diagnostic;

/// (canonical builtin name, two-letter agent alias).
pub const PRELUDE: &[(&str, &str)] = &[
    ("core_slice_len_i16", "ln"),
    ("core_slice_len_mut_i16", "lm"),
    ("core_slice_len_u8", "lu"),
    ("core_slice_copy_i16", "cp"),
    ("core_math_clamp_i16", "cl"),
    ("core_math_saturating_add_i16", "sa"),
    ("core_math_checked_add_i32", "ca"),
    ("core_math_checked_sub_i32", "cs"),
    ("core_math_checked_mul_i32", "cm"),
    ("core_bits_popcount_u32", "pc"),
    ("core_bits_clz_u32", "cz"),
    ("core_bits_ctz_u32", "tz"),
];

/// Resolve a two-letter agent alias to its canonical builtin name.
pub fn alias_to_canonical(alias: &str) -> Option<&'static str> {
    PRELUDE
        .iter()
        .find(|(_, a)| *a == alias)
        .map(|(canonical, _)| *canonical)
}

/// The two-letter alias for a canonical builtin name, if any.
pub fn canonical_to_alias(name: &str) -> Option<&'static str> {
    PRELUDE
        .iter()
        .find(|(canonical, _)| *canonical == name)
        .map(|(_, alias)| *alias)
}

/// True when `name` is one of the reserved two-letter aliases.
pub fn is_reserved_alias(name: &str) -> bool {
    PRELUDE.iter().any(|(_, alias)| *alias == name)
}

/// Replace whole-word canonical builtin names with their two-letter alias.
/// Used as the final pass of the agent formatter. Canonical builtin names
/// are long and distinctive, so whole-word replacement is unambiguous.
pub fn apply_aliases(agent_text: &str) -> String {
    let chars: Vec<char> = agent_text.chars().collect();
    let mut out = String::with_capacity(agent_text.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match canonical_to_alias(&word) {
                Some(alias) => out.push_str(alias),
                None => out.push_str(&word),
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// `reserved_prelude_alias`: a user function may not be named like a
/// two-letter prelude alias, so agent aliases never collide with user code.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for function in program.functions() {
        if is_reserved_alias(&function.name) {
            diagnostics.push(
                Diagnostic::new(
                    "reserved_prelude_alias",
                    &function.id,
                    format!(
                        "function name '{}' is a reserved prelude alias",
                        function.name
                    ),
                )
                .suggest(
                    "rename the function; two-letter prelude aliases are reserved for builtins",
                ),
            );
        }
    }
    diagnostics
}
