# Response to the 2026-09-05 language evaluation

Every finding in [`language_evaluation_2026-09-05.md`](language_evaluation_2026-09-05.md)
was re-checked here against the working tree rather than accepted as written.
The table records what reproduced, what needed refining, and what was done.

Verification date: 2026-09-05. Two findings are already fixed; the rest are
sequenced below.

## Verification results

| Finding | Reproduced? | Notes from re-checking |
| --- | --- | --- |
| **F5** — `check` reports errors and exits 0 | **Yes** | `check examples/smoke/borrow_alias_bad.fgx --json` printed `mutable_borrow_alias` and exited 0. Both the file and `--project` branches returned `Ok(())` unconditionally. **Fixed.** |
| **F8** — `fgx_checked_i64_mul` sign bug | **Yes, and worse than reported** | Not an edge case: the guard traps on *any* product with a negative right operand. Verified in C — `2 * -3`, `-2 * -3`, `1000 * -1000` all falsely trapped. **Fixed.** |
| **F8** — reachability of that guard | **Refined** | The evaluation left this open. It is **not reachable**: a plain `i64` multiply emits `return (x * y)`, and nothing in `c_backend.rs` calls the helper. A latent defect, not an active miscompilation — which lowers its urgency but not its priority, since it should be correct before anything wires it up. |
| **F8** — emitted C uses raw operators | **Yes** | Generated source is `return (x * y)` with no wrapping guarantee, so signed overflow is undefined behaviour. The memory-safety document's wrapping claim is too strong. |
| **F4** — `@exact` is not transitive | **Yes — now fixed** | The evaluation's exact program passed `check` with an empty diagnostic list. It is now rejected: the contract is checked over the call graph, including recursive groups and resolved named types — see *Sequenced next*. |
| **F6** — `diff-test` does not execute | **Yes — now fixed** | `run_backend_differential` ran the interpreter, generated C and IR, then asserted `header.contains(entry) && source.contains(entry)`. It never compiled or ran the C. The name implied behavioural equivalence; the check was name presence in generated text. It now compiles and runs the generated C and compares results — see *Sequenced next*. |
| **F2** — edit benchmark edge case | **Yes, exactly** | `edit-bench examples/capsule/web_session.fgx --edits 8` reproduced `functions=0`, `symbol_tokens=0`, `patch_over_whole=0.0564`. The figure charges nothing for reading the target body of a file that has no functions, while still pricing a function patch. |
| **F1** — compact syntax can cost more tokens | **Mechanism corroborated** | The BPE counts could not be reproduced (no tokenizer installed here), but the *mechanism* was confirmed independently: `inline_tests.fgx` is **8.3% shorter in characters** in agent form while the evaluation measured **7.3% more tokens**; `wav_waveform/src/main.fgx` is 7.5% shorter yet measured 4.6% larger. Characters and BPE tokens move in opposite directions on two of three samples. This is expected — `while`, `else`, `match` are common single tokens, whereas `WH`, `EL`, `MT` are not — and it means a character-based heuristic can predict the wrong *direction*, not merely the wrong magnitude. |

