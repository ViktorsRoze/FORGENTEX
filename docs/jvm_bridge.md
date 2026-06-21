# JVM Bridge — Real Java Capsules (v2.22)

Business-language support: Java capsules run for real on the host JDK. Unlike
the interpreted runtimes, Java is **compiled then run** — Forgentex generates
a typed driver, compiles it with `javac`, and runs each fixture with `java`.

## Real Java execution

```text
$ fgx capsule-run examples/jvm/tax.fgx --java
Forgentex JVM capsule run
schema=forgentex.jvm_run.v1 capsule=cents_with_tax status=ok
  runtime: javac 21.0.11
  entry: OrderTotal.centsWithTax()
  pass (10000) -> 10825 (expected 10825)
  pass (0) -> 0 (expected 0)
  pass (50000) -> 54125 (expected 54125)
  3/3 fixtures passed
```

How it runs:

1. The body is a Java class; Forgentex parses the **class name** and the first
   **`public static` method** as the entry.
2. A typed driver is generated that parses process args to the boundary types
   (`Long.parseLong`, `Double.parseDouble`, …), calls the method, and
   `System.out.println`s the result — so no JSON dependency is needed.
3. `javac` compiles the body class and the driver together; `java` runs the
   driver once per fixture; the output is checked against the expectation.

## Usage

```text
fgx capsule-run <file> --java [--capsule <name>] [--json]
```

Java capsules also route through `fgx polyglot-run` (the v2.18 unified ABI) via
the `jvm` backend, with no change to the calling convention.

## Honest scope

- Scalar typed boundary: `int`/`long`/`double`/`boolean`/`String`. Object
  graphs and multi-class bodies are follow-ons.
- The entry is the first `public static` method; the body class may be the
  public class of the file.
- Maven/Gradle dependencies declared on the capsule flow through the v2.3
  supply-chain audit. The demo body uses only the JDK standard library, so no
  artifact resolution is needed to run it.
- Missing JDK degrades gracefully to `runtime_unavailable` (so CI needs no
  JDK); verified on OpenJDK 21.0.11.
- The token discipline carries over: the report shows `body_tokens` vs
  `boundary_tokens`, so the agent pays for the boundary, not the verbose Java
  body.
