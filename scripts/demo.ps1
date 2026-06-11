$ErrorActionPreference = "Stop"

$CargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($CargoCommand) {
    $Cargo = $CargoCommand.Source
} else {
    $Cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
}

& $Cargo run --bin fgx -- ast examples/hello.fgx --canonical
& $Cargo run --bin fgx -- parse examples/hello.fgx --emit-ast
& $Cargo run --bin fgx -- diagnose examples/hello.fgx --json
& $Cargo run --bin fgx -- diagnostics list --json
& $Cargo run --bin fgx -- explain TYP001 --json
& $Cargo run --bin fgx -- fmt examples/media/audio_mix.fgx --to fgai
& $Cargo run --bin fgx -- roundtrip examples/media/audio_mix.fgx --to fgai
& $Cargo run --bin fgx -- check examples/media/audio_mix.fgx --json
& $Cargo run --bin fgx -- borrow-report examples/media/audio_mix.fgx --json
& $Cargo run --bin fgx -- borrow-conformance --json
& $Cargo run --bin fgx -- backends
& $Cargo run --bin fgx -- ir . --json
& $Cargo run --bin fgx -- stdlib
& $Cargo run --bin fgx -- lock examples/packages/codec_app --json
& $Cargo run --bin fgx -- test examples/smoke/inline_tests.fgx --json
& $Cargo run --bin fgx -- test --project examples/packages/codec_app --all-targets --json
& $Cargo run --bin fgx -- build examples/packages/codec_app --backend c --target aarch64-unknown-linux-gnu --out build/stage23-demo --json
& $Cargo run --bin fgx -- publish examples/packages/codec_app --dry-run --json
& $Cargo run --bin fgx -- audit examples/packages/codec_app --json
& $Cargo run --bin fgx -- verify examples/packages/codec_app --json
& $Cargo run --bin fgx -- effects examples/media/popcount_x86.fgx --json
& $Cargo run --bin fgx -- check examples/io/file_copy.fgx --json
& $Cargo run --bin fgx -- effects examples/io/file_copy.fgx --json
& $Cargo run --bin fgx -- check examples/network/agent_ping.fgx --json
& $Cargo run --bin fgx -- effects examples/network/agent_ping.fgx --json
& $Cargo run --bin fgx -- check examples/agent/signed_patch.fgx --json
& $Cargo run --bin fgx -- effects examples/agent/signed_patch.fgx --json
& $Cargo run --bin fgx -- check examples/agent/payment_receipt.fgx --json
& $Cargo run --bin fgx -- effects examples/agent/payment_receipt.fgx --json
& $Cargo run --bin fgx -- abi examples/abi/layout_demo.fgx --target x86_64-unknown-linux-gnu
& $Cargo run --bin fgx -- header examples/abi/layout_demo.fgx --target riscv64gc-unknown-linux-gnu
& $Cargo run --bin fgx -- simd-review examples/media/audio_mix_simd.fgx --target aarch64-unknown-linux-gnu
& $Cargo run --bin fgx -- check examples/media/audio_mix_simd.fgx --target riscv64gc-unknown-linux-gnu --json
& $Cargo run --bin fgx -- bench examples/media/audio_mix_simd.fgx --entry mix_samples_simd --json-input examples/bench/audio_mix.json --target aarch64-unknown-linux-gnu --json
& $Cargo run --bin fgx -- perf-report --scalar examples/media/audio_mix.fgx --optimized examples/media/audio_mix_simd.fgx --entry mix_samples --optimized-entry mix_samples_simd --json-input examples/bench/audio_mix.json --json
& $Cargo run --bin fgx -- run examples/media/audio_mix.fgx
& $Cargo run --bin fgx -- run examples/io/file_copy.fgx --entry file_copy_demo --json-input examples/run/file_copy.json
& $Cargo run --bin fgx -- run examples/network/agent_ping.fgx --entry agent_ping --json-input examples/run/agent_ping.json
& $Cargo run --bin fgx -- run examples/agent/signed_patch.fgx --entry sign_patch --json-input examples/run/signed_patch.json
& $Cargo run --bin fgx -- run examples/agent/payment_receipt.fgx --entry issue_receipt --json-input examples/run/payment_receipt.json
& $Cargo run --bin fgx -- patch examples/media/audio_mix.fgx patches/add_fast_path.fgp --review
& $Cargo run --bin fgx -- patch examples/media/audio_mix.fgx patches/signed_add_crypto_effect.fgp --emit-review-json
& $Cargo run --bin fgx -- map examples --out examples/project.fgmap
& $Cargo run --bin fgx -- get '@function.mix_samples' --from examples --deps types
& $Cargo run --bin fgx -- context '@function.mix_samples' --deps types,effects,tests --json
& $Cargo run --bin fgx -- review patches/signed_add_crypto_effect.fgp --json
& $Cargo run --bin fgx -- graph dump examples/smoke/inline_tests.fgx --out build/inline_tests.fggraph
& $Cargo run --bin fgx -- graph validate build/inline_tests.fggraph --json
& $Cargo run --bin fgx -- graph context build/inline_tests.fggraph '@function.clamp_byte' --deps effects,tests --json
& $Cargo run --bin fgx -- graph roundtrip examples/smoke/inline_tests.fgx --json
& $Cargo run --bin fgx -- fuzz --iterations 8 --json
& $Cargo run --bin fgx -- diff-test --json
& $Cargo run --bin fgx -- fix --plan examples/smoke/raw_pointer_bad.fgx --json
& $Cargo run --bin fgx -- release-check --json