Nothing in the evaluation was found to be incorrect. One claim it explicitly
left open (guard reachability) resolved in the *less* severe direction, and one
(F8's multiply) is more severe than stated.

## Fixed

### F5 — a checker that finds errors now fails

`fgx check` and `fgx check --project` return a non-zero status when they report
diagnostics. The JSON report still prints to stdout, so a wrapper receives every
diagnostic; what changes is that a build script or agent trusting the exit
status can no longer mistake a rejected program for a clean one.

This matches `fgx test`, which already failed on failure — `check` was the
outlier. Exit status was not part of the frozen public compatibility surface.
Exactly one test asserted the old behaviour (it expected success *while*
expecting a `target_guard_required` diagnostic) and was corrected. Every file
used by `scripts/demo.ps1` is clean, so the demo path is unaffected.

### F8 — sign-correct checked multiplication

The guard now separates each sign pairing and rejects the `INT64_MIN / -1`
combination directly rather than dividing by it. A test compiles the emitted
guard and compares it against a **128-bit reference product** across every sign
pairing and integer boundary — so it is checked against the real answer rather
than a restatement of its own arithmetic. Restoring the original guard makes
that test fail, which was verified rather than assumed.

## Sequenced next

1. ~~**F6 — `diff-test` honesty.**~~ **Done** (2026-09-06), taking the second
   option: `diff-test` now compiles the generated translation unit together with
   a harness that calls the entry on the same arguments the interpreter received,
   runs it, and compares the results. Floating-point values are compared as bit
   patterns at their declared width, so two numbers that print alike but are not
   alike still fail; arguments are emitted as exact hexadecimal float literals so
   the compiler parses the number the interpreter held. The report carries a
   `native_execution` verdict — `matched`, `mismatched`, `unsupported`,
   `unavailable`, or `failed` — because a run that could not execute the C must
   not read as one that did. Signatures the harness cannot construct are named
   rather than skipped silently.

   The first real execution immediately found a backend defect the name-presence
   check could never see: visibility was decided per item, so a **public function
   whose signature named a non-public struct** produced a header that declared
   the prototype but never defined the type. The generated header did not compile
   on its own, and any consumer including it failed with `unknown type name`.
   Types reachable from a public signature are now emitted into the header
   regardless of their own visibility, expanded transitively through struct
   fields and enum payloads, and excluded from the `.c` so each type is defined
   exactly once. `the_generated_header_compiles_on_its_own` compiles the header
   as a lone translation unit; reverting the fix makes it and the three
   differential tests fail, which was verified rather than assumed. A companion
   test doctors one sample of the expected output and requires the comparison to
   object, so the passing case means the answers actually matched.
2. ~~**F4 — `@exact` on the call graph.**~~ **Done** (2026-09-06), taking the
   first option. `@exact` is now a reachability question over the call graph
   rather than a check of one function body:

   - **Transitive.** Every function reachable from an `@exact` function is
     checked. The evaluation's own program — an exact function whose entire body
     was a call into floating point — is now rejected with
     `exact_arithmetic_calls_inexact`. The message names both the callee and the
     function at the far end of the chain, so a reader is not left tracing the
     graph by hand to find where the float actually is.
   - **Recursive groups.** A worklist fixpoint over the reverse call graph, not a
     depth-first walk: self- and mutual recursion are ordinary edges. A recursive
     group containing a float is rejected; one without is accepted. (A recursive
     "does this reach a float" query has to special-case cycles and can memoise
     an answer that was only correct for the path it arrived by; the fixpoint has
     no such hazard.)
   - **Named types resolved.** A struct or enum holding an `f32` is floating
     point wherever it appears, however deeply nested, so a parameter can no
     longer smuggle a float in behind a type name. Recursive type definitions
     terminate.
   - **Lambda parameters.** Previously only a lambda's body was checked, so
     `|x: f32| x` read as float-free.

   Two edges are trusted rather than proven, and the documentation now says so:
   compiler intrinsics are judged by their declared contract (no intrinsic is
   float-typed today, so this admits nothing at present), and nothing is claimed
   about foreign capsule bodies. The docs also now separate the two properties
   the old wording ran together: `@exact` means *the absence of IEEE-754
   rounding*, which makes a function reproducible on every backend — it is not a
   statement about overflow, division, or shift behaviour, which F8 covers.

   Five regression tests cover transitivity, chain reporting, nested named types,
   recursive groups in both directions, and lambda parameters; all five fail
   against the previous behaviour, which was verified rather than assumed. The
   existing `@exact` functions in `examples/audio` and `examples/native` still
   check clean, so the stricter rule did not simply reject everything.
3. ~~**F8 — numeric semantics.**~~ **Done** (2026-09-06). Integer arithmetic is
   now checked at the **declared width**, in both backends, and specified per
   operation in `docs/numeric_determinism.md`.

   The gap was wider than the finding described, and worth stating plainly. The
   interpreter checked every operation at 64 bits regardless of how it was
   written, so `fn f(a: i32, b: i32) -> i32` returned **4000000000** — a value no
   `i32` can hold — and `1i32 << 40` returned 1099511627776. The C backend
   emitted C's own operators, so the same addition produced **-294967296** by
   wrapping, which C does not actually promise: signed overflow, division by
   zero, `MIN / -1`, and a shift amount at or beyond the width are all
   *undefined*, and wrapping is a GCC option (`-fwrapv`) rather than a property
   of the language. Two backends, two different answers, neither specified.

   What changed:

   - **The C backend lowers each integer operation to an explicit guard** —
     `fgx_checked_i32_add` and its siblings, seven operations across ten widths.
     The three `fgx_checked_i64_*` helpers the header already defined were never
     referenced by any emitted expression; they are now generated per width and
     actually used.
   - **The interpreter checks at the declared width.** It has no type
     environment, so `int_width` supplies one: it walks the body about to be
     executed and records the width of each arithmetic node. Per call frame,
     because the FIR path rebuilds statements on every call and those addresses
     are reused once the frame is gone.
   - **Shifts, division, and remainder are specified rather than inherited.** A
     shift discards the bits that leave — that is what a shift is — but the
     *amount* must be inside `[0, width)`. `MIN / -1` traps because the quotient
     is one past the maximum, and `MIN % -1` traps too, so the two forms of one
     operation answer alike. Both match Rust.
   - **An integer literal has no width of its own**, and takes it from the other
     operand or from the binding, parameter, or return type it flows into — so
     `let x: i8 = 100 + 27` is an `i8` addition in both backends.

   Seven regression tests cover overflow at each width, boundary values that must
   *not* trap, division and remainder, shift amounts, and bits shifted off the
   top; the last compiles the generated C and requires it to trap on the same
   input the interpreter traps on. `fgx diff-test` compiles and runs the
   generated C, so the two backends staying in agreement is now a test.

   The memory-safety wording is corrected in two places. The claim that emitted C
   "wraps" is replaced with what it does now and why the old sentence was wrong.
   Separately, the three-zone table said memory issues were flatly impossible in
   safe code; it now distinguishes what the *language* gives you no way to write
   from what the *implementation* has been proven to do, which is Astra's wider
   point about that document.

   Two rows of F8 were already closed: the checked-multiplication guard was fixed
   and verified against a 128-bit reference (above), and node identity, mock
   signatures, reproducibility, apparent-success accounting, and language breadth
   remain open below.
4. ~~**F1 — tokenizer honesty.**~~ **Done** (2026-09-18). The heuristic is
   labelled as one everywhere it is reported: `fgx token-report` JSON carries
   `"ratio_kind": "heuristic"` and a caveat, and the text output repeats it.
   `--bpe-command` measures every surface with a real tokenizer of the user's
   choosing (any program that reads text on stdin and prints one integer;
   `tools/tokenizer/tiktoken_count.py` wraps tiktoken), reported beside the
   heuristic and never merged into it; a tokenizer that fails fails the whole
   measurement rather than leaving a mixed table. The module's claim that the
   heuristic was "fair and monotonic, so the ratios are meaningful" is removed,
   and the docs that drew conclusions from it are corrected using the
   evaluation's real-BPE figures: the agent surface is **not** a general token
   saving (13–15% smaller on small declaration-heavy files, up to ~7% larger on
   body-heavy ones, ~1% larger across the sample). No tokenizer was installed
   on the host, so those figures are the evaluation's measurements, attributed
   as such, not reproduced here.

5. ~~**F2 — benchmark honesty.**~~ **Done** (2026-09-18). `edit-bench` is
   labelled a cost model in text and JSON (`"kind": "cost_model"`) with its
   assumptions listed in every report. A file with no functions — and zero
   edits — is rejected rather than estimated. The representative patch was
   worse than "not generally valid": it used a `REPLACE_STATEMENT` operation
   the patch protocol does not have. It is now a real v1 patch with a
   `NODE_HASH` precondition, and the model refuses to run unless the patch
   engine accepts it. Most importantly, every report now includes the
   **conventional baseline** the evaluation asked for — read the file once,
   then send a small patch per edit — and against it the semantic-patch
   strategy shows no general advantage: 0.94× on the largest example and
   1.3–1.8× (more expensive) on the others. The published "~2× to ~8× cheaper"
   compared only against re-sending the whole file on every edit; README,
   findings, and roadmap are corrected accordingly.

6. ~~**F7 — enforcement matrix.**~~ **Published** (2026-09-18) as
   `docs/enforcement_matrix.md`: per runtime and platform, separating declared,
   checker-approved, enforced, and observed. The central fact is now
   demonstrated rather than stated: a Python capsule declared `fs.none` passes
   the checker, then reads a file outside its declaration and sees the parent's
   environment (`a_managed_python_capsule_is_checked_but_not_confined`, which
   is expected to fail — forcing a matrix update — once managed runners gain
   confinement). The sandbox model no longer opens by calling itself
   "enforced". The matrix also surfaced one problem that is a decision rather
   than a fix: **declared `limit.*` on managed capsules is silently not
   applied** (`web_counter`, `web_session`), where the native path refuses a
   limit it cannot enforce. The supervised runner the evaluation proposes for
   managed workers is not built; it is the matrix's second open problem.

The remaining F8 table rows — mock signatures, node identity under statement
insertion, reproducibility beyond interpreter selection, and skipped-versus-
executed reporting — are accepted and belong after the above.

## Closed later: debug output on its own channel (2026-09-28)

The evaluation's containment suggestion included one line that turned out to
describe a defect rather than a refinement: *"Put debug output on a separate
channel from the response protocol."*

It was a defect. A managed capsule's entire standard output was its result, so a
capsule whose body printed anything — and real libraries print — failed with
`worker returned invalid JSON` rather than returning what it computed. A probe
capsule with one `print()` reproduced it on both the warm and cold paths.

The boundary now tags every line with a per-call nonce and a kind (`R` for the
result, `D` for one line the capsule printed), rebinds the guest's print facility
where the language allows it so prints stay ordered against the answer, carries
arguments on stdin instead of argv, and gives a warm call a deadline. Details and
the remaining edges are in
[the enforcement matrix](enforcement_matrix.md#the-managed-boundary-protocol).
