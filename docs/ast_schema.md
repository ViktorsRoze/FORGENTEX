# Forgentex AST Schema v0

The first demo treats `.fgx` and `.fgai` as lossless views over one shared AST.
Node IDs are stable symbolic IDs derived from declarations.

```text
Program
  items: [Item]

Item
  StructDecl
  FunctionDecl

StructDecl
  id: @struct.Name
  name: Name
  fields: [Field]

FunctionDecl
  id: @function.name
  name: name
  unsafe_fn: bool
  params: [Param]
  ret: Type
  effects: [string]
  body: [Stmt]

Stmt
  Let
  Return
  Require
  If
  For
  Assign
  Expr
  Asm

Expr
  Literal (int, float, bool, string)
  Var
  Unary
  Binary
  Call
  Cast
  Field
  Index
```

AST equivalence means structural equality after canonical formatting and parsing.
Whitespace and source comments are not preserved in v0.
