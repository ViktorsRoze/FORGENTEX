//! Deterministic, lossless agent-view compression (v2.10).
//!
//! An agent reads the compact `.fgai` rendering; v2.10 compresses it
//! further with a dictionary of repeated identifiers and dotted paths. The
//! transform is **deterministic** (same input -> same output) and
//! **lossless** (`decompress(compress(x)) == x`), which is a conformance
//! requirement. Whether it nets fewer *tokens* (not just characters) is
//! measured by `fgx token-report`; compression that does not help is
//! simply not applied.
//!
//! Format:
//!
//! ```text
//! #FGAI-Z v1 sigil=<char>
//! #D <sigil>0=<word>
//! #D <sigil>1=<word>
//! <body with words replaced by <sigil><index>>
//! ```
//!
//! The sigil is a character chosen to be absent from the source, so a
//! handle can never collide with original text.

/// Candidate handle sigils, tried in order; the first absent from the
/// source is used. All are rare in source text.
const SIGILS: &[char] = &['\u{a7}', '\u{b6}', '\u{2021}', '\u{a4}', '\u{2756}'];

const MIN_WORD_LEN: usize = 4;
const MIN_FREQ: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressReport {
    pub schema: String,
    pub original_chars: usize,
    pub compressed_chars: usize,
    pub dictionary_entries: usize,
    pub sigil: Option<char>,
    pub roundtrip_ok: bool,
    pub compressed: String,
}

/// Split text into a sequence of tokens: word tokens (identifiers / dotted
/// paths starting with a letter or `_`) and verbatim single chars.
fn tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
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
            tokens.push(chars[start..i].iter().collect());
        } else {
            tokens.push(ch.to_string());
            i += 1;
        }
    }
    tokens
}

fn choose_sigil(source: &str) -> Option<char> {
    SIGILS.iter().copied().find(|s| !source.contains(*s))
}

/// Compress the agent rendering of a source text.
pub fn compress(source: &str) -> CompressReport {
    let original_chars = source.chars().count();
    let Some(sigil) = choose_sigil(source) else {
        return CompressReport {
            schema: "forgentex.capsule_compress.v1".to_string(),
            original_chars,
            compressed_chars: original_chars,
            dictionary_entries: 0,
            sigil: None,
            roundtrip_ok: true,
            compressed: source.to_string(),
        };
    };

    let tokens = tokenize(source);

    // Frequency of word tokens worth compressing.
    let mut order: Vec<String> = Vec::new();
    let mut freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for token in &tokens {
        if is_word(token) && token.len() >= MIN_WORD_LEN {
            if !freq.contains_key(token) {
                order.push(token.clone());
            }
            *freq.entry(token.clone()).or_insert(0) += 1;
        }
    }

    // Candidate words: frequent enough that substitution saves characters.
    // Rank by net saving (desc), then first-seen order for determinism.
    let mut candidates: Vec<String> = order
        .into_iter()
        .filter(|word| freq[word] >= MIN_FREQ)
        .collect();
    candidates.sort_by(|a, b| {
        let sa = (a.chars().count().saturating_sub(2)) * freq[a];
        let sb = (b.chars().count().saturating_sub(2)) * freq[b];
        sb.cmp(&sa).then_with(|| a.cmp(b))
    });

    let mut dictionary: Vec<(String, String)> = Vec::new();
    for word in candidates {
        let index = dictionary.len();
        let handle = format!("{sigil}{index}");
        let count = freq[&word];
        // Token-aware selection: a handle costs ~2 tokens, so only words
        // that cost more than the handle and repeat enough to amortize the
        // dictionary line are worth substituting. Selecting by characters
        // (the previous approach) made compression token-negative.
        let word_tokens = crate::tokens::estimate(&word);
        let handle_tokens = crate::tokens::estimate(&handle);
        let line_tokens = crate::tokens::estimate(&format!("#D {handle}={word}\n"));
        let per_use_saving = word_tokens.saturating_sub(handle_tokens);
        if per_use_saving * count > line_tokens {
            dictionary.push((handle, word));
        }
    }

    let lookup: std::collections::HashMap<&str, &str> = dictionary
        .iter()
        .map(|(handle, word)| (word.as_str(), handle.as_str()))
        .collect();

    let mut body = String::new();
    for token in &tokens {
        if let Some(handle) = lookup.get(token.as_str()) {
            body.push_str(handle);
        } else {
            body.push_str(token);
        }
    }

    let mut compressed = String::new();
    if dictionary.is_empty() {
        // Nothing worth compressing; return source unchanged.
        return CompressReport {
            schema: "forgentex.capsule_compress.v1".to_string(),
            original_chars,
            compressed_chars: original_chars,
            dictionary_entries: 0,
            sigil: Some(sigil),
            roundtrip_ok: true,
            compressed: source.to_string(),
        };
    }
    compressed.push_str(&format!(
        "#FGAI-Z v1 sigil={sigil} dict={}\n",
        dictionary.len()
    ));
    for (handle, word) in &dictionary {
        compressed.push_str(&format!("#D {handle}={word}\n"));
    }
    compressed.push_str(&body);

    let roundtrip_ok = decompress(&compressed)
        .map(|round| round == source)
        .unwrap_or(false);

    // Never emit a token-larger artifact: if the dictionary header
    // outweighs the savings (in tokens, the unit that matters), keep the
    // source unchanged.
    if crate::tokens::estimate(&compressed) >= crate::tokens::estimate(source) || !roundtrip_ok {
        return CompressReport {
            schema: "forgentex.capsule_compress.v1".to_string(),
            original_chars,
            compressed_chars: original_chars,
            dictionary_entries: 0,
            sigil: Some(sigil),
            roundtrip_ok: true,
            compressed: source.to_string(),
        };
    }

    CompressReport {
        schema: "forgentex.capsule_compress.v1".to_string(),
        original_chars,
        compressed_chars: compressed.chars().count(),
        dictionary_entries: dictionary.len(),
        sigil: Some(sigil),
        roundtrip_ok,
        compressed,
    }
}

