# Public 0.1 release proof

Forgentex 0.1 makes one claim that source cannot check: that a release build of
the toolchain really was produced, checksummed, and clean-installed. This file
records that run, and `fgx dist-manifest` parses it — so the distribution gate
closes on recorded evidence rather than on anyone's assertion that the work was
done. While a field reads `pending`, the gate reports exactly which one.

The run is the `release-artifacts` job in
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml). It builds
`--release`, computes SHA-256 checksums, copies `fgx.exe` alone into an empty
directory, and requires `dist-manifest`, `release-evidence --strict`, and
`forgepack evidence --strict` to succeed there — a binary that needs anything
from its build tree fails that step.

0.1 verifies **one** host. Linux and macOS appear in the manifest as
`source-build-unverified`: the compiler is dependency-free Rust and is expected
to build on them, but no CI job has proven it, so 0.1 promises nothing there.
Verifying them is a post-0.1 task, not a 0.1 omission to be quietly implied.

## The recorded run

```
run_url: pending
commit: pending
host: x86_64-pc-windows-msvc
fgx: pending
forgentex-lsp: pending
```

- `run_url` — the release-artifacts run, so a reader can open its log.
- `commit` — the 40-character commit that run built.
- `host` — the verified host triple.
- `fgx`, `forgentex-lsp` — the SHA-256 digests that run published, to compare
  against a downloaded binary.

## Reproducing it without CI

The same checks run locally on a Windows host:

```powershell
cargo build --release --bins
Get-FileHash target\release\fgx.exe -Algorithm SHA256
.\scripts\forgepack-clean-install.ps1 -FgxPath .\target\release\fgx.exe
```

A local run is a useful rehearsal, but it is not what this file records: the
point of the recorded run is that someone other than the author can open it.
