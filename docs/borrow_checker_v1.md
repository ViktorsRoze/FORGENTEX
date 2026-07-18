# Forgentex Borrow Checker v1 Demo

Stage 27 starts closing the v1.0 ownership gap identified by
`fgx release-check`. It is still not a production-grade borrow checker, but it
makes important safety checks more conservative and reviewable. The v0.14
ownership increment adds field-sensitive move paths for owned struct values.
The v0.15 ownership increment adds conservative borrow-return provenance
summaries for direct parameter lifetimes. The v0.16 ownership increment extends
those summaries across simple conditional returns that can yield multiple borrow
parameters. These increments keep the same `forgentex.borrow_report.v1` review
schema. The v0.17 ownership increment adds lexical borrow alias locks for
indexed writes through mutable slices. The v0.18 ownership increment adds
lifetime region graph evidence to the borrow report so later stages can reason
about borrow roots, loans, and use sites. The v0.20 ownership increment adds
local reborrow parent-child chains and two-phase mutable call activation for the
same simple local-alias model. The v0.21 ownership increment adds place and
projection alias analysis for field projections and constant index projections.
The v0.22 ownership increment turns inferred helper summaries into explicit
interprocedural lifetime contracts. The v0.23 ownership increment adds a
built-in borrow-checker conformance suite that runs representative positive,
negative, and contract-summary cases through the real parser and checker. The
v0.24 release-candidate hardening increment expands that suite and makes raw
pointer parameters in safe functions produce the stable
`raw_pointer_requires_unsafe` diagnostic directly. The v0.25 ownership
increment adds loop-carried borrow liveness so aliases created before a loop
remain live across the loop back-edge when the loop body reads them. The v0.26
ownership increment joins possible borrow roots after conditional alias
reassignments.

## Command

```powershell
fgx borrow-report examples/media/audio_mix.fgx --json
fgx borrow-conformance --json
```

`fgx borrow-report` emits `forgentex.borrow_report.v1` with:

- source file
- pass/fail status
- enabled borrow-checking rules
- interprocedural lifetime contracts for returned and mutating borrow parameters
- lifetime regions with loans and use sites
- filtered ownership and borrow diagnostics

The command exits with failure when borrow diagnostics are present. Use
`fgx check` when you want all diagnostics; use `fgx borrow-report` when a review
or agent workflow needs only ownership/borrow evidence.
Use `fgx borrow-conformance` when a release or agent workflow needs the compact
built-in suite result instead of a report for one source file.

## Stage 27 Rule Additions

Borrow alias roots:

- local borrow aliases preserve their original root
- calls compare normalized roots, not just variable names
- `let alias: &mut [u8] = xs; fill(xs, alias)` now reports
  `mutable_borrow_alias`

Conditional and loop move merging:

- moves inside `if` bodies propagate to the environment after the `if`
- moves inside `for` bodies propagate to the environment after the loop
- later reads report `use_after_move`

These rules are intentionally conservative because the language does not yet
have full control-flow proofs, disjoint slice proofs, or lifetime parameters.

## v0.14 Rule Additions

Field-sensitive move paths:

- moving an owned struct field records the field path instead of moving the
  whole binding
- unmoved sibling fields remain readable and movable
- using the moved field, or using the whole struct while a field is missing,
  reports `use_after_move`
- assigning a moved field again clears that field path so the whole struct can
  be used

```forgentex
struct Packet:
    value: u8

struct Pair:
    left: Packet
    right: Packet

fn ok(pair: Pair) -> u8:
    let left: Packet = pair.left
    return pair.right.value

fn bad(pair: Pair) -> Pair:
    let left: Packet = pair.left
    return pair              # use_after_move: partially moved value
```

The implementation tracks dotted field paths such as `pair.left` and
`pair.inner.value`. It does not yet prove arbitrary place expressions,
index-based disjointness, or non-lexical borrow scopes.

## v0.15 Rule Additions

Borrow-return provenance:

- functions returning a borrow are summarized when the checker can prove the
  return value is one of the function's borrow parameters
- local borrow aliases preserve the same summary
- calls to summarized functions preserve parameter-origin lifetime evidence at
  the call site
- unknown calls, field/index returns, and complex control-flow returns remain
  unproven and still report `escaping_reference`

