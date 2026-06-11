# Forgentex Semantic Patch Protocol v1

Stage 7 makes `.fgp` patches conflict-aware, reviewable, and suitable for agent
edit proposals. The format is still line-oriented so the demo can stay
dependency-free, but every line has structured intent.

## Envelope

```text
VERSION 1
AUTHOR agent.demo
MOCK_SIGNATURE demo-key-1
SIGNED_BY agent-public-demo
SIGNATURE_ALGORITHM Ed25519
SIGNATURE mock-sig-demo
TARGET @function.mix_samples
PRECONDITION NODE_EXISTS @function.mix_samples
PRECONDITION NODE_HASH @function.mix_samples 5e8ad53a0d7c91af
EXPECT_DIAGNOSTIC none
REVIEW risk=low area=dsp
OP ADD_EFFECT effect=alloc
OP ADD_TEST name=mix_samples_fast_path_regression
```

Envelope fields:

- `VERSION 1` selects the v1 protocol. `PATCH_PROTOCOL v1` is also accepted.
- `AUTHOR <name>` records the patch producer.
- `MOCK_SIGNATURE <key-id>` records a non-cryptographic signature placeholder.
- `SIGNED_BY <key-id>` records the public identity used by a signed mock patch.
- `SIGNATURE_ALGORITHM Ed25519` selects the Stage 18 signed patch verifier.
- `SIGNATURE mock-sig-*` supplies the deterministic mock signature value.
- `TARGET <node-id>` sets the default target for following operations.
- `PRECONDITION ...` must pass before any operation is applied.
- `EXPECT_DIAGNOSTIC <code>` records diagnostics an agent expects after preview.
- `REVIEW key=value` attaches review metadata.
- `OP <operation>` applies one semantic operation.

The legacy v0 form remains accepted:

```text
PATCH @function.mix_samples
INSERT_FAST_PATH target=x86_64,avx2 call=mix_samples_avx2
```

## Preconditions

Supported preconditions:

- `PRECONDITION NODE_EXISTS <node-id>`
- `PRECONDITION NODE_MISSING <node-id>`
- `PRECONDITION NODE_HASH <node-id> <hash>`
- `PRECONDITION NODE_HASH <hash>` uses the current `TARGET`

Node hashes are stable demo hashes of canonical AST fragments. They are intended
for conflict detection, not security. If a hash does not match, patch application
stops before mutating the AST preview.

## Operations

Declaration operations:

- `OP ADD_STRUCT name=Pair fields=left:i32,right:i32`
- `OP ADD_FUNCTION name=answer params=x:i32 ret=i32 effects=alloc return=0`
- `OP REMOVE_FUNCTION target=@function.answer`

Field operations:

- `OP ADD_STRUCT_FIELD name=count type=u32`
- `OP REMOVE_STRUCT_FIELD name=count`

Function operations:

- `OP REPLACE_FUNCTION_BODY return=0`
- `OP REPLACE_RETURN_TYPE type=i32`
- `OP REPLACE_RETURN_EXPR expr=7`
- `OP RENAME_FUNCTION new_name=mix_samples_scalar`
- `OP MARK_UNSAFE`
- `OP CLEAR_UNSAFE`

Effect operations:

- `OP ADD_EFFECT effect=alloc`
- `OP REMOVE_EFFECT effect=alloc`

Architecture and unsafe operations:

- `OP INSERT_FAST_PATH target=x86_64,avx2 call=mix_samples_avx2`
- `OP ADD_ASM_BLOCK target=aarch64 op=demo_neon_path`
- `OP ADD_UNSAFE_ASM_STUB name=mix_samples_avx2 target=x86_64 effect=cpu.avx2`

Test-intent operation:

- `OP ADD_TEST name=mix_samples_fast_path_regression`

Operations use the current `TARGET` unless they create a top-level declaration or
explicitly provide `node=` / `target_node=`. For operations where `target=` means
architecture or CPU features, use `node=` for the AST node target.

`return=` and `expr=` consume the rest of the line, so keep them last when the
expression contains spaces.

## Review Output

`fgx patch <source> <patch> --review` prints the human summary and regenerated
source views.

`fgx patch <source> <patch> --check-only` applies the patch to an in-memory
preview, type-checks it, prints the review, and does not emit patched source.

`fgx patch <source> <patch> --emit-review-json` emits machine-readable review
JSON:

```json
{
  "schema": "forgentex.patch_review.v1",
  "target_notes": [],
  "changes": {
    "public_api": false,
    "unsafe_or_assembly": false,
    "abi_or_layout": false,
    "allocation": true,
    "filesystem": false,
    "network": false,
    "effects": true,
    "architecture": false
  }
}
```

Reports distinguish public API, unsafe or assembly, ABI or layout, allocation,
filesystem access, network access, effect or capability, and
architecture-specific changes. `operation_log` records precondition successes
and applied operations. `target_notes` records architecture-specific fast paths,
scalar fallback status, and structured assembly review-only paths.

## Safety Rules

- Missing targets fail before operation application reaches that operation.
- Failed preconditions fail before any operation is applied.
- Node hash mismatches are reported as patch conflicts.
- Mock signatures are reported as `mock-accepted`, `mock-missing`, or
  `v0-compat`; they are not cryptographic signatures.
- Signed Stage 18 envelopes require `SIGNED_BY`, `SIGNATURE_ALGORITHM Ed25519`,
  and a `SIGNATURE mock-sig-*` value before any preconditions or operations run.
  They report `signed-mock-accepted` or `signed-mock-invalid`.
- Structured assembly is represented for parse/check/review only. Native
  lowering remains outside the demo.
