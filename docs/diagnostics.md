# Forgentex Diagnostics

Diagnostics are designed for both human review and agent repair. The JSON shape
emitted by `fgx check <file> --json` is:

```json
{
  "diagnostics": [
    {
      "error": "return_type_mismatch",
      "node": "@function.name",
      "message": "return expression does not match function return type",
      "expected": "i32",
      "found": "bool",
      "suggested_patches": ["replace_return_type", "insert_explicit_cast"]
    }
  ]
}
```

`expected` and `found` are present when a useful type comparison exists.
`suggested_patches` contains stable repair intents that a semantic patch engine
or agent can map to concrete `.fgp` operations.

## Stable Type Codes

| Code | Meaning | Common patch kinds |
| --- | --- | --- |
| `type_mismatch` | `let` annotation and expression disagree. | `insert_explicit_cast`, `replace_local_type` |
| `local_type_required` | Local `let` binding omits an explicit type. | `add_local_type_annotation` |
| `return_type_mismatch` | Return expression or bare return disagrees with function return type. | `replace_return_type`, `insert_explicit_cast`, `replace_return_expression` |
| `require_not_bool` | `require` condition is not `bool`. | `replace_require_condition` |
| `if_not_bool` | `if` condition is not `bool`. | `replace_if_condition` |
| `for_start_type` | Loop start expression is not assignable to `usize`. | `insert_explicit_cast`, `replace_for_start` |
| `for_end_type` | Loop end expression is not assignable to `usize`. | `insert_explicit_cast`, `replace_for_end` |
| `assignment_type_mismatch` | Assignment value does not match target type. | `insert_explicit_cast`, `change_target_type` |
| `immutable_write` | Indexed write targets an immutable slice or non-mutable target. | `change_parameter_to_mutable_slice` |
| `unknown_symbol` | Local, parameter, or assignment target is unknown. | `add_local_or_parameter` |
| `invalid_lvalue` | Assignment target is not writable. | `replace_assignment_target` |
| `index_not_integer` | Index expression is not assignable to `usize`. | `insert_explicit_cast`, `replace_index_expression` |
| `index_type` | Indexed base is not an array, slice, or pointer. | `replace_index_base` |
| `not_bool` | Logical not input is not `bool`. | `replace_unary_expression` |
| `negation_type_mismatch` | Numeric negation input is not numeric. | `replace_unary_expression` |
| `comparison_type_mismatch` | Comparison operands are incompatible. | `insert_explicit_cast` |
| `logical_type_mismatch` | `&&` or `||` operand is not `bool`. | `replace_logical_operand` |
| `arithmetic_type_mismatch` | Arithmetic operands are not numeric or require an explicit cast. | `replace_arithmetic_operand`, `insert_explicit_cast` |
| `constructor_arity` | Struct constructor argument count is wrong. | `fix_constructor_arguments` |
| `constructor_type_mismatch` | Struct constructor argument does not match the field type. | `insert_explicit_cast`, `replace_constructor_argument` |
| `unknown_function` | Called function is not declared. | `add_function_declaration` |
| `call_arity` | Function call argument count is wrong. | `fix_call_arguments` |
| `argument_type_mismatch` | Function call argument does not match parameter type. | `insert_explicit_cast`, `replace_call_argument` |
| `invalid_cast` | Cast source and destination are incompatible. | `replace_cast_target` |
| `unknown_struct` | Field access uses an unknown struct type. | `add_struct_declaration` |
| `unknown_field` | Field access names a missing struct field. | `add_struct_field` |
| `field_access_type` | Field access base has no fields. | `replace_field_access` |
| `integer_literal_out_of_range` | Integer literal does not fit the destination type. | `replace_literal`, `change_destination_type` |
| `unsafe_required` | Assembly appears outside an `unsafe fn`. | `mark_function_unsafe` |
| `use_after_move` | Owned value is used after being moved. | `reorder_use_before_move`, `pass_by_reference` |
| `mutable_borrow_alias` | A call or indexed write aliases a mutable borrow with another live borrow of the same root value. | `split_slice_or_copy_input`, `avoid_aliasing_mutable_argument`, `reassign_borrow_alias_before_write` |
| `escaping_reference` | Returned reference is not proven to outlive the function. | `return_owned_value`, `add_lifetime_parameter_later` |
| `raw_pointer_requires_unsafe` | Raw pointer argument or return appears in safe code. | `mark_function_unsafe`, `replace_pointer_with_slice` |
| `raw_pointer_dereference_requires_unsafe` | Raw pointer indexing/dereference appears in safe code. | `mark_function_unsafe`, `replace_pointer_with_slice` |
| `secret_leak` | Safe code passed `SecretKey[...]` to logging, serialization, copying, or output APIs. | `use_approved_crypto_api`, `mark_function_unsafe_for_secret_export` |
| `unknown_effect` | Declared effect is not part of the v1 vocabulary. | `replace_effect_name` |
| `undeclared_effect` | Function body infers an effect that is not declared at the function boundary. | `add_effect_declaration` |
| `unsafe_effect_requires_unsafe` | `memory.raw` or `cpu.asm` appears on a safe function boundary. | `mark_function_unsafe`, `replace_raw_memory_effect` |
| `unknown_target` | `--target` names a triple outside the v0 target catalog. | `run_fgx_targets` |
| `unknown_target_feature` | A `cpu.<feature>` effect is not a known target feature. | `choose_known_target_feature`, `run_fgx_targets` |
| `unknown_target_family` | A `target.<family>` effect or assembly block names an unknown architecture family. | `choose_known_target_family`, `run_fgx_targets` |
| `target_arch_mismatch` | A target-specific path is incompatible with the selected target family. | `choose_matching_target`, `guard_target_specific_path` |
| `target_feature_mismatch` | A CPU feature is not available for the selected target family. | `choose_matching_target`, `replace_arch_specific_call` |
| `target_guard_required` | A feature-specific call lacks a compatible `cpu_has_<feature>()` guard. | `wrap_call_in_cpu_feature_guard`, `add_scalar_fallback` |
| `asm_unknown_dialect` | Structured assembly names a dialect outside the v1 assembly set. | `choose_known_asm_dialect` |
| `asm_target_mismatch` | Assembly dialect is incompatible with the selected `--target` triple. | `choose_matching_target`, `move_assembly_to_matching_arch_path` |
| `asm_invalid_register` | Assembly input, output, clobber, or operand register is invalid for the dialect. | `replace_register_for_target` |
| `asm_reserved_register_write` | Assembly writes or clobbers a reserved stack, program-counter, or constant register. | `use_abi_safe_register` |
| `asm_invalid_clobber` | Assembly clobber is not valid for the dialect. | `remove_invalid_clobber` |
| `asm_unknown_input` | Assembly input binding references a local or parameter that is not visible. | `bind_input_before_asm` |
| `asm_empty_op` | Assembly operation has no mnemonic. | `remove_empty_op` |
| `asm_feature_required` | Assembly instruction requires an explicit `cpu.<feature>` effect or target default feature. | `add_cpu_feature_effect`, `replace_feature_specific_instruction` |
| `asm_feature_mismatch` | Assembly instruction requires a feature not available on the selected target family. | `choose_matching_target`, `replace_feature_specific_instruction` |
| `simd_unknown_intrinsic` | A `simd_*` call is not part of the portable intrinsic set. | `replace_simd_intrinsic` |
| `simd_intrinsic_arity` | A SIMD intrinsic has the wrong number of arguments. | `fix_simd_arguments` |
| `simd_type_mismatch` | A SIMD element type, lane count, load/store source, or transform suffix does not match. | `replace_vector_argument`, `replace_vector_element_type`, `replace_load_source`, `replace_store_destination` |
| `simd_requires_vector` | A SIMD intrinsic expected a vector argument. | `replace_with_vector_argument` |
| `simd_lane_mismatch` | Mask and vector lane counts do not match. | `replace_mask_or_vector_argument` |
| `simd_requires_mutable_slice` | SIMD store destination is not a mutable slice or mutable pointer. | `change_destination_to_mutable_slice` |
| `simd_fallback_required` | Target checking found SIMD code without scalar fallback or explicit target-only marker. | `add_scalar_fallback`, `mark_target_only` |
| `simd_equivalence_test_missing` | SIMD review found no scalar/vector equivalence test marker. | `add_simd_equivalence_test` |
| `abi_repr_required` | Struct lacks `@repr(C)` for stable C ABI layout. | `add_repr_c` |
| `abi_invalid_alignment` | `@aligned(N)` does not contain a power-of-two alignment. | `replace_alignment` |
| `abi_invalid_endian` | Endian annotation is not `little`, `big`, or `native`. | `choose_little_big_or_native` |
| `abi_endian_non_integer` | Endian annotation appears on a non-integer field. | `remove_endian_annotation` |
| `abi_invalid_padding` | `padding[N]` does not contain an integer byte count. | `replace_padding_count` |
| `abi_recursive_type` | Struct embeds itself recursively by value. | `replace_recursive_field_with_pointer` |
| `abi_unknown_type` | ABI layout references an unknown named type. | `add_struct_declaration` |
| `abi_unsupported_type` | Type has no stable ABI layout in v1. | `replace_with_repr_c_struct` |

## Parse Diagnostics

Parser and patch-source diagnostics are emitted by:

```powershell
fgx diagnose <file> --json
```

Those diagnostics use the parser infrastructure schema from
[parser_infrastructure_v1.md](parser_infrastructure_v1.md), including spans,
repair hints, preserved comments, and token/source-map output.
