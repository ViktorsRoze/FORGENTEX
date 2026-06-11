# Inline Tests and Executable Specs

Stage 30 adds source-level executable specs to Forgentex.

## Human Syntax

```forgentex
test "clamp high":
    expect clamp_byte(400) == 255

xfail test "safe assembly rejected":
    expect_diagnostic ASM001
```

## Agent Syntax

```text
TEST "clamp high" {
  EXPECT clamp_byte(400) == 255
}

XTEST "safe assembly rejected" {
  EXPECT_DIAGNOSTIC ASM001
}
```

The parser also accepts compact agent equality expectations:

```text
EXPECT EQ clamp_byte(400) 255
```

## CLI

```powershell
fgx test examples/smoke/inline_tests.fgx
fgx test examples/smoke/inline_tests.fgx --json
fgx test examples/smoke/inline_tests.fgx --filter "clamp"
fgx test examples/smoke/inline_tests.fgx --target x86_64-pc-windows-msvc --json
```

Package matrix tests remain available through the existing project form:

```powershell
fgx test --project examples/packages/codec_app --all-targets --json
```

## Report Schema

Inline tests emit `forgentex.test_report.v1` with:

- source path and optional target triple
- aggregate pass/fail/xfail/unexpected-pass counts
- one result per test block
- one result per expectation
- expected diagnostic code matching through the Stage 29 diagnostic catalog

`xfail` tests pass only when the expected failure is still present. If an xfail
diagnostic disappears, the result is `unexpected-pass` so humans and agents know
to review and possibly remove the stale xfail.

## Current Scope

The Stage 30 runner executes boolean `expect` expressions through the existing
interpreter and checks `expect_diagnostic` against parser/type/target diagnostics.
It does not yet attach tests to semantic patches automatically, run fixture
matrices per test block, or provide coverage metrics.