```forgentex
fn same(xs: &[u8]) -> &[u8]:
    return xs

fn also_same(xs: &[u8]) -> &[u8]:
    return same(xs)          # accepted in v0.15
```

This is intentionally a summary pass, not general lifetime inference. It accepts
direct and alias-preserving borrow-return helpers while rejecting cases whose
returned lifetime cannot be tied back to a caller parameter.

## v0.16 Rule Additions

Conditional borrow-return joins:

- borrow-return summaries can contain more than one possible parameter
- simple `if` and loop bodies contribute proven return paths to the summary
- callers are accepted when every possible returned argument is parameter-origin
- calls that might return a local, unknown, field, or index-origin borrow remain
  unproven and still report `escaping_reference`

```forgentex
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys

fn caller(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    return choose(flag, xs, ys)  # accepted in v0.16
```

This closes another common false positive without introducing explicit lifetime
syntax. It remains conservative: every possible returned borrow must be tied to
a caller parameter.

## v0.17 Rule Additions

Lexical borrow alias locks:

- local borrow aliases are treated as live until reassigned or until the
  function body ends
- writing through the original root while another live alias points at that root
  reports `mutable_borrow_alias`
- writing through the alias itself is accepted
- reassigning the alias to another borrow root releases the old root

```forgentex
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = 1                # mutable_borrow_alias

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = 1             # accepted
```

This is not full non-lexical borrowing. It is a simple, reviewable lexical rule
for indexed writes and local borrow aliases.

## v0.18 Rule Additions

Lifetime region graph evidence:

- `fgx borrow-report --json` now emits a `regions` section
- each borrow parameter creates a region root and a parameter loan
- local borrow aliases create additional loans tied back to the same root
- indexed reads, indexed writes, call arguments, returns, and assignments are
  recorded as use sites with stable synthesized statement nodes
- nested statement bodies preserve their own evidence without leaking local
  aliases into the parent scope

```forgentex
fn demo(xs: &mut [u8], ys: &[u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = ys[0]
```

The report records one mutable region rooted at `xs`, with loans for `xs` and
`alias`, plus an `index_write` use through `alias`. It records a separate
immutable region rooted at `ys`, with an `index_read` use.

This is evidence, not full lifetime inference. v0.18 gives future stages a
stable graph to consume; it does not yet shorten borrow lifetimes or prove
projection disjointness.

## v0.19 Rule Additions

Simple non-lexical borrow endings:

- indexed writes through a root only conflict with local aliases that are used
  in the current statement or a later statement
- writes after the alias's last use are accepted
- aliases that are never used after creation do not block later root writes
- writes still conflict when the alias is read in the same assignment or used
  after the root write
- this rule applies to simple local alias liveness; it does not yet model
  projection disjointness

```forgentex
fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = 1
    xs[0] = 2                # accepted in v0.19

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = 1                # mutable_borrow_alias
    alias[0] = 2
```

## v0.20 Rule Additions

Reborrows and two-phase mutable access:

- local mutable aliases now remember their immediate reborrow parent as well as
  their original root
- a child reborrow can write before its parent is used again
- a parent/root write or mutable call activation still conflicts while a child
  reborrow has a future use
- same-statement parent/child alias use remains rejected for indexed writes
- sibling reborrows from the same parent remain conservative and conflict while
  either sibling is live
- `&mut [T]` can be used where `&[T]` is expected, modeling a shared reborrow
  from a mutable slice
- a mutable call argument is treated as reserved while non-borrow helper
  arguments are evaluated and activated at the call boundary
- passing the same root as multiple borrow arguments to one call remains
  rejected

```forgentex
fn fill(xs: &mut [u8]) -> void:
    xs[0] = 1

fn first(xs: &[u8]) -> u8:
    return xs[0]

fn set_first(xs: &mut [u8], value: u8) -> void:
    xs[0] = value

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    child[0] = 1
    alias[0] = 2             # accepted in v0.20
    fill(alias)              # accepted after child last use

fn nested_ok(xs: &mut [u8]) -> void:
    set_first(xs, first(xs)) # mutable arg activates after helper value

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    fill(alias)              # mutable_borrow_alias: child is still live
    child[0] = 1
```

The rule is still intentionally local. It tracks simple variable reborrows, not
general lifetime inference, arbitrary place/projection disjointness, or complex
control-flow loan splitting.

