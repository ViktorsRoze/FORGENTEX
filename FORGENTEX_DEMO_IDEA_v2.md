# Forgentex Demo Idea v2

**Working name:** Forgentex  
**Version:** v2 demo concept  
**Meaning:** *Forge* + *agentic* + a dense technical suffix suggesting a language/runtime/compiler target.  
**Core target:** A systems language designed primarily for powerful AI coding agents, with humans acting as architects, reviewers, and safety auditors.

---

## 1. Vision

Forgentex is a proposed systems programming language inspired by **Rust**, **Mojo**, **Zig**, and low-level assembly workflows.

The goal is not to create the easiest language for humans to type. The goal is to create a language where future AI agents can build large, performance-critical libraries — for example FFmpeg-like codecs, DSP engines, tensor runtimes, GPU kernels, embedded systems, and media processing stacks — in a token-efficient, compiler-verifiable way.

The language should support:

- Strong static typing.
- Ownership and borrowing.
- Explicit unsafe blocks.
- Full assembly integration.
- C ABI compatibility.
- Native compilation.
- Optional dynamic/prototyping mode later.
- Compact AI-readable representation.
- Human-readable review representation.
- Semantic patching instead of fragile raw-text edits.
- Compiler-generated review reports for humans.
- Cryptographic primitives and safe crypto APIs.
- Wallet/token/transaction-oriented workflows for an agent economy.
- Strong I/O, networking, and secure agent-to-agent communication.
- VS Code integration where humans review `.fgx` while agents primarily generate and patch `.fgai`.

The core slogan:

```text
Humans design and audit.
Agents build and optimize.
Compiler enforces truth.
```

---

## 2. Why Forgentex Should Exist

Modern programming languages are mostly designed for humans. AI coding agents use them indirectly by editing text files. This creates several problems:

- Too many tokens wasted on formatting and boilerplate.
- Agents often need too much surrounding context.
- Source-code rewrites are fragile.
- Compiler errors are written mostly for humans, not repair agents.
- Unsafe code and assembly are difficult to audit at scale.
- Large projects become hard for LLMs to navigate efficiently.

Forgentex should treat code as a structured, typed semantic database, not just text.

Humans should still be able to read the code, but the primary editing workflow should eventually be:

```text
Agent reads compact project map
Agent requests only relevant semantic nodes
Agent applies semantic patch
Compiler validates patch
Compiler emits human review summary
Human reviews dangerous/high-impact changes
```

This is the main difference between Forgentex and a normal language.

---

## 2.5 Target Workflow: VS Code Agent Workbench

A major target use-case for Forgentex is a **VS Code plugin / agent workbench**.

The intended workflow:

```text
Human opens project in VS Code
Human chats with coding agents in the sidebar
Agents read compact `.fgmap` project maps and `.fgai` agent source
Agents generate semantic `.fgp` patches
Compiler validates patches
VS Code shows human-readable `.fgx` review view
Human approves, rejects, or asks agents for another patch
```

The important design decision:

```text
The primary coding surface should be agent/LLM optimized.
The human-readable view is mainly for review, architecture, safety, and debugging.
```

Many languages already optimize for human friendliness, such as Python. Forgentex should intentionally optimize for a different future: an **agent economy** where smart agents generate large libraries, tools, codecs, runtimes, networking stacks, and infrastructure with low token waste.

If some `.fgai` code is hard for humans to read directly, that is acceptable. The language should provide review tools, summaries, risk reports, and human-mode projections instead of forcing the primary representation to be beginner-friendly.

VS Code plugin features should eventually include:

- Chat with one or more coding agents.
- View `.fgx` human review source beside `.fgai` canonical agent source.
- Inspect semantic node IDs.
- Ask an agent to explain a function, patch, unsafe block, or assembly block.
- Generate `.fgp` semantic patches instead of raw file rewrites.
- Show compiler-generated review summaries.
- Highlight unsafe code, assembly, ABI changes, allocation changes, and crypto/network permission changes.
- Display token-cost estimates for loading modules, functions, maps, and patches.
- Let humans approve only high-risk changes while agents handle routine edits.

This makes Forgentex less like a normal programming language editor and more like an **agent control cockpit**.

---

## 2.6 Agent Economy, Crypto, Networking, and I/O

