# Time-stretch and Pitch-shift

This document records the current Gen3 Key Lock decision and the future replacement path for a
pro-grade master-tempo backend.

## Backend research

The current repo cannot treat every library called "Signalsmith" as equivalent. The installed
`cute_dsp::SignalsmithStretch` implementation is a simplified Rust port whose `process()` method
scales buffer indices and does not apply the stored transpose value to rendered output. It is not
the real Signalsmith Stretch algorithm and does not satisfy the Key Lock contract by itself.

Options considered:

| Backend | Fit | Trade-off |
| --- | --- | --- |
| Superpowered TimeStretching | Strongest DJ-oriented commercial candidate. The official SDK documents a mode intended for DJ apps and complete music, with independent tempo/pitch controls and low-latency real-time use. | Proprietary SDK. Needs licensing, distribution, and Rust build integration before use. |
| Rubber Band Library | Strong open-source native candidate. The official integration notes document real-time mode, dynamic time/pitch ratios, and RT-safe processing after initialization with normal options. | C++ integration and licensing/build surface need a deliberate project decision. |
| Real Signalsmith Stretch | Lightweight C++ algorithmic candidate. Official docs expose input/output latency, split computation, and transpose controls. | The current Rust crate in the project is not the real implementation. A future integration must bind the real backend or a verified Rust port. |

## Current implementation

The current repair keeps the processing inside Rust and behind `rust/src/audio_engine/stretch_processor.rs`.
It corrects the user-facing semantics without adding plugin loading or heavyweight worker traffic
to the CPAL callback:

- Key Lock off: varispeed playback. Tempo and pitch move together.
- Key Lock on: master-tempo playback. The source playhead still advances by the active tempo
  ratio, then a bounded pitch-compensation stage reduces the pitch movement caused by varispeed.
- BPM Lock supplies the same tempo ratio as before (`master_bpm / pad_bpm` when metadata exists,
  otherwise global speed fallback). Key Lock only decides how that ratio sounds.
- Full-mix and prepared-stem playback share the same path.
- Key Lock DSP parameters are persisted as bounded manual Settings values. New projects default
  to the former High baseline: delay minimum `64` samples, delay range `1536` samples, `2` heads,
  cubic interpolation, Hann window, smoothing step `0.05`, and output gain `1.0`.
- Supported manual ranges are delay minimum `16..512` samples, delay range `256..1984` samples,
  combined delay minimum plus range at most `2032` samples, head count `2..4`, interpolation
  `linear` or `cubic`, window `triangle` or `hann`, smoothing step `0.01..0.10`, and output gain
  `0.25..2.0`.
- Legacy Key Lock quality preset values remain compatibility aliases, but the Settings page now
  publishes the concrete parameter set.

The current bounded pitch-compensation stage is a pragmatic low-latency implementation, not the
final statement on DJ-grade quality. Its main purpose is to make the Key Lock contract real,
deterministic, and safe today while keeping the wrapper replaceable.

## Real-time constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation.

Each voice owns fixed input, intermediate/output, and delay-line buffers before rendering starts.
The callback updates only scalar ratio/mode state and reuses those buffers.

## Future pro-grade backend path

A future replacement should preserve the same wrapper contract:

- initialization and allocation happen outside the callback,
- render uses bounded per-voice state,
- parameter updates are scalar and smoothed,
- no file paths, plugin handles, Python objects, or heap-owned audio payloads cross into callback
  messages,
- algorithmic latency is measured and either compensated or documented.

For a commercial DJ-grade result, Superpowered is the best candidate to evaluate first. For an
open-source native path, evaluate Rubber Band before a real Signalsmith binding because Rubber Band
documents real-time time/pitch-ratio operation more directly. A real Signalsmith binding remains
attractive if the project prioritizes a smaller algorithmic footprint over Rubber Band's broader
feature set.

Reference URLs:

- https://docs.superpowered.com/reference/latest/time-stretching/
- https://breakfastquay.com/rubberband/integration.html
- https://signalsmith-audio.co.uk/code/stretch/
