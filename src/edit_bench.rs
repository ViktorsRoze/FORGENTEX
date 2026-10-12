//! Edit-task token cost model (v2.11).
//!
//! **This is a cost model, not a measurement of a coding task.** No agent
//! performs the edits, nothing checks that both strategies reach the same
//! result, and every token count is the `heuristic-v1` estimate, which can
//! point the wrong way (see `tokens.rs`). It answers one narrow question: under
//! the stated assumptions, how do three editing strategies' context costs
//! compare as the number of edits grows?
//!
//! The strategies, for N edits to one file:
//!
//! - **Whole-file resend** — read the file, then re-emit all of it per edit:
//!   `file * (1 + N)`. A deliberately weak baseline: no competent agent works
//!   like this, which is why it cannot establish an advantage on its own.
//! - **Text patch** — read the file once, keep it in context, and emit a small
//!   patch per edit: `file + N * patch`. This is what a well-configured
//!   conventional agent does, and it is the baseline that matters.
//! - **Map + symbol + semantic patch** — read a one-line-per-item map, then per
//!   edit read only the target function and emit a semantic patch:
//!   `map + N * (symbol + patch)`.
//!
//! The representative patch is a real v1 semantic patch against the input —
//! a `NODE_HASH` precondition and one small operation on the first function —
//! and the model refuses to run unless it actually applies. A file with no
//! functions is rejected outright: the model is about editing a function, and
//! charging a function patch against a file that has none would report a
//! saving on work that cannot happen.

use crate::ast::{Item, Program, Stmt};
use crate::format::format_human;
use crate::tokens::estimate;

/// What the model assumes, reported with every result so a number is never
/// separated from the conditions that produced it.
pub const ASSUMPTIONS: &[&str] = &[
    "cost model, not a measured coding task: no agent performs the edits and task success is not checked",
    "token counts are heuristic-v1 estimates, which can point the wrong way on real BPE",
    "every edit targets a different function of average size",
    "the text-patch and semantic-patch strategies keep what they read in context between edits",
    "the representative patch is one small operation on one function; larger edits cost more",
    "both patch strategies are charged the same patch size, so the comparison isolates what each strategy reads",
];

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
    /// Read the file once, then a small text patch per edit — the conventional
    /// baseline a competent agent actually uses.
    pub text_patch_total: usize,
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
            Item::Pipeline(p) => out.push_str(&format!(
                "{} {} stages={} edges={}\n",
                p.id,
                p.name,
                p.stages.len(),
                p.edges.len()
            )),
            Item::Test(t) => out.push_str(&format!("{} {}\n", t.id, t.name)),
            Item::Data(section) => out.push_str(&format!(
                "{} {} {} lines={}
",
                section.id,
                section.name,
                section.kind,
                section.content.len()
            )),
            Item::Blob(section) => out.push_str(&format!(
                "{} {} {} bytes={}
",
                section.id, section.name, section.media_type, section.bytes
            )),
            Item::Note(note) => out.push_str(&format!(
                "{} on {} by {} {}
",
                note.id, note.target, note.author, note.status
            )),
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

/// A representative small semantic patch against `program`, and proof that it
/// applies.
///
/// Built from real protocol operations on the first function, with the
/// `NODE_HASH` precondition a careful agent sends: replace the function's own
/// return expression with itself when it has one (the shape of a one-expression
/// behaviour change), otherwise record a regression test against it. The patch
/// is then applied to a copy of the program, so its cost is the cost of a patch
/// that would actually be accepted — the previous fixed text used an operation
/// the protocol does not have.
fn representative_patch(program: &Program) -> Result<String, String> {
    let function = program
        .functions()
        .next()
        .ok_or_else(|| "the file has no functions".to_string())?;
    let hash = crate::patch::node_hash(program, &function.id)
        .ok_or_else(|| format!("could not hash {} for the patch precondition", function.id))?;
    let operation = function
        .body
        .iter()
        .rev()
        .find_map(|stmt| match stmt {
            Stmt::Return(Some(expr)) => Some(format!(
                "OP REPLACE_RETURN_EXPR expr={}",
                crate::format::format_expr(expr)
            )),
            _ => None,
        })
        .unwrap_or_else(|| format!("OP ADD_TEST name={}_regression", function.name));
    let patch = format!(
        "VERSION 1\nTARGET {id}\nPRECONDITION NODE_HASH {id} {hash}\n{operation}\n",
        id = function.id
    );
    crate::patch::apply_patch(program.clone(), &patch)
        .map_err(|err| format!("the representative patch does not apply: {err}"))?;
    Ok(patch)
}

