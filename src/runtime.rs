//! Managed runtime catalog and ecosystem reconciliation (v2.5).
//!
//! v2.5 extends the capsule model to managed runtimes (JavaScript,
//! TypeScript, Java, C#, Go alongside Python) with their expected package
//! ecosystems and package-lock evidence. It exposes the catalog through
//! `fgx runtimes` and reconciles each capsule's dependencies against its
//! language: a Python capsule pulling an `npm:` dependency is a
//! `capsule_ecosystem_mismatch`. Generic sources (`github:`, `git:`,
//! `path:`, `https:`) are allowed for any language.

use crate::ast::Program;
use crate::capsule_lock::parse_dependency;
use crate::diag::Diagnostic;
use crate::json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSpec {
    pub language: &'static str,
    pub display: &'static str,
    pub class: &'static str,
    pub ecosystems: &'static [&'static str],
    pub lock_files: &'static [&'static str],
}

pub fn catalog() -> Vec<RuntimeSpec> {
    vec![
        RuntimeSpec {
            language: "python",
            display: "Python (CPython)",
            class: "managed",
            ecosystems: &["pypi"],
            lock_files: &["requirements.txt", "poetry.lock", "uv.lock"],
        },
        RuntimeSpec {
            language: "javascript",
            display: "JavaScript (Node)",
            class: "managed",
            ecosystems: &["npm"],
            lock_files: &["package-lock.json", "pnpm-lock.yaml", "yarn.lock"],
        },
        RuntimeSpec {
            language: "typescript",
            display: "TypeScript (Node)",
            class: "managed",
            ecosystems: &["npm"],
            lock_files: &["package-lock.json", "pnpm-lock.yaml", "yarn.lock"],
        },
        RuntimeSpec {
            language: "java",
            display: "Java (JVM)",
            class: "managed",
            ecosystems: &["maven", "gradle"],
            lock_files: &["pom.xml", "gradle.lockfile"],
        },
        RuntimeSpec {
            language: "csharp",
            display: "C# (.NET)",
            class: "managed",
            ecosystems: &["nuget"],
            lock_files: &["packages.lock.json"],
        },
        RuntimeSpec {
            language: "go",
            display: "Go",
            class: "managed",
            ecosystems: &["go"],
            lock_files: &["go.sum"],
        },
    ]
}

fn spec_for(language: &str) -> Option<RuntimeSpec> {
    catalog().into_iter().find(|spec| spec.language == language)
}

/// The catalog languages that own a package ecosystem (e.g. `npm` is owned
/// by `javascript` and `typescript`). Generic sources own no language.
fn ecosystem_owners(ecosystem: &str) -> Vec<&'static str> {
    catalog()
        .into_iter()
        .filter(|spec| spec.ecosystems.contains(&ecosystem))
        .map(|spec| spec.language)
        .collect()
}

/// `capsule_ecosystem_mismatch`: a capsule whose language is a known
/// managed/Python runtime must not depend on another language's package
/// ecosystem. Generic sources and unknown ecosystems are allowed.
pub fn validate(program: &Program) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for capsule in program.capsules() {
        if spec_for(&capsule.language).is_none() {
            continue;
        }
        for dep in &capsule.deps {
            let parsed = parse_dependency(dep);
            let owners = ecosystem_owners(&parsed.ecosystem);
            if !owners.is_empty() && !owners.contains(&capsule.language.as_str()) {
                diagnostics.push(
                    Diagnostic::new(
                        "capsule_ecosystem_mismatch",
                        &capsule.id,
                        format!(
                            "capsule '{}' is {} but dependency '{}' uses the {} ecosystem (owned by {})",
                            capsule.name,
                            capsule.language,
                            parsed.raw,
                            parsed.ecosystem,
                            owners.join("/")
                        ),
                    )
                    .suggest("use a dependency from the capsule language's ecosystem, or a generic github:/path: source"),
                );
            }
        }
    }
    diagnostics
}

pub fn catalog_json() -> String {
    let specs = catalog();
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"schema\": \"forgentex.runtime_catalog.v1\",\n");
    out.push_str("  \"runtimes\": [");
    for (idx, spec) in specs.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {\n");
        out.push_str(&format!(
            "      \"language\": {},\n",
            json::string(spec.language)
        ));
        out.push_str(&format!(
            "      \"display\": {},\n",
            json::string(spec.display)
        ));
        out.push_str(&format!("      \"class\": {},\n", json::string(spec.class)));
        out.push_str(&format!(
            "      \"ecosystems\": {},\n",
            json::string_array(&owned_strings(spec.ecosystems))
        ));
        out.push_str(&format!(
            "      \"lock_files\": {}\n",
            json::string_array(&owned_strings(spec.lock_files))
        ));
        out.push_str("    }");
    }
    out.push_str("\n  ]\n}");
    out
}

pub fn catalog_text() -> String {
    let mut out = String::new();
    out.push_str("Forgentex managed runtime catalog\n");
    for spec in catalog() {
        out.push_str(&format!(
            "  - {} ({}) class={} ecosystems=[{}] lock_files=[{}]\n",
            spec.language,
            spec.display,
            spec.class,
            spec.ecosystems.join(","),
            spec.lock_files.join(",")
        ));
    }
    out
}

fn owned_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}
