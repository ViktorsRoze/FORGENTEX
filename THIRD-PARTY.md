# Third-party material

Forgentex itself is dependency-free Rust: the compiler, checker, interpreter,
LSP, and CLI are built from this repository's own source, with no crates.io
dependencies. `Cargo.lock` therefore records no third-party code.

Two things in the tree are not Forgentex source.

## miniaudio (`miniaudio.h`)

A single-file audio playback and capture library by David Reid, used by the
audio capsule demonstrations in `examples/`. Its own header states the choice of
terms: **public domain or MIT-0**. It is vendored rather than fetched so an
audio demonstration builds from a checkout alone, and it is unmodified — the
licence statements at the end of the file are the authoritative ones.

## Sample audio (`examples/apps/wav_waveform/input/loop_8bars_128bpm.wav`)

An 8-bar drum loop used as input by the waveform example, so the example runs on
real audio instead of a generated tone. It ships under this repository's own
licence terms.

If you vendor anything further, record it here: a reader of a published
repository should be able to tell what is ours and what is not without reading
every file.
