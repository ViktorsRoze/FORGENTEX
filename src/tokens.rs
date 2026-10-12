//! Token-efficiency measurement (token-report).
//!
//! The crude `split_whitespace().count()` used for `.fgmap` estimates does
//! not reflect how an LLM tokenizer splits text — it cannot see that the
//! dense punctuation of `.fgai` may tokenize differently than readable
//! `.fgx`. This module provides a heuristic, dependency-free token estimate
//! that is closer to byte-pair encoding, and a `token-report` that measures
//! the same program across its surfaces (human, agent, canonical AST, and
//! compressed agent) so token-efficiency claims become evidence.
//!
//! # What the estimate is, and is not
//!
//! The estimate is a *heuristic*: a few rules about letters, digits,
//! whitespace, and punctuation that run instantly and need no tokenizer. It is
//! useful for local budgets — "will this fit" — and nothing more.
//!
//! This module used to say the heuristic was "fair and monotonic across
//! surfaces, so the ratios between surfaces are meaningful". That was wrong,
//! and measurably so: against real byte-pair encoders (`cl100k_base` and
//! `o200k_base`) the heuristic put `audio/src/dsp.fgx` at 0.97 agent/human when
//! the agent form was actually about 1% *larger*, and `smoke/inline_tests.fgx`
//! at 0.97 when it was about 7% larger. It does not merely misjudge the size of
//! a difference; it can report the wrong direction. So no ratio it produces is
//! evidence that one surface costs fewer tokens than another.
//!
//! Measured counts come from a real tokenizer through `--bpe-command`, which
//! hands each surface to an external program and reads the count back. Those
//! numbers are reported alongside the heuristic, never merged with it, and name
//! the tokenizer that produced them.

use crate::ast::Program;
use crate::compress::{compress, minify_agent};
use crate::format::{format_agent, format_human};

/// Heuristic BPE-style token estimate.
///
/// Tuned to be closer to real byte-pair encoding than a whitespace word
/// count:
/// - alphabetic run: ~4 chars per subword token (`ceil(len/4)`),
/// - digit run: ~3 chars per token,
/// - newline: 1 token,
/// - other whitespace run: 1-3 spaces merge into the adjacent token (free),
///   larger indentation costs `len/4` (real BPE merges leading spaces),
/// - every other character (punctuation/symbol): 1 token.
pub fn estimate(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            tokens += (i - start).div_ceil(4).max(1);
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            tokens += (i - start).div_ceil(3).max(1);
        } else if c == '\n' {
            tokens += 1;
            i += 1;
        } else if c == ' ' || c == '\t' || c == '\r' {
            let start = i;
            while i < chars.len() && matches!(chars[i], ' ' | '\t' | '\r') {
                i += 1;
            }
            // Small runs of spaces merge into the following token (free);
            // only larger indentation runs cost tokens.
            tokens += (i - start) / 4;
        } else {
            tokens += 1;
            i += 1;
        }
    }
    tokens
}

#[derive(Clone, Debug, PartialEq)]
pub struct TokenReport {
    pub schema: String,
    pub file: String,
    pub tokenizer: String,
    pub human_tokens: usize,
    pub agent_tokens: usize,
    pub agent_minified_tokens: usize,
    pub agent_compressed_tokens: usize,
    pub canonical_ast_tokens: usize,
    pub human_chars: usize,
    pub agent_chars: usize,
    pub functions: usize,
    pub tests: usize,
    pub capsules: usize,
    /// Counts from a real tokenizer, when one was supplied. Kept apart from the
    /// heuristic fields so the two can never be read as the same measurement.
    pub measured: Option<MeasuredTokens>,
}

/// Token counts produced by an external, real tokenizer.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredTokens {
    /// What produced the counts, as the external program described itself, or
    /// the command line when it did not.
    pub tokenizer: String,
    pub human_tokens: usize,
    pub agent_tokens: usize,
    pub agent_minified_tokens: usize,
    pub agent_compressed_tokens: usize,
}

/// The standing caveat attached to every heuristic ratio, in text and JSON, so
/// a reader who only sees one number still sees what kind of number it is.
pub const HEURISTIC_CAVEAT: &str = "heuristic-v1 ratios are estimates and can point the wrong way; they are not evidence that one surface costs fewer tokens. Use --bpe-command for measured counts.";

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        1.0
    } else {
        part as f64 / whole as f64
    }
}

pub fn report(file: &str, program: &Program) -> TokenReport {
    let human = format_human(program);
    let agent = format_agent(program);
    let canonical = program.to_canonical_json();
    let minified = minify_agent(&agent);
    // Compress the minified form for maximum effect.
    let compressed = compress(&minified).compressed;

    TokenReport {
        schema: "forgentex.token_efficiency.v1".to_string(),
        file: file.to_string(),
        tokenizer: "heuristic-v1".to_string(),
        human_tokens: estimate(&human),
        agent_tokens: estimate(&agent),
        agent_minified_tokens: estimate(&minified),
        agent_compressed_tokens: estimate(&compressed),
        canonical_ast_tokens: estimate(&canonical),
        human_chars: human.chars().count(),
        agent_chars: agent.chars().count(),
        functions: program.functions().count(),
        tests: program.tests().count(),
        capsules: program.capsules().count(),
        measured: None,
    }
}

