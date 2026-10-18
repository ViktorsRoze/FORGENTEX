#!/usr/bin/env bash
# Linux/macOS counterpart of scripts/demo.ps1: a smoke tour of the fgx CLI.
# Stops at the first failing command.
set -euo pipefail
cd "$(dirname "$0")/.."

fgx() { cargo run --quiet --bin fgx -- "$@"; }

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
fgx backends
fgx ir . --json
fgx stdlib
fgx lock examples/packages/codec_app --json
fgx test examples/smoke/inline_tests.fgx --json
fgx test --project examples/packages/codec_app --all-targets --json
fgx build examples/packages/codec_app --backend c --target aarch64-unknown-linux-gnu --out build/stage23-demo --json
fgx publish examples/packages/codec_app --dry-run --json
fgx audit examples/packages/codec_app --json
fgx verify examples/packages/codec_app --json
fgx effects examples/media/popcount_x86.fgx --json
fgx check examples/io/file_copy.fgx --json
fgx effects examples/io/file_copy.fgx --json
fgx check examples/network/agent_ping.fgx --json
fgx effects examples/network/agent_ping.fgx --json
fgx check examples/agent/signed_patch.fgx --json
fgx effects examples/agent/signed_patch.fgx --json
fgx check examples/agent/payment_receipt.fgx --json
fgx effects examples/agent/payment_receipt.fgx --json
fgx abi examples/abi/layout_demo.fgx --target x86_64-unknown-linux-gnu
fgx header examples/abi/layout_demo.fgx --target riscv64gc-unknown-linux-gnu
fgx simd-review examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu
fgx check examples/media/audio_mix_simd.fgx --target riscv64gc-unknown-linux-gnu --json
fgx bench examples/media/audio_mix_simd.fgx --entry mix_samples_simd --json-input examples/bench/audio_mix.json --target aarch64-unknown-linux-gnu --json
fgx perf-report --scalar examples/media/audio_mix.fgx --optimized examples/media/audio_mix_simd.fgx --entry mix_samples --optimized-entry mix_samples_simd --json-input examples/bench/audio_mix.json --json
fgx run examples/media/audio_mix.fgx
fgx run examples/io/file_copy.fgx --entry file_copy_demo --json-input examples/run/file_copy.json
fgx run examples/network/agent_ping.fgx --entry agent_ping --json-input examples/run/agent_ping.json
fgx run examples/agent/signed_patch.fgx --entry sign_patch --json-input examples/run/signed_patch.json
fgx run examples/agent/payment_receipt.fgx --entry issue_receipt --json-input examples/run/payment_receipt.json
fgx patch examples/media/audio_mix.fgx patches/add_fast_path.fgp --review
fgx patch examples/media/audio_mix.fgx patches/signed_add_crypto_effect.fgp --emit-review-json
fgx map examples --out examples/project.fgmap
fgx get '@function.mix_samples' --from examples --deps types
fgx context '@function.mix_samples' --deps types,effects,tests --json
fgx review patches/signed_add_crypto_effect.fgp --json
fgx graph dump examples/smoke/inline_tests.fgx --out build/inline_tests.fggraph
fgx graph validate build/inline_tests.fggraph --json
fgx graph context build/inline_tests.fggraph '@function.clamp_byte' --deps effects,tests --json
fgx graph roundtrip examples/smoke/inline_tests.fgx --json
fgx fuzz --iterations 8 --json
fgx diff-test --json
fgx fix --plan examples/smoke/raw_pointer_bad.fgx --json
fgx release-check --json
