# Forgentex AST Schema v1

The v1 canonical AST is the compiler contract used for semantic equality,
roundtripping, node IDs, and agent tooling.

Canonical AST output is available through:

```powershell
fgx ast <file> --canonical
```

The root object always contains:

```json
{
  "schema": "forgentex.ast.v1",
  "items": []
}
```

## Item Kinds

```text
Item
  Struct
  Trait
  TraitImpl
  Function
```

User-defined interfaces use two canonical item shapes:

```json
{"kind":"trait","id":"@trait.Stable","name":"Stable","methods":[],"constants":[],"associated_types":[],"annotations":[]}
{"kind":"trait_impl","id":"@impl.Stable.i32","trait":"Stable","for_type":"i32","methods":[],"constants":[],"associated_types":[],"annotations":[]}
```

Generic impls additionally emit `"type_params":["T"]` and optional bound
strings such as `"type_bounds":["T:Eq"]`. These fields are omitted for concrete
impls. Generic impls may carry the same method, constant, and associated-type
entries as concrete impls; item types retain their symbolic impl parameters.

Standalone associated-type projections reuse the canonical generic type string
shape: `Extract::Item[T]` or `Extract::Item[Vec[i32]]`. The qualified name is
the trait plus associated item and the single bracket argument is the projected
subject type, so this slice adds no AST node or schema field.

Generic function values also reuse canonical generic type strings. A leading
`forall[...]` marker records ordered binders and optional bounds, for example
`Fn[forall[T:Eq],T,T,bool]`; a final `effects[...]` marker remains optional.
The marker is part of the existing `Type::Generic` representation, so no schema
field is added.

Traits may additionally contain canonical method strings such as
`measure(Self)->i32`; impls map them to top-level functions as
`measure=measure_i32`. Associated constant contracts use `max:Self`; impl values
use `max=21`. Method bodies remain ordinary checked functions, while associated
constants are restricted to literal constant expressions. Associated type
contracts use `Item`; impl bindings use `Item=i32`, and trait contracts refer to
them as `Self::Item`.

A default method appends its pure backing function to the canonical signature,
for example `normalize(Self)->Self=identity`. Concrete impl mappings override
the default.

Struct fields:

```json
{
  "kind": "struct",
  "id": "@struct.Name",
  "name": "Name",
  "fields": [],
  "annotations": []
}
```

Function fields:

```json
{
  "kind": "function",
  "id": "@function.name",
  "name": "name",
  "type_params": ["T"],
  "type_bounds": ["T:Eq"],
  "unsafe": false,
  "return": "void",
  "effects": [],
  "target_metadata": [],
  "params": [],
  "body": [],
  "annotations": []
}
```

`effects` is the explicit capability list attached to the function declaration.
Source formatters preserve the function's effect list in declaration order.
`type_params` and `type_bounds` are emitted only for generic functions. Bounds
use canonical `Parameter:Trait+Trait` strings; slice 5a recognizes `Eq`.
`target_metadata` is derived metadata for target-aware tooling. It includes
labels such as `cpu.avx2` and `target.x86_64` from declared effects and
structured assembly blocks.

Unsafe functions also include:

```json
{
  "unsafe_region_id": "@function.name.unsafe"
}
```

## Statement Kinds

```text
let
return
require
if
for
assign
expr
asm
```

Every statement has an `id`. Nested statements derive IDs from their containing
statement path.

Examples:

```text
@function.mix_samples.stmt.0
@function.mix_samples.stmt.2.body.0
@function.mix_samples.stmt.2.body.1
```

Loops additionally expose:

```text
@function.name.stmt.N.loop
```

Assembly statements additionally expose:

```text
@function.name.stmt.N.asm
```

## Expression Kinds

```text
int
float
bool
string
var
unary
binary
call
cast
field
index
trait_const
```

Every expression has an `id` derived from the containing statement and expression
role:

```text
@function.mix_samples.stmt.0.expr
@function.mix_samples.stmt.2.cond.left
@function.mix_samples.stmt.2.body.0.expr.arg.1
```

## Types

Types are serialized in canonical agent form:

```text
bool
u8 u16 u32 u64 usize
i8 i16 i32 i64 isize
f32 f64
str
void
Name
Name[T]
Name[T,E]
vec[T,N]
mask[N]
[T;N]
&[T]
&mut[T]
*constT
*mutT
```

`Option[T]`, `Result[T,E]`, portable SIMD types such as `vec[i16,8]` and
`mask[8]`, and other named generic types serialize through the `Name[...]`
form. Arrays serialize without spaces in canonical form, for example `[u8;4]`.

## Annotations

Every declaration-level canonical node reserves an `annotations` array. V1 does
not preserve comments yet. Stage 14 uses this array for parsed layout and ABI
attributes.

Future annotations will have this shape:

```json
{
  "kind": "review",
  "name": "risk",
  "payload": "abi-change",
  "target": "@function.decode"
}
```

Parsed attributes have this shape:

```json
{
  "kind": "attribute",
  "name": "repr",
  "args": ["C"]
}
```

## Compatibility Rules

- The `schema` field changes when the canonical AST contract changes.
- Field order in canonical output is stable.
- Formatter changes must not change canonical AST unless program semantics change.
- Roundtrip equality is based on canonical AST serialization.