## v0.21 Rule Additions

Place and projection alias analysis:

- local borrow aliases now remember a place, not only a root
- field projections such as `pair.left` and `pair.right` are treated as
  obviously disjoint
- identical field projections and parent/child places still overlap
- constant index projections are represented as places; different constant
  indexes are considered disjoint when both indexes are known
- unknown index expressions and slice/range reasoning remain conservative
- call argument alias checks and live-alias write checks use the same overlap
  rule
- `fgx borrow-report` lists `place_projection_alias_analysis`

```forgentex
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn ok(pair: Pair) -> void:
    let left: &mut [u8] = pair.left
    let right: &mut [u8] = pair.right
    left[0] = 1              # accepted in v0.21
    right[0] = 2
    fill(pair.left, pair.right)

fn bad(pair: Pair) -> void:
    let left: &mut [u8] = pair.left
    let also_left: &mut [u8] = pair.left
    left[0] = 1              # mutable_borrow_alias: pair.left overlaps
    also_left[0] = 2
```

The rule is still a local proof. It accepts clear field and constant-index
disjointness, but does not prove slice ranges, loop-derived disjoint indexes, or
cross-function lifetime contracts.

## v0.22 Rule Additions

Interprocedural lifetime contracts:

- each function gets a contract listing borrow parameters it may return
- each function gets a contract listing mutable borrow parameters it mutates
- returned-borrow contracts preserve the v0.15/v0.16 parameter provenance rules
  in an explicit report shape
- mutating-parameter contracts propagate through helper calls, so wrappers around
  mutating helpers remain visible to callers
- mutable call activation now uses the callee mutation contract; read-only
  helpers taking `&mut [T]` do not activate against a live child reborrow
- recursive mutating calls are conservative and treat mutable borrow parameters
  as mutating
- `fgx borrow-report --json` emits `lifetime_contracts`
- `fgx borrow-report` lists `interprocedural_lifetime_contracts`

```forgentex
fn view(xs: &mut [u8]) -> u8:
    return xs[0]

fn write(xs: &mut [u8]) -> void:
    xs[0] = 1

fn wrapper(xs: &mut [u8]) -> void:
    write(xs)

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    let value: u8 = view(alias) # accepted in v0.22: view does not mutate
    child[0] = value

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    wrapper(alias)              # mutable_borrow_alias: wrapper mutates xs
    child[0] = 2
```

The contract is still deliberately simple. It summarizes parameter return and
mutation relationships for the current program graph; it does not provide
general lifetime parameters, trait-based lifetime bounds, or proof of arbitrary
borrow splitting.

## v0.23 Rule Additions

Borrow checker conformance suite:

- `fgx borrow-conformance` runs a deterministic built-in suite with no input
  file required
- `fgx borrow-conformance --json` emits
  `forgentex.borrow_conformance.v1`
- cases cover positive and negative examples for moves, borrow aliases,
  reborrows, lifetime returns, raw pointer gates, unsafe boundaries, projection
  aliasing, and lifetime contract summaries
- the suite compares stable ownership/borrow diagnostic codes against expected
  results
- contract-summary cases check returned borrow parameters and transitive
  mutating mutable-borrow parameters
- `fgx release-check --json` now lists `fgx borrow-conformance --json` as
  ownership/borrow evidence
- v0.24 RC cases add raw pointer parameters, conditional lifetime escape,
  constant-index projections, and unknown-index projection overlap
- v0.25 loop cases add loop-carried alias rejection and loop-local alias
  acceptance
- v0.26 branch cases add conditional alias reassignment joins for later writes
  and call boundaries
- the safe.1 reference slice adds 510 generated whole-value/partial-field move
  programs and requires an independent availability model to agree with the
  production checker before conformance or strict release checks pass

Representative case IDs include:

- `moves.use_after_move`
- `borrows.mutable_call_alias_rejected`
- `borrows.parent_before_child_rejected`
- `lifetimes.conditional_return_ok`
- `contracts.report_wrapper_mutates`
- `branches.conditional_alias_reassign_rejected`
- `raw.raw_pointer_deref_safe_fn_rejected`
- `raw.raw_pointer_param_safe_fn_rejected`

## v0.24 Rule Additions

Release-candidate borrow hardening:

- safe functions with raw pointer parameters report
  `raw_pointer_requires_unsafe` even when the parameter is not dereferenced,
  returned, or passed to another helper
