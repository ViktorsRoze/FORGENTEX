# lang.4 — Function Values, Closures, Generics (design + slice ladder)

The #1 critical gap (see `v3_progress.md`): without first-class functions and
parametric types you cannot write reusable libraries, so nothing "large and
complex" is buildable. This doc is the design record; each slice updates it.

## Design decisions

**Function type.** `Fn[P1, …, Pn, R]` — a bracket generic like `Option[T]` /
`Vec[T]`, so **no new `Type` node and no new grammar**: the last argument is
the return type, the ones before it are parameters. `Fn[i32, i32]` is
"takes i32, returns i32"; `Fn[str]` is "takes nothing, returns str".
Rationale: reuses `Type::Generic` parsing/formatting/JSON on both syntax
surfaces unchanged; an arrow form (`Fn[(i32) -> i32]`) can be layered on later
purely as parser sugar. Dual-syntax note: brackets are already the dense form,
so `.fgx` and `.fgai` render identically here.

**Function values.** A reference to a top-level `fn` by name. At runtime it is
`Value::Function(name)` — a name, not a pointer, so it is a **copy type** (no
ownership interactions) and serializes trivially. Evaluating `double` where no
local `double` exists but a program function does yields the function value.

**Indirect calls.** `f(x)` where `f` is a local of `Fn` type dispatches to the
named function. Locals shadow: the interpreter checks the environment binding
*before* program-function lookup, and the type checker checks locals first for
the same reason.

**Static checking.** A call through an `Fn[P…, R]` local checks arity and each
argument against `P…` (`fn_value_arity`, `fn_value_arg_type`) and types as
`R`. Assigning a named function to an `Fn` binding is checked structurally —
the reference types as its `Fn[P…, R]` signature (`fn_value_type`), so a
mismatch surfaces as the ordinary `type_mismatch` at the binding. Calling
through a non-`Fn` local is `not_callable`, both statically and as a runtime
trap.

**Effect contracts.** Bare `Fn[P…, R]` is explicitly pure. An effectful
callable uses `Fn[P…, R, effects[E…]]`; the optional final marker stays inside
the existing bracket-generic grammar and therefore round-trips identically in
`.fgx`, `.fgai`, and canonical AST data. Named functions and closures infer
their declared/body effects into that type. Assignment permits effect-set
widening (the found set must be a subset of the expected set), while every
indirect call conservatively contributes the full annotated set to its caller.
Unknown, duplicate, or non-final effect markers are rejected before use.

**Generic function-value binders.** Slice 6 adds an optional leading
`forall[...]` marker: `Fn[forall[T: Eq], T, T, bool]`. A generic top-level
function used as a value retains its ordered parameters and bounds in this
contract instead of erasing them. Binder names are alpha-renamed structurally
when function values cross higher-order arguments; a callable with weaker
bounds may satisfy a narrower contract, but a bounded callable cannot claim an
unbounded contract. Each indirect call infers a fresh substitution, checks all
bounds, and substitutes its parameters and result independently. Malformed,
duplicate, unknown-bound, misplaced, or uninferable binders are rejected.

**Bounded generics.** Slice 5a introduces the built-in static `Eq` interface:
`fn same[T: Eq](a: T, b: T) -> bool`. Both syntaxes use the same compact bound
form. Equality on a generic parameter requires `Eq`; direct-call inference
checks every concrete instantiation, and bounds propagate through bounded
generic wrappers. Scalars and structurally equality-capable arrays,
`Vec`/`Map`/`Option`/`Result`, structs, and enums satisfy the bound. Eligible
private helpers retain deterministic C/Zig monomorphization after their bounds
are checked.

**User-defined marker traits.** Slice 5b adds zero-method declarations and
explicit implementations: `trait Stable:` plus `impl Stable for i32:` (dense
agent forms `TR Stable` and `IM Stable i32`). A `T: Stable` call is accepted
only when every inferred concrete type has a matching impl; bounded generic
wrappers may forward the same constraint. `Eq` remains reserved and structural.
Duplicate traits/impls and impls for unknown traits are rejected. Marker items
are visible in AST, source-map, graph, project-map, and workbench data, then
erase after checking so eligible private helpers still specialize for C/Zig.

**Required method contracts.** Slice 5c lets traits declare pure signatures
with `requires method(value: Self) -> R`; impls link each requirement to an
ordinary top-level function with `method = backing_function`. The checker
substitutes `Self`, requires complete and unique mappings, and verifies exact
parameter/return types, monomorphism, and purity.

**Trait-qualified dispatch.** Slice 5d adds `Trait::method(args...)` without a
new expression node. The checker resolves the declared requirement, infers
`Self` from its arguments, enforces either a generic bound or a concrete impl,
and checks the substituted signature. The interpreter selects the validated
impl mapping from runtime argument types and invokes its backing function.
Because runtime integer values currently erase their source widths, ambiguous
matches trap deterministically.

