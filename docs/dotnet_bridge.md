# .NET Bridge — Real C# Capsules (v2.23)

Business-language support. C# is compiled then run: Forgentex scaffolds a
minimal console project, builds it with `dotnet build`, and runs the produced
assembly per fixture.

## Real C# execution

```text
$ fgx capsule-run examples/dotnet/discount.fgx --csharp
Forgentex .NET capsule run
schema=forgentex.dotnet_run.v1 capsule=apply_discount status=ok
  runtime: dotnet 2.1.801 (netcoreapp2.1)
  entry: Pricing.ApplyDiscount()
  pass (10000) -> 9000 (expected 9000)
  pass (0) -> 0 (expected 0)
  pass (25000) -> 22500 (expected 22500)
  3/3 fixtures passed
```

How it runs:

1. Forgentex parses the body's **class** and first **`public static` method**
   (skipping `public static class`).
2. It writes a single `Program.cs` (a driver `Main` + the capsule class) and
   an `app.csproj` whose `TargetFramework` is **detected from the installed
   SDK** (`net{N}.0`, or `netcoreapp2.1`/`3.1` for older SDKs).
3. `dotnet build -c Release` compiles it once; the produced `app.dll` is run
   per fixture with marshaled process args; output is checked via
   `Console.WriteLine` (no JSON dependency).

## Usage

```text
fgx capsule-run <file> --csharp [--capsule <name>] [--json]
```

C# capsules also route through `fgx polyglot-run` (the v2.18 unified ABI) via
the `dotnet` backend.

## Honest scope

- Scalar typed boundary: `int`/`long`/`double`/`bool`/`string` (numbers parsed
  with invariant culture).
- The entry is the first `public static` method; multi-class object graphs are
  a follow-on.
- The target framework is detected from the highest installed SDK, so the same
  capsule builds on .NET Core 2.1 through modern .NET.
- NuGet dependencies declared on the capsule flow through the v2.3
  supply-chain audit; the demo uses only the base class library.
- Missing .NET degrades gracefully to `runtime_unavailable` (so CI needs no
  SDK); verified on .NET Core SDK 2.1.801.
- The token discipline carries over: `body_tokens` vs `boundary_tokens` — the
  agent pays for the boundary, not the body.