/// The four surfaces a measurement covers, in the order they are reported.
pub fn surfaces(program: &Program) -> [(&'static str, String); 4] {
    let human = format_human(program);
    let agent = format_agent(program);
    let minified = minify_agent(&agent);
    let compressed = compress(&minified).compressed;
    [
        ("human", human),
        ("agent", agent),
        ("agent_minified", minified),
        ("agent_compressed", compressed),
    ]
}

/// Measure every surface with an external tokenizer.
///
/// `count` receives one surface's exact text and returns its token count. The
/// CLI passes a closure that runs the user's command; tests pass a pure one.
/// Every surface must be measured by the same tokenizer, so a failure on any of
/// them fails the whole measurement rather than leaving a partial table that
/// mixes measured and estimated numbers.
pub fn measure(
    program: &Program,
    tokenizer: &str,
    mut count: impl FnMut(&str) -> Result<usize, String>,
) -> Result<MeasuredTokens, String> {
    let [human, agent, minified, compressed] = surfaces(program);
    Ok(MeasuredTokens {
        tokenizer: tokenizer.to_string(),
        human_tokens: count(&human.1).map_err(|err| format!("human surface: {err}"))?,
        agent_tokens: count(&agent.1).map_err(|err| format!("agent surface: {err}"))?,
        agent_minified_tokens: count(&minified.1)
            .map_err(|err| format!("agent_minified surface: {err}"))?,
        agent_compressed_tokens: count(&compressed.1)
            .map_err(|err| format!("agent_compressed surface: {err}"))?,
    })
}

impl TokenReport {
    pub fn agent_vs_human(&self) -> f64 {
        ratio(self.agent_tokens, self.human_tokens)
    }

    pub fn minified_vs_agent(&self) -> f64 {
        ratio(self.agent_minified_tokens, self.agent_tokens)
    }

    pub fn compressed_vs_agent(&self) -> f64 {
        ratio(self.agent_compressed_tokens, self.agent_tokens)
    }

    /// The smallest agent-side surface vs the human source — the best
    /// lossless per-file token result Forgentex can offer today.
    pub fn best_vs_human(&self) -> f64 {
        ratio(self.agent_compressed_tokens, self.human_tokens)
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex token-efficiency report\nschema={} file={} tokenizer={}\n  human_tokens={} agent_tokens={} agent_minified_tokens={} agent_compressed_tokens={} canonical_ast_tokens={}\n  agent_vs_human={:.3} minified_vs_agent={:.3} compressed_vs_agent={:.3} best_vs_human={:.3}\n  human_chars={} agent_chars={} functions={} tests={} capsules={}\n",
            self.schema,
            self.file,
            self.tokenizer,
            self.human_tokens,
            self.agent_tokens,
            self.agent_minified_tokens,
            self.agent_compressed_tokens,
            self.canonical_ast_tokens,
            self.agent_vs_human(),
            self.minified_vs_agent(),
            self.compressed_vs_agent(),
            self.best_vs_human(),
            self.human_chars,
            self.agent_chars,
            self.functions,
            self.tests,
            self.capsules
        ) + &self.measured_text()
    }

    fn measured_text(&self) -> String {
        let mut out = format!(
            "  note: {HEURISTIC_CAVEAT}
"
        );
        if let Some(m) = &self.measured {
            out.push_str(&format!(
                "  measured tokenizer={}
  measured human_tokens={} agent_tokens={} agent_minified_tokens={} agent_compressed_tokens={}
  measured agent_vs_human={:.3} best_vs_human={:.3}
",
                m.tokenizer,
                m.human_tokens,
                m.agent_tokens,
                m.agent_minified_tokens,
                m.agent_compressed_tokens,
                ratio(m.agent_tokens, m.human_tokens),
                ratio(m.agent_compressed_tokens, m.human_tokens),
            ));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let base = format!(
            "{{\n  \"schema\": \"{}\",\n  \"file\": \"{}\",\n  \"tokenizer\": \"{}\",\n  \"human_tokens\": {},\n  \"agent_tokens\": {},\n  \"agent_minified_tokens\": {},\n  \"agent_compressed_tokens\": {},\n  \"canonical_ast_tokens\": {},\n  \"agent_vs_human\": {:.4},\n  \"minified_vs_agent\": {:.4},\n  \"compressed_vs_agent\": {:.4},\n  \"best_vs_human\": {:.4},\n  \"human_chars\": {},\n  \"agent_chars\": {},\n  \"functions\": {},\n  \"tests\": {},\n  \"capsules\": {}\n}}",
            self.schema,
            self.file,
            self.tokenizer,
            self.human_tokens,
            self.agent_tokens,
            self.agent_minified_tokens,
            self.agent_compressed_tokens,
            self.canonical_ast_tokens,
            self.agent_vs_human(),
            self.minified_vs_agent(),
            self.compressed_vs_agent(),
            self.best_vs_human(),
            self.human_chars,
            self.agent_chars,
            self.functions,
            self.tests,
            self.capsules
        );
        // Additive fields, so existing consumers of the v1 schema keep working:
        // the kind of every ratio above, and the measured counts when present.
        let body = base.trim_end().trim_end_matches('}').trim_end().to_string();
        let measured = match &self.measured {
            None => "null".to_string(),
            Some(m) => format!(
                "{{\"tokenizer\": \"{}\", \"human_tokens\": {}, \"agent_tokens\": {}, \"agent_minified_tokens\": {}, \"agent_compressed_tokens\": {}, \"agent_vs_human\": {:.4}, \"best_vs_human\": {:.4}}}",
                crate::json::escape(&m.tokenizer),
                m.human_tokens,
                m.agent_tokens,
                m.agent_minified_tokens,
                m.agent_compressed_tokens,
                ratio(m.agent_tokens, m.human_tokens),
                ratio(m.agent_compressed_tokens, m.human_tokens),
            ),
        };
        format!(
            "{body},
  \"ratio_kind\": \"heuristic\",
  \"caveat\": \"{}\",
  \"measured\": {measured}
}}",
            crate::json::escape(HEURISTIC_CAVEAT)
        )
    }
}
