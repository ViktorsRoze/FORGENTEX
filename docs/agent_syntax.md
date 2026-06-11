# Forgentex Agent Syntax v0

Agent syntax uses `.fgai` files. It is compact, brace-delimited, deterministic,
and maps to the same AST as `.fgx`.

```text
S AudioBlock { samples &[i16]; rate u32; channels u8 }

F clamp_i16(x f32) -> i16 {
  IF x > 32767.0 {
    R 32767
  }
  IF x < -32768.0 {
    R -32768
  }
  R i16(x)
}
```

Declaration forms:

- `S Name { field Type; field Type }`
- `F name(param Type) -> Type { ... }`
- `F name(param Type) -> Type effects[io.read] { ... }`
- `UF name(param Type) -> Type { ... }` for unsafe functions

Statement forms:

- `LET name: Type = expr`
- `LET name = expr` is parsed for compatibility, but Stage 4 checking reports
  `local_type_required`
- `R expr`
- `R`
- `REQ expr`
- `IF expr { ... }`
- `FOR i start end { ... }`
- `SET target = expr`
- `ASM x86_64 { ... }`

Assembly block forms:

- `IN reg expr`
- `OUT reg name`
- `CLOBBER flags`
- `OP instruction operands`

Supported type forms include primitives including `str`, named structs, named
generics such as `Option[u8]` and `Result[u8,ParseError]`, fixed arrays such as
`[u8;4]`, slices, mutable slices, and pointers.

Stage 5 treats raw pointer indexing as unsafe behavior; use `UF` for pointer
dereference examples.
Stage 6 and Stage 17 require effectful functions to declare compact
capabilities such as `effects[io.read,fs.read,network.write,network.http,cpu.avx2]`.
