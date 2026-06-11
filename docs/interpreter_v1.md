# Forgentex Interpreter v1

Stage 9 turns the interpreter into a small executable semantics layer for safe
Forgentex code. It remains intentionally separate from native code generation:
structured assembly and unsafe target-specific paths are still review-only.

## CLI

Run the original built-in demo selection:

```powershell
fgx run examples/media/audio_mix.fgx
```

Run a specific entrypoint with JSON input:

```powershell
fgx run examples/media/audio_mix.fgx --entry mix_samples --json-input examples/run/audio_mix.json
```

The output is JSON:

```json
{
  "entry": "mix_samples",
  "status": "pass",
  "return": null,
  "bindings": {
    "dst": [5500, 0, -12000, 17000]
  }
}
```

`status` is `ok` when no expectation is supplied and `pass` when the supplied
expectation matches.

## JSON Test DSL

The input file may be either an argument array or an object with named args:

```json
{
  "args": {
    "dst": [1000, -20000, 16000, 32000],
    "a": [1000, -20000, 16000, 32000],
    "b": [5000, 10000, -20000, 1000],
    "gain": 0.5
  },
  "expect": {
    "return": null,
    "bindings": {
      "dst": [5500, 0, -12000, 17000]
    }
  }
}
```

Expectation fields:

- `return`: expected return value.
- `bindings`: expected final parameter/local bindings, useful for mutable slices.

Values supported by the DSL:

- `null`, booleans, integers, floats
- arrays, used as arrays or slices
- objects as struct-like values
- `{"$struct": "Name", "field": value}` for explicit struct expectations
- `{"Some": value}`, `{"None": null}`, `{"Ok": value}`, `{"Err": value}`

## Runtime Semantics

Interpreter v1 supports the safe subset already represented by the AST:

- numeric values, booleans, and `str`
- arrays and slices supplied by the JSON harness
- mutable slice writes
- nested function calls
- propagation of mutable slice changes through safe calls
- structs, positional struct constructors, and field access
- `if`, `for`, `require`, `return`, casts, arithmetic, comparisons, calls, indexing
- `Option` and `Result` runtime values through intrinsic constructors
- deterministic Stage 17 mock I/O and networking intrinsics
- deterministic Stage 18 mock crypto, signed patch, token, and receipt intrinsics

Stage 9 uses intrinsic pattern helpers instead of adding full match syntax:

- `Some(value)`, `None()`
- `Ok(value)`, `Err(value)`
- `is_some(value)`, `is_none(value)`
- `is_ok(value)`, `is_err(value)`
- `unwrap_or(value, fallback)`

These intrinsics are a bridge until enum pattern syntax is designed in a later
language stage.

## Traps

Runtime failures are deterministic strings prefixed with `trap:`.

Current trap codes include:

- `require_failed`
- `unknown_entry`
- `unknown_function`
- `unknown_symbol`
- `unknown_field`
- `type_error`
- `call_arity`
- `input_arity`
- `missing_argument`
- `index_out_of_bounds`
- `negative_index`
- `division_by_zero`
- `integer_overflow`
- `invalid_shift`
- `invalid_json`
- `expectation_failed`
- `review_only_assembly`

These traps are part of the executable spec for safe demo behavior. They are not
yet a stable ABI.

## Limits

- Full pattern matching syntax is not implemented yet.
- JSON parsing is deliberately small and dependency-free.
- String operations are intentionally tiny; Stage 17 uses `str` mainly for
  deterministic I/O, networking, serialization, and streaming demos.
- Crypto and agent-economy runtime values are opaque deterministic strings in
  v1; no real cryptography or ledger access occurs.
- Unsafe functions and assembly remain parse/check/review-only.