**Native trait-call specialization.** Slice 5e extends the deterministic
monomorphization walk to `Trait::method(...)`. In ordinary concrete functions
and concrete generic clones, one exact impl rewrites to its validated backing
function before FIR lowering. Ambiguous literals, unresolved types, and absent
mappings remain unchanged and report `op:trait_dispatch` before C/Zig emission.

**Associated constants.** Slice 5f adds `requires const name: Type` contracts,
`const name = literal` impl values, and explicit `Trait::name[Type]` access. The
type selector prevents ambiguity between multiple impls and permits `Self`
substitution without runtime type guessing. Completeness, uniqueness, literal
constness, and value types are checked. The interpreter evaluates the selected
value; C/Zig preparation inlines it before FIR lowering.

**Associated types.** Slice 5g adds `requires type Item` contracts and
`type Item = Concrete` impl bindings. Required method and constant contracts
may use `Self::Item`; exact concrete impl selection substitutes the binding for
signature validation and call inference. Interpreter dispatch and C/Zig static
specialization keep using the validated backing function. Bindings are checked
for declaration, uniqueness, and completeness.

**Generic marker impls.** Slice 5h adds bounded impl parameters, including
blanket targets (`impl[T: Eq] Stable for T`) and structural targets such as
`Vec[T]`. Static matching recursively binds target patterns and enforces their
bounds. A conservative overlap check preserves coherence; unconstrained
parameters are rejected. This slice deliberately restricts generic impls to
marker traits, so native specialization remains zero-cost metadata erasure.

**Default methods.** Slice 5i lets a required method name a safe, pure top-level
default function. Concrete impls may omit the mapping or override it. Each
default is checked against the `Self`- and associated-type-substituted contract;
an unbounded generic default is specialized from the uniquely selected impl for
C/Zig. Generic defaults over mutable slices remain rejected because interpreter
copy-back needs a concrete mutable parameter type.

**Generic impl items.** Slice 5j lifts the marker-only restriction from coherent
bounded and blanket impls. Associated type bindings substitute captured target
parameters, literal constants use the substituted contract, and method mappings
may name safe pure unbounded generic backing functions checked symbolically.
Concrete calls select the unique applicable impl in the checker and interpreter;
C/Zig resolve associated items and deterministically specialize generic backing
functions. Impl bounds and conservative overlap rejection remain unchanged.

**Associated-type projections.** Slice 5k adds the standalone
`Trait::Item[Type]` form without a new AST node: it is the existing generic type
shape with a qualified constructor name and one subject argument. Projections
validate the trait, item, and applicable impl; remain symbolic for a generic
subject carrying the matching bound; and normalize recursively after concrete
substitution. Generic trait calls expose unresolved `Self::Item` occurrences as
the same public projection form, including nested occurrences. C/Zig preparation
normalizes projections in concrete functions and monomorphized signatures,
locals, casts, and lambdas before FIR contract checking.

## Honest limitations (current slice)

- Function values cannot be capsules or builtins — only top-level program
  functions. Keeps identity/serialization trivial; revisit if needed.
- C/Zig source backends lower named values and indirect calls for pure,
  binder-free `Fn` signatures over their shared scalar types. Closures remain
  explicitly blocked by `op:lambda`.
- User-defined traits support required pure methods and statically checked
  qualified dispatch, associated constants, concrete associated-type bindings,
  and standalone/generic `Trait::Item[Type]` projections, plus bounded
  generic/blanket impls carrying methods, literal constants, and associated
  types. Safe pure default and impl methods may use validated backing functions.
  Trait objects remain out of scope; native lowering is limited to uniquely
  resolvable static calls and projections.
- C/Zig keep generic, nested, aggregate, and borrowed/container `Fn`
  signatures behind the explicit `generic_type:Fn` blocker; effect metadata
  is ABI-erased only after the containing function passes its backend effect
  contract. Slice 6 remains an interpreter/static-contract feature.
- No partial application or instance-method syntax.

**Native named-function subset (`9e7fcd2`, `7333f9a`).** Monomorphic scalar
`Fn` values lower to deterministic C function-pointer typedefs and Zig
`*const fn` types. The source backends cover higher-order parameters, local
binding and reassignment, and indirect calls; generated C is compiled and run
when a host compiler is available. Effect annotations are erased from the
machine signature, so pure and effectful contracts with the same signature
share one C typedef, while runtime-only effects remain blocked by the
function-level backend contract. Contextually typed non-capturing scalar
lambdas are lifted into deterministic private functions from annotated locals
(`f140f71`) and their typed reassignments (`5a988fb`), direct statically known
higher-order arguments (`5e3c320`), or typed `Fn` returns (`c2b15c3`);
generic function values, capturing closures, and remaining lambda positions
keep their explicit FIR blockers.

## Slice ladder

