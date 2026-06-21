# Forgentex Demo

> 👋 **New here?** Start with [WELCOME.md](WELCOME.md) — the friendly tour with
> examples, project stats, and a two-minute quick start. This file is the full
> command reference and stage-by-stage scope record.

Forgentex is a systems-language demo focused on an agent-native workflow:

```text
.fgx human review syntax <-> shared AST <-> .fgai agent syntax
semantic patches -> type check -> human review report
```

This repository contains a dependency-free Rust CLI named `fgx`. The first demo
does not try to be a full compiler; it proves the workflow with a small media/DSP
example set.

## Commands

```powershell
fgx ast examples/hello.fgx --canonical
fgx parse examples/hello.fgx --emit-ast
fgx diagnose examples/hello.fgx --json
fgx diagnostics list --json
fgx explain TYP001 --json
fgx fmt examples/media/audio_mix.fgx --to fgai
fgx roundtrip examples/media/audio_mix.fgx --to fgai
fgx check examples/media/audio_mix.fgx --json
fgx borrow-report examples/media/audio_mix.fgx --json
fgx borrow-conformance --json
fgx check examples/media/popcount_x86.fgx --target x86_64-pc-windows-msvc --json
fgx asm-check examples/arch/x86_64_popcount.fgx --target x86_64-pc-windows-msvc
fgx abi examples/abi/layout_demo.fgx --target x86_64-unknown-linux-gnu
fgx header examples/abi/layout_demo.fgx --target riscv64gc-unknown-linux-gnu
fgx simd-review examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu
fgx check examples/media/audio_mix_simd.fgx --target riscv64gc-unknown-linux-gnu --json
fgx bench examples/media/audio_mix_simd.fgx --entry mix_samples_simd --json-input examples/bench/audio_mix.json --target aarch64-unknown-linux-gnu --json
fgx perf-report --scalar examples/media/audio_mix.fgx --optimized examples/media/audio_mix_simd.fgx --entry mix_samples --optimized-entry mix_samples_simd --json-input examples/bench/audio_mix.json --json
fgx targets
fgx backends
fgx ir . --json
fgx ir . --contract-json
fgx stdlib --json
fgx check --project --json
fgx build . --backend c --out build/c
fgx build . --backend zig --out build/zig
fgx new examples/packages/my_package --name my-package
fgx lock examples/packages/codec_app --json
fgx test examples/smoke/inline_tests.fgx --json
fgx test --project examples/packages/codec_app --all-targets --json
fgx build examples/packages/codec_app --backend c --all-targets --out build/stage23 --json
fgx publish examples/packages/codec_app --dry-run --json
fgx audit examples/packages/codec_app --json
fgx verify examples/packages/codec_app --json
fgx api --public
fgx effects examples/media/popcount_x86.fgx --json
fgx effects examples/io/file_copy.fgx --json
fgx effects examples/network/agent_ping.fgx --json
fgx effects examples/agent/signed_patch.fgx --json
fgx patch examples/media/audio_mix.fgx patches/add_fast_path.fgp --review
fgx patch examples/media/audio_mix.fgx patches/add_fast_path.fgp --check-only
fgx patch examples/media/audio_mix.fgx patches/add_fast_path.fgp --emit-review-json
fgx patch examples/media/audio_mix.fgx patches/signed_add_crypto_effect.fgp --emit-review-json
fgx run examples/media/audio_mix.fgx
fgx run examples/media/audio_mix.fgx --entry mix_samples --json-input examples/run/audio_mix.json
fgx run examples/io/file_copy.fgx --entry file_copy_demo --json-input examples/run/file_copy.json
fgx run examples/network/agent_ping.fgx --entry agent_ping --json-input examples/run/agent_ping.json
fgx run examples/agent/signed_patch.fgx --entry sign_patch --json-input examples/run/signed_patch.json
fgx run examples/agent/payment_receipt.fgx --entry issue_receipt --json-input examples/run/payment_receipt.json
fgx map examples --include-targets --out examples/project.fgmap
fgx map --project --out examples/project_modules.fgmap
fgx get '@function.mix_samples' --from examples --deps types
fgx context '@function.mix_samples' --deps types,effects,tests --json
fgx review patches/signed_add_crypto_effect.fgp --json
fgx graph dump examples/smoke/inline_tests.fgx --out build/inline_tests.fggraph
fgx graph validate build/inline_tests.fggraph --json
fgx graph context build/inline_tests.fggraph '@function.clamp_byte' --deps effects,tests --json
fgx graph roundtrip examples/smoke/inline_tests.fgx --json
forgentex-lsp --probe
fgx fuzz --iterations 32 --json
fgx diff-test --json
fgx fix --plan examples/smoke/raw_pointer_bad.fgx --json
fgx release-check --json
```

