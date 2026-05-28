# Design: State ownership boundary

## Ownership model

`ProjectState` owns durable performer intent: project-local sample references, analysis metadata,
manual BPM and key overrides, loop settings, per-pad gain/EQ, global speed/volume/modes, trigger
quantization, stem cache metadata, stem mix preference, and project-scoped settings.

`SessionState` owns transient control/UI projections: active and paused pad indicators,
playheads, peaks, pending task progress, Learn capture state, waveform editor state, selected
overlay state, and session-only stem mask display state. It is rebuilt on application start and is
not live audio truth.

Rust owns live audio state: loaded immutable buffers, accepted prepared stem handles, source
playheads, active voices, pause/render state, loop frame positions after publication, transport
timeline, scheduler, current scalar audio parameters, Key Lock processing state, and future DSP
smoothing state.

## Restore ordering

Startup loads `ProjectState` first, starts the audio engine, constructs controllers, publishes
global and per-pad durable intent as bounded Rust control or parameter messages, validates project
stem cache metadata, schedules cached sample loads, and publishes input-runtime snapshots.

This change documents the ordering rather than changing sample loading semantics.

## Telemetry and acknowledgements

Audio-to-control messages are bounded and best-effort. The controller treats `SampleStarted`,
`SampleStopped`, `PadPeak`, and `PadPlayhead` messages as Rust-origin telemetry used to update
Python `SessionState` projections. Dropped telemetry can leave a projection stale until a later
controller action or later telemetry reconciles it.

For this slice, the small behavior cleanup is architectural: telemetry type dispatch moves from
the UI context to `AppController`. The UI can request polling during render, but it no longer owns
which message mutates which controller/session field.

## Realtime constraints

The audio callback must still avoid disk I/O, JSON reads/writes, Python/GIL access, UI calls,
blocking locks or waits, logging, neural inference, plugin loading/scanning, unbounded loops,
heavy allocation, and long-running work.
