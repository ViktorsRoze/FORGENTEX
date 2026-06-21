# Managed Runtime Capsules (v2.5)

v2.5 extends the capsule model from Python to the managed runtimes —
JavaScript, TypeScript, Java, C#, and Go — with their expected package
ecosystems and package-lock evidence, and reconciles each capsule's
dependencies against its language. The capsule contract, typed boundary,
sandbox, fixtures, and dependency locking from v2.0–v2.4 apply unchanged;
v2.5 adds language awareness.

## Managed runtime catalog

```text
fgx runtimes [--json]
```

`forgentex.runtime_catalog.v1` lists each managed runtime, its package
ecosystems, and the package-lock files that pin them:

| Language | Ecosystems | Lock-file evidence |
| --- | --- | --- |
| python | pypi | requirements.txt, poetry.lock, uv.lock |
| javascript | npm | package-lock.json, pnpm-lock.yaml, yarn.lock |
| typescript | npm | package-lock.json, pnpm-lock.yaml, yarn.lock |
| java | maven, gradle | pom.xml, gradle.lockfile |
| csharp | nuget | packages.lock.json |
| go | go | go.sum |

## Ecosystem reconciliation

`fgx check` reconciles each capsule's dependencies against its language. A
capsule in a catalog language must draw its package-manager dependencies
from that language's ecosystem:

- `capsule_ecosystem_mismatch` — a dependency uses a package ecosystem
  owned by a *different* language (for example a Python capsule with an
  `npm:` dependency).

Generic sources (`github:`, `git:`, `path:`, `https:`) and unknown
ecosystems are allowed for any language — cross-ecosystem GitHub starting
projects remain valid. Languages outside the catalog (Solidity, C, Bash,
…) are not opined on here; their dependency rules live in their own
milestones.

## One roof: a shared typed schema

`examples/capsule/web_session.fgx` is the one-roof web-server demo with a
shared typed *schema* rather than a scalar:

```fgx
struct Session:
    user_id: i64
    visits: i64

share session: Session

foreign backend python version "3.13" ...:
    bind session rw          # Python updates the typed session

foreign frontend typescript version "node-22" ...:
    bind session read        # TypeScript renders the same session
```

Both capsules bind one Forgentex-owned `Session` struct by name: the Python
backend mutates it (`rw`), the TypeScript frontend reads it (`read`), and
`fgx namespace` shows the single writer / single reader. The backend uses a
`pypi:` dependency and the frontend an `npm:` dependency — each matching its
language's ecosystem.

## Acceptance gates (v2.5)

- `fgx runtimes` lists python, javascript, typescript, java, csharp, and go
  with their ecosystems and lock files.
- A Python capsule with an `npm:` dependency reports
  `capsule_ecosystem_mismatch`; a `github:` dependency is allowed in any
  language.
- The web-session demo shares one `Session` struct across a Python backend
  and a TypeScript frontend, checks clean, and roundtrips.