## Demo Flow

After installing Rust, run:

```powershell
cargo test
cargo run --bin fgx -- parse examples/hello.fgx --emit-ast
cargo run --bin fgx -- fmt examples/media/audio_mix.fgx --to fgai
cargo run --bin fgx -- diagnostics list --json
cargo run --bin fgx -- explain TYP001 --json
cargo run --bin fgx -- check examples/media/audio_mix.fgx --json
cargo run --bin fgx -- borrow-report examples/media/audio_mix.fgx --json
cargo run --bin fgx -- borrow-conformance --json
cargo run --bin fgx -- targets
cargo run --bin fgx -- backends
cargo run --bin fgx -- ir . --json
cargo run --bin fgx -- stdlib
cargo run --bin fgx -- lock examples/packages/codec_app --json
cargo run --bin fgx -- test examples/smoke/inline_tests.fgx --json
cargo run --bin fgx -- test --project examples/packages/codec_app --all-targets --json
cargo run --bin fgx -- build examples/packages/codec_app --backend c --all-targets --out build/stage23 --json
cargo run --bin fgx -- build examples/packages/codec_app --backend zig --all-targets --out build/stage28-zig --json
cargo run --bin fgx -- publish examples/packages/codec_app --dry-run --json
cargo run --bin fgx -- audit examples/packages/codec_app --json
cargo run --bin fgx -- verify examples/packages/codec_app --json
cargo run --bin fgx -- check examples/media/popcount_x86.fgx --target x86_64-pc-windows-msvc --json
cargo run --bin fgx -- asm-check examples/arch/riscv64_add.fgx --target riscv64gc-unknown-linux-gnu
cargo run --bin fgx -- abi examples/abi/layout_demo.fgx --target x86_64-unknown-linux-gnu
cargo run --bin fgx -- simd-review examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu
cargo run --bin fgx -- bench examples/media/audio_mix_simd.fgx --entry mix_samples_simd --json-input examples/bench/audio_mix.json --target aarch64-unknown-linux-gnu --json
cargo run --bin fgx -- perf-report --scalar examples/media/audio_mix.fgx --optimized examples/media/audio_mix_simd.fgx --entry mix_samples --optimized-entry mix_samples_simd --json-input examples/bench/audio_mix.json --json
cargo run --bin fgx -- effects examples/media/popcount_x86.fgx --json
cargo run --bin fgx -- effects examples/io/file_copy.fgx --json
cargo run --bin fgx -- effects examples/agent/signed_patch.fgx --json
cargo run --bin fgx -- run examples/network/agent_ping.fgx --entry agent_ping --json-input examples/run/agent_ping.json
cargo run --bin fgx -- run examples/agent/payment_receipt.fgx --entry issue_receipt --json-input examples/run/payment_receipt.json
cargo run --bin fgx -- run examples/media/audio_mix.fgx
cargo run --bin fgx -- patch examples/media/audio_mix.fgx patches/signed_add_crypto_effect.fgp --emit-review-json
cargo run --bin fgx -- map examples --out examples/project.fgmap
cargo run --bin fgx -- context '@function.mix_samples' --deps types,effects,tests --json
cargo run --bin fgx -- review patches/signed_add_crypto_effect.fgp --json
cargo run --bin fgx -- graph dump examples/smoke/inline_tests.fgx --out build/inline_tests.fggraph
cargo run --bin fgx -- graph validate build/inline_tests.fggraph --json
cargo run --bin fgx -- graph context build/inline_tests.fggraph '@function.clamp_byte' --deps effects,tests --json
cargo run --bin fgx -- graph roundtrip examples/smoke/inline_tests.fgx --json
cargo run --bin forgentex-lsp -- --probe
cargo run --bin fgx -- fuzz --iterations 8 --json
cargo run --bin fgx -- diff-test --json
cargo run --bin fgx -- fix --plan examples/smoke/raw_pointer_bad.fgx --json
cargo run --bin fgx -- release-check --json
```

Or run the bundled script:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1
```

For the full Stage 1 check:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

