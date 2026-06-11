# Forgentex Borrow Checker v0

The Stage 5 borrow checker runs as part of:

```powershell
fgx check <file> --json
```

It is a second validation pass after type checking. The pass walks each function
body with an ownership environment that records local type, move state, and
reference origin.

## Environment State

Each binding tracks:

```text
name
type
moved: true | false
origin: parameter | local | unknown
```

Parameters start as available. Local bindings start as available after their
initializer is checked. Assigning to a local marks it available again.

## Move Rules

The checker treats these as copy values:

```text
bool
integer and float primitives
&[T]
&mut[T]
*constT
*mutT
void
unknown
```

Structs, arrays, and named generics are owned values. They move when consumed by
an owned destination. A later read reports `use_after_move`.

The v0 checker tracks whole bindings only. Moving a field is not modeled as a
separate path yet.

## Borrow Alias Rules

For each function call, the checker looks at arguments whose parameter types are
borrows. If two borrow arguments share the same root variable, these combinations
are rejected:

```text
&mut + &mut
&mut + & shared
& shared + &mut
```

The diagnostic is `mutable_borrow_alias`.

This is a call-local rule. Long-lived borrow scopes, explicit borrow creation,
and disjoint slice proofs are future work.

## Reference Escape Rules

Returning a slice reference is accepted only when its origin is known to be a
function parameter. Local aliases of a parameter preserve that origin.

Other origins produce `escaping_reference`.

This lets v0 accept simple identity functions while rejecting values whose
lifetime would require deeper inference.

## Raw Pointer Rules

The checker reports:

- `raw_pointer_dereference_requires_unsafe` for raw pointer indexing in safe functions.
- `raw_pointer_requires_unsafe` for raw pointer arguments and raw pointer returns in safe functions.

Unsafe functions are still review-only in v0; the checker allows raw pointer
behavior there so assembly and pointer examples can be represented and reviewed.

## Diagnostics

All Stage 5 diagnostics include:

- stable error code.
- node ID.
- message.
- expected/found type when useful.
- suggested patch kinds.

The stable codes are listed in [diagnostics.md](diagnostics.md).

## Stage 27 Successor

Stage 27 adds a v1 demo pass documented in
[borrow_checker_v1.md](borrow_checker_v1.md). The newer pass keeps the v0 rules
and adds local borrow-alias root tracking, conservative conditional/loop move
merging, field-sensitive move paths for owned struct fields, borrow-return
provenance summaries for direct parameter lifetimes, conditional borrow-return
joins for multiple parameter lifetimes, lexical borrow alias locks for indexed
writes, lifetime region graph evidence, simple non-lexical borrow endings,
local reborrow chains, two-phase mutable call activation, place/projection alias
analysis, interprocedural lifetime contracts, loop-carried borrow liveness, the
conditional borrow-state join rule, the `fgx borrow-report` review command,
and the `fgx borrow-conformance` suite.
