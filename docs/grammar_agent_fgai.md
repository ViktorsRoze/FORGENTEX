# Forgentex Agent Grammar v1

This is the formal v1 grammar contract for `.fgai`, the compact agent syntax.
The current parser implements the v0 subset of this grammar.

Agent syntax is brace-delimited, compact, and canonical. It maps to the same AST
as `.fgx`.

## File

```text
file        ::= item*
item        ::= attribute* (struct_decl | trait_decl | trait_impl | function_decl | pipeline_decl)
```

## Declarations

```text
attribute   ::= "A" ident ("(" attr_args? ")")?
attr_args   ::= attr_arg ("," attr_arg)*
attr_arg    ::= ident | int_literal | string_literal

struct_decl ::= "S" ident "{" field_list? "}"
field_list  ::= field (";" field)* ";"?
field       ::= ident type inline_attribute*
inline_attribute ::= "@" ident ("(" attr_args? ")")?

trait_decl  ::= "TR" ident "{" trait_item* "}"
trait_item  ::= "REQ" ident "(" params? ")" return_type ("DEFAULT" ident)? | "CONST" ident type | "TYPE" ident
trait_impl  ::= "IM" type_params? ident type "{" impl_item* "}"
impl_item   ::= "BIND" ident ident | "CONST" ident "=" const_expr | "TYPE" ident "=" type

function_decl ::= function_marker ident type_params? "(" params? ")" return_type? effects? "{" stmt* "}"
function_marker ::= "F" | "UF"
type_params ::= "[" type_param ("," type_param)* "]"
type_param  ::= ident (":" trait_bound ("+" trait_bound)*)?
trait_bound ::= ident
params      ::= param ("," param)*
param       ::= ident type
return_type ::= "->" type
effects     ::= "effects" "[" effect ("," effect)* "]"
effect      ::= ident ("." ident)*

pipeline_decl ::= "PIPELINE" ident pipeline_stage+ pipeline_edge* "ENDPIPELINE"
pipeline_stage ::= "STAGE" ident
pipeline_edge ::= "EDGE" ident "->" ident "VIA" ident
```

`PIPELINE` is the compact view of the same authored graph manifest as the
human `pipeline` declaration. Stage order is canonical and significant only as
the deterministic tie-breaker among independent ready nodes.

`UF` means unsafe function.

`Eq` is built in. Other bound names must have a `TR` declaration, and concrete
types satisfy them only through an `IM` declaration.

## Statements

```text
stmt        ::= let_stmt
             | return_stmt
             | require_stmt
             | if_stmt
             | for_stmt
             | set_stmt
             | asm_block
             | expr_stmt

let_stmt    ::= "LET" ident type_annotation? "=" expr
type_annotation ::= ":" type

return_stmt ::= "R" expr?
require_stmt ::= "REQ" expr
if_stmt     ::= "IF" expr "{" stmt* "}"
for_stmt    ::= "FOR" ident expr expr "{" stmt* "}"
set_stmt    ::= "SET" expr "=" expr
expr_stmt   ::= expr
```

## Assembly Blocks

```text
asm_block   ::= "ASM" target_arch "{" asm_stmt* "}"
asm_stmt    ::= asm_input | asm_output | asm_clobber | asm_op
asm_input   ::= "IN" register ident
asm_output  ::= "OUT" register ident
asm_clobber ::= "CLOBBER" ident
asm_op      ::= "OP" asm_token+
```

Assembly blocks are only valid inside `UF` functions.

## Types

```text
type        ::= primitive_type
             | named_type
             | associated_type
             | projected_type
             | generic_type
             | array_type
             | slice_type
             | pointer_type

primitive_type ::= "bool"
                 | "u8" | "u16" | "u32" | "u64" | "usize"
                 | "i8" | "i16" | "i32" | "i64" | "isize"
                 | "f32" | "f64"
                 | "str"
                 | "void"

named_type  ::= ident
associated_type ::= "Self" "::" ident
projected_type ::= ident "::" ident "[" type "]"
generic_type ::= ident "[" type ("," type)* "]"
fn_binder   ::= ident [":" ident ("+" ident)*]
array_type  ::= "[" type ";" int_literal "]"
slice_type  ::= "&[" type "]" | "&mut[" type "]"
pointer_type ::= "*const" type | "*mut" type
```

The `Fn` generic reserves an optional leading `forall[fn_binder, …]` marker
and an optional final `effects[...]` marker. For example,
`Fn[forall[T:Eq],T,T,bool]` is a generic equality-shaped callable.

The parser accepts unannotated `LET` statements, but Stage 4 type checking
requires local `LET` bindings to include an explicit type annotation.

## Expressions

The expression grammar is shared with `.fgx` after tokenization:

```text
expr        ::= binary_expr
binary_expr ::= unary_expr (binary_op unary_expr)*
unary_expr  ::= unary_op unary_expr | postfix_expr
postfix_expr ::= primary_expr postfix*
postfix     ::= "." ident | "[" expr "]" | "[" expr ".." expr "]"
primary_expr ::= literal | ident | call_expr | trait_const_expr | cast_expr | "(" expr ")"
call_expr   ::= ident "(" args? ")"
trait_const_expr ::= ident "::" ident "[" type "]"
cast_expr   ::= primitive_type "(" expr ")"
args        ::= expr ("," expr)*
literal     ::= int_literal | float_literal | bool_literal | string_literal
const_expr  ::= literal | unary_op const_expr | primitive_type "(" const_expr ")"
```

Slice ranges are half-open, require both bounds in the current grammar, and
remain expressions rather than assignment targets.

## Canonical Formatting Rules

- One declaration per block.
- Braces are always explicit.
- Statement keywords are uppercase.
- Types use compact forms such as `&mut[i16]`.
- Formatting output ends with exactly one newline.

## Comments and Annotations

V1 reserves comments and review annotations, but `.fgai` comments are not
preserved in the current AST.

```text
line_comment ::= "#" text
review_annotation ::= "#@" annotation_name annotation_payload?
```

Agents should prefer explicit node IDs in annotations once annotation
preservation lands.
