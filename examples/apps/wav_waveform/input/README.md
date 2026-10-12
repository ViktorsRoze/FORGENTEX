# Waveform input

The waveform application reads a 16-bit PCM `.wav` from this directory. The
sample used during development — an 8-bar drum loop at 128 BPM — is **not
distributed with the published repository**: it is 2.5 MB of audio, and a
compiler repository is a poor place to carry media.

Supply any 16-bit PCM `.wav` to run the example:

```powershell
# any 16-bit PCM wav will do; name it whatever the manifest expects
Copy-Item C:\path\to\your.wav .\loop_8bars_128bpm.wav
fgx app-run ..\..\wav_waveform --arg loop_8bars_128bpm.wav
```

`tests/cli.rs::wav_waveform_application_renders_a_real_pcm_envelope` skips when
no sample is present, so the suite stays green on a clone without one. A missing
media fixture is not a defect in the renderer, and a test that fails for a file
nobody shipped teaches a reader to ignore failures.
