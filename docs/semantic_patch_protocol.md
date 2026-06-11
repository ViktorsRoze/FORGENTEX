# Forgentex Semantic Patch Protocol v0

This page documents the original v0 compatibility format. New agent proposals
should use [semantic_patch_protocol_v1.md](semantic_patch_protocol_v1.md) for
preconditions, conflict hashes, check-only previews, and JSON review output.

Semantic patches use `.fgp` files and target AST symbols instead of raw text.

```text
PATCH @function.mix_samples
INSERT_FAST_PATH target=x86_64,avx2 call=mix_samples_avx2
ADD_UNSAFE_ASM_STUB name=mix_samples_avx2 target=x86_64 effect=cpu.avx2
```

Supported v0 operations:

- `ADD_STRUCT_FIELD <name> <type>`
- `REPLACE_RETURN_TYPE <type>`
- `RENAME_FUNCTION <new_name>`
- `ADD_EFFECT <effect>` or `ADD_EFFECT effect=<effect>`
- `INSERT_FAST_PATH target=<features> call=<function>`
- `ADD_UNSAFE_ASM_STUB name=<function> target=<target> effect=<effect>`

Patch application flow:

1. Parse source into AST.
2. Apply the patch to AST nodes.
3. Re-run type checks.
4. Emit regenerated `.fgx` and `.fgai`.
5. Emit a human review report.

Review reports include public API, unsafe/assembly, and effect/capability
change summaries.

The first patch protocol is intentionally small and line-oriented. It is a demo
of semantic intent, not the final wire format.
