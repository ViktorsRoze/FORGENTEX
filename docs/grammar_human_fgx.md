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
item        ::= attribute* (struct_decl | enum_decl | trait_decl | trait_impl
              | function_decl | test_decl | capsule_decl | share_decl
              | pipeline_decl | data_decl | blob_decl | note_decl)
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

pipeline_decl ::= "pipeline" ident ":" newline indent pipeline_stage+ pipeline_edge* dedent
pipeline_stage ::= "stage" ident newline
pipeline_edge ::= "edge" ident "->" ident "via" ident newline

enum_decl   ::= "enum" ident ":" newline indent variant+ dedent
variant     ::= ident ("(" type ")")? newline

test_decl   ::= "xfail"? "test" string_literal ":" newline indent expectation+ dedent
expectation ::= "expect" expr newline | "expect_diagnostic" ident newline

share_decl  ::= "share" ident ":" type ("=" literal)? newline

capsule_decl ::= "foreign" ident ident "version" string_literal capsule_env? effects? ":"
                 newline indent capsule_item* "code" ":" newline verbatim_block dedent
capsule_env ::= "env" string_literal
capsule_item ::= "input" ident ":" type newline
              | "output" ident ":" type newline
              | "dep" string_literal newline
              | "sandbox" string_literal newline
              | "bind" ident share_mode newline
              | "mock" literal_list "->" literal_list newline
share_mode  ::= "read" | "move" | "update" | "rw"
literal_list ::= literal ("," literal)*
```

A `verbatim_block` is an indented run of lines carried exactly as authored: the
formatter returns them unchanged, because a foreign body or a data payload that
gained a space would be corrupted rather than reformatted.

Resource limits are `sandbox` rules rather than separate syntax, for example
`sandbox "limit.wall_ms 1000"`.

## Carried sections

A file may carry the data it reads, the bytes it ships, and remarks about any
node, as ordinary items. See
[the project filesystem](project_filesystem.md) for what the toolchain does
with them.

```text
data_decl   ::= "data" ident shape ":" newline
                indent section_meta "content" ":" newline verbatim_block dedent
shape       ::= ident                        // csv, json, yaml, xml, sql, md, txt, …

blob_decl   ::= "blob" ident media_type ":" newline
                indent section_meta "bytes" int_literal ("sha256" string_literal)?
                blob_payload dedent
media_type  ::= ident "/" ident              // image/png, audio/wav, …
blob_payload ::= "inline" ":" newline verbatim_block | "store" string_literal newline

note_decl   ::= "note" ident "on" string_literal ":" newline
                indent "by" string_literal ("created" string_literal)?
                ("reply-to" string_literal)? "status" string_literal
                "text" ":" newline verbatim_block dedent

section_meta ::= ("created" string_literal ("by" string_literal)?)?
                 ("modified" string_literal ("by" string_literal)?)?
```

```fgx
data sales csv:
    created "2026-09-20T09:00:00Z" by "human:viktor"
    modified "2026-09-21T08:00:00Z" by "agent:claude-opus-5"
    content:
        region,units
        north,120

blob logo image/png:
    bytes 6
    sha256 "c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2"
    inline:
        Zm9vYmFy

note n1 on "@data.sales":
    by "agent:claude-opus-5"
    created "2026-09-20T10:20:00Z"
    status "open"
    text:
        North looks doubled after the Q3 import.
```

Timestamps are fixed-width UTC (`YYYY-MM-DDTHH:MM:SSZ`) so they sort as text;
anything else is rejected with `section_timestamp_invalid`. An author is
`agent:<name>` or `human:<name>`, which is what lets a reader separate a file's
machine-written remarks from its human ones. A `status` is `open` or `resolved`.
An `inline` payload is base64 and may be at most 256 KB; larger payloads live in
the content-addressed store and must declare a `sha256`.

A pipeline declaration is an authored deterministic graph manifest. Live
execution requires its complete stage/edge set to match capsule share bindings;
its stage order breaks ties between independent ready nodes.

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
postfix     ::= "." ident | "[" expr "]" | "[" expr ".." expr "]"
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

The range postfix is half-open and currently requires both bounds. It produces
a borrowed slice; it is an expression, not an assignment target.

## Comments and Annotations

V1 reserves comments and review annotations, but the current AST does not
preserve them yet.

```text
line_comment ::= "#" text newline
review_annotation ::= "#@" annotation_name annotation_payload? newline
```

When comment preservation is implemented, annotations attach to the next AST node
at the same indentation level unless they use an explicit node ID.
