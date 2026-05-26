# Key Lock Backend

This document records the current Key Lock implementation and the future
replacement path for a pro-grade master-tempo backend.

Key Lock is one replaceable part of the Rust audio/DSP foundation. It does not
imply plugin hosting, a new FX module, or a broad rewrite.

## Current Implementation

The current path lives behind:

```text
rust/src/audio_engine/stretch_processor.rs
```

User-facing semantics:

- Key Lock off: varispeed playback. Tempo and pitch move together.
- Key Lock on: master-tempo-style playback. The source playhead still advances
  by the active tempo ratio, then bounded pitch compensation reduces the pitch
  movement caused by varispeed.
- BPM Lock supplies the same tempo ratio as before:
  `master_bpm / pad_bpm` when metadata exists, otherwise global speed fallback.
- Full-mix and prepared-stem playback share the same path.

The current bounded pitch-compensation stage is pragmatic and deterministic. It
is not the final statement on DJ-grade time-stretch quality.

## Settings Contract

Key Lock DSP settings are persisted as bounded scalar values and published to
Rust as fixed-size control messages:

- delay minimum: `16..512` samples,
- delay range: `256..1984` samples,
- delay minimum plus range: at most `2032` samples,
- head count: `1..4`,
- interpolation: `linear` or `cubic`,
- window: `triangle` or `hann`,
- smoothing step: `0.01..0.099`,
- output gain: `0.25..2.0`.

New projects use the former High baseline: delay minimum `64`, delay range
`1536`, `2` heads, cubic interpolation, Hann window, smoothing step `0.05`, and
output gain `1.0`.

Legacy quality presets remain compatibility aliases that map to concrete
parameter sets.

## Realtime Constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation.

Each voice owns fixed input, intermediate/output, and delay-line buffers before
rendering starts. The callback updates only scalar ratio/mode state and reuses
those buffers.

## Future Backend Candidates

The current repo cannot treat every library called "Signalsmith" as equivalent.
The installed `cute_dsp::SignalsmithStretch` path was a simplified Rust port and
not the real Signalsmith Stretch algorithm.

Candidates:

| Backend | Fit | Trade-off |
| --- | --- | --- |
| Superpowered TimeStretching | Strong DJ-oriented commercial candidate with independent tempo/pitch controls and low-latency realtime use. | Proprietary SDK. Needs licensing, distribution, and Rust build integration. |
| Rubber Band Library | Strong open-source native candidate with documented realtime mode and dynamic ratios. | C++ integration and licensing/build surface need a deliberate project decision. |
| Real Signalsmith Stretch | Lightweight algorithmic candidate with documented latency and transpose controls. | Needs binding to the real backend or a verified Rust port. |

A future replacement should preserve the wrapper contract:

- initialization and allocation outside the callback,
- bounded per-voice render state,
- scalar smoothed parameter updates,
- no file paths, plugin handles, Python objects, or heap-owned audio payloads in
  callback messages,
- measured and documented algorithmic latency.

Reference URLs:

- https://docs.superpowered.com/reference/latest/time-stretching/
- https://breakfastquay.com/rubberband/integration.html
- https://signalsmith-audio.co.uk/code/stretch/