/// Reverse `compress`, reproducing the original source exactly.
pub fn decompress(compressed: &str) -> Result<String, String> {
    let mut lines = compressed.split_inclusive('\n');
    let Some(header) = lines.next() else {
        return Ok(compressed.to_string());
    };
    let header_trimmed = header.trim_end_matches('\n');
    if !header_trimmed.starts_with("#FGAI-Z ") {
        // Not a compressed artifact; return as-is.
        return Ok(compressed.to_string());
    }
    // `dict=N` makes decompression robust even when the body contains
    // lines that start with `#D` (for example a foreign-code comment).
    let dict_count: usize = header_trimmed
        .rsplit_once("dict=")
        .and_then(|(_, n)| n.trim().parse().ok())
        .ok_or_else(|| "compressed header missing dict count".to_string())?;

    let mut dictionary: Vec<(String, String)> = Vec::new();
    let mut consumed = header.len();
    for line in lines.take(dict_count) {
        let rest = line
            .trim_end_matches('\n')
            .strip_prefix("#D ")
            .ok_or_else(|| format!("invalid dictionary line: {line}"))?;
        let (handle, word) = rest
            .split_once('=')
            .ok_or_else(|| format!("invalid dictionary line: {rest}"))?;
        dictionary.push((handle.to_string(), word.to_string()));
        consumed += line.len();
    }
    let mut body = compressed[consumed..].to_string();

    // Replace handles highest-index-first so e.g. §10 is restored before §1.
    for (handle, word) in dictionary.iter().rev() {
        body = body.replace(handle.as_str(), word);
    }
    Ok(body)
}

/// Strip insignificant whitespace from an agent rendering: drop blank
/// lines and per-line indentation. The agent parser trims lines and
/// ignores blanks, so this is lossless at the canonical-AST level while
/// removing newline and indentation tokens. Capsule `CODE <n>` blocks are
/// copied verbatim so foreign code (and its own indentation) is preserved.
pub fn minify_agent(agent: &str) -> String {
    let lines: Vec<&str> = agent.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("CODE ") {
            if let Ok(count) = rest.trim().parse::<usize>() {
                out.push_str(trimmed);
                out.push('\n');
                i += 1;
                for _ in 0..count {
                    if i < lines.len() {
                        out.push_str(lines[i]);
                        out.push('\n');
                        i += 1;
                    }
                }
                continue;
            }
        }
        out.push_str(trimmed);
        out.push('\n');
        i += 1;
    }
    out
}

fn is_word(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|ch| ch.is_ascii_alphabetic() || ch == '_')
        .unwrap_or(false)
}

impl CompressReport {
    pub fn ratio(&self) -> f64 {
        if self.original_chars == 0 {
            1.0
        } else {
            self.compressed_chars as f64 / self.original_chars as f64
        }
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex agent-view compression\nschema={} original_chars={} compressed_chars={} dictionary_entries={} ratio={:.3} roundtrip_ok={}\n",
            self.schema,
            self.original_chars,
            self.compressed_chars,
            self.dictionary_entries,
            self.ratio(),
            self.roundtrip_ok
        )
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"schema\": \"{}\",\n  \"original_chars\": {},\n  \"compressed_chars\": {},\n  \"dictionary_entries\": {},\n  \"ratio\": {:.4},\n  \"roundtrip_ok\": {}\n}}",
            self.schema,
            self.original_chars,
            self.compressed_chars,
            self.dictionary_entries,
            self.ratio(),
            self.roundtrip_ok
        )
    }
}
