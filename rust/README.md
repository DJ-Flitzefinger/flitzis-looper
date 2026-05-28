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
rust/src/
|-- lib.rs                         # PyO3 module export
|-- messages.rs                    # fixed-size command/parameter/telemetry types
`-- audio_engine/
    |-- mod.rs                     # AudioEngine API and background orchestration
    |-- audio_stream.rs            # CPAL callback and scheduler integration
    |-- buffer_retirement.rs       # non-audio retirement of large handles
    |-- constants.rs               # banks, slots, ranges, queue budgets
    |-- dsp.rs                     # per-pad DSP chain and DJ isolator
    |-- input_mapping.rs           # MIDI capture outside the audio callback
    |-- mixer.rs                   # RtMixer, voices, loops, stems, gain, DSP
    |-- scheduler.rs               # fixed-capacity output-frame scheduler
    |-- transport.rs               # output-frame timeline and musical phase
    |-- voice_slot.rs              # voice state and per-voice processing buffers
    |-- stretch_processor.rs       # bounded Key Lock/master-tempo wrapper
    |-- sample_loader.rs           # non-realtime decode/cache/resample
    |-- analysis.rs                # non-realtime BPM/key/beat-grid analysis
    |-- stem_cache.rs              # prepared-stem validation/loading
    |-- progress.rs
    |-- channels.rs
    `-- errors.rs
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
uv run cargo check --manifest-path rust/Cargo.toml
uv run cargo test --manifest-path rust/Cargo.toml
uv run cargo fmt --manifest-path rust/Cargo.toml --check
```

Use `uv run cargo ...` so PyO3 and maturin use the project Python environment.

## Design Notes

- Rust owns live audio truth: transport, scheduler, mixer, loaded buffers,
  source playheads, prepared-stem selection, realtime parameter application,
  smoothed dB Gain/Trim, metering, and DSP state.
- Python owns UI, durable project intent, persistence, settings, mapping edit
  UX, and offline/background orchestration.
- Ordered commands and high-rate scalar parameters use separate bounded queues.
- Parameter messages are coalesced by identity in the callback before applying
  the latest drained value.
- Scheduled mixer segments carry absolute output-frame positions. BPM-locked
  active voices with valid master and pad BPM metadata use fixed
  output-frame/source-frame anchors to derive source loop phase from the Rust
  transport timeline.
- Sample and prepared-stem handles removed from callback-owned state are retired
  through a bounded non-audio worker to avoid large final drops on the audio
  thread.

See `../docs/architecture.md` for the full architecture reference.
