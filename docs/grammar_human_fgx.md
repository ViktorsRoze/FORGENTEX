# Forgentex Human Grammar v1

This is the formal v1 grammar contract for `.fgx`, the human review syntax.
The current parser implements the v0 subset of this grammar.

Notation:

- Literal tokens are quoted.
- `*` means zero or more.
- `+` means one or more.
- `?` means optional.
- Newlines and indentation are significant in block structure.

## File

```text
file        ::= blank_line* item (blank_line+ item)* blank_line*
item        ::= attribute* (struct_decl | trait_decl | trait_impl | function_decl)
blank_line  ::= newline
```

## Declarations

```text
attribute   ::= "@" ident ("(" attr_args? ")")? newline
attr_args   ::= attr_arg ("," attr_arg)*
attr_arg    ::= ident | int_literal | string_literal

struct_decl ::= "struct" ident ":" newline indent field+ dedent
field       ::= attribute* ident ":" type newline

trait_decl  ::= "trait" ident ":" newline (indent trait_item+ dedent)?
trait_item  ::= trait_requirement | trait_const_requirement | trait_type_requirement
trait_requirement ::= "requires" ident "(" params? ")" return_type ("default" ident)? newline
trait_const_requirement ::= "requires" "const" ident ":" type newline
trait_type_requirement ::= "requires" "type" ident newline
trait_impl  ::= "impl" type_params? ident "for" type ":" newline (indent impl_item+ dedent)?
impl_item   ::= method_binding | const_binding | type_binding
method_binding ::= ident "=" ident newline
const_binding ::= "const" ident "=" const_expr newline
type_binding ::= "type" ident "=" type newline

function_decl ::= unsafe_marker? "fn" ident type_params? "(" params? ")" return_type? effects? ":" newline block
unsafe_marker ::= "unsafe"
type_params ::= "[" type_param ("," type_param)* "]"
type_param  ::= ident (":" trait_bound ("+" trait_bound)*)?
trait_bound ::= ident
params      ::= param ("," param)*
param       ::= ident ":" type
return_type ::= "->" type
effects     ::= "effects" "[" effect ("," effect)* "]"
effect      ::= ident ("." ident)*
```

If `return_type` is omitted, the return type is `void`.

## Statements

```text
block       ::= indent stmt+ dedent
stmt        ::= let_stmt
             | return_stmt
             | require_stmt
             | if_stmt
             | for_stmt
             | assign_stmt
             | asm_block
             | expr_stmt

let_stmt    ::= "let" ident type_annotation? "=" expr newline
type_annotation ::= ":" type

return_stmt ::= "return" expr? newline
require_stmt ::= "require" expr newline

if_stmt     ::= "if" expr ":" newline block
for_stmt    ::= "for" ident "in" expr ".." expr ":" newline block

assign_stmt ::= lvalue "=" expr newline
expr_stmt   ::= expr newline
lvalue      ::= ident | field_expr | index_expr
```

## Assembly Blocks

```text
asm_block   ::= "asm" target_arch ":" newline indent asm_stmt+ dedent
asm_stmt    ::= asm_input | asm_output | asm_clobber | asm_op
asm_input   ::= "in" register ":" ident newline
asm_output  ::= "out" register ":" ident newline
asm_clobber ::= "clobber" ident newline
asm_op      ::= "op" asm_token+ newline
```

Assembly blocks are only valid inside `unsafe fn` in v1.

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
slice_type  ::= "&[" type "]" | "&mut" "[" type "]"
pointer_type ::= "*const" type | "*mut" type
```

The `Fn` generic reserves an optional leading `forall[fn_binder, …]` marker
and an optional final `effects[...]` marker. For example,
`Fn[forall[T: Eq], T, T, bool]` is a generic equality-shaped callable.

Stage 14 attributes include `@repr(C)`, `@packed`, `@aligned(N)`,
`@endian(little|big|native)`, and `@extern(C)`.

The parser accepts unannotated `let` statements, but Stage 4 type checking
requires local `let` bindings to include an explicit `type_annotation`.

## Expressions

```text
expr        ::= binary_expr
binary_expr ::= unary_expr (binary_op unary_expr)*
unary_expr  ::= unary_op unary_expr | postfix_expr
postfix_expr ::= primary_expr postfix*
postfix     ::= "." ident | "[" expr "]"
primary_expr ::= literal
               | ident
               | call_expr
               | trait_const_expr
               | cast_expr
              | "(" expr ")"

call_expr   ::= ident "(" args? ")"
trait_const_expr ::= ident "::" ident "[" type "]"
cast_expr   ::= primitive_type "(" expr ")"
args        ::= expr ("," expr)*

binary_op   ::= "||" | "&&" | "==" | "!=" | "<" | ">" | "<=" | ">="
              | "|" | "&" | "<<" | ">>" | "+" | "-" | "*" | "/" | "%"
unary_op    ::= "-" | "!"
literal     ::= int_literal | float_literal | bool_literal | string_literal
bool_literal ::= "true" | "false"
const_expr  ::= literal | unary_op const_expr | primitive_type "(" const_expr ")"
```

## Comments and Annotations

V1 reserves comments and review annotations, but the current AST does not
preserve them yet.

```text
line_comment ::= "#" text newline
review_annotation ::= "#@" annotation_name annotation_payload? newline
```

When comment preservation is implemented, annotations attach to the next AST node
at the same indentation level unless they use an explicit node ID.