Forgentex should be designed for a future where agents do not only write local code, but also communicate, authenticate, pay, receive payments, sign work, exchange capabilities, and participate in tokenized software/service economies.

Therefore the language should have strong support for:

```text
cryptography
wallets and signatures
tokens and transaction workflows
secure I/O
networking
agent-to-agent protocols
capability permissions
auditable receipts
replay protection
identity and key management
```

### 2.6.1 Safe cryptographic APIs

Cryptography should not be an afterthought or only a third-party library. Forgentex should provide safe, typed, misuse-resistant crypto APIs.

Important primitives and concepts:

- Hashing.
- Message authentication codes.
- Digital signatures.
- Public/private key types.
- Key derivation.
- Authenticated encryption.
- Secure randomness.
- Constant-time comparison.
- Secret memory types.
- Zeroization of secret data.
- Signed manifests and signed semantic patches.

Example human syntax:

```forgentex
fn sign_patch(key: SecretKey[Ed25519], patch: SemanticPatch) -> Signature[Ed25519]:
    let digest = hash.blake3(patch.canonical_bytes())
    return crypto.sign(key, digest)
```

Agent syntax:

```text
F sign_patch(key SecretKey[Ed25519], patch SemanticPatch) -> Signature[Ed25519] {
  digest = call hash.blake3 (call patch.canonical_bytes)
  R call crypto.sign key digest
}
```

The compiler should understand that `SecretKey` is sensitive and prevent accidental logging, printing, copying, serialization, or unsafe memory movement unless explicitly approved.

### 2.6.2 Token and cryptocurrency workflows

Forgentex should be friendly to cryptocurrency and token-related workflows because agents may eventually need to:

- Pay other agents for work.
- Receive payment for completed tasks.
- Lock funds in escrow.
- Sign task receipts.
- Verify proof-of-work or proof-of-service records.
- Interact with smart contracts.
- Build wallets, indexers, bridges, and payment daemons.

Forgentex should not hardcode one blockchain as the only target. It should provide abstractions for:

```text
Account
Address[Chain]
Token[Chain, Symbol]
Amount[Token]
Transaction[Chain]
SignedTransaction[Chain]
Receipt[Chain]
ContractCall[Chain]
AgentPayment
Escrow
```

This allows strong typing around assets so agents do not accidentally mix chains, tokens, decimals, or addresses.

Example:

```forgentex
fn pay_agent(
    wallet: &mut Wallet,
    agent: AgentId,
    amount: Amount[Token["USDC"]],
    task: TaskId
) -> Result[Receipt, PaymentError]
    effects: crypto.sign, network.write
```

### 2.6.3 Strong I/O and networking

Forgentex should be strong at I/O because agent systems will constantly read, write, stream, synchronize, and communicate.

Needed I/O capabilities:

- Filesystem APIs.
- Memory-mapped files.
- Async networking.
- TCP, UDP, QUIC-style transports.
- HTTP client/server support.
- WebSocket/streaming protocols.
- Binary protocol parsing.
- Structured serialization.
- Backpressure-aware streams.
- Capability-limited filesystem/network access.

Networking should be effect-typed:

```forgentex
fn send_patch(peer: AgentPeer, patch: SemanticPatch) -> Result[Ack, NetError]
    effects: network.write, crypto.sign
```

Agent syntax:

```text
F send_patch(peer AgentPeer, patch SemanticPatch) -> Result[Ack,NetError] !network.write !crypto.sign
```

### 2.6.4 Agent communication

Agent-to-agent communication should be a first-class use-case.

The language/runtime should support:

- Signed messages.
- Encrypted channels.
- Agent identity.
- Capability tokens.
- Task contracts.
- Patch proposals.
- Review requests.
- Payment requests.
- Receipts and audit logs.
- Replay protection with nonces/timestamps.
- Rate limits and permission scopes.

Example message type:

```forgentex
struct AgentMessage[T]:
    from: AgentId
    to: AgentId
    nonce: Nonce
    timestamp: Time
    capability: CapabilityToken
    payload: T
    signature: Signature[Ed25519]
```

A semantic patch in the agent economy should be signable and auditable:

```text
PATCH @function.decode_slice
AUTHOR agent:codec_optimizer_17
CAPABILITY optimize.media.h264
PAYMENT escrow:task_8821
SIGNATURE ed25519:...
```

