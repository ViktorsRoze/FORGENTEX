# Parser Infrastructure v1

Stage 3 adds a parser infrastructure layer around the existing v0 parser.

The goals are:

- tokenize source with file/line/column spans.
- preserve comments, doc comments, and review annotations in source maps.
- return structured parse diagnostics with suggested repair patches.
- expose node spans for agent tools and IDE integrations.
- provide machine-readable parser output through `fgx diagnose`.

## Commands

```powershell
fgx diagnose <file> --json
```

The command supports:

- `.fgx`
- `.fgai`
- `.fgp`

For `.fgx` and `.fgai`, successful output includes canonical AST and a source
map. For `.fgp`, output includes patch diagnostics and lexer information.

## Output Shape

```json
{
  "file": "examples/hello.fgx",
  "kind": "fgx",
  "diagnostics": [],
  "tokens": [],
  "source_map": {
    "file": "examples/hello.fgx",
    "nodes": [],
    "comments": []
  },
  "canonical_ast": {
    "schema": "forgentex.ast.v1",
    "items": []
  }
}
```

## Lexer

The lexer emits tokens with spans:

```json
{
  "kind": "keyword",
  "lexeme": "fn",
  "span": {
    "file": "examples/hello.fgx",
    "start_line": 5,
    "start_col": 1,
    "end_line": 5,
    "end_col": 3
  }
}
```

Token kinds:

- `ident`
- `keyword`
- `number`
- `symbol`
- `operator`
- `newline`
- `unknown`

Unknown tokens produce `invalid_token` parse diagnostics.

## Comments and Annotations

Supported comment forms:

```text
# normal line comment
## doc comment
#@ risk @function.mix_samples unsafe-review
```

Comment kinds:

- `line`
- `doc`
- `review_annotation`

Review annotations expose:

- `name`
- `payload`
- `target` when the payload contains an `@node.id`

In Stage 3, comments and annotations are preserved in the source map. They are
not yet attached to AST nodes.

## Source Map

Source maps provide node-to-source-span relationships:

```json
{
  "node": "@function.mix_samples.stmt.2.body.1.target",
  "span": {
    "file": "examples/media/audio_mix.fgx",
    "start_line": 17,
    "start_col": 9,
    "end_line": 17,
    "end_col": 34
  }
}
```

Spans are generated for:

- structs
- fields
- functions
- parameters
- unsafe regions
- statements
- loops
- expressions
- assembly blocks

The current source map is deterministic and good enough for IDE/agent context,
but future stages may improve expression-level column precision.

## Structured Parse Diagnostics

Diagnostic shape:

```json
{
  "error": "missing_colon",
  "message": "if statement must end with ':'",
  "span": {},
  "suggested_patches": ["insert ':' at the end of the block header"]
}
```

Current parse diagnostic codes:

- `missing_colon`
- `missing_closing_brace`
- `invalid_token`
- `unknown_item`
- `unexpected_token`
- `parse_error`
- `patch_missing_target`
- `patch_missing_operand`
- `patch_unknown_operation`
- `patch_parse_error`

## Current Limitations

- The v0 parser remains line-oriented internally.
- Expression spans currently point to the containing statement line.
- Comments are preserved but not attached to AST nodes.
- `.fgp` diagnostics validate the current line-oriented patch format, not a final
  binary or structured patch wire format.

