# Key Lock Backend

This document records the current Rubber Band based Key Lock implementation.

Key Lock is one bounded part of the Rust audio/DSP foundation. It does not imply
plugin hosting, a separate FX graph, or realtime stem generation.

## Runtime Modules

The active backend is implemented behind:

```text
rust/src/audio_engine/stretch_processor.rs
rust/src/audio_engine/rubberband_backend.rs
```

`RtMixer` owns tempo-ratio selection, per-pad Key Lock state, source-frame
addressing, full-mix/stem source reads, and per-voice `StretchProcessor` calls.
`VoiceSlot` owns the per-voice `StretchProcessor`, smoothed tempo ratio,
explicit seek mode, and optional `PlaybackTimelineAnchor`.

## Playback Semantics

- Per-pad Key Lock off: playback is varispeed, so tempo and pitch move
  together.
- Per-pad Key Lock on: source-frame tempo progression remains active, and the
  varispeed block is processed through a per-voice Rubber Band LiveShifter with
  pitch scale derived from `1.0 / tempo_ratio`.
- The global Key Lock control overwrites every per-pad Key Lock value. A later
  per-pad toggle changes only that pad.
- BPM Lock off: the active tempo ratio is the global speed multiplier.
- BPM Lock on with valid master and pad BPM metadata: the active tempo ratio is
  `master_bpm / pad_bpm`.
- Pads without valid BPM metadata use the global speed multiplier.
- Full-mix and prepared-stem playback share the same source addressing and Key
  Lock path.

## BPM-Locked Timing

Scheduled render segments carry absolute output-frame positions from
`audio_stream.rs` into `RtMixer::render_rt_at_output_frame(...)`.

BPM-locked active voices with valid master and pad BPM metadata store a fixed
`PlaybackTimelineAnchor` containing:

- the absolute output frame where the current voice timeline is anchored,
- the source frame corresponding to that output frame.

For each rendered segment, `RtMixer` maps absolute output-frame delta through
the active tempo ratio, converts that non-cumulative source offset into the
voice's half-open source-frame loop region, and records the next anchored source
frame for playhead telemetry. Pads that represent the same musical loop length
therefore share the same output-frame loop boundaries across repeated loop
wraps.

START/STOP, explicit retrigger, seek, pause/resume, and loop-boundary state
changes refresh the voice timeline anchor. Ordinary loop wrapping does not
redefine the Rust master output timeline, Rubber Band state, the Loop Editor
source grid, or prepared-stem alignment.

## Rubber Band Processing

Each voice slot prepares its Rubber Band handle, fixed block-size metadata,
input buffers, channel pointer arrays, shifted-output buffers, and bounded FIFOs
outside the per-sample render work. The audio callback reuses that state.

The tested Windows vcpkg Rubber Band 4.0.0 package reports a 512-frame
LiveShifter block size and a 3678-sample start delay at 48 kHz stereo. Playhead
telemetry and loop ownership remain source-frame based. Rubber Band output
latency does not shift trigger quantization, transport scheduling, or source
loop ownership.

If shifted output is unavailable for part of a callback block, the processor
fills the missing frames with silence as a deterministic bounded result and
continues rendering.

## Settings Contract

Project persistence stores the global `key_lock` boolean as global-control
intent and `pad_key_lock` as one durable boolean per pad. It does not store
Rubber Band handles, DLL/shared-library paths, runtime buffers, measured
latency, algorithmic delay, or callback-internal backend state.

The performer Settings UI exposes no Rubber Band backend tuning surface. Rust
uses a fixed internal tempo-ratio smoothing step for active voices; that value is
not persisted or user-tunable.

## Realtime Constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation,
- spin while waiting for Rubber Band output.

The callback updates scalar mode/ratio state, reads bounded per-pad Key Lock
state, reads prepared source buffers, uses fixed Rubber Band staging storage,
consumes or produces bounded FIFO data, and mixes the resulting output through
Gain/Trim, DSP, metering, and master volume.

## Native Dependency

The backend requires a Rubber Band C API that exports `rubberband_live_*`
symbols. The Windows vcpkg Rubber Band 4.0.0 package satisfies that requirement.
Ubuntu 24.04 `librubberband-dev` 3.3.0 does not provide the required LiveShifter
C API.

Build discovery uses documented platform mechanisms and explicit environment
overrides:

- Linux: `pkg-config` for a Rubber Band package with LiveShifter C API support,
  or explicit `RUBBERBAND_LIB_DIR` and `RUBBERBAND_INCLUDE_DIR` overrides.
- Windows: `RUBBERBAND_LIB_DIR`, `VCPKG_ROOT`, or the documented
  `%LOCALAPPDATA%\vcpkg` development location.
- Runtime DLL/shared-library availability is established before the audio engine
  enters realtime callback rendering.

The Windows source-run helper registers Rubber Band DLL directories before
loading the native extension. Packaging should provide the required runtime
libraries with the application artifact and account for Rubber Band licensing
before binary distribution.

Reference URLs:

- https://breakfastquay.com/rubberband/integration.html
