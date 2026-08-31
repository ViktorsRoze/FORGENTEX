# Public 0.1 Compatibility Contract

This document freezes the machine-facing compatibility surface for the
Forgentex 0.1 patch line. The executable inventory lives in
`src/public_compat.rs`; tests reject drift between that inventory, the
diagnostic catalog, the designated command outputs, and this document.

## Diagnostic codes

The following 49 catalog codes are public in 0.1:

```text
PAR001 PAR002 PAR003 PAR004 PAR005 PAR006
NAM001 NAM002 NAM003 NAM004 NAM005 NAM006
TYP001 TYP002 TYP003 TYP004 TYP005 TYP006 TYP007 TYP008 TYP009 TYP010 TYP011 TYP012 TYP013
BOR001 BOR002 BOR003 BOR004 BOR005
EFF001 EFF002 EFF003 EFF004
ASM001 ASM002 ASM003 ASM004
ABI001 ABI002 ABI003
TAR001 TAR002 TAR003 TAR004 TAR005
SIMD001
SEC001 SEC002
```

For each code, `code`, `canonical_code`, `family`, `family_title`, `rule`,
`summary`, `safety`, `suggested_patch_kinds`, `review_gates`,
`target_consequences`, and `related_diagnostics` remain present with the same
JSON types throughout 0.1.x. Wording in `summary` or an emitted `message` may
improve without changing the represented rule. New codes and additive fields
may be introduced in a patch release. A code is not reused for another rule.

Raw internal diagnostics not represented by one of these catalog codes remain
experimental. The public `diagnostics list`, `explain`, and `fix --plan`
surfaces translate known internal names into this catalog.

## Versioned JSON reports

These schema identifiers and required top-level fields are public. Required
fields remain present with their current JSON type for the whole 0.1 patch
line. Optional nested evidence may be added. Removing or renaming a required
field, changing its type, or changing its meaning requires a new schema
identifier.

| Schema | Command | Required top-level fields |
| --- | --- | --- |
| `forgentex.ast.v1` | `fgx ast --canonical` | `schema`, `items` |
| `forgentex.diagnose.v1` | `fgx diagnose --json` | `schema`, `file`, `kind`, `diagnostics`, `tokens`, `source_map`, `canonical_ast` |
| `forgentex.diagnostics.v1` | `fgx check --json` | `schema`, `diagnostics` |
| `forgentex.diagnostic_catalog.v1` | `fgx diagnostics list --json` | `schema`, `stage`, `diagnostics` |
| `forgentex.diagnostic_explain.v1` | `fgx explain --json` | `schema`, `query`, `found`, `diagnostic` |
| `forgentex.effects.v1` | `fgx effects --json` | `schema`, `functions` |
| `forgentex.test_report.v1` | `fgx test --json` | `schema`, `source`, `target`, `status`, `passed`, `failed`, `xfailed`, `unexpected_passed`, `tests` |
| `forgentex.run_result.v1` | `fgx run --json-input` | `schema`, `entry`, `status`, `return`, `bindings` |
| `forgentex.agent_context.v1` | `fgx context --json` | `schema`, `symbol`, `requested_deps`, `project`, `source`, `symbol_map`, `symbol_source`, `types`, `effects`, `tests`, `diagnostics`, `unsafe_slices`, `token_estimate` |
| `forgentex.agent_review.v1` | `fgx review --json` | `schema`, `patch`, `source`, `negotiation`, `explanation`, `token_estimate`, `patch_review` |
| `forgentex.release_check.v1` | `fgx release-check --target 0.1 --json` | `schema`, `target`, `current_track`, `decision`, `gates_pass`, `gates_partial`, `gates_planned`, `hardening_pass`, `hardening_fail`, `gates`, `hardening_checks`, `release_labels`, `notes` |

The diagnostic objects in `forgentex.diagnostics.v1` always include `error`,
`node`, `message`, and `suggested_patches`. `expected` and `found` are optional
and remain strings when present. Parse diagnostics nested in
`forgentex.diagnose.v1` always include `error`, `message`, `span`, and
`suggested_patches`; `span` may be `null`.

## Deprecation and removal

An accepted public example, diagnostic code, or required report field is not
removed during 0.1.x. A later minor release may remove it only after:

1. the old behavior is marked deprecated in a published release note;
2. the tool emits a stable diagnostic and a concrete migration action;
3. at least one intervening minor release continues to accept the old form;
4. a replacement schema identifier is published when JSON shape or meaning
   changes.

Experimental surfaces listed in `docs/public_0_1_profile.md` do not receive
this guarantee until they are explicitly added to the public profile and its
execution or compatibility inventory.