| # | Slice | Status |
| --- | --- | --- |
| 1 | **Function values + indirect calls** — `Fn[P…, R]` type, named-function values, typed indirect calls, higher-order params | ✅ shipped + native scalar subset — interpreter binding/call, value-varying higher-order dispatch, dual-syntax roundtrip, and static/runtime rejection coverage remain; `9e7fcd2` adds C function-pointer typedefs and Zig `*const fn` lowering for pure monomorphic scalar named functions, including locals, reassignment, higher-order parameters, and indirect calls. `7333f9a` erases effect metadata from the same ABI and retains function-level runtime-effect blockers. Generic signatures retain explicit FIR blockers |
| 2 | **Generic functions** — `fn first[T](v: Vec[T]) -> T`; call-site inference by unification; interpreter executes directly; eligible C/Zig builds monomorphize | ✅ shipped + native subset — `type_params` on `FunctionDecl`; both parsers accept `name[T, U]`; interpreter call sites bind structurally and substitute the signature. `dcb0aef` adds deterministic C/Zig monomorphization for private generic functions reached by fully concrete direct calls: expected return/parameter types resolve literals, a sorted worklist discovers nested generic call graphs, clones receive stable type-mangled names, body annotations/casts are substituted, and optimization runs after specialization. Backend-trait builds, raw builds, source generation, and `--opt` share the same preparation. Exported, uncalled, ambiguous, function-value-only, or still backend-ineligible instantiations remain intact and fail through `generic_function` / `generic_type` FIR blockers. 7 generic tests; 19 lang.4 tests total |
| 3 | **Closures** — anonymous `|x: T| expr` capturing by value; `Value::Closure{params, body, captured}` | ✅ shipped + native non-capturing subset (`f140f71`, `5e3c320`, `c2b15c3`, `5a988fb`) — `Expr::Lambda` (typed params, expression body) parses in the shared expression grammar (`|` in prefix position is unambiguous — infix `|` stays bitwise-or; `||` in prefix is the empty parameter list; `:` added to the expression tokenizer for annotations). Types as `Fn[P…, R]` from declared params + inferred body, so bindings and higher-order arguments check through the existing Fn machinery. At creation the interpreter captures the body's **free variables by value** (`collect_var_names` minus params); calling runs the body against captured env + arguments, not the caller's scope — verified: `n = 100` after capture is invisible to the closure. Ownership hardening treats named owned free-variable roots as moves, rejects capture-after-move, permits immutable borrow captures, rejects mutable borrow captures from copyable `Fn`, and subtracts lambda parameters for lexical shadowing. Explicitly typed `FnOnce[P…, R]` locals now provide the interpreter-first affine subset: mutable borrow captures move their binding/root into a one-shot environment, and call, transfer, or task spawn consumes the callable. Persistent/general mutable environments and native closure-environment lowering remain blocked. C/Zig lift monomorphic scalar non-capturing lambdas with an annotated-local or typed-reassignment contract, a statically known direct-call parameter contract, or a typed `Fn` return contract into private C/Zig functions. Generated C behavior covers all four contexts and both optimized source paths are covered |
| 4 | **Effect-annotated function types** — closes the indirect-call effect gap | ✅ shipped (`2ac5c56`, native subset `7333f9a`) — pure `Fn[P…, R]` plus `Fn[P…, R, effects[E…]]`; named functions and closures carry effect sets, higher-order assignment checks subset compatibility, indirect calls and `task_spawn` propagate annotated effects, malformed contracts receive stable diagnostics, and `.fgx`/`.fgai` round-trip unchanged. C/Zig erase effects from eligible monomorphic scalar function-pointer signatures only after applying function-level runtime-effect blockers. 8 original slice tests; 71 tests in `tests/fn_values.rs` total |
| 5 | **Traits/interfaces** — bounded generics, user-defined interfaces, then dispatch | 🟡 slices 5a–5k shipped (`154f8e5`, `30fb96b`, `ff7e766`, `3ccfc7e`, `8ffdc0d`, `633ac99`, `fe1ffc9`, `01a81ac`, `23f4509`, `326b7c5`, `502447d`) — structural `Eq`, explicit traits/impls, required and default pure methods, checked/native-specialized dispatch, typed associated constants, associated-type bindings and projections, and coherent bounded generic/blanket impls carrying associated items. Defaults and mapped methods permit validated generic backing-function specialization. 37 new tests across 5a–5k. Remaining: dynamic dispatch |
| 6 | **Generic function values** — higher-rank `forall[...]` binders with preserved bounds and fresh per-call inference | ✅ shipped (`be14b0f`) — generic named functions type as `Fn[forall[T: Bound], P…, R]`; alpha-renamed higher-order assignment respects bound narrowing, indirect calls infer and validate each instantiation, both syntaxes round-trip, the interpreter executes mixed concrete instantiations, and C/Zig retain `generic_type:Fn`. 71 tests in `tests/fn_values.rs` |

## Soundness notes

- `Fn` values are copy: no moves, no borrows — the borrow checker needs no new
  rules for slice 1 (verified by the existing ownership tests staying green).
- The CFG sees an indirect call as a `Call` instruction with its argument uses
  collected, same as a direct call; arguments still move/borrow per their own
  types.
- The interpreter traps `not_callable` when a non-function value is called and
  `unknown_function` when an `Fn` value names a function that no longer exists
  (possible after semantic patches remove a function).