/// Run the model, or explain why it does not apply to this file.
pub fn run(file: &str, program: &Program, edits: usize) -> Result<EditBench, String> {
    if program.functions().next().is_none() {
        return Err(format!(
            "edit_bench_unsupported: {file} has no functions, so a function-edit cost model does not apply; \
             an estimate here would charge no target reads and invent a function patch"
        ));
    }
    if edits == 0 {
        return Err("edit_bench_unsupported: --edits must be at least 1".to_string());
    }
    let whole_file_tokens = estimate(&format_human(program));
    let map_tokens = estimate(&map_skeleton(program));
    let symbol_tokens = average_symbol_tokens(program);
    let patch_tokens = estimate(
        &representative_patch(program).map_err(|err| format!("edit_bench_unsupported: {err}"))?,
    );

    let whole_file_total = whole_file_tokens * (1 + edits);
    let text_patch_total = whole_file_tokens + edits * patch_tokens;
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

    Ok(EditBench {
        schema: "forgentex.edit_bench.v1".to_string(),
        file: file.to_string(),
        edits,
        functions: program.functions().count(),
        whole_file_tokens,
        map_tokens,
        symbol_tokens,
        patch_tokens,
        whole_file_total,
        text_patch_total,
        patch_total,
        breakeven_edits,
    })
}

impl EditBench {
    /// Semantic-patch strategy against the weak whole-file baseline.
    pub fn ratio(&self) -> f64 {
        ratio(self.patch_total, self.whole_file_total)
    }

    /// Semantic-patch strategy against the conventional text-patch baseline —
    /// the comparison that says whether the approach beats a competent agent.
    pub fn ratio_vs_text_patch(&self) -> f64 {
        ratio(self.patch_total, self.text_patch_total)
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "Forgentex edit-task token COST MODEL (not a measured coding task)\nschema={} file={} edits={} functions={}\n  components (heuristic-v1): whole_file={} map={} symbol={} patch={}\n  whole_file_resend_total={} text_patch_total={} semantic_patch_total={}\n  semantic/whole_file={:.3} semantic/text_patch={:.3} breakeven_edits_vs_whole_file={}\n  assumptions:\n",
            self.schema,
            self.file,
            self.edits,
            self.functions,
            self.whole_file_tokens,
            self.map_tokens,
            self.symbol_tokens,
            self.patch_tokens,
            self.whole_file_total,
            self.text_patch_total,
            self.patch_total,
            self.ratio(),
            self.ratio_vs_text_patch(),
            self.breakeven_edits
                .map(|n| n.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        for assumption in ASSUMPTIONS {
            out.push_str(&format!("    - {assumption}\n"));
        }
        out
    }

    pub fn to_json(&self) -> String {
        let assumptions = ASSUMPTIONS
            .iter()
            .map(|assumption| format!("\"{}\"", crate::json::escape(assumption)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{{\n  \"schema\": \"{}\",\n  \"kind\": \"cost_model\",\n  \"tokenizer\": \"heuristic-v1\",\n  \"file\": \"{}\",\n  \"edits\": {},\n  \"functions\": {},\n  \"whole_file_tokens\": {},\n  \"map_tokens\": {},\n  \"symbol_tokens\": {},\n  \"patch_tokens\": {},\n  \"whole_file_total\": {},\n  \"text_patch_total\": {},\n  \"patch_total\": {},\n  \"patch_over_whole\": {:.4},\n  \"patch_over_text_patch\": {:.4},\n  \"breakeven_edits\": {},\n  \"assumptions\": [{}]\n}}",
            self.schema,
            crate::json::escape(&self.file),
            self.edits,
            self.functions,
            self.whole_file_tokens,
            self.map_tokens,
            self.symbol_tokens,
            self.patch_tokens,
            self.whole_file_total,
            self.text_patch_total,
            self.patch_total,
            self.ratio(),
            self.ratio_vs_text_patch(),
            self.breakeven_edits
                .map(|n| n.to_string())
                .unwrap_or_else(|| "null".to_string()),
            assumptions
        )
    }
}

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        1.0
    } else {
        part as f64 / whole as f64
    }
}
