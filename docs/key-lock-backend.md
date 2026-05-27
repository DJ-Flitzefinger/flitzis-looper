# Key Lock Backend

This document records the current Rubber Band based Key Lock implementation and
the remaining cleanup work around the older manual Key Lock settings surface.

Key Lock is one replaceable part of the Rust audio/DSP foundation. It does not
imply plugin hosting, a new FX module, or a broad rewrite.

## Current Implementation

The current path lives behind:

```text
rust/src/audio_engine/stretch_processor.rs
rust/src/audio_engine/rubberband_backend.rs
```

User-facing semantics:

- Key Lock off: varispeed playback. Tempo and pitch move together.
- Key Lock on: master-tempo-style playback. The source playhead still advances
  by the active tempo ratio, then the varispeed block is processed through a
  per-voice Rubber Band LiveShifter with pitch scale derived from
  `1.0 / tempo_ratio`.
- BPM Lock supplies the same tempo ratio as before:
  `master_bpm / pad_bpm` when metadata exists, otherwise global speed fallback.
- Full-mix and prepared-stem playback share the same path.

The previous custom delay-line pitch-compensation stage has been removed from
the active runtime path. Rubber Band handle construction, fixed block-size and
start-delay queries, staging buffers, channel pointer arrays, and bounded FIFOs
are prepared with the voice slot before callback rendering. The callback reuses
those buffers and never performs library discovery, handle construction, buffer
resize, disk I/O, logging, blocking waits, Python/GIL work, plugin work, or
unbounded retry loops.

The selected Windows vcpkg Rubber Band 4.0.0 package reports a 512-frame
LiveShifter block size and a 3678-sample start delay at 48 kHz stereo. The first
implementation accepts that output delay while keeping playhead telemetry and
loop ownership source-frame based. If shifted output is not available for a
callback block because a fixed Rubber Band block has not yet been completed, the
processor fills the missing frames with silence as a deterministic bounded
fallback.

## Settings Contract

Key Lock DSP settings are still persisted as bounded scalar values and
published to Rust as fixed-size control messages:

- delay minimum: `16..512` samples,
- delay range: `256..1984` samples,
- delay minimum plus range: at most `2032` samples,
- head count: `1..4`,
- interpolation: `linear` or `cubic`,
- window: `triangle` or `hann`,
- smoothing step: `0.01..0.099`,
- output gain: `0.25..2.0`.

New projects still use the former High baseline: delay minimum `64`, delay
range `1536`, `2` heads, cubic interpolation, Hann window, smoothing step
`0.05`, and output gain `1.0`.

Legacy quality presets remain compatibility aliases that map to concrete
parameter sets. During the Rubber Band transition, only the smoothing step is
still active in the Rust voice path because it limits tempo-ratio changes; the
old delay-line delay, head-count, interpolation, window, and output-gain fields
no longer affect Rubber Band processing. A later cleanup phase should remove or
replace those obsolete performer-facing settings from Python, the type stubs,
and the UI.

## Realtime Constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation.

Each voice owns fixed source input, varispeed, Rubber Band staging, shifted
output, and FIFO buffers before rendering starts. The callback updates only
scalar ratio/mode state, pushes or pops from bounded preallocated storage, and
reuses those buffers.

## Historical Backend Candidates

The current repo cannot treat every library called "Signalsmith" as equivalent.
The installed `cute_dsp::SignalsmithStretch` path was a simplified Rust port and
not the real Signalsmith Stretch algorithm.

Candidates:

| Backend | Fit | Trade-off |
| --- | --- | --- |
| Superpowered TimeStretching | Strong DJ-oriented commercial candidate with independent tempo/pitch controls and low-latency realtime use. | Proprietary SDK. Needs licensing, distribution, and Rust build integration. |
| Rubber Band Library | Strong open-source native candidate with documented realtime mode and dynamic ratios. | C++ integration and licensing/build surface need a deliberate project decision. |
| Real Signalsmith Stretch | Lightweight algorithmic candidate with documented latency and transpose controls. | Needs binding to the real backend or a verified Rust port. |

The Rubber Band replacement preserves the wrapper contract:

- initialization and allocation outside the callback,
- bounded per-voice render state,
- scalar smoothed parameter updates,
- no file paths, plugin handles, Python objects, or heap-owned audio payloads in
  callback messages,
- measured and documented algorithmic latency.

## Rubber Band Replacement Branch Requirements

The `gen3-rubberband` branch replaces the custom delay-line compensation path
with Rubber Band. That branch must preserve the current Linux support and add
Windows support without making vcpkg a hardcoded production dependency.

Required build/runtime direction:

- Linux uses the system Rubber Band development package and `pkg-config` where
  possible.
- Windows development may use vcpkg through `VCPKG_ROOT` or documented override
  variables.
- Production source must not contain local paths such as a developer's vcpkg
  checkout or Linux home directory.
- Runtime library discovery and missing-library diagnostics happen before audio
  rendering starts, not in the CPAL callback.
- The later Windows setup installer should bundle the Rubber Band runtime DLLs
  so non-technical users do not need build tools or vcpkg.
- Binary distribution planning must account for Rubber Band's GPL/commercial
  licensing model.

Reference URLs:

- https://docs.superpowered.com/reference/latest/time-stretching/
- https://breakfastquay.com/rubberband/integration.html
- https://signalsmith-audio.co.uk/code/stretch/