This ties programming, identity, payment, and review together.

---

## 3. Core Design Principles

### 3.1 Agent-native, human-auditable

Forgentex should be designed for AI agents first, but humans must be able to audit important code paths.

The human syntax should be readable enough for review, but it does not need to be as beginner-friendly as Python.

The canonical `.fgai` representation should be treated as the most important representation for daily agent work. Human `.fgx` source should be a projection/review surface generated from the same AST.

### 3.2 Strong systems core

The core should be closer to Rust, Zig, and Mojo than to Python.

Core properties:

- No hidden allocation by default.
- Explicit ownership and borrowing.
- Explicit effects.
- Explicit unsafe regions.
- Explicit ABI and layout.
- Strong type checking.
- Strong compile-time diagnostics.

### 3.3 Full assembly support

Assembly should not be treated as an opaque string. It should be a typed sublanguage.

Assembly should support:

- Inline assembly.
- External assembly modules.
- Architecture-specific dialects.
- Register constraints.
- Clobber declarations.
- ABI validation.
- Naked functions for low-level runtime/kernel work.

### 3.4 Dual representation

Forgentex should have at least two source representations:

```text
.fgx    Human review syntax
.fgai   Agent/canonical syntax
```

Both must map to the same AST.

Important rule:

```text
Human syntax and agent syntax are not two different languages.
They are two lossless views of the same program.
```

### 3.5 Semantic patches

Agents should not usually rewrite full source files. They should produce small, structured patches:

```text
PATCH @function.decode_slice
ADD fast_path target=x86_64,avx2
REPLACE loop @loop.inverse_transform WITH @block.new_idct_loop
```

The compiler validates the patch before writing files.

### 3.6 Compiler-generated review reports

Human review should focus on risk.

The compiler should summarize:

- Unsafe changes.
- Assembly changes.
- ABI/layout changes.
- Allocation behavior changes.
- Public API changes.
- Performance-critical path changes.
- Tests/fuzzing/benchmarks affected.

---

## 4. Inspirations

### 4.1 Rust-inspired parts

Borrow:

- Ownership.
- Borrow checking.
- Traits/interfaces.
- Pattern matching.
- `Result`-style error handling.
- Explicit `unsafe`.
- Zero-cost abstractions.
- Strong enum types.

Avoid:

- Excessive syntax verbosity where an agent representation can be denser.
- Human-centered syntax debates that do not help agents.

### 4.2 Mojo-inspired parts

Borrow:

- Python-like readability in human mode.
- Performance-first compiler design.
- Value semantics.
- Hardware/tensor awareness.
- Path from high-level code to optimized systems code.

Extend:

- Add agent-canonical syntax.
- Add semantic patch protocol.
- Add structured assembly as a first-class sublanguage.
- Add compiler-generated human review reports.

### 4.3 Zig-inspired parts

Borrow:

- Explicit allocation.
- Simple low-level mental model.
- Strong cross-compilation culture.
- Compile-time execution ideas.
- No hidden control flow.

### 4.4 Assembly-inspired parts

Borrow:

- Direct hardware control.
- Clear registers and instructions.
- Ability to write target-specific optimized kernels.

But improve it with typed constraints and compiler validation.

---

## 5. Source Modes

### 5.1 Human review mode: `.fgx`

Example:

```forgentex
struct BBox:
    x: f32
    y: f32
    w: f32
    h: f32

fn area(box: BBox) -> f32:
    return box.w * box.h
```

This mode should be readable and reviewable.

Possible syntax style:

- Rust/Mojo-like function declarations.
- Python/Mojo-like indentation or explicit block markers.
- Strong type annotations.
- Optional shorthand in local code.
- Clear `unsafe` blocks.

Open question:

```text
Should human mode use indentation or braces?
```

For agent robustness, braces or explicit block endings may be better. Human mode can still look clean, but canonical formatting should avoid ambiguity.

### 5.2 Agent mode: `.fgai`

Example:

```text
S BBox { x f32; y f32; w f32; h f32 }

F area (box BBox) -> f32 {
  R mul (get box w) (get box h)
}
```

More compact version:

```text
S#1 BBox{x:f32,y:f32,w:f32,h:f32}
F#2 area(b:#1)->f32{R *(b.w,b.h)}
```