- `fgx borrow-conformance --json` now includes 20 built-in cases
- the additional cases cover conditional lifetime escape through unproven
  helper arguments, constant-index projection disjointness, unknown-index
  projection overlap, and raw pointer parameter boundaries
- `fgx release-check --json` includes `borrow_escape_hatch_conformance` in
  `hardening_checks`

## v0.25 Rule Additions

Loop-carried borrow liveness:

- local borrow aliases created before a `for` loop remain live across the loop
  back-edge when the loop body reads them
- a later indexed write through the original root in that same loop body reports
  `mutable_borrow_alias`
- borrow aliases declared inside the loop body remain local to the current
  iteration and can end before a later root write in the same iteration
- `fgx borrow-report` lists `loop_carried_borrow_liveness`
- `fgx borrow-conformance --json` now includes loop liveness cases

```forgentex
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    for i in 0 .. 2:
        alias[0] = 1
        xs[0] = 2            # mutable_borrow_alias in v0.25

fn ok(xs: &mut [u8]) -> void:
    for i in 0 .. 2:
        let alias: &mut [u8] = xs
        alias[0] = 1
        xs[0] = 2            # accepted: alias is loop-local
```

## v0.26 Rule Additions

Conditional borrow-state joins:

- aliases that exist before an `if` keep all possible borrow roots after a
  branch reassigns them
- later writes through any possible root report `mutable_borrow_alias` when the
  alias is used afterward
- call argument alias checks compare every joined possible place, not just the
  pre-branch root
- branch-local aliases still do not leak into the parent scope
- `fgx borrow-report` lists `conditional_borrow_state_join`
- `fgx borrow-conformance --json` now includes branch join cases

```forgentex
fn bad(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    xs[0] = 1                # mutable_borrow_alias in v0.26
    alias[0] = 2

fn ok(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    alias[0] = 1
    xs[0] = 2                # accepted: alias's last use is before root write
```

## safe.1 Generated Reference-Model Hardening

`src/borrow_reference.rs` is an independent model for one deliberately narrow
ownership subset. It does not inspect the Forgentex AST or invoke the production
borrow checker. It tracks definite availability for an owned `Pair` and its
`left`/`right` move paths, joins branch states by intersection, and evaluates a
known two-iteration loop twice.

The model exhaustively generates 510 equivalent `.fgx` programs:

- 258 one-, two-, and three-action straight-line programs;
- 216 one-action-per-arm branches followed by another action;
- 36 one-action loop bodies followed by another action.

Actions read or move the whole pair, its left field, or its right field. The
model predicts whether `use_after_move` must occur; the conformance runner then
parses and checks the generated source independently.

The next independent slices model alias places and liveness without using the
Forgentex AST or production checker. They exhaustively generate:

- 39 one-, two-, and three-write programs over a mutable root, parent alias,
  and child reborrow;
- 56 programs over two aliases independently projected to `left` or `right`,
  including both overlapping and provably disjoint fields;
- 16 conditional reassignments whose joined alias target set is checked against
  a direct mutable-root write before or after the alias's last use.
- 126 programs over two aliases independently projected to three nested field
  paths, covering equal paths and paths separated at either projection depth;
- 8 two-iteration loops covering same-root and disjoint-root writes in both
  alias-before-root and root-before-alias statement orders;
- 16 ordered pairs over constant-zero, constant-one, and two dynamic index
  expressions, where only different constants prove disjointness;
- 16 conditional alias reassignments inside loops, with both possible targets
  carried across the back edge and checked in both statement orders.

The model accepts child-before-parent activation after the child's last use,
uses projection-path equality for overlap, unions conditional target sets, and
unrolls bounded loop bodies twice. It predicts `mutable_borrow_alias` when an
overlapping loan remains live.

The lifetime-provenance family uses an independent two-point lattice:
`parameter` or `unproven`. Its 132 programs cover direct returns through up to
two aliases, every conditional and match-arm pair of origins, loop/while body
and fallback returns, helper calls whose contracts select the left argument,
right argument, or both, one-to-three-layer transitive wrappers, field/index
projection returns, and self/mutual recursive return cycles. Helper-local and
caller-local aliases are varied independently. A helper result is proven only
when every argument selected by its return contract has parameter origin;
unselected unproven field/index arguments do not taint the result. Returned
recursive cycles remain conservatively unproven because no finite parameter
summary can be established while the function is already being analyzed.

