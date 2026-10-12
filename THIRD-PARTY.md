# Third-party material

Forgentex itself is dependency-free Rust: the compiler, checker, interpreter,
LSP, and CLI are built from this repository's own source, with no crates.io
dependencies. `Cargo.lock` therefore records no third-party code.

Two things in the tree are not Forgentex source.

## Not distributed here

Two files the audio work uses are deliberately absent from this repository. A
compiler repository is a poor place to carry megabytes of media, and neither
file is ours to relicense.

**miniaudio (`miniaudio.h`)** — a single-file audio playback and capture library
by David Reid, used by the audio capsule demonstrations in `examples/`. Its own
header states the choice of terms, **public domain or MIT-0**, and those
statements are the authoritative ones. To run those demonstrations, fetch the
header into the repository root:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/mackron/miniaudio/master/miniaudio.h -OutFile miniaudio.h
```

**Sample audio (`examples/apps/wav_waveform/input/`)** — the waveform example
reads a 16-bit PCM `.wav`. The 2.5 MB loop used during development is not
shipped; any 16-bit PCM file works, and
[the input directory](examples/apps/wav_waveform/input/README.md) says how to
supply one. The CLI test for that example skips when no sample is present, so a
clone without one stays green.

Both paths are listed in `.gitignore`, so a local copy cannot be committed by
accident.

If you vendor anything further, record it here: a reader of a published
repository should be able to tell what is ours and what is not without reading
every file.