Agent mode should be:

- Canonical.
- Compact.
- Stable.
- Easy to parse.
- Easy to diff.
- Easy for LLMs to patch.
- Less dependent on whitespace.
- Free from style choices.

### 5.3 Semantic patch mode: `.fgp`

Example:

```text
PATCH @struct.BBox
ADD field score f32 default=1.0

PATCH @function.area
REPLACE return_type f64
REPLACE body:
  R cast.f64 (*(box.w, box.h))
```

Semantic patches are essential for token-efficient agent work.

### 5.4 Project map mode: `.fgmap`

Example:

```text
M codec.h264
  S Frame @s1
  S MacroBlock @s2
  F decode_frame @f10
  F decode_slice @f11
  F inverse_transform @f12
  ASM idct_avx2 @a4

M format.mp4
  S Atom @s20
  F parse_atom @f31
  F demux_packet @f32
```

Agents should use this to navigate large codebases without loading all source files.

---

## 6. Assembly Design

Assembly should be structured, not opaque text.

### 6.1 Human syntax example

```forgentex
unsafe fn popcount_x86(x: u64) -> u32:
    asm x86_64:
        in  rax: x
        out rax: y
        clobber flags

        popcnt rax, rax

    return u32(y)
```

### 6.2 Agent syntax example

```text
F popcount_x86(x u64) -> u32 unsafe target=x86_64 {
  ASM x86_64 {
    IN rax x
    OUT rax y
    CLOBBER flags
    OP popcnt rax rax
  }
  R cast.u32 y
}
```

### 6.3 Assembly levels

Forgentex should support multiple levels:

```text
Level 1: portable CPU/SIMD intrinsics
Level 2: target-specific intrinsics
Level 3: typed inline assembly
Level 4: naked functions
Level 5: external assembly modules
Level 6: compiler IR dialect escape hatches
```

### 6.4 Why structured assembly matters

AI agents should understand:

- What registers are used.
- What values enter and leave assembly.
- What flags/memory/registers are clobbered.
- Whether an assembly block changes ABI assumptions.
- Which targets the code supports.

This makes assembly review and automated repair much safer.

---

## 7. Type System Direction

Forgentex should start with a strong static type system.

Minimum types for demo:

```text
bool
u8, u16, u32, u64, usize
i8, i16, i32, i64, isize
f32, f64
struct
enum
array
slice
pointer
Result[T, E]
Option[T]
```

Later types:

```text
bit-sized integers: u1, u2, u5, u24
SIMD vectors: vec<i16, 8>, vec<f32, 16>
tensor types: Tensor[f32, B, C, H, W]
layout types: repr(C), packed, aligned
region/lifetime types
borrowed references
owned values
effect types
```

For media libraries, bit-sized integers and explicit layout types are very important.

Example:

```forgentex
bitstruct H264NalHeader:
    forbidden_zero: u1
    nal_ref_idc: u2
    nal_unit_type: u5
```

Agent mode:

```text
BITSTRUCT H264NalHeader { forbidden_zero u1; nal_ref_idc u2; nal_unit_type u5 }
```

---

## 8. Effects and Safety

Functions should declare or infer effects.

Examples:

```forgentex
fn parse_header(data: &[u8]) -> Header
    effects: none
```

```forgentex
fn read_packet(file: &mut File) -> Result[Packet, IOError]
    effects: io.read, alloc
```

```forgentex
unsafe fn idct_avx2(src: *const i16, dst: *mut i16)
    effects: memory.raw, cpu.avx2
```

Effect annotations help both compiler and agents.

Safety modes:

```text
safe       default, no undefined behavior allowed
trusted    low-level but still compiler-checked
unsafe     raw pointers, inline assembly, ABI control
kernel     no runtime assumptions
```

Humans should be able to filter review reports by unsafe/effect changes.

---

## 9. Demo Goal

The first demo should not try to build a real FFmpeg replacement.

The first demo should prove the core idea:

```text
One small program can be represented in human mode and agent mode,
round-tripped through the same AST,
type-checked,
patched semantically,
and compiled or interpreted.
```

Recommended first demo domain:

```text
Small media/DSP micro-library
```

Why media/DSP?

