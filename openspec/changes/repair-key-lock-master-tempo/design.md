## Library Decision

The best-performing DJ-oriented commercial option found for this feature is Superpowered
TimeStretching: its official documentation exposes a mode tuned for DJ apps and complete music,
documents low-latency real-time usage, and provides independent tempo and pitch controls. It is
not appropriate to drop into this repo in the current slice because it is a proprietary SDK and
would need licensing, distribution, and Rust build integration decisions.

Rubber Band is the strongest open-source/library candidate for a future native backend. Its
documentation describes real-time mode, dynamic time/pitch ratios, and RT-safe operation after
initialization with normal parameters. The trade-off is C++ integration and licensing/build
surface that should be handled deliberately, not as an incidental dependency swap in a small
behavior repair.

Signalsmith Stretch remains a good lightweight C++ algorithmic option and documents the latency
and split-computation model needed for strict audio callbacks. The currently installed Rust crate
named through `cute_dsp` is not the real algorithm, so using it unchanged would not satisfy Key
Lock.

For this repository slice, the implementation keeps the processing in Rust and repairs the
contract with a bounded wrapper:

- Key Lock off uses direct varispeed resampling, so pitch changes with tempo.
- Key Lock on applies a low-latency bounded pitch-compensation stage after varispeed so the tempo
  change remains while pitch movement is reduced.
- Key Lock DSP settings are bounded manual global parameters. The former High preset values remain
  the default baseline, while legacy preset names remain compatibility aliases that map to
  concrete delay, interpolation, window, smoothing, and gain parameters.
- All buffers and delay lines are allocated when the voice slots are constructed, not while the
  CPAL callback renders.

This keeps the public and real-time contracts correct now and leaves a narrow wrapper boundary for
replacing the internal algorithm later with Rubber Band, Superpowered, or the real Signalsmith
Stretch.

## Audio-Thread Architecture

The CPAL callback continues to own the mixer and voice slots. Each voice owns one DSP wrapper with:

- fixed per-channel input buffers,
- fixed per-channel intermediate/output buffers,
- fixed per-channel pitch-compensation delay lines,
- scalar phase/write-position state.

The audio callback may update scalar target ratio, Key Lock mode, and Key Lock parameter state from
already-owned mixer state. It must not allocate, block, log, inspect files, decode audio, call
Python, or load plugins.

## Combination Behavior

Prepared stems flow through the same source-sample reader as full-mix playback, so Key Lock applies
to full mix and all-stems mode identically.

BPM Lock continues to compute the per-pad tempo ratio from master BPM and pad BPM. Key Lock only
changes whether that ratio is heard as varispeed or master-tempo playback.

Trigger quantization and Multi Loop continue to choose output start frames and voice start state.
Key Lock does not move scheduled events, reset loop starts, or redefine the permanent transport.

Loop Editor playhead reporting remains source-playhead based: the voice advances through the loop
by the tempo ratio, and Key Lock changes pitch compensation, not source position bookkeeping.

## Validation Strategy

- Add Rust unit coverage for Key Lock off versus on using a deterministic sine loop and
  zero-crossing pitch estimation after initial compensation latency.
- Add coverage that neutral speed with Key Lock stays transparent.
- Add coverage that the per-voice DSP buffers are preallocated for the render bounds used by the
  callback.
- Run the official OpenSpec validator for this change.
- Run full uv-managed Rust/Python validation because this touches shared audio behavior.
