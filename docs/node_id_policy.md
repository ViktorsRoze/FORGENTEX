# Forgentex Node ID Policy v1

Node IDs are stable symbolic handles for parser output, semantic patches,
diagnostics, review reports, project maps, and agent context retrieval.

## Goals

- Stable across formatting changes.
- Stable across unrelated edits in other declarations.
- Human-recognizable when possible.
- Dense enough for agents to reference without loading whole files.
- Deterministic without a database in v1.

## Declaration IDs

Structs:

```text
@struct.Name
```

Struct fields:

```text
@struct.Name.field.field_name
```

Functions:

```text
@function.function_name
```

Function parameters:

```text
@function.function_name.param.param_name
```

Unsafe function region:

```text
@function.function_name.unsafe
```

## Statement IDs

Top-level function body statements use source order within the function:

```text
@function.function_name.stmt.0
@function.function_name.stmt.1
@function.function_name.stmt.2
```

Nested statements derive from their parent:

```text
@function.function_name.stmt.2.then.0
@function.function_name.stmt.3.body.0
```

Loops also expose:

```text
@function.function_name.stmt.N.loop
```

Assembly statements also expose:

```text
@function.function_name.stmt.N.asm
```

## Expression IDs

Expressions derive from their statement and role:

```text
@function.name.stmt.0.expr
@function.name.stmt.1.cond
@function.name.stmt.2.target
@function.name.stmt.2.expr
```

Nested expression roles:

```text
.left
.right
.base
.index
.arg.N
```

Example:

```text
@function.mix_samples.stmt.2.body.0.expr.right.arg.0
```

## Stability Guarantees

Stable in v1:

- declaration IDs survive formatting changes.
- field IDs survive field body formatting changes.
- parameter IDs survive formatting changes.
- statement and expression IDs survive unrelated edits in other declarations.
- statement and expression IDs survive `.fgx <-> .fgai` roundtrips.

Not fully stable in v1:

- inserting or removing earlier statements in the same function may renumber later
  statement IDs.
- rewriting an expression may change nested expression IDs.

Future semantic patch protocol versions may add persistent content hashes or
author-assigned node IDs for high-stability edit regions.

## Diagnostics and Patches

Diagnostics should always include the most specific available node ID. Semantic
patches should target declaration IDs when possible and statement/expression IDs
only when a narrower edit is required.