- It needs strong types.
- It benefits from SIMD/assembly.
- It has clear tests.
- It resembles the future FFmpeg-like goal.
- It is small enough for early prototyping.

Example demo functions:

- `clamp_i16`
- `mix_samples`
- `read_bits`
- `parse_h264_nal_header`
- `idct_4x4_scalar`
- `popcount_x86` using inline assembly

---

## 10. Demo Example

### 10.1 Human mode

```forgentex
struct AudioBlock:
    samples: &[i16]
    rate: u32
    channels: u8

fn clamp_i16(x: f32) -> i16:
    if x > 32767.0:
        return 32767
    if x < -32768.0:
        return -32768
    return i16(x)

fn mix_samples(dst: &mut [i16], a: &[i16], b: &[i16], gain: f32):
    require dst.len == a.len
    require dst.len == b.len

    for i in 0 .. dst.len:
        let x = f32(a[i]) * gain + f32(b[i])
        dst[i] = clamp_i16(x)
```

### 10.2 Agent mode

```text
S AudioBlock { samples &[i16]; rate u32; channels u8 }

F clamp_i16(x f32) -> i16 {
  IF >(x,32767.0) { R 32767 }
  IF <(x,-32768.0) { R -32768 }
  R cast.i16 x
}

F mix_samples(dst &mut[i16], a &[i16], b &[i16], gain f32) -> void {
  REQ eq(dst.len,a.len)
  REQ eq(dst.len,b.len)
  FOR i 0 dst.len {
    x:f32 = +( *(cast.f32 a[i], gain), cast.f32 b[i])
    dst[i] = call clamp_i16 x
  }
}
```

### 10.3 Semantic patch example

```text
PATCH @function.mix_samples
ADD fast_path target=x86_64,avx2
BODY:
  IF cpu.has(avx2) AND aligned(dst,32) AND aligned(a,32) AND aligned(b,32) {
    CALL mix_samples_avx2(dst,a,b,gain)
    R
  }
```

Compiler review report:

```text
Patch summary:
  - Added AVX2 fast path to mix_samples.
  - Public API unchanged.
  - Safe scalar fallback unchanged.
  - New unsafe function required: mix_samples_avx2.
  - New target-specific effect: cpu.avx2.
  - Tests required: scalar/avx2 equivalence.
```

---

## 11. Build Language in Rust or Something Else?

### 11.1 Recommendation

The first Forgentex compiler/tooling should be built in **Rust**.

Rust is the best default choice for the initial implementation because Forgentex itself wants Rust-like guarantees:

- Strong typing.
- Memory safety.
- Good performance.
- Good CLI/tooling ecosystem.
- Good parser/compiler library options.
- Good testing/fuzzing culture.
- Good fit for writing compilers and language tooling.
- Easier to keep the compiler core safe than with C++.

Recommended core implementation language:

```text
Rust for compiler, parser, AST, formatter, type checker, patch engine, CLI.
```

### 11.2 Where Rust is strongest

Use Rust for:

```text
lexer/parser
AST and typed IR
formatter
agent-mode serializer/deserializer
semantic patch engine
type checker
borrow/effect checker prototype
CLI tools
project map generator
diagnostics engine
test runner
fuzzing harness
```

### 11.3 Where other languages may help

#### C++

Use C++ only if deeply integrating with LLVM/MLIR internals becomes necessary.

Possible approach:

```text
Rust frontend -> emits MLIR text/bytecode -> MLIR/LLVM tools lower to native code
```

This avoids writing the entire compiler in C++ while still allowing serious backend work later.

#### Python

Use Python only for:

```text
experiments
small scripts
test generation
reference implementations
agent orchestration prototypes
```

Do not use Python for the trusted compiler core.

#### TypeScript

Use TypeScript for:

```text
web-based human review UI
agent dashboard
project map explorer
semantic patch visualizer
```

#### Zig

Zig could be useful for:

```text
runtime experiments
low-level standard library prototypes
cross-compilation experiments
comparison with Forgentex design choices
```

But Zig should not be the first compiler implementation language unless the team specifically prefers it.

#### OCaml / Haskell

These are excellent for compiler theory and type systems, but less practical for an agent-built systems toolchain aimed at broad adoption.

### 11.4 Backend recommendation

Start simple. Do not begin with MLIR/LLVM on day one.