The first soundness-directed property slice adds 256 deterministic long-form
programs beyond the exhaustive small matrices: 64 move sequences, 64
root/parent/child sequences, 64 projected-alias sequences, and 64 lifetime
wrapper chains at depths four through eight. Four fixed seeds and stable case
IDs make every failure exactly replayable. The generators reuse only the
independent abstract models and source renderers; they do not inspect production
checker state.

The runtime campaign layer adds 128 default cases (32 per family) on top of the
fixed matrix. `--campaign-seed <u64>` accepts decimal or hexadecimal seeds and
`--campaign-cases <1..256>` controls the bounded number of cases per family.
Move, descendant-loan, and projected-alias sequences span eight through 24
operations; lifetime wrappers span depths eight through 16. Every case records
a versioned replay command. If the reference model and production checker
disagree, the runner tests smaller model-equivalent candidates and emits the
smallest reproduced source, original/minimized complexity, and replay command
in `campaign_failure_artifacts`. `--failure-dir <path>` also persists each
minimized `.fgx` reproducer beside `forgentex.borrow_regression.v1` metadata so
CI failures can be downloaded and promoted without copying source out of logs.

Four minimal regression fixtures are checked into `tests/borrow_regressions`
and replayed on every conformance and strict release run. The scheduled CI job
also runs four fixed seeds at 64 cases per family (1,024 runtime-generated
programs per weekly campaign) and retains minimized source/metadata artifacts
when any model/checker disagreement occurs.

The JSON report embeds `forgentex.borrow_reference.v1` with aggregate and
per-family generated/mismatch counts: 510 move cases, 277 alias/reborrow cases,
132 lifetime-provenance cases, 256 fixed seeded property cases, and 128 default
runtime-seeded campaign cases, plus four persisted regression cases. The default
report therefore cross-checks 1,307 reference/regression programs plus 30
curated cases. Any family mismatch fails `fgx borrow-conformance` and strict
release hardening. Remaining reference-model targets include deeper projection
and recursion combinations, slice-range alias projections, richer nested-loop
interactions, collections, closure captures, and generics. Larger and more
frequent campaigns plus promotion of newly discovered minimized failures remain.

## Stable Diagnostics

Borrow reports include these diagnostics when they appear:

- `use_after_move`
- `mutable_borrow_alias`
- `escaping_reference`
- `raw_pointer_requires_unsafe`
- `raw_pointer_dereference_requires_unsafe`
- `immutable_write`

Each diagnostic keeps the normal machine-readable fields: code, node, message,
expected/found type when useful, and suggested patch kinds. Region graph
evidence is emitted even when there are no borrow diagnostics.

## v1.0: CFG Dataflow Borrow Checker

v1.0 closes the ownership gate with `src/borrowck.rs`, the unified dataflow
form of the staged rules. It lowers each safe function to the control-flow
graph (`src/cfg.rs`) and computes:

- **moved paths** as a forward may-analysis joined at branch and loop edges,
  so a move on any path to a read is rejected for every control-flow shape,
  including re-moves across loop back edges;
- **alias places** as sets of possible borrow targets per binding with
  field/constant-index projections, joined at merge points, so conditional
  reassignment chains and reborrows of joined aliases stay tracked;
- **liveness** as real backward CFG dataflow, so "last use" and "live across
  a back edge" need no per-construct special cases;
- **reborrow ancestry** for the child-before-parent exemption, and
  **interprocedural lifetime contracts** for mutating-call activation.

The pass runs behind the staged rules as a second net: it adds a diagnostic
only when the staged checker reported nothing of the same code in the same
function, so all staged diagnostics, nodes, and messages stay stable while
previously-unreachable control-flow shapes become errors. The conformance
suite's `dataflow.*` cases pin the new coverage: loop re-moves, reborrows of
conditionally-joined aliases, nested-loop alias liveness, and the matching
precision cases that must stay accepted.

## Remaining Non-Goals

Disjoint slice *range* proofs (`xs[0..n]` vs `xs[n..]`), generic lifetime
parameters, and trait-based `Copy`/`Clone` remain outside the v1.0 language;
they become relevant only when the surface language grows those constructs.
