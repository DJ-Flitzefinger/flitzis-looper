# Rust Audio Engine Module

This crate builds the native `flitzis_looper_audio` Python extension. It owns
the realtime audio path for Flitzis Looper and exposes the `AudioEngine` PyO3
class used by the Python controllers.

## Module Structure

The audio engine is organized into small internal modules:

- `lib.rs`: PyO3 module export for `flitzis_looper_audio`.
- `messages.rs`: fixed-size command, parameter, loader, and telemetry message
  types shared between threads.
- `audio_engine/mod.rs`: Python-facing `AudioEngine` API, background task
  orchestration, loader/stem publication helpers, and input runtime lifecycle.
- `audio_engine/audio_stream.rs`: CPAL stream setup, bounded callback message
  draining, scheduler integration, and realtime rendering entry point.
- `audio_engine/buffer_retirement.rs`: bounded non-audio retirement worker for
  large sample and prepared-stem handles removed by the callback.
- `audio_engine/constants.rs`: shared limits such as banks, grid size, slot
  count, voice count, and parameter ranges.
- `audio_engine/dsp.rs`: fixed-size per-pad DSP chain, typed DSP parameter
  identities, smoothing helpers, and the current DJ isolator node.
- `audio_engine/input_mapping.rs`: MIDI capture, timestamping, filtering,
  in-memory mapping lookup, and command dispatch outside the CPAL callback.
- `audio_engine/mixer.rs`: `RtMixer`, sample slots, prepared-stem state, voice
  rendering, loop playback, gain, DSP routing, metering, and playhead state.
- `audio_engine/scheduler.rs`: fixed-capacity absolute output-frame scheduler.
- `audio_engine/transport.rs`: output-frame transport timeline and musical
  grid/phase helpers.
- `audio_engine/voice_slot.rs`: active voice state, pause/resume state, and
  per-voice stretch/key-lock buffers.
- `audio_engine/stretch_processor.rs`: bounded varispeed/master-tempo wrapper.
- `audio_engine/sample_loader.rs`: non-realtime audio decode, channel mapping,
  resampling, and project-local source caching.
- `audio_engine/analysis.rs`: non-realtime BPM/key/beat-grid analysis.
- `audio_engine/stem_cache.rs`: prepared-stem cache validation and loading.
- `audio_engine/progress.rs`, `audio_engine/channels.rs`, `audio_engine/errors.rs`:
  supporting helpers.

Most implementation modules are `pub(crate)`; `audio_engine/mod.rs` and
`lib.rs` define the Python-facing boundary.

## Runtime Signal Path

```text
Python controllers
-> AudioEngine PyO3 methods
-> bounded command ring + bounded parameter ring
-> CPAL callback
-> TransportTimeline + TransportScheduler
-> RtMixer
-> source selection, loop wrap, playback-rate / Key Lock
-> per-pad DSP chain
-> gain / volume / metering
-> output buffer
```

The callback must not perform disk I/O, JSON access, Python/GIL work, logging,
plugin loading, neural inference, blocking waits, or unbounded work.

## Development Commands

Run these from the repository root:

```powershell
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml
uv run cargo test --manifest-path rust/Cargo.toml
uv run cargo fmt --manifest-path rust/Cargo.toml --check
```

Use the `uv run cargo ...` form so the PyO3 build uses the project Python
environment consistently.

## Design Notes

- Rust owns live audio truth: transport, scheduler, mixer, source playheads,
  prepared-stem selection, realtime parameter application, and DSP state.
- Python owns UI, project persistence, settings, mapping edit UX, and
  offline/background orchestration.
- Discrete commands and high-rate scalar parameters use separate bounded
  control-to-audio queues. Parameter messages are coalesced by identity in the
  callback before applying the latest drained value.
- Sample and prepared-stem handles removed from callback-owned state are retired
  through a bounded non-audio worker to avoid large final drops on the audio
  thread.