Recommended backend path:

```text
Stage 1: Interpreter for tiny AST
Stage 2: Transpile to C
Stage 3: Transpile to Rust or Zig for experiments
Stage 4: Cranelift-style JIT or LLVM backend
Stage 5: MLIR/LLVM serious optimizing backend
Stage 6: Target-specific assembly and GPU dialects
```

Initial priority should be correctness of source modes and semantic patching, not maximum performance.

---

## 12. Repository Structure

Suggested repository:

```text
forgentex/
  README.md
  docs/
    FORGENTEX_DEMO_IDEA.md
    language_vision.md
    human_syntax.md
    agent_syntax.md
    assembly_model.md
    safety_model.md
    semantic_patch_protocol.md
    compiler_architecture.md
    roadmap.md
    vscode_agent_workbench.md
    crypto_agent_economy.md
    networking_io_model.md

  crates/
    forgentex_lexer/
    forgentex_parser/
    forgentex_ast/
    forgentex_format/
    forgentex_typecheck/
    forgentex_effects/
    forgentex_patch/
    forgentex_diag/
    forgentex_cli/
    forgentex_crypto/
    forgentex_net/
    forgentex_agent_proto/

  examples/
    hello.fgx
    hello.fgai
    audio_mix.fgx
    audio_mix.fgai
    bitreader.fgx
    asm_popcount.fgx
    signed_patch.fgx
    agent_message.fgx
    token_payment.fgx

  tests/
    roundtrip/
    parser/
    formatter/
    typecheck/
    patch/
    diagnostics/

  fuzz/
    parser_fuzz/
    roundtrip_fuzz/

  tools/
    fgx
    fgai-viewer
    fgmap
```

---

## 13. Basic Development Phases

### Phase 0 — Specification seed

Goal:

```text
Create a small but strict language spec before coding too much.
```

Tasks:

- Define human syntax v0.
- Define agent syntax v0.
- Define AST schema v0.
- Define semantic patch format v0.
- Define demo examples.
- Define non-goals.

Deliverables:

```text
docs/human_syntax.md
docs/agent_syntax.md
docs/ast_schema.md
docs/semantic_patch_protocol.md
examples/hello.fgx
examples/hello.fgai
```

Success test:

```text
A human and an agent can look at the same tiny program in two modes and agree it is the same program.
```

---

### Phase 1 — Parser and AST roundtrip

Goal:

```text
Parse .fgx and .fgai into the same AST.
```

Support only:

- Functions.
- Structs.
- Primitive types.
- Let bindings.
- If statements.
- For loops.
- Return.
- Basic arithmetic.

Deliverables:

```text
fgx parse examples/hello.fgx --emit-ast
fgx parse examples/hello.fgai --emit-ast
fgx roundtrip examples/hello.fgx --to fgai --back fgx
```

Success test:

```text
.fgx -> AST -> .fgai -> AST produces equivalent AST.
.fgai -> AST -> .fgx -> AST produces equivalent AST.
```

---

### Phase 2 — Canonical formatter

Goal:

```text
Every valid program has one canonical human format and one canonical agent format.
```

Tasks:

- Implement `.fgx` formatter.
- Implement `.fgai` formatter.
- Make formatting deterministic.
- Add snapshot tests.

Success test:

```text
format(format(code)) == format(code)
```

---

### Phase 3 — Type checker v0

Goal:

```text
Catch basic type errors and produce machine-readable diagnostics.
```

Support:

- Primitive types.
- Function signatures.
- Struct field access.
- Numeric casts.
- Slice indexing.
- Basic `Result`/`Option` later.

Diagnostic example:

```json
{
  "error": "type_mismatch",
  "node": "@stmt_42",
  "expected": "i16",
  "found": "f32",
  "suggested_patches": [
    "insert cast.i16",
    "change destination type to f32"
  ]
}
```

Success test:

```text
The compiler gives errors useful enough for an AI agent to repair code automatically.
```

---

### Phase 4 — Semantic patch engine

Goal:

```text
Agents can modify programs without rewriting full files.
```

Support patch operations:

- Add function.
- Add struct field.
- Replace function body.
- Replace expression.
- Rename symbol.
- Add unsafe block.
- Add target-specific fast path.

Deliverables:

