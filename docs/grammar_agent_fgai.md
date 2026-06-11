# Forgentex Agent Grammar v1

This is the formal v1 grammar contract for `.fgai`, the compact agent syntax.
The current parser implements the v0 subset of this grammar.

Agent syntax is brace-delimited, compact, and canonical. It maps to the same AST
as `.fgx`.

## File

```text
file        ::= item*
item        ::= attribute* (struct_decl | function_decl)
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

function_decl ::= function_marker ident "(" params? ")" return_type? effects? "{" stmt* "}"
function_marker ::= "F" | "UF"
params      ::= param ("," param)*
param       ::= ident type
return_type ::= "->" type
effects     ::= "effects" "[" effect ("," effect)* "]"
effect      ::= ident ("." ident)*
```

`UF` means unsafe function.

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
generic_type ::= ident "[" type ("," type)* "]"
array_type  ::= "[" type ";" int_literal "]"
slice_type  ::= "&[" type "]" | "&mut[" type "]"
pointer_type ::= "*const" type | "*mut" type
```

The parser accepts unannotated `LET` statements, but Stage 4 type checking
requires local `LET` bindings to include an explicit type annotation.

## Expressions

The expression grammar is shared with `.fgx` after tokenization:

```text
expr        ::= binary_expr
binary_expr ::= unary_expr (binary_op unary_expr)*
unary_expr  ::= unary_op unary_expr | postfix_expr
postfix_expr ::= primary_expr postfix*
postfix     ::= "." ident | "[" expr "]"
primary_expr ::= literal | ident | call_expr | cast_expr | "(" expr ")"
call_expr   ::= ident "(" args? ")"
cast_expr   ::= primitive_type "(" expr ")"
args        ::= expr ("," expr)*
literal     ::= int_literal | float_literal | bool_literal | string_literal
```

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
