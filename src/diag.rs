use crate::ast::Type;

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub code: String,
    pub node: String,
    pub message: String,
    pub expected: Option<String>,
    pub found: Option<String>,
    pub suggested_patches: Vec<String>,
}

impl Diagnostic {
    pub fn new(code: &str, node: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            node: node.into(),
            message: message.into(),
            expected: None,
            found: None,
            suggested_patches: Vec::new(),
        }
    }

    pub fn with_types(mut self, expected: &Type, found: &Type) -> Self {
        self.expected = Some(expected.to_agent());
        self.found = Some(found.to_agent());
        self
    }

    pub fn suggest(mut self, patch: &str) -> Self {
        self.suggested_patches.push(patch.to_string());
        self
    }
}

pub fn diagnostics_to_json(diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    out.push_str("{\n  \"diagnostics\": [");
    for (idx, diagnostic) in diagnostics.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("\n    {");
        out.push_str(&format!(
            "\n      \"error\": \"{}\",\n      \"node\": \"{}\",\n      \"message\": \"{}\"",
            escape(&diagnostic.code),
            escape(&diagnostic.node),
            escape(&diagnostic.message)
        ));
        if let Some(expected) = &diagnostic.expected {
            out.push_str(&format!(",\n      \"expected\": \"{}\"", escape(expected)));
        }
        if let Some(found) = &diagnostic.found {
            out.push_str(&format!(",\n      \"found\": \"{}\"", escape(found)));
        }
        out.push_str(",\n      \"suggested_patches\": [");
        for (patch_idx, patch) in diagnostic.suggested_patches.iter().enumerate() {
            if patch_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("\"{}\"", escape(patch)));
        }
        out.push_str("]\n    }");
    }
    out.push_str("\n  ]\n}");
    out
}

use crate::json::escape;