```text
fgx patch examples/audio_mix.fgx patches/add_avx2_fastpath.fgp
fgx review last_patch
```

Success test:

```text
Patch applies to AST, type checks, regenerates .fgx and .fgai, and emits review summary.
```

---

### Phase 5 — Interpreter or simple transpiler

Goal:

```text
Run small Forgentex programs.
```

Simplest options:

1. AST interpreter.
2. Transpile to C.
3. Transpile to Rust.

Recommended:

```text
Start with AST interpreter for correctness.
Then add C transpiler for native demo.
```

Success test:

```text
fgx run examples/audio_mix.fgx
```

---

### Phase 6 — Assembly model prototype

Goal:

```text
Represent typed assembly blocks in AST and agent mode.
```

Start with one target:

```text
x86_64
```

Support:

- `asm x86_64` blocks.
- Inputs.
- Outputs.
- Clobbers.
- Basic instruction list.
- Unsafe requirement.

Demo:

```forgentex
unsafe fn popcount_x86(x: u64) -> u32:
    asm x86_64:
        in  rax: x
        out rax: y
        clobber flags
        popcnt rax, rax
    return u32(y)
```

Success test:

```text
Compiler refuses assembly outside unsafe context.
Compiler can display assembly block in structured agent form.
```

---

### Phase 7 — Media/DSP demo library

Goal:

```text
Build a tiny FFmpeg-like micro-domain.
```

Implement:

- Audio sample mixing.
- BitReader.
- Small H264 NAL header parser.
- Scalar IDCT toy function.
- Optional x86_64 optimized function stub.

Deliverables:

```text
examples/media/audio_mix.fgx
examples/media/bitreader.fgx
examples/media/h264_nal.fgx
examples/media/idct4x4.fgx
```

Success test:

```text
Agents can add or optimize one media function through semantic patching.
```

---

### Phase 8 — Project map and agent context protocol

Goal:

```text
Make a codebase navigable with very few tokens.
```

Implement:

```text
fgx map src/ --out project.fgmap
fgx get @function.mix_samples --deps types,tests
fgx get @asm.popcount_x86 --review unsafe
```

Success test:

```text
An agent can inspect only one function and its dependencies instead of reading the full project.
```

---

### Phase 9 — VS Code Agent Workbench and Human Review UI

Goal:

```text
Make Forgentex usable inside VS Code as an agent-first programming environment.
```

The VS Code plugin should emphasize `.fgai`, `.fgmap`, and `.fgp` as the efficient agent workflow, while showing `.fgx` as a human review surface.

Core plugin features:

- Chat with coding agents in a sidebar.
- Show source side by side: human `.fgx` mode and agent `.fgai` mode.
- Highlight semantic node IDs.
- Show token-cost estimates for loading modules/nodes.
- Let agents request compact context from `.fgmap`.
- Let agents propose `.fgp` semantic patches.
- Let humans approve/reject patches.
- Highlight unsafe blocks.
- Highlight assembly.
- Show ABI/layout changes.
- Show allocation/effect changes.
- Show semantic patch summary.
- Show affected tests.
- Show compiler diagnostics.
- Ask agents to explain hard `.fgai` regions in human language.

Implementation suggestion:

```text
Rust CLI/backend + TypeScript VS Code extension + optional React/WebView review panels.
```

---

### Phase 9.5 — Crypto, Token, I/O, and Agent Communication Demo

Goal:

```text
Prove that Forgentex can express safe cryptographic and networked agent-economy workflows in compact agent-readable form.
```

Demo components:

- `SecretKey`, `PublicKey`, and `Signature` types.
- Sign and verify a semantic patch.
- Serialize a signed `AgentMessage`.
- Send/receive a message over a mock network transport.
- Represent a typed token payment request.
- Emit an audit receipt.

Example deliverables:

```text
examples/agent/signed_patch.fgx
examples/agent/signed_patch.fgai
examples/agent/agent_message.fgx
examples/agent/token_payment.fgx
docs/crypto_agent_economy.md
docs/networking_io_model.md
```

Success test:

```text
An agent can propose a semantic patch, sign it, send it to another agent, receive a signed review response, and produce a human-readable audit report.
```

This phase should remain a safe demo. It should not handle real private keys or real funds in early versions.

---

