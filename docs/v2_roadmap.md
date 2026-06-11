# Forgentex v2 Roadmap

This roadmap captures post-v1 ideas that fit Forgentex's agent-native identity
but should not distract from the v1.0 safety release. v2 begins after the core
language, ownership model, effect system, package flow, and review reports are
stable enough to host less-trusted code deliberately.

## Main Theme: Foreign Code Capsules

v2 should let Forgentex projects contain typed, reviewable blocks of code from
other programming languages. The goal is to let agents reuse strong existing
ecosystems, libraries, and starting projects while keeping the Forgentex
boundary explicit.

Example sketch:

```fgx
foreign python version "3.13" env "cpython-3.13-linux-x86_64" effects [fs.read]:
    input value: i32
    output normalized: i32
    deps [
        "pypi:numpy==2.2.0#sha256:..."
    ]
    code """
    def normalize(value: int) -> int:
        return max(0, min(255, value))
    """
```

The exact syntax is not fixed yet. The contract is more important than the
surface syntax.

## Capsule Contract

Every foreign-language capsule must declare:

- language, language version, runtime, and target environment
- typed inputs and outputs at the Forgentex boundary
- declared effects and capabilities, such as filesystem, network, subprocess,
  GPU, package download, or environment-variable access
- dependency list pinned by version, lockfile entry, commit, checksum, or
  artifact hash
- sandbox policy and allowed host paths or network endpoints
- deterministic test inputs, expected outputs, and error shape
- review metadata for humans and agents

Foreign code is external code, not trusted Forgentex code. It should be easy to
use, but never invisible.

## Priority Foreign Language Catalog

The v2 capsule model should include the languages agents are most likely to use
when strong existing libraries or starting projects already exist. Python 3.13
is the first prototype, but the roadmap should explicitly prepare for this
catalog:

| Language | Capsule role | Extra review requirements |
| --- | --- | --- |
| Python | Data, ML, statistics, scripting, automation, and quick use of existing libraries. | Exact runtime such as Python 3.13, pinned package set, deterministic input/output fixtures. |
| JavaScript | Web tooling, JSON-heavy workflows, automation, and existing Node-style libraries. | Exact runtime, package-lock evidence, network/package-download effects separated from execution effects. |
| TypeScript | Typed web/service code, SDKs, protocol clients, and generated API clients. | Type-check command, emitted JavaScript target, pinned compiler/runtime, generated artifact hash. |
| Java | Enterprise libraries, JVM services, Android-adjacent tooling, and mature ecosystem projects. | JDK/runtime version, build-tool lock evidence, classpath/module boundary, sandboxed process policy. |
| C# | .NET libraries, Windows/server tooling, game/tooling ecosystems, and enterprise integrations. | .NET runtime version, NuGet lock evidence, assembly boundary, platform and permission review. |
| C | System libraries, ABI wrappers, device/toolchain integrations, and existing portable code. | Treat as unsafe foreign code unless fully isolated; require ABI, compiler, target triple, and memory-boundary review. |
| C++ | Performance libraries, simulation engines, graphics, robotics, and existing large projects. | Treat as high-risk native code; require compiler, standard version, ABI, allocator, exception, and ownership boundary review. |
| CUDA | GPU kernels, ML acceleration, simulation, image/media processing, and numeric workloads. | GPU target, CUDA toolkit/driver assumptions, resource limits, deterministic CPU fallback or golden fixtures. |
| Go | Network services, CLI tools, infrastructure agents, and concurrency-heavy libraries. | Go toolchain version, module lock evidence, process/network effects, goroutine/resource limits. |
| Bash | Unix automation and build/deploy glue. | Highest-risk shell capsule; require explicit command allowlist, no implicit env access, no unreviewed globbing, and strict sandbox. |
| PowerShell | Windows automation, admin tooling, and cross-platform operational scripts. | Highest-risk shell capsule; require command allowlist, execution policy, module pins, host-path limits, and strict sandbox. |
| Solidity | EVM smart contracts and crypto protocol integration. | Exact compiler version, EVM target, ABI hash, gas model, chain/network effects, and no secret-key access by default. |
| Vyper | EVM smart contracts where a smaller contract language is preferred. | Exact compiler version, EVM target, ABI hash, gas model, chain/network effects, and no secret-key access by default. |

SQL and WASM remain useful capsule targets too, but the language catalog above
captures the first famous general-purpose, shell, GPU, native, and crypto
languages that Forgentex should plan around.

Smart-contract capsules for Solidity and Vyper should be treated as crypto
artifacts, not ordinary scripts. They must declare compiler version, EVM target,
chain or local-VM target, ABI hash, storage-layout hash when available, gas
limits, deployment permissions, network effects, and wallet/key access. Wallet
or private-key access should be denied by default and require an explicit
high-risk capability.

## Planned v2 Milestones

