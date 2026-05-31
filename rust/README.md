# Rust Audio Engine Module

This crate builds the native `flitzis_looper_audio` Python extension. It owns
the realtime audio path and exposes the PyO3 `AudioEngine` class used by the
Python application package.

The Python package wrapper lives in:

```text
src/flitzis_looper_audio/
```

`maturin develop` builds the platform extension into that wrapper package.

## Module Structure

```text
rust/
|-- Cargo.toml                     # virtual workspace
`-- crates/
    |-- looper/                    # flitzis-looper (PyO3 + audio engine)
    |   |-- Cargo.toml
    |   |-- build.rs
    |   `-- src/
    |       |-- lib.rs             # PyO3 module export
    |       |-- messages.rs        # fixed-size command/parameter/telemetry types
    |       `-- audio_engine/
    |           |-- mod.rs         # AudioEngine API and background orchestration
    |           |-- audio_stream.rs
    |           |-- buffer_retirement.rs
    |           |-- constants.rs
    |           |-- dsp.rs
    |           |-- input_mapping.rs
    |           |-- mixer.rs
    |           |-- scheduler.rs
    |           |-- transport.rs
    |           |-- voice_slot.rs
    |           |-- stretch_processor.rs
    |           |-- sample_loader.rs
    |           |-- stem_cache.rs
    |           |-- progress.rs
    |           |-- channels.rs
    |           `-- errors.rs
    `-- analysis/                  # flitzis-looper-analysis (BPM pipeline)
        |-- Cargo.toml
        `-- src/
            |-- lib.rs
            |-- detection_function.rs
            |-- tempotrack.rs
            |-- phase_vocoder.rs
            |-- downbeat.rs
            |-- math_utils.rs
            `-- window.rs
```

Most modules are `pub(crate)`. `lib.rs`, `audio_engine/mod.rs`, and
`src/flitzis_looper_audio/__init__.pyi` define the Python-facing boundary.

## Runtime Path

```text
Python controllers
-> AudioEngine PyO3 methods
-> bounded command ring + bounded parameter ring
-> CPAL callback
-> TransportTimeline + TransportScheduler
-> RtMixer
-> output-frame anchored BPM Lock timing
-> source selection, loop wrap, playback-rate / Key Lock
-> smoothed per-pad Gain/Trim
-> per-pad DSP chain
-> trigger velocity / master volume / metering
-> output buffer
```

The callback must not perform disk I/O, JSON access, Python/GIL work, UI work,
logging, plugin loading, neural inference, blocking waits, unbounded loops, or
heavy allocation.

## Development Commands

Run these from the repository root:

```powershell
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml --workspace
.\scripts\run-rust-tests.ps1
uv run cargo fmt --manifest-path rust/Cargo.toml --all --check
```

Use `uv run cargo ...` so PyO3 and maturin use the project Python environment.
The Windows script adds uv's selected Python runtime and the documented Rubber
Band runtime directories to `PATH` before launching the standalone Rust test
executable.
On non-Windows platforms, or in a Windows shell where the Rubber Band runtime
DLLs are already visible to standalone test executables, the Rust test command
is `uv run cargo test --manifest-path rust/Cargo.toml --workspace`.

## Design Notes

- Rust owns live audio truth: transport, scheduler, mixer, loaded buffers,
  source playheads, prepared-stem selection, realtime parameter application,
  smoothed dB Gain/Trim, metering, and DSP state.
- Python owns UI, durable project intent, persistence, settings, mapping edit
  UX, and offline/background orchestration.
- Ordered commands and high-rate scalar parameters use separate bounded queues.
- PyO3 setters for must-apply command and parameter publications report full
  queues as caller-visible `RuntimeError`s instead of silently accepting the
  write.
- Parameter messages are coalesced by identity in the callback before applying
  the latest drained value. The callback applies only identities touched by the
  drained batch instead of sweeping every pad slot.
- Pad peak/playhead telemetry is cadence-gated and published only for pads
  touched by rendering in that callback; inactive bank slots are not scanned for
  telemetry.
- Scheduled mixer segments carry absolute output-frame positions. BPM-locked
  active voices with valid master and pad BPM metadata use fixed
  output-frame/source-frame anchors to derive source loop phase from the Rust
  transport timeline.
- Per-pad load and analysis work carries request identity across the PyO3
  boundary. Rust rejects stale sample publication after unload or replacement,
  and Python ignores stale progress, error, success, and analysis events.
- Rust MIDI capture runs outside the callback and receives mapping snapshots,
  input-runtime pad state, and Learn/capture state from Python. Direct MIDI
  command dispatch is all-or-nothing; failed direct attempts are reported back
  to Python for controller-owned fallback outside the MIDI dispatcher.
- Sample and prepared-stem handles removed from callback-owned state are retired
  through a bounded non-audio worker to avoid large final drops on the audio
  thread.

See `../docs/architecture.md` for the full architecture reference.