### Phase 10 — Serious compiler backend experiment

Goal:

```text
Move beyond interpreter/transpiler toward real native compilation.
```

Options:

- Cranelift-style backend for simpler JIT/native code.
- LLVM backend.
- MLIR frontend/lowering path.

Recommended path:

```text
Prototype with C transpilation first.
Use LLVM/MLIR only after language semantics stabilize.
```

---

## 14. Non-Goals for the First Demo

Do not attempt in v0:

- Full Rust-level borrow checker.
- Full optimizer.
- Full MLIR backend.
- GPU kernels.
- Complete inline assembly validation.
- Package manager.
- Macro system.
- Real FFmpeg replacement.
- Dynamic runtime.
- Async system.
- Standard library beyond tiny examples.

The first milestone is not performance.

The first milestone is:

```text
dual representation + shared AST + semantic patching + useful diagnostics
```

---

## 15. First Codex / Claude Code Task Prompts

### Prompt 1 — Create spec skeleton

```text
Create a Rust workspace for a toy language called Forgentex.
Add docs for human syntax, agent syntax, AST schema, and semantic patch protocol.
Do not implement a full compiler yet.
Add examples/hello.fgx and examples/hello.fgai that represent the same program.
```

### Prompt 2 — Parser v0

```text
Implement a minimal parser for Forgentex human mode and agent mode.
Support only structs, functions, primitive types, return statements, and binary arithmetic.
Both parsers must emit the same AST type.
Add roundtrip tests.
```

### Prompt 3 — Formatter v0

```text
Implement canonical formatters for .fgx and .fgai.
The formatter must be idempotent.
Add snapshot tests for examples/hello and examples/audio_mix.
```

### Prompt 4 — Semantic patch v0

```text
Implement a semantic patch engine for the AST.
Support add_struct_field, replace_function_body, rename_function, and replace_return_type.
Add patch tests and regenerate both .fgx and .fgai after patch application.
```

### Prompt 5 — Assembly AST prototype

```text
Add typed assembly block nodes to the AST.
Support x86_64 assembly blocks with inputs, outputs, clobbers, and instruction lines.
Require assembly blocks to appear only inside unsafe functions.
Add one example: popcount_x86.fgx and popcount_x86.fgai.
```

---

## 16. Main Risks

### Risk 1 — Too many features too early

Forgentex can easily become too large.

Mitigation:

```text
Build tiny vertical slices.
Do not add a feature unless it is represented in human mode, agent mode, AST, formatter, tests, and diagnostics.
```

### Risk 2 — Agent syntax becomes unreadable garbage

Agent mode can be dense, but it must remain structured and deterministic.

Mitigation:

```text
Keep a formal grammar.
Keep canonical formatting.
Keep stable node IDs.
Keep machine-readable diagnostics.
```

### Risk 3 — Unsafe/assembly becomes impossible to audit

Mitigation:

```text
Force every unsafe and assembly block to carry effects, targets, clobbers, and review metadata.
Generate human review reports automatically.
```

### Risk 4 — Compiler correctness

Mitigation:

```text
Use Rust for implementation.
Add roundtrip tests.
Add fuzzing.
Add differential tests against generated C/Rust where possible.
Keep v0 semantics small.
```

---

## 17. Final Recommendation

Build Forgentex in **Rust** first.

Keep the central product vision clear:

```text
Forgentex is not trying to beat Python at human simplicity.
Forgentex is trying to become a dense, safe, token-efficient systems language for AI agents in an agent economy.
```

The VS Code experience should make this practical: agents work mostly in `.fgai`, humans review `.fgx`, and the compiler/tooling provides summaries, diagnostics, risk reports, signatures, receipts, and semantic patch validation.


Do not start with a huge compiler backend. Start with the part that makes Forgentex unique:

```text
human mode <-> shared AST <-> agent mode
semantic patches
machine-readable diagnostics
human review reports
structured assembly representation
```

A good first milestone:

```text
Forgentex v0.1 can parse, format, roundtrip, type-check, patch, and review a tiny media/DSP example in both human and agent syntax.
```

A good long-term target:

```text
Forgentex becomes a systems language where AI agents can build FFmpeg-scale libraries through compact semantic edits, while humans audit only architecture, unsafe code, assembly, ABI, and performance-critical paths.
```