| Label | Status | Goal | Exit criteria |
| --- | --- | --- | --- |
| `v2.0` | Planned | Foreign capsule specification | Define the capsule AST shape, manifest fields, dependency pinning rules, effect mapping, sandbox policy, and review-report schema. |
| `v2.1` | Planned | Python 3.13 capsule prototype | Parse and report Python 3.13 capsules with typed Forgentex inputs/outputs, pinned dependencies, target environment metadata, and deterministic mock execution hooks. |
| `v2.2` | Planned | Shared typed namespace across capsules | Let `share` declarations bind one Forgentex-owned typed value by name into Python and JavaScript capsules with snapshot-in/write-back-out semantics, lossless value mapping, and review of every binding. See `docs/v2_shared_namespace.md`. |
| `v2.3` | Planned | Capsule dependency and GitHub source locking | Allow capsules to reference package managers and GitHub starting projects only through pinned revisions, checksums, lockfile records, and supply-chain audit reports. |
| `v2.4` | Planned | Sandbox-backed capsule execution | Execute approved capsules in a constrained environment with explicit filesystem, network, process, environment, and resource limits. |
| `v2.5` | Planned | Web and managed runtime capsules | Extend the same model to JavaScript, TypeScript, Java, C#, and Go with toolchain versions, package locks, and typed boundaries. Includes the one-roof web-server demo: Python backend and JavaScript frontend sharing one typed schema. |
| `v2.6` | Planned | Native and GPU capsules | Add C, C++, and CUDA capsules with compiler/toolkit versions, ABI and memory-boundary review, GPU target metadata, and stable fallback fixtures. |
| `v2.7` | Planned | Shell capsule hardening | Add Bash and PowerShell only behind strict sandbox, command allowlists, host-path limits, environment review, and high-risk patch reporting. |
| `v2.8` | Planned | Smart-contract capsules | Add Solidity and Vyper capsules with compiler pins, EVM targets, ABI hashes, gas evidence, chain/network effects, and crypto-secret safety gates. |
| `v2.9` | Planned | Agent workflow integration | Let agents search, import, adapt, review, and patch foreign capsules while preserving compact context packets and human-readable risk summaries. |
| `v2.10` | Planned | Capsule compression and compact documentation register | Deterministic lossless compressed agent views for foreign capsules (dictionary headers, region hashing, round-trip conformance), plus the doc-compact controlled language for docs/reviews/commits, all measured by the token-efficiency benchmark corpus. See `docs/token_optimization_v2.md`. Sequencing is flexible: doc-compact and reading-side techniques can start before earlier v2 milestones finish. |

## Review Requirements

Capsule reports should make these facts visible:

- which language and runtime are introduced or changed
- whether the runtime version is exact, for example Python 3.13
- which packages, GitHub repositories, commits, and hashes are trusted
- which effects and permissions are requested
- which typed Forgentex functions call into the capsule
- which tests prove the boundary behavior
- whether the capsule can run on each configured target environment
- whether native backends reject, stub, or bridge the capsule
- whether the capsule belongs to a high-risk class such as shell, native,
  GPU, or smart-contract code

Semantic patch review must treat foreign code changes as high-signal review
events. A one-line change inside a foreign block should still report dependency,
effect, sandbox, target, and typed-boundary consequences.

## Non-Goals For v2 Start

- treating embedded foreign code as safe Forgentex code
- allowing unpinned package installs or floating GitHub branches
- hiding package downloads or subprocess execution behind ordinary function calls
- promising native C/Zig/LLVM backend lowering for foreign capsules
- supporting arbitrary shell snippets as the first capsule language
- letting Bash or PowerShell capsules run without command allowlists and strict
  host/environment limits
- letting C, C++, CUDA, Solidity, or Vyper capsules bypass unsafe/native,
  target, gas, crypto, or ABI review
- allowing capsules to bypass the effect, package, audit, or review systems

## First Demo Ideas

- Python 3.13 data-cleaning capsule using a pinned dependency and typed
  Forgentex inputs/outputs.
- Python 3.13 statistics capsule compared with planned `core.stats` for cases
  where an existing library is a better starting point.
- GitHub-sourced parser or simulation project imported at a pinned commit, then
  wrapped by a small Forgentex typed boundary.
- TypeScript SDK capsule that wraps a pinned generated API client.
- Java or C# library capsule that exposes one typed service boundary.
- C or C++ numeric/library wrapper with an explicit ABI and memory boundary.
- CUDA kernel capsule with deterministic CPU fallback fixtures.
- Bash and PowerShell automation capsules that demonstrate command allowlists
  and sandbox rejection paths.
- Solidity and Vyper contract capsules with pinned compiler versions, ABI
  hashes, local EVM test fixtures, and no secret-key access.
- WASM plugin capsule for deterministic sandboxed computation.
- SQL query capsule with typed rows, explicit database effects, and fixed schema
  hashes.

## Relationship To v1

v1.0 should remain focused on stable language safety. v2 foreign capsules depend
on v1-grade ownership, effects, package locking, audit reports, target
environment modeling, graph artifacts, and agent review packets. Without those
foundations, embedded foreign-language code would be powerful but too opaque.
