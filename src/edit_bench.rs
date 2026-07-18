//! Edit-task token benchmark (v2.11).
//!
//! The token measurement in `tokens.rs` showed that per-file encoding is a
//! wash; the order-of-magnitude win was hypothesized to be in *not sending
//! the file* — using a project map, reading only the target symbol, and
//! emitting a small semantic patch instead of re-reading and re-emitting
//! the whole file on every edit. This module measures that hypothesis on a
//! realistic multi-edit task, with explicit assumptions, using the same
//! heuristic tokenizer.
//!
//! Model (token costs, both reads and writes count):
//!
//! - Whole-file strategy: read the file once, then re-emit the whole file
//!   on each of N edits  ->  file * (1 + N).
//! - Patch strategy: read the symbol map once, then per edit read only the
//!   target symbol and emit a small semantic patch  ->
//!   map + N * (symbol + patch).
//!
//! The components are reported so the result can be recomputed under
//! different assumptions; the headline is the ratio at N edits and the
//! break-even edit count.

use crate::ast::{Item, Program};
use crate::format::format_human;
use crate::tokens::estimate;

#[derive(Clone, Debug, PartialEq)]
pub struct EditBench {
    pub schema: String,
    pub file: String,
    pub edits: usize,
    pub functions: usize,
    pub whole_file_tokens: usize,
    pub map_tokens: usize,
    pub symbol_tokens: usize,
    pub patch_tokens: usize,
    pub whole_file_total: usize,
    pub patch_total: usize,
    pub breakeven_edits: Option<usize>,
}

/// A one-line signature skeleton per item — what a project map exposes.
fn map_skeleton(program: &Program) -> String {
    let mut out = String::new();
    for item in &program.items {
        match item {
            Item::Function(f) => {
                let params = f
                    .params
                    .iter()
                    .map(|p| format!("{} {}", p.name, p.ty.to_agent()))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "{} {}({}) -> {}\n",
                    f.id,
                    f.name,
                    params,
                    f.ret.to_agent()
                ));
            }
            Item::Struct(s) => {
                out.push_str(&format!("{} {} fields={}\n", s.id, s.name, s.fields.len()));
            }
            Item::Enum(e) => {
                out.push_str(&format!(
                    "{} {} variants={}\n",
                    e.id,
                    e.name,
                    e.variants.len()
                ));
            }
            Item::Trait(t) => out.push_str(&format!("{} {}\n", t.id, t.name)),
            Item::TraitImpl(i) => out.push_str(&format!(
                "{} {} {}\n",
                i.id,
                i.trait_name,
                i.for_type.to_agent()
            )),
            Item::Capsule(c) => {
                out.push_str(&format!("{} {} {}\n", c.id, c.name, c.language));
            }
            Item::Share(s) => out.push_str(&format!("{} {}\n", s.id, s.name)),
            Item::Test(t) => out.push_str(&format!("{} {}\n", t.id, t.name)),
        }
    }
    out
}

/// Average human-rendered token size of a single function — what an agent
/// reads when it pulls just the target symbol instead of the whole file.
fn average_symbol_tokens(program: &Program) -> usize {
    let funcs: Vec<&_> = program.functions().collect();
    if funcs.is_empty() {
        return 0;
    }
    let total: usize = funcs
        .iter()
        .map(|f| {
            let single = Program {
                items: vec![Item::Function((*f).clone())],
            };
            estimate(&format_human(&single))
        })
        .sum();
    total / funcs.len()
}

/// A representative small semantic patch that changes one behavior in the
/// target function (replace one statement). This is the targeted-edit
/// assumption; a patch that rewrites a whole function would cost about as
/// much as the symbol itself.
fn representative_patch_tokens(program: &Program) -> usize {
    let name = program
        .functions()
        .next()
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "f".to_string());
    let patch = format!("PATCH @function.{name}\nREPLACE_STATEMENT 1 = R width * gain\n");
    estimate(&patch)
}

pub fn run(file: &str, program: &Program, edits: usize) -> EditBench {
    let whole_file_tokens = estimate(&format_human(program));
    let map_tokens = estimate(&map_skeleton(program));
    let symbol_tokens = average_symbol_tokens(program);
    let patch_tokens = representative_patch_tokens(program);

    let whole_file_total = whole_file_tokens * (1 + edits);
    let patch_total = map_tokens + edits * (symbol_tokens + patch_tokens);

    // Smallest edit count at which the patch strategy is cheaper.
    let mut breakeven_edits = None;
    for n in 1..=4096 {
        let wf = whole_file_tokens * (1 + n);
        let pt = map_tokens + n * (symbol_tokens + patch_tokens);
        if pt < wf {
            breakeven_edits = Some(n);
            break;
        }
    }

    EditBench {
        schema: "forgentex.edit_bench.v1".to_string(),
        file: file.to_string(),
        edits,
        functions: program.functions().count(),
        whole_file_tokens,
        map_tokens,
        symbol_tokens,
        patch_tokens,
        whole_file_total,
        patch_total,
        breakeven_edits,
    }
}

impl EditBench {
    pub fn ratio(&self) -> f64 {
        if self.whole_file_total == 0 {
            1.0
        } else {
            self.patch_total as f64 / self.whole_file_total as f64
        }
    }

    pub fn to_text(&self) -> String {
        format!(
            "Forgentex edit-task token benchmark\nschema={} file={} edits={} functions={}\n  components: whole_file={} map={} symbol={} patch={}\n  whole_file_total={} patch_total={} patch/whole={:.3} breakeven_edits={}\n",
            self.schema,
            self.file,
            self.edits,
            self.functions,
            self.whole_file_tokens,
            self.map_tokens,
            self.symbol_tokens,
            self.patch_tokens,
            self.whole_file_total,
            self.patch_total,
            self.ratio(),
            self.breakeven_edits
                .map(|n| n.to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"schema\": \"{}\",\n  \"file\": \"{}\",\n  \"edits\": {},\n  \"functions\": {},\n  \"whole_file_tokens\": {},\n  \"map_tokens\": {},\n  \"symbol_tokens\": {},\n  \"patch_tokens\": {},\n  \"whole_file_total\": {},\n  \"patch_total\": {},\n  \"patch_over_whole\": {:.4},\n  \"breakeven_edits\": {}\n}}",
            self.schema,
            self.file,
            self.edits,
            self.functions,
            self.whole_file_tokens,
            self.map_tokens,
            self.symbol_tokens,
            self.patch_tokens,
            self.whole_file_total,
            self.patch_total,
            self.ratio(),
            self.breakeven_edits
                .map(|n| n.to_string())
                .unwrap_or_else(|| "null".to_string())
        )
    }
}
