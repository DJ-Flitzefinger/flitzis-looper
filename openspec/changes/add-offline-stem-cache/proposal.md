# Change: Add offline stem cache and prepared stem-buffer mixing

## Why
Gen3 transport and phase-aware playback now give the audio engine a real-time-safe timing
foundation. The next legacy parity gap is stems: performers eventually need generated stem
sets and synchronized stem playback controls, but real-time stem separation would violate
the project's audio callback constraints.

This change defines the first stem contract before implementation: stem generation is
offline/background work, cached per pad/sample version, and only already prepared immutable
stem buffers may be published for future Rust mixing.

## What Changes
- Define a pad-scoped stem cache model for vocals, melody, bass, drums, and instrumental buffers.
- Require stem generation to run outside the audio callback as a background/offline task.
- Require generation and stem replacement to be blocked or deferred for pads that are
  currently playing.
- Define prepared stem buffers as immutable, sample-rate/channel/length-aligned audio data
  that can be published to Rust by handle.
- Define future stem mixing as bounded audio-thread state that shares the same voice
  playhead, loop region, transport timing, speed, BPM-lock, and key-lock behavior as the
  full mix.
- Add a production source-separation backend boundary and use Demucs as the first adapter.
- Keep Demucs model files outside the repository and project samples in the standard Torch Hub
  checkpoint cache. UI-driven generation requires the model to be installed ahead of time and
  reports `no Model installed` when it is missing.
- Require working `ffprobe` and `ffmpeg` executables for the Demucs adapter, with a short
  `FFmpeg/ffprobe unavailable` error when they are missing or inaccessible.
- Resolve FFmpeg tools from process `PATH`, explicit `FLITZIS_FFMPEG_DIR`, or local WinGet
  `Gyan.FFmpeg*` package installs before reporting them unavailable.
- Require usable TorchCodec support for the current Torchaudio output path, with a short
  `TorchCodec unavailable` error before Demucs starts if native libraries cannot load.
- Use Demucs defaults of `--shifts 1` and `--overlap 0.5`, modeled as bounded
  request parameters that can be replaced by validated Settings page values.
- Delete tracked pad stem cache artifacts on pad unload or explicit stem deletion outside the
  audio callback.
- Attempt CUDA only from a background worker and fall back to CPU when the CUDA path fails.
- Keep the existing fixed-size ring-buffer message-passing architecture.
- Keep real-time stem separation, neural network inference, disk I/O, logging, blocking,
  heap allocation, and Python/GIL access out of the audio callback.

## Impact
- Affected specs: `stem-cache` (new), `background-tasks`, `ring-buffer-messaging`,
  `play-samples`.
- Affected docs: `docs/architecture.md`, `docs/stem-generation-setup.md`.
- Later affected code: controller/background-task orchestration, project stem cache metadata,
  Rust message enum, Rust mixer stem-buffer storage, and focused Rust/Python tests.

## Non-Goals
- No real-time stem separation.
- No model training or fine-tuning workflow.
- No stem generation, disk I/O, decoding, neural inference, logging, blocking, heap
  allocation, or Python/GIL access in the audio callback.
- No new stem UI controls, waveform UI changes, or component right-click solo behavior in this
  production-backend slice.
- No replacement of the existing `rtrb` message-passing architecture.
- No continuous stem phase correction separate from the existing voice playhead and loop
  timing model.
